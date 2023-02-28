//! Types and functions for validations requiring IO.

use std::{env, fs, iter, path::PathBuf};

use error_stack::{IntoReport, Report, ResultExt};
use tokio::task;

use crate::ext::{
    error_stack::{DescribeContext, ErrorHelper},
    iter::{AlternativeIter, ChainOnceWithIter},
};

/// Errors that are possibly surfaced during IO actions.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The provided path-like item failed validation.
    /// Often these errors are related to permissions or the path not existing.
    #[error("validate path")]
    ValidatePath,

    /// The provided file path does not reference a file on disk.
    #[error("path is not a regular file")]
    NotRegularFile,

    /// Failed to locate the HOME directory for the current user.
    #[error("failed to locate home directory for the current user")]
    LocateUserHome,

    /// Failed to locate the current working directory.
    #[error("failed to locate working directory")]
    LocateWorkingDirectory,

    /// Failed to join the background thread that performed the backing IO operation.
    #[error("join background thread")]
    JoinWorker,
}

/// Searches configured locations (see [`find`])
/// for one of several provided names, returning the first one that was found.
pub async fn find_some(names: &'static [&str]) -> Result<PathBuf, Report<Error>> {
    run_background(move || names.iter().map(|name| find_sync(name)).alternative_fold()).await
}

/// Searches configured locations for the file with the provided name.
///
/// Locations searched:
/// - The current working directory
/// - On Linux and macOS: `~/.fossa/broker/`
/// - On Windows: `%USERPROFILE%\.fossa\broker`
pub async fn find(name: &'static str) -> Result<PathBuf, Report<Error>> {
    run_background(move || find_sync(name)).await
}

/// Internal sync driver for [`find`].
fn find_sync(name: &str) -> Result<PathBuf, Report<Error>> {
    iter::once_with(|| check_cwd(name).and_then(validate_file))
        .chain_once_with(|| check_home(name).and_then(validate_file))
        .alternative_fold()
        .describe("searches the working directory and '{USER_DIR}/.fossa/broker'")
}

/// Validate that a file path exists and is a regular file.
fn validate_file(path: PathBuf) -> Result<PathBuf, Report<Error>> {
    let meta = fs::metadata(&path)
        .into_report()
        .change_context(Error::ValidatePath)
        .describe_lazy(|| format!("validate file: {path:?}"))
        .help("validate that you have access to the file and that it exists")?;

    if meta.is_file() {
        Ok(path)
    } else {
        Err(Error::NotRegularFile)
            .into_report()
            .attach_printable_lazy(|| format!("validate file: {path:?}"))
    }
}

/// Validate that the given file name exists in the current working directory.
fn check_cwd(name: &str) -> Result<PathBuf, Report<Error>> {
    let cwd = env::current_dir()
        .into_report()
        .change_context(Error::LocateWorkingDirectory)
        .describe("on macOS and Linux, this uses the system call 'getcwd'")
        .describe("on Windows, this uses the Windows API call 'GetCurrentDirectoryW'")
        .describe("this kind of error is typically caused by the current user not having access to the working directory")?;
    Ok(cwd.join(name))
}

/// Validate that the given file name exists in `$HOME/.fossa/broker/`.
fn check_home(name: &str) -> Result<PathBuf, Report<Error>> {
    let home = dirs::home_dir().ok_or(Error::LocateUserHome).into_report()
        .describe("on macOS and Linux, this uses the $HOME environment variable or the system call 'getpwuid_r'")
        .describe("on Windows, this uses the Windows API call 'SHGetKnownFolderPath'")
        .describe("this is a very rare condition, and it's not likely that Broker will be able to resolve this issue")?;
    Ok(home.join(".fossa").join("broker").join(name))
}

/// Run the provided blocking closure in the background.
async fn run_background<T, F>(work: F) -> Result<T, Report<Error>>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, Report<Error>> + Send + 'static,
{
    task::spawn_blocking(work)
        .await
        .into_report()
        .change_context(Error::JoinWorker)
        .describe("Broker runs some IO actions in a background process, and that thread was unable to be synchronized with the main Broker process.")
        .help("This is unlikely to be resolvable by an end user, although it may be environmental; try restarting Broker.")?
}
