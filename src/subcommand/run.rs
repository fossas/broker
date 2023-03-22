//! Implementation for the `run` subcommand.

use std::sync::Arc;
use std::time::Duration;

use error_stack::{Result, ResultExt};
use futures::TryStreamExt;
use futures::{future::try_join_all, try_join, StreamExt};
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

use crate::api::remote::Reference;
use crate::ext::tracing::span_record;
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

    /// If we fail to send tasks to the async task queue, this error is raised.
    #[error("enqueue task for processing")]
    TaskEnqueue,

    /// If we fail to receive tasks to the async task queue, this error is raised.
    #[error("receive task for processing")]
    TaskReceive,

    /// If we fail to mark a task complete, this error is raised.
    #[error("mark task complete")]
    TaskComplete,
}

/// The primary entrypoint.
#[tracing::instrument(skip_all)]
pub async fn main(ctx: &AppContext, config: Config, db: impl Database) -> Result<(), Error> {
    let (scan_tx, scan_rx) = queue::open(ctx, Queue::Scan)
        .await
        .change_context(Error::SetupPipeline)?;

    // This function runs a bunch of async background tasks.
    // Create them all, then just `try_join!` on all of them at the end.
    let healthcheck_worker = do_healthcheck(&db);
    let integration_worker = do_poll_integrations(&config, &db, scan_tx);
    let scan_git_reference_worker = do_scan_git_references(&db, scan_rx);

    // `try_join!` keeps all of the workers running until one of them fails,
    // at which point the failure is returned and remaining tasks are dropped.
    // It also returns all of their results as a tuple, which we don't care about,
    // so we discard that value.
    try_join!(
        healthcheck_worker,
        integration_worker,
        scan_git_reference_worker
    )
    .discard_ok()
}

/// Conduct internal diagnostics to ensure Broker is still in a good state.
#[tracing::instrument]
async fn do_healthcheck(db: &impl Database) -> Result<(), Error> {
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

/// Loops forever, polling configured integrations on their `poll_interval`.
#[tracing::instrument(skip_all)]
async fn do_poll_integrations(
    config: &Config,
    db: &impl Database,
    sender: Sender<ScanGitVCSReference>,
) -> Result<(), Error> {
    // The sender is not thread safe; ensure it's only run one at a time.
    let sender = Arc::new(Mutex::new(sender));

    // Each integration is configured with a poll interval.
    // Rather than have one big poll loop that has to track polling times for each integration,
    // just create a task per integration; they're cheap.
    let integration_workers = config
        .integrations()
        .iter()
        .map(|integration| async { do_poll_integration(db, integration, sender.clone()).await });

    // Run all the workers in parallel. If one errors, return that error and drop the rest.
    try_join_all(integration_workers).await.discard_ok()
}

#[tracing::instrument(skip_all)]
async fn do_poll_integration(
    db: &impl Database,
    integration: &Integration,
    sender: Arc<Mutex<Sender<ScanGitVCSReference>>>,
) -> Result<(), Error> {
    // We use this in a few places and may send it across threads, so just clone it locally.
    let remote = integration.remote().to_owned();
    let poll_interval = integration.poll_interval().as_duration();

    // [`Retry`] needs a function that runs without any arguments to perform the retry, so turn the method into a closure.
    let get_references = || async {
        match integration.references().await {
            Ok(success) => Ok(success),
            Err(err) => {
                warn!("attempt to poll integration at {remote} failed: {err:#}");
                Err(err)
            }
        }
    };

    loop {
        // Given that this operation is not latency sensitive, and temporary network issues can interfere,
        // retry several times before permanently failing since a permanent failure means Broker shuts down
        // entirely.
        let strategy = ExponentialBackoff::from_millis(1000).map(jitter).take(1);
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
            info!("No changes to {integration} since last poll interval");
        }
        for reference in references {
            let job = ScanGitVCSReference::new(integration, &reference);
            let mut sender = sender.lock().await;
            sender.send(&job).await.change_context(Error::TaskEnqueue)?;

            info!("Enqueued task to scan {integration} at {reference}");
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

        info!("Next poll interval for {integration} in {poll_interval:?}");
        tokio::time::sleep(poll_interval).await;
    }
}

#[tracing::instrument(skip_all)]
async fn do_scan_git_references(
    db: &impl Database,
    mut receiver: Receiver<ScanGitVCSReference>,
) -> Result<(), Error> {
    loop {
        let guard = receiver.recv().await.change_context(Error::TaskReceive)?;
        let job = guard.item().change_context(Error::TaskReceive)?;

        do_scan_git_reference(db, &job).await?;
        guard.commit().change_context(Error::TaskComplete)?;
    }
}

#[tracing::instrument(skip(_db), fields(scan_id))]
async fn do_scan_git_reference(
    _db: &impl Database,
    job: &ScanGitVCSReference,
) -> Result<(), Error> {
    span_record!(scan_id, &job.scan_id);
    info!(
        "Pretending to scan {} at {}",
        job.integration, job.reference
    );
    Ok(())
}
