//! Types and functions for validations requiring IO.
//!
//! This module should only export async operations.
//!
//! # Async implementation
//!
//! These functions generally consist of an async wrapper around
//! synchronously executed blocking functions (these are run in a background worker thread).
//!
//! One pattern you'll also notice is that these async wrappers usually accept flexible reference inputs
//! (e.g., instead of `String` or `&str`, they accept `AsRef<str>`) which they then immediately reference
//! and convert to the owned type (e.g. via `input.as_ref().to_string()`).
//!
//! This is because in order for the data to be `Send` (meaning, it can be sent across threads)
//! it also must have the lifetime bound `'static`, which means "the value exists as long as the closure".
//! The easiest way to ensure this is the case is by cloning the value into an owned type locally;
//! this ensures that the reference is valid as long as the closure is running.
//!
//! While clones are "expensive", since these background thread operations involve 1) potentially spawning a thread[^note],
//! and 2) disk IO, the clones are irrelevant in the grand scheme.
//!
//! [^note]: Tokio has a lot of optimizations in place to maximize background threadpool reuse,
//! but still any call to `spawn_blocking` _may_ result in a spawned thread.

use std::{
    env, fmt, fs, iter,
    path::{Path, PathBuf},
};

use error_stack::{IntoReport, Report, ResultExt};
use once_cell::sync::OnceCell;
use tokio::task;
use tracing::debug;

use crate::ext::{
    error_stack::{DescribeContext, ErrorHelper, IntoContext},
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
    #[error("locate home directory for the current user")]
    LocateUserHome,

    /// Failed to locate the current working directory.
    #[error("locate working directory")]
    LocateWorkingDirectory,

    /// Failed to read the contents of the file at the provided path.
    #[error("read contents of file")]
    ReadFileContent,

    /// Failed to join the background worker that performed the backing IO operation.
    #[error("join background worker")]
    JoinWorker,
}

/// Searches configured locations (see [`find`])
/// for one of several provided names, returning the first one that was found.
#[tracing::instrument]
pub async fn find_some<V, S>(names: V) -> Result<PathBuf, Report<Error>>
where
    S: AsRef<str> + fmt::Debug,
    V: IntoIterator<Item = S> + fmt::Debug,
{
    let names = names
        .into_iter()
        .map(|name| name.as_ref().to_string())
        .collect::<Vec<String>>();
    run_background(move || names.iter().map(find_sync).alternative_fold()).await
}

/// Searches configured locations for the file with the provided name.
///
/// Locations searched:
/// - The current working directory
/// - On Linux and macOS: `~/.config/fossa/broker/`
/// - On Windows: `%USERPROFILE%\.config\fossa\broker`
#[tracing::instrument]
pub async fn find<S: AsRef<str> + fmt::Debug>(name: S) -> Result<PathBuf, Report<Error>> {
    let name = name.as_ref().to_string();
    run_background(move || find_sync(name)).await
}

/// Reads the provided file content to a string.
#[tracing::instrument]
pub async fn read_to_string<P: AsRef<Path> + fmt::Debug>(file: P) -> Result<String, Report<Error>> {
    let file = file.as_ref().to_path_buf();
    run_background(move || {
        fs::read_to_string(file)
            .context(Error::ReadFileContent)
            .help("validate that you have access to the file and that it exists")
    })
    .await
}

/// The root data directory for Broker.
/// Broker uses this directory to store working state and to read configuration information.
#[tracing::instrument]
pub async fn data_root() -> Result<PathBuf, Report<Error>> {
    run_background(data_root_sync).await
}

#[tracing::instrument]
fn data_root_sync() -> Result<PathBuf, Report<Error>> {
    home_dir().map(|home| home.join(".config").join("fossa").join("broker"))
}

/// Internal sync driver for [`find`].
#[tracing::instrument]
fn find_sync<S: AsRef<str> + fmt::Debug>(name: S) -> Result<PathBuf, Report<Error>> {
    let name = PathBuf::from(name.as_ref());
    iter::once_with(|| working_dir().map(|d| d.join(&name)).and_then(validate_file))
        .chain_once_with(|| {
            data_root_sync()
                .map(|d| d.join(&name))
                .and_then(validate_file)
        })
        .alternative_fold()
        .describe("searches the working directory and '{USER_DIR}/.config/fossa/broker'")
}

/// Validate that a file path exists and is a regular file.
#[tracing::instrument]
fn validate_file(path: PathBuf) -> Result<PathBuf, Report<Error>> {
    let meta = fs::metadata(&path)
        .context(Error::ValidatePath)
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

/// Look up the current working directory.
#[tracing::instrument]
fn working_dir() -> Result<&'static PathBuf, Report<Error>> {
    static LAZY: OnceCell<PathBuf> = OnceCell::new();
    LAZY.get_or_try_init(|| {
        debug!("Performing uncached lookup of working directory");
        env::current_dir()
            .context(Error::LocateWorkingDirectory)
            .describe("on macOS and Linux, this uses the system call 'getcwd'")
            .describe("on Windows, this uses the Windows API call 'GetCurrentDirectoryW'")
            .describe("this kind of error is typically caused by the current user not having access to the working directory")
    })
}

/// Look up the user's home directory.
#[tracing::instrument]
fn home_dir() -> Result<&'static PathBuf, Report<Error>> {
    static LAZY: OnceCell<PathBuf> = OnceCell::new();
    LAZY.get_or_try_init(|| {
        debug!("Performing uncached lookup of home directory");
        dirs::home_dir().ok_or(Error::LocateUserHome).into_report()
            .describe("on macOS and Linux, this uses the $HOME environment variable or the system call 'getpwuid_r'")
            .describe("on Windows, this uses the Windows API call 'SHGetKnownFolderPath'")
            .describe("this is a very rare condition, and it's not likely that Broker will be able to resolve this issue")
    })
}

/// Run the provided blocking closure in the background.
async fn run_background<T, F>(work: F) -> Result<T, Report<Error>>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, Report<Error>> + Send + 'static,
{
    task::spawn_blocking(work)
        .await
        .context(Error::JoinWorker)
        .describe("Broker runs some IO actions in a background process, and that thread was unable to be synchronized with the main Broker process.")
        .help("This is unlikely to be resolvable by an end user, although it may be environmental; try restarting Broker.")?
}
