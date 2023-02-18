//! Interactions and data types for the Broker config file live here.

use error_stack::{Result, ResultExt};

mod args;
mod io;

/// Errors that are possibly surfaced during validation of config values.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Validate CLI arguments
    #[error("validate command line arguments")]
    ValidateArgs,
}

pub use args::{BaseArgs, RawBaseArgs};

/// Validate the args provided by the user.
pub fn validate_args(provided: RawBaseArgs) -> Result<BaseArgs, Error> {
    provided.try_into().change_context(Error::ValidateArgs)
}
