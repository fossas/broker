//! Implementation for the `init` subcommand.
use error_stack::Result;

/// Errors encountered during init.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A config file already exists
    #[error("config file exists")]
    ConfigFileExists,
}

/// generate the config and db files in the default location
#[tracing::instrument(skip_all)]
pub async fn main() -> Result<(), Error> {
    Ok(())
}
