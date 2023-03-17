//! Implementation for the `run` subcommand.

use std::{collections::HashMap, time::Duration};

use backon::ExponentialBuilder;
use backon::Retryable;
use error_stack::{Report, Result, ResultExt};
use futures::{future::try_join_all, try_join, StreamExt};
use tap::TapFallible;
use tokio_retry::strategy::jitter;
use tokio_retry::strategy::ExponentialBackoff;
use tokio_retry::Retry;
use tracing::debug;
use tracing::warn;

use crate::{
    api::remote::{Integration, RemoteProvider},
    config::Config,
    db::Database,
    ext::{
        error_stack::{merge_error_stacks, merge_errors, DescribeContext, ErrorHelper},
        io,
        result::{DiscardResult, WrapErr},
    },
};

/// Errors encountered during runtime.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Application health check failed.
    #[error("health check failed")]
    Healthcheck,

    /// The application periodically polls for new references in configured integrations.
    /// If one of those polls fails, this error is returned.
    #[error("poll integration")]
    PollIntegration,
}

/// The primary entrypoint.
#[tracing::instrument(skip_all)]
pub async fn main(config: Config, db: impl Database) -> Result<(), Error> {
    // This function runs a bunch of async background tasks.
    // Create them all, then just `try_join!` on all of them at the end.
    let integration_worker = do_poll_integrations(&config, &db);
    let healthcheck_worker = do_healthcheck(&db);

    // `try_join!` keeps all of the workers running until one of them fails,
    // at which point the failure is returned and remaining tasks are dropped.
    // It also returns all of their results as a tuple, which we don't care about,
    // so we discard that value.
    try_join!(integration_worker, healthcheck_worker).discard_ok()
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

/// Loops forever, polling configured integrations on their `poll_interval`.
async fn do_poll_integrations(config: &Config, db: &impl Database) -> Result<(), Error> {
    // Each integration is configured with a poll interval.
    // Rather than have one big poll loop that has to track polling times for each integration,
    // just create a task per integration; they're cheap.
    let integration_workers = config
        .integrations()
        .iter()
        .map(|integration| async { do_poll_integration(config, db, integration).await });

    // Run all the workers in parallel. If one errors, return that error and drop the rest.
    try_join_all(integration_workers).await.discard_ok()
}

async fn do_poll_integration(
    config: &Config,
    db: &impl Database,
    integration: &Integration,
) -> Result<(), Error> {
    let get_references = || async {
        match integration.references().await {
            Ok(success) => Ok(success),
            Err(err) => {
                warn!("attempt to get references failed: {err:#}");
                Err(err)
            }
        }
    };

    loop {
        // It's possible for temporary issues to prevent polling.
        // Rather than fail immediately on any polling error, keep going until we get too many failures in a row.
        let strategy = ExponentialBackoff::from_millis(1000).map(jitter).take(5);
        let references = Retry::spawn(strategy, get_references)
            .await
            .change_context(Error::PollIntegration)
            .help("help_text")?;

        // Then wait for the next poll time.
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
        tokio::time::sleep(integration.poll_interval().as_duration()).await;
    }
}
