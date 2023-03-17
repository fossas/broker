//! Types and functions for validations requiring IO.
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
    fmt,
    path::{Path, PathBuf},
};

use error_stack::{Context, IntoReport, Report, ResultExt};
use tokio::task;

use crate::ext::error_stack::{DescribeContext, ErrorHelper, IntoContext};

pub mod sync;
pub use sync::DATA_ROOT_VAR;

/// Errors that are possibly surfaced during IO actions.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An error occurred in the underlying IO layer.
    #[error("IO layer error")]
    IO,

    /// Failed to join the background worker that performed the backing IO operation.
    #[error("join background worker")]
    JoinWorker,
}

/// The root data directory for Broker.
/// Broker uses this directory to store working state and to read configuration information.
///
/// - On Linux and macOS: `~/.config/fossa/broker/`
/// - On Windows: `%USERPROFILE%\.config\fossa\broker`
///
/// Users may also customize this root via the [`DATA_ROOT_VAR`] environment variable.
#[tracing::instrument]
pub async fn data_root() -> Result<PathBuf, Report<Error>> {
    run_background(sync::data_root).await
}

/// Searches configured locations for the file with the provided name.
///
/// Locations searched:
/// - The [`data_root`] location.
/// - The [`working_dir`] location.
#[tracing::instrument]
pub async fn find<S: AsRef<str> + fmt::Debug>(name: S) -> Result<PathBuf, Report<Error>> {
    let name = name.as_ref().to_string();
    run_background(move || sync::find(name)).await
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
        .collect::<Vec<_>>();
    run_background(move || sync::find_some(names)).await
}

/// Reads the provided file content to a string.
#[tracing::instrument]
pub async fn read_to_string<P: AsRef<Path> + fmt::Debug>(file: P) -> Result<String, Report<Error>> {
    let file = file.as_ref().to_path_buf();
    run_background(move || sync::read_to_string(file)).await
}

/// Validate that a file path exists and is a regular file.
#[tracing::instrument]
pub async fn validate_file(path: PathBuf) -> Result<PathBuf, Report<Error>> {
    run_background(move || sync::validate_file(path)).await
}

/// Look up the current working directory.
///
/// This function is lazy and memoized:
/// the lookup is performed the first time on demand
/// and (assuming no error was encountered)
/// that result is saved for future invocations.
#[tracing::instrument]
pub async fn working_dir() -> Result<&'static PathBuf, Report<Error>> {
    run_background(sync::working_dir).await
}

/// Look up the user's home directory.
///
/// This function is lazy and memoized:
/// the lookup is performed the first time on demand
/// and (assuming no error was encountered)
/// that result is saved for future invocations.
#[tracing::instrument]
pub async fn home_dir() -> Result<&'static PathBuf, Report<Error>> {
    run_background(sync::home_dir).await
}

/// Run the provided blocking closure in the background.
#[tracing::instrument(skip_all)]
async fn run_background<T, E, F>(work: F) -> Result<T, Report<Error>>
where
    T: Send + 'static,
    E: Context,
    F: FnOnce() -> Result<T, Report<E>> + Send + 'static,
{
    task::spawn_blocking(work)
        .await
        .context(Error::JoinWorker)
        .describe("Broker runs some IO actions in a background process, and that thread was unable to be synchronized with the main Broker process.")
        .help("This is unlikely to be resolvable by an end user, although it may be environmental; try restarting Broker.")?
        .change_context(Error::IO)
}

/// Run the provided blocking closure in the background,
/// wrapping any error returned in this module's `Error::IO` context.
#[tracing::instrument(skip_all)]
pub async fn spawn_blocking<T, E, F>(work: F) -> Result<T, Report<Error>>
where
    T: Send + 'static,
    E: std::error::Error + Sync + Send + 'static,
    Report<E>: From<E>,
    F: FnOnce() -> Result<T, E> + Send + 'static,
{
    spawn_blocking_stacked(|| work().into_report()).await
}

/// Run the provided blocking closure in the background,
/// wrapping any error returned in this module's `Error::IO` context.
#[tracing::instrument(skip_all)]
pub async fn spawn_blocking_stacked<T, E, F>(work: F) -> Result<T, Report<Error>>
where
    T: Send + 'static,
    E: Context,
    F: FnOnce() -> Result<T, Report<E>> + Send + 'static,
{
    run_background(|| work()).await
}
