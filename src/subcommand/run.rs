//! Implementation for the `run` subcommand.

use std::time::Duration;

use error_stack::Report;
use tracing::info;

use crate::config::Config;

/// Errors encountered during runtime.
#[derive(Debug, thiserror::Error)]
pub enum Error {}

/// The primary entrypoint.
#[tracing::instrument(skip_all)]
pub async fn main(_config: Config) -> Result<(), Report<Error>> {
    info!("Broker will run until it is terminated, but isn't doing anything special: this subcommand is still basically NYI");
    for i in 0.. {
        do_pretend_work_cycle(i).await;
    }
    Ok(())
}

#[tracing::instrument]
async fn do_pretend_work_cycle(cycle: usize) {
    tokio::time::sleep(Duration::from_secs(10)).await;
    info!("Yep, still running{}", "!".repeat(cycle % 5));
}
