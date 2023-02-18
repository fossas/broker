//! Interactions and data types for the Broker config file live here.

use error_stack::{Result, ResultExt};

// Keep `config` opaque externally, only export what is required for callers.
// To re-export a symbol, just `pub use`.
mod args;
mod file;
mod io;

pub use args::{BaseArgs, RawBaseArgs};
pub use file::Config;

/// Errors that are possibly surfaced during validation of config values.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Validate CLI arguments
    #[error("validate command line arguments")]
    ValidateArgs,

    /// Validate config file
    #[error("validate config file")]
    ValidateConfigFile,

    /// Parse the config file on disk.
    /// This is distinct from the "validation" step- where validation is about the _content_ being valid,
    /// parsing is about the _shape_ being valid.
    #[error("parse config file")]
    ParseConfigFile,
}

/// Validate the args provided by the user.
pub fn validate_args(provided: RawBaseArgs) -> Result<BaseArgs, Error> {
    provided.try_into().change_context(Error::ValidateArgs)
}

/// Load the config for the application.
pub fn load(args: &BaseArgs) -> Result<file::Config, Error> {
    file::RawConfig::parse(args.config_path().path())
        // TODO: Point users to a reference (in `docs/`) for the config file shape.
        .change_context(Error::ParseConfigFile)?
        .try_into()
        // TODO: Point users to help content (in `docs/`) for writing a valid config file.
        //       Keep in mind this TODO is different than the above:
        //       this error is generated during the validation step, which has different context
        //       and deserves different help text/documentation.
        .change_context(Error::ValidateConfigFile)
}
