//! Implementation for the `run` subcommand.

use std::time::Duration;

use error_stack::{Result, ResultExt};
use futures::try_join;
use tracing::info;

use crate::{
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
}

/// The primary entrypoint.
#[tracing::instrument(skip_all)]
pub async fn main(_config: Config, db: impl Database) -> Result<(), Error> {
    info!("Broker will run until it is terminated, but isn't doing anything special: this subcommand is still basically NYI");
    try_join!(do_pretend_work(), do_healthcheck(db)).discard_ok()
}

#[tracing::instrument]
async fn do_pretend_work() -> Result<(), Error> {
    for i in 0.. {
        tokio::time::sleep(Duration::from_secs(10)).await;
        do_pretend_work_cycle(i).await;
    }
    Ok(())
}

#[tracing::instrument]
async fn do_pretend_work_cycle(cycle: usize) {
    info!("Yep, still running{}", "!".repeat(cycle % 5));
}

/// Conduct internal diagnostics to ensure Broker is still in a good state.
#[tracing::instrument]
async fn do_healthcheck(db: impl Database) -> Result<(), Error> {
    do_healthcheck_cycle(&db).await?;

    for _ in 0.. {
        tokio::time::sleep(Duration::from_secs(60)).await;
        do_healthcheck_cycle(&db).await?;
    }

    Ok(())
}

#[tracing::instrument]
async fn do_healthcheck_cycle(db: &impl Database) -> Result<(), Error> {
    db.healthcheck()
        .await
        .change_context(Error::Healthcheck)
        .describe("Broker periodically runs internal healthchecks to validate that it is still in a good state")
        .help("this health check failing may have been related to a temporary condition, restarting Broker may resolve the issue")
}
