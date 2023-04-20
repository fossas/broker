//! Implementation for the `run` subcommand.

use std::sync::Arc;
use std::time::Duration;

use error_stack::{Context, Result, ResultExt};
use futures::{future::try_join_all, try_join, StreamExt};
use futures::{Future, TryStreamExt};
use indoc::indoc;
use serde::{Deserialize, Serialize};
use tap::TapFallible;
use tokio::sync::Mutex;
use tokio_retry::strategy::jitter;
use tokio_retry::strategy::ExponentialBackoff;
use tokio_retry::Retry;
use tracing::warn;
use tracing::{debug, info};
use uuid::Uuid;

use crate::api::fossa::{self, CliMetadata, ProjectMetadata};
use crate::api::remote::Reference;
use crate::ext::tracing::span_record;
use crate::fossa_cli::{self, DesiredVersion, SourceUnits};
use crate::queue::{self, Queue, Receiver, Sender};
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

    /// If we fail to mark a task complete, this error is raised.
    #[error("mark task complete")]
    TaskComplete,

    /// If we fail to download FOSSA CLI, this error is raised.
    #[error("download FOSSA CLI")]
    DownloadFossaCli,

    /// If we fail to run FOSSA CLI, this error is raised.
    #[error("run FOSSA CLI")]
    RunFossaCli,
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

    let (scan_tx, scan_rx) = queue::open(&ctx.app, Queue::Scan)
        .await
        .change_context(Error::SetupPipeline)?;
    let (upload_tx, upload_rx) = queue::open(&ctx.app, Queue::Upload)
        .await
        .change_context(Error::SetupPipeline)?;

    // This function runs a bunch of async background tasks.
    // Create them all, then just `try_join!` on all of them at the end.
    let healthcheck_worker = healthcheck(&ctx.db);
    let integration_worker = poll_integrations(&ctx, scan_tx);
    let scan_git_reference_worker = scan_git_references(&ctx, scan_rx, upload_tx);
    let upload_worker = upload_scans(&ctx, upload_rx);

    // `try_join!` keeps all of the workers running until one of them fails,
    // at which point the failure is returned and remaining tasks are dropped.
    // It also returns all of their results as a tuple, which we don't care about,
    // so we discard that value.
    try_join!(
        healthcheck_worker,
        integration_worker,
        scan_git_reference_worker,
        upload_worker,
    )
    .discard_ok()
}

/// Conduct internal diagnostics to ensure Broker is still in a good state.
#[tracing::instrument(skip_all)]
async fn healthcheck<D: Database>(db: &D) -> Result<(), Error> {
    for _ in 0.. {
        db.healthcheck()
            .await
            .tap_ok(|_| debug!("db healtheck ok"))
            .change_context(Error::Healthcheck)
            .describe("Broker periodically runs internal healthchecks to validate that it is still in a good state")
            .help("this health check failing may have been related to a temporary condition, restarting Broker may resolve the issue")?;

        tokio::time::sleep(Duration::from_secs(60)).await;
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

/// Loops forever, polling configured integrations on their `poll_interval`.
#[tracing::instrument(skip_all)]
async fn poll_integrations<D: Database>(
    ctx: &CmdContext<D>,
    sender: Sender<ScanGitVCSReference>,
) -> Result<(), Error> {
    // The sender is not thread safe; ensure it's only run one at a time.
    let sender = Arc::new(Mutex::new(sender));

    // Each integration is configured with a poll interval.
    // Rather than have one big poll loop that has to track polling times for each integration,
    // just create a task per integration; they're cheap.
    let integration_workers =
        ctx.config.integrations().iter().map(|integration| async {
            poll_integration(&ctx.db, integration, sender.clone()).await
        });

    // Run all the workers in parallel. If one errors, return that error and drop the rest.
    try_join_all(integration_workers).await.discard_ok()
}

#[tracing::instrument(skip(db, sender))]
async fn poll_integration<D: Database>(
    db: &D,
    integration: &Integration,
    sender: Arc<Mutex<Sender<ScanGitVCSReference>>>,
) -> Result<(), Error> {
    // We use this in a few places and may send it across threads, so just clone it locally.
    let remote = integration.remote().to_owned();
    let poll_interval = integration.poll_interval().as_duration();

    loop {
        info!("Polling '{integration}'");

        let references = retry_default(format!("poll '{integration}'"), || async {
            match integration.references().await {
                Ok(success) => Ok(success),
                Err(err) => {
                    warn!("attempt to poll integration at {remote} failed: {err:#}");
                    Err(err)
                }
            }
        })
        .await
        .change_context(Error::PollIntegration)
        .describe_lazy(|| format!("poll for changes at {remote} in integration: {integration}"))?;

        // Filter to the list of references that are new since we last saw them.
        let references = futures::stream::iter(references.into_iter())
            // Using `filter_map` instead of `filter` so that this closure gets ownership of `reference`,
            // which makes binding it across an await point easier (no lifetimes to mess with).
            .filter_map(|reference| async {
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
            let mut sender = sender.lock().await;
            sender.send(&job).await.change_context(Error::TaskEnqueue)?;

            info!("Enqueued task to scan '{integration}' at '{reference}'");
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
async fn scan_git_references<D: Database>(
    ctx: &CmdContext<D>,
    mut receiver: Receiver<ScanGitVCSReference>,
    mut uploader: Sender<UploadSourceUnits>,
) -> Result<(), Error> {
    let cli = retry_default("download_fossa_cli", || async {
        fossa_cli::find_or_download(
            &ctx.app,
            ctx.config.debug().location(),
            DesiredVersion::Latest,
        )
        .await
    })
    .await
    .change_context(Error::DownloadFossaCli)
    .describe("Broker relies on fossa-cli to perform analysis of your projects")?;

    loop {
        let guard = receiver.recv().await.change_context(Error::TaskReceive)?;
        let job = guard.item().change_context(Error::TaskReceive)?;

        let upload = scan_git_reference(ctx, &job, &cli)
            .await
            .change_context(Error::TaskHandle)?;

        uploader
            .send(&upload)
            .await
            .change_context(Error::TaskEnqueue)?;

        guard.commit().change_context(Error::TaskComplete)?;
    }
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
    let cloned_location = retry_default(
        format!("Clone '{}' at '{}'", job.integration, job.reference),
        || async { job.integration.clone_reference(&job.reference).await },
    )
    .await
    .change_context_lazy(|| Error::CloneReference(job.reference.clone()))?;

    // Record the CLI version for debugging purposes.
    let cli_version = cli.version().await.change_context(Error::RunFossaCli)?;
    span_record!(cli_version, display cli_version);

    // Run the scan.
    let source_units = retry_default(
        format!("Analyze '{}' at '{}'", job.integration, job.reference),
        || async { cli.analyze(&job.scan_id, cloned_location.path()).await },
    )
    .await
    .change_context(Error::RunFossaCli)?;

    info!("Scanned '{}' at '{}'", job.integration, job.reference);
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
    mut receiver: Receiver<UploadSourceUnits>,
) -> Result<(), Error> {
    loop {
        let guard = receiver.recv().await.change_context(Error::TaskReceive)?;
        let job = guard.item().change_context(Error::TaskReceive)?;

        let meta = ProjectMetadata::new(&job.integration, &job.reference);
        info!("Uploading scan for project: '{meta}'");

        let locator = retry_default(format!("Upload results for '{meta}'"), || async {
            fossa::upload_scan(ctx.config.fossa_api(), &meta, &job.cli, &job.source_units).await
        })
        .await
        .change_context(Error::TaskHandle)?;

        debug!(scan_id = %job.scan_id, locator = %locator, "Uploaded scan");
        info!("Uploaded scan for project '{meta}' as locator: '{locator}'");

        guard.commit().change_context(Error::TaskComplete)?;
    }
}

/// Retries the provided action with the provided label, using the default retry strategy.
///
/// The default retry strategy:
/// - Retries using an exponential backoff delay, with a jitter to prevent thundering herds.
/// - The delay starts at 1 second.
/// - Retries a total of ten times.
///
/// Each time the action fails, a warning is output using the label,
/// in the form `{label}: attempt failed, will retry. error: {error}`.
///
/// If the overall process fails, the returned error has help text attached instructing users
/// to review the warnings in the logs for troubleshooting.
///
/// If the process eventually succeeds, no error is returned, but warnings are still emitted.
async fn retry_default<S, A, F, T, E>(label: S, action: A) -> Result<T, E>
where
    S: AsRef<str>,
    A: Fn() -> F,
    F: Future<Output = Result<T, E>>,
    E: Context,
{
    let wrapped_action = || async {
        match action().await {
            Ok(result) => Ok(result),
            Err(err) => {
                warn!(
                    "{}: attempt failed, will retry. error: {err:#}",
                    label.as_ref()
                );
                Err(err)
            }
        }
    };

    let strategy = ExponentialBackoff::from_millis(1000).map(jitter).take(10);
    Retry::spawn(strategy, wrapped_action).await.help(indoc! {"
        Each time this operation was attempted, it logged a warning; please review those
        warnings in the logs for more details.
    "})
}
