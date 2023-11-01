//! Implementation for the `run` subcommand.

use std::time::Duration;

use error_stack::{report, Result, ResultExt};
use futures::TryStreamExt;
use futures::{future::try_join_all, try_join, StreamExt};
use governor::{Quota, RateLimiter};
use indoc::indoc;
use nonzero_ext::nonzero;
use serde::{Deserialize, Serialize};
use tap::TapFallible;
use tokio_retry::strategy::jitter;
use tokio_retry::strategy::ExponentialBackoff;
use tokio_retry::Retry;
use tracing::warn;
use tracing::{debug, info};
use uuid::Uuid;

use crate::api::fossa::{self, CliMetadata, ProjectMetadata};
use crate::api::remote::git::repository;
use crate::api::remote::{
    git, BranchImportStrategy, Integrations, Protocol, Reference, TagImportStrategy,
};
use crate::ext::result::WrapErr;
use crate::ext::tracing::span_record;
use crate::fossa_cli::{self, DesiredVersion, Location, SourceUnits};
use crate::queue::Queue;
use crate::AppContext;
use crate::{
    api::remote::{Integration, RemoteProvider},
    config::Config,
    db::Database,
    ext::{
        error_stack::{DescribeContext, ErrorHelper},
        result::DiscardResult,
    },
};

/// Errors encountered during runtime.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Application health check failed.
    #[error("health check failed")]
    Healthcheck,

    /// Setting up async pipeline failed.
    #[error("set up task pipeline")]
    SetupPipeline,

    /// The application periodically polls for new references in configured integrations.
    /// If one of those polls fails, this error is returned.
    #[error("poll integration")]
    PollIntegration,

    /// If we fail to clone a reference, this error is returned.
    #[error("clone reference: {0:?}")]
    CloneReference(Reference),

    /// If we fail to send tasks to the async task queue, this error is raised.
    #[error("enqueue task for processing")]
    TaskEnqueue,

    /// If we fail to receive tasks to the async task queue, this error is raised.
    #[error("receive task for processing")]
    TaskReceive,

    /// If we fail to handle a task, this error is raised.
    #[error("handle task")]
    TaskHandle,

    /// If we fail to set a task's state in the sqlite DB, this error is raised.
    #[error("set task state")]
    TaskSetState,

    /// If we fail to mark a task complete, this error is raised.
    #[error("mark task complete")]
    TaskComplete,

    /// If we fail to download FOSSA CLI, this error is raised.
    #[error("download FOSSA CLI")]
    DownloadFossaCli,

    /// If we fail to run FOSSA CLI, this error is raised.
    #[error("run FOSSA CLI")]
    RunFossaCli,

    /// If we fail to delete tasks' state in the sqlite DB, this error is raised
    #[error("delete tasks' state")]
    TaskDeleteState,

    /// Preflight checks failed
    #[error("preflight checks")]
    PreflightChecks,

    /// Failed to connect to at least one integration
    #[error("integration connections")]
    IntegrationConnection,

    /// Failed to connect to FOSSA  
    #[error("FOSSA connection")]
    FossaConnection,
}

/// Similar to [`AppContext`], but scoped for this subcommand.
#[derive(Debug)]
struct CmdContext<D> {
    /// The application context.
    app: AppContext,

    /// The application configuration.
    config: Config,

    /// The database connection.
    db: D,
}

/// The primary entrypoint.
#[tracing::instrument(skip_all, fields(subcommand = "run"))]
pub async fn main<D: Database>(ctx: &AppContext, config: Config, db: D) -> Result<(), Error> {
    let ctx = CmdContext {
        app: ctx.clone(),
        config,
        db,
    };

    for integration in ctx.config.integrations().iter() {
        if let Err(err) = remove_repository_scan_targets(&ctx.db, integration).await {
            warn!("Unable to remove scan targets for '{integration}': {err:#?}. Contact Support for further guidance.");
        }
    }

    let preflight_checks = preflight_checks(&ctx);
    let healthcheck_worker = healthcheck(&ctx.db);
    let integration_worker = integrations(&ctx);
    try_join!(preflight_checks, healthcheck_worker, integration_worker).discard_ok()
}

/// Checks and catches network misconfigurations before Broker attempts its operations
async fn preflight_checks<D: Database>(ctx: &CmdContext<D>) -> Result<(), Error> {
    let check_integration_connections = check_integration_connections(ctx.config.integrations());
    let check_fossa_connection = check_fossa_connection(&ctx.config);
    try_join!(check_integration_connections, check_fossa_connection)
        .discard_ok()
        .change_context(Error::PreflightChecks)
}

#[tracing::instrument(skip_all)]
/// Check that Broker can connect to at least one integration
async fn check_integration_connections(integrations: &Integrations) -> Result<(), Error> {
    if integrations.as_ref().is_empty() {
        return Ok(());
    }

    for integration in integrations.iter() {
        let Protocol::Git(transport) = integration.protocol();
        if repository::ls_remote(transport).await.is_ok() {
            return Ok(());
        }
    }

    report!(Error::IntegrationConnection)
        .wrap_err()
        .help("run broker fix for detailed explanation on failing integration connections")
        .describe("integration connections")
}

#[tracing::instrument(skip_all)]
/// Check that Broker can connect to FOSSA
async fn check_fossa_connection(config: &Config) -> Result<(), Error> {
    match fossa::OrgConfig::lookup(config.fossa_api()).await {
        Ok(_) => Ok(()),
        Err(err) => err
            .change_context(Error::FossaConnection)
            .wrap_err()
            .help("run broker fix for detailed explanation on failing fossa connection")
            .describe("fossa connection"),
    }
}

#[tracing::instrument(skip_all)]
async fn remove_repository_scan_targets<D: Database>(
    db: &D,
    integration: &Integration,
) -> Result<(), Error> {
    let repository = integration.remote().for_coordinate();
    let import_branches = integration.import_branches();
    let import_tags = integration.import_tags();

    if let BranchImportStrategy::Disabled = import_branches {
        db.delete_states(&repository, true)
            .await
            .change_context(Error::TaskDeleteState)?;
    }
    if let TagImportStrategy::Disabled = import_tags {
        db.delete_states(&repository, false)
            .await
            .change_context(Error::TaskDeleteState)?
    }

    Ok(())
}

/// Conduct internal diagnostics to ensure Broker is still in a good state.
#[tracing::instrument(skip_all)]
async fn healthcheck<D: Database>(db: &D) -> Result<(), Error> {
    let period = Duration::from_secs(60);
    for _ in 0.. {
        db.healthcheck()
            .await
            .tap_ok(|_| debug!("db healtheck ok"))
            .change_context(Error::Healthcheck)
            .describe("Broker periodically runs internal healthchecks to validate that it is still in a good state")
            .help("this health check failing may have been related to a temporary condition, restarting Broker may resolve the issue")?;

        tokio::time::sleep(period).await;
    }

    Ok(())
}

/// Job for scanning git vcs
#[derive(Debug, Deserialize, Serialize)]
struct ScanGitVCSReference {
    scan_id: String,
    integration: Integration,
    reference: Reference,
}

impl ScanGitVCSReference {
    fn new(integration: &Integration, reference: &Reference) -> Self {
        Self {
            scan_id: Uuid::new_v4().to_string(),
            integration: integration.to_owned(),
            reference: reference.to_owned(),
        }
    }
}

/// Job for uploading a scan
#[derive(Debug, Deserialize, Serialize)]
struct UploadSourceUnits {
    scan_id: String,
    integration: Integration,
    reference: Reference,
    cli: CliMetadata,
    source_units: SourceUnits,
}

/// Manage the lifecycle of all integrations.
async fn integrations<D: Database>(ctx: &CmdContext<D>) -> Result<(), Error> {
    // Each integration is configured with a poll interval.
    // Rather than have one big poll loop that has to track polling times for each integration,
    // just create a task per integration; they're cheap.
    let integration_workers = ctx
        .config
        .integrations()
        .iter()
        .map(|conf| async { integration(ctx, conf).await });

    // Run all the workers in parallel. If one errors, return that error and drop the rest.
    try_join_all(integration_workers).await.discard_ok()
}

/// Manage the lifecycle of an integration.
async fn integration<D: Database>(
    ctx: &CmdContext<D>,
    integration: &Integration,
) -> Result<(), Error> {
    // Queues are per-integration.
    //
    // The reason for having the queues in the first place is basically to allow for polls, scans, and uploads
    // to operate concurrently with some buffer space.
    //
    // The upload queue is small since this is per-integration
    // and contains (potentially) lots of data sitting around in memory.
    //
    // Queues are backpressured, so if the upload queue fills up then additional scans will wait.
    let scan = Queue::default();
    let upload = Queue::new(5);

    let poll_worker = poll_integration(&ctx.db, integration, &scan);
    let scan_worker = scan_git_references(ctx, &scan, &upload);
    let upload_worker = upload_scans(ctx, &upload);

    // `try_join!` keeps all of the workers running until one of them fails,
    // at which point the failure is returned and remaining tasks are dropped.
    // It also returns all of their results as a tuple, which we don't care about,
    // so we discard that value.
    try_join!(poll_worker, scan_worker, upload_worker).discard_ok()
}

#[tracing::instrument(skip(db, sender))]
async fn poll_integration<D: Database>(
    db: &D,
    integration: &Integration,
    sender: &Queue<ScanGitVCSReference>,
) -> Result<(), Error> {
    let poll_interval = integration.poll_interval().as_duration();
    loop {
        if let Err(err) = execute_poll_integration(db, integration, sender).await {
            warn!("Unable to poll '{integration}': {err:#?}");
        }

        // Now wait for the next poll time.
        // The fact that we poll, _then_ wait for the poll time, means that the actual
        // time at which the poll occurs will slowly creep forward by whatever time
        // it takes to perform the poll.
        //
        // This is considered okay, because we're interpreting "poll interval" as
        // "poll at most this often"; i.e. it's used primarily as a rate limiting feature
        // than as a "track changes this often" feature.
        //
        // The alternative would be starting the clock at the top of the loop,
        // which while doable could (in the worst case) allow for endlessly
        // polling if the poll takes at least as long as the interval.
        //
        // If we decide to make polling more consistent, [`tokio::time::interval`]
        // is most likely the correct way to implement it.
        info!("Next poll interval for '{integration}' in {poll_interval:?}");
        tokio::time::sleep(poll_interval).await;
    }
}

#[tracing::instrument(skip_all)]
async fn execute_poll_integration<D: Database>(
    db: &D,
    integration: &Integration,
    sender: &Queue<ScanGitVCSReference>,
) -> Result<(), Error> {
    // We use this in a few places and may send it across threads, so just clone it locally.
    let remote = integration.remote().to_owned();

    // [`Retry`] needs a function that runs without any arguments to perform the retry, so turn the method into a closure.
    let get_references = || async {
        match integration.references().await {
            Ok(success) => Ok(success),
            Err(err) => {
                warn!("Unable to poll integration at {remote}: {err:#}");
                Err(err)
            }
        }
    };

    info!("Polling '{integration}'");

    // Given that this operation is not latency sensitive, and temporary network issues can interfere,
    // retry several times before permanently failing since a permanent failure means Broker shuts down
    // entirely.
    let strategy = ExponentialBackoff::from_millis(1000).map(jitter).take(10);
    let references = Retry::spawn(strategy, get_references)
            .await
            .change_context(Error::PollIntegration)
            .describe_lazy(|| format!("poll for changes at {remote} in integration: {integration}"))
            .help(indoc! {"
            Issues with this process are usually related to network errors, but may be due to misconfiguration.
            Each time this polling operation was attempted, it logged a warning; please review those
            warnings in the logs for more details.
            "})?;

    // Filter to the list of references that are new since we last saw them.
    let references = futures::stream::iter(references.into_iter())
            // Using `filter_map` instead of `filter` so that this closure gets ownership of `reference`,
            // which makes binding it across an await point easier (no lifetimes to mess with).
            .filter_map(|reference| async {
                match &reference {
                    Reference::Git(git_reference) => match git_reference {
                        git::Reference::Branch {..} => {
                            // Skipping because integration is not configured to scan branches or branch was not in the integration's watched branches
                            if integration.import_branches().should_skip_branches() || !integration.should_scan_reference(reference.name()){
                                return None
                            }
                        },
                        git::Reference::Tag{..}  => {
                            // Skipping because integration was not configured to scan tags
                            if integration.import_tags().should_skip_tags() {
                                return None
                            }
                        },
                    }
                }

                let coordinate = reference.as_coordinate(&remote);
                match db.state(&coordinate).await {
                    // No previous state; this must be a new reference.
                    Ok(None) => Some(Ok(reference)),
                    // There was previous state, it's only new if the state is different.
                    // We're assuming "different state" always means "newer state".
                    // This is because state is currently expressed as a git commit string,
                    // which on its own doesn't have any form of ordering.
                    Ok(Some(db_state)) => {
                        if db_state == reference.as_state() {
                            None
                        } else {
                            Some(Ok(reference))
                        }
                    }
                    // Pass through errors.
                    Err(err) => Some(Err(err)),
                }
            })
            // Fold ourselves because `collect` expects its destination to implement `Default`.
            .try_fold(Vec::new(), |mut references, reference| async {
                references.push(reference);
                Ok(references)
            })
            .await
            .change_context(Error::PollIntegration)
            .describe_lazy(|| {
                format!("filter to only changes at {remote} in integration: {integration}")
            })
            .help(indoc! {"
            Problems at this stage are most likely caused by a database error.
            Broker manages a local sqlite database; deleting it so it can be re-generated from scratch may resolve the issue.
            "})?;

    // We sink the references here instead of during the stream so that
    // if an error is encountered reading the stream, we don't send partial lists.
    if references.is_empty() {
        info!("No changes to '{integration}'");
    }
    for reference in references {
        let job = ScanGitVCSReference::new(integration, &reference);
        sender.send(&job).await.change_context(Error::TaskEnqueue)?;

        info!("Enqueued task to scan '{integration}' at '{reference}'");
    }

    Ok(())
}

#[tracing::instrument(skip_all)]
async fn scan_git_references<D: Database>(
    ctx: &CmdContext<D>,
    receiver: &Queue<ScanGitVCSReference>,
    uploader: &Queue<UploadSourceUnits>,
) -> Result<(), Error> {
    let cli = fossa_cli::find_or_download(
        &ctx.app,
        ctx.config.debug().location(),
        DesiredVersion::Latest,
    )
    .await
    .change_context(Error::DownloadFossaCli)
    .describe("Broker relies on fossa-cli to perform analysis of your projects")?;

    loop {
        if let Err(err) = execute_scan_git_references(ctx, receiver, uploader, &cli).await {
            warn!("Unable to scan git reference: {err:#?}");
        }
    }
}

#[tracing::instrument(skip_all)]
async fn execute_scan_git_references<D: Database>(
    ctx: &CmdContext<D>,
    receiver: &Queue<ScanGitVCSReference>,
    uploader: &Queue<UploadSourceUnits>,
    cli: &Location,
) -> Result<(), Error> {
    let job = receiver.recv().await.change_context(Error::TaskReceive)?;
    let upload = scan_git_reference(ctx, &job, cli)
        .await
        .change_context(Error::TaskHandle)?;
    uploader
        .send(&upload)
        .await
        .change_context(Error::TaskEnqueue)
}

#[tracing::instrument(skip(_ctx, cli), fields(scan_id, cli_version))]
async fn scan_git_reference<D: Database>(
    _ctx: &CmdContext<D>,
    job: &ScanGitVCSReference,
    cli: &fossa_cli::Location,
) -> Result<UploadSourceUnits, Error> {
    info!("Scanning '{}' at '{}'", job.integration, job.reference);
    span_record!(scan_id, &job.scan_id);

    // Clone the reference into a temporary directory.
    let cloned_location = job
        .integration
        .clone_reference(&job.reference)
        .await
        .change_context_lazy(|| Error::CloneReference(job.reference.clone()))?;

    // Record the CLI version for debugging purposes.
    let cli_version = cli.version().await.change_context(Error::RunFossaCli)?;
    span_record!(cli_version, display cli_version);

    // Run the scan.
    let source_units = cli
        .analyze(&job.scan_id, cloned_location.path())
        .await
        .change_context(Error::RunFossaCli)?;

    info!(
        "Scanned '{}' at '{}', enqueueing for upload",
        job.integration, job.reference
    );
    Ok(UploadSourceUnits {
        cli: CliMetadata::new(cli_version),
        integration: job.integration.clone(),
        reference: job.reference.clone(),
        scan_id: job.scan_id.clone(),
        source_units,
    })
}

#[tracing::instrument(skip_all)]
async fn upload_scans<D: Database>(
    ctx: &CmdContext<D>,
    receiver: &Queue<UploadSourceUnits>,
) -> Result<(), Error> {
    // This worker is per integration, so the rate limiter should be constructed here instead of globally.
    let quota = Quota::per_minute(nonzero!(1u32));
    let limiter = RateLimiter::direct(quota);

    loop {
        let job = match receiver.recv().await.change_context(Error::TaskReceive) {
            Ok(job) => job,
            Err(err) => {
                warn!("Unable to read enqueued upload job: {err:#?}");
                continue;
            }
        };

        let meta = ProjectMetadata::new(&job.integration, &job.reference);
        if limiter.check().is_err() {
            info!("Integration '{meta}': waiting for rate limit");
            limiter.until_ready().await;
        }

        if let Err(err) = execute_upload_scans(ctx, &meta, job).await {
            warn!("Unable to upload scan for '{meta}': {err:#?}");
        }
    }
}

#[tracing::instrument(skip_all)]
async fn execute_upload_scans<D: Database>(
    ctx: &CmdContext<D>,
    meta: &ProjectMetadata,
    job: UploadSourceUnits,
) -> Result<(), Error> {
    info!("Uploading scan for project: '{meta}'");
    let locator = fossa::upload_scan(ctx.config.fossa_api(), meta, &job.cli, job.source_units)
        .await
        .change_context(Error::TaskHandle)?;

    debug!(scan_id = %job.scan_id, locator = %locator, "Uploaded scan");
    info!("Uploaded scan for project '{meta}' as locator: '{locator}'");

    let remote = job.integration.remote().to_owned();
    let coordinate = job.reference.as_coordinate(&remote);
    let state = job.reference.as_state();
    let is_branch = match &job.reference {
        Reference::Git(git_reference) => match git_reference {
            git::Reference::Branch { .. } => true,
            git::Reference::Tag { .. } => false,
        },
    };

    // Mark this reference as scanned in the local DB.
    ctx.db
        .set_state(&coordinate, state, &is_branch)
        .await
        .change_context(Error::TaskSetState)
}
