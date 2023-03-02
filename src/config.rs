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
    /// This crate doesn't actually parse command line arguments, it only validates them.
    /// It hands off parsing to `clap` by exporting [`args::BaseArgs`].
    ///
    /// Given this, the error message is only concerned with _validating_ the args,
    /// since `clap` already reports parse errors itself.
    #[error("validate command line arguments")]
    ValidateArgs,

    /// Unlike with args, this crate is responsible for both parsing and validating the config file.
    /// As such, [`file`] has its own errors reflecting this two-step process.
    ///
    /// At this level, this crate just reports the overall process as "loading",
    /// and bubbles up the context from [`file`] to the user.
    #[error("load config file")]
    LoadConfigFile,
}

/// Validate the args provided by the user.
pub async fn validate_args(provided: RawBaseArgs) -> Result<BaseArgs, Error> {
    provided
        .validate()
        .await
        .change_context(Error::ValidateArgs)
}

/// Load the config for the application.
pub async fn load(args: &BaseArgs) -> Result<file::Config, Error> {
    file::Config::load(args.config_path().path())
        .await
        .change_context(Error::LoadConfigFile)
}
