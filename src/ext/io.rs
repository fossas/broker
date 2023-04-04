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

use error_stack::{report, IntoReport, Report, ResultExt};
use tokio::task;

use crate::{
    ext::{
        error_stack::{DescribeContext, ErrorHelper, IntoContext},
        result::WrapErr,
    },
    AppContext,
};

pub mod sync;

/// Errors that are possibly surfaced during IO actions.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An error occurred in the underlying IO layer.
    #[error("IO layer error")]
    IO,

    /// The destination directory for a file move operation could not be created.
    #[error("create parent directory for destination {}", .0.display())]
    CreateParentDir(PathBuf),

    /// Failed to join the background worker that performed the backing IO operation.
    #[error("join background worker")]
    JoinWorker,
}

/// Lists the contents of a directory.
/// Returns the file names without their path components.
#[tracing::instrument]
pub async fn list_contents(dir: &Path) -> Result<Vec<String>, Report<Error>> {
    let mut contents = tokio::fs::read_dir(dir).await.context(Error::IO)?;
    let mut entries = Vec::new();
    while let Some(entry) = contents.next_entry().await.context(Error::IO)? {
        entries.push(entry.file_name().to_string_lossy().to_string());
    }
    Ok(entries)
}

/// Moves a file from one location to another.
/// If the destination parent directory doesn't exist, it is created automatically.
#[tracing::instrument]
pub async fn rename(src: &Path, dst: &Path) -> Result<(), Report<Error>> {
    let Some(parent) = dst.parent() else { return report!(Error::CreateParentDir(dst.to_path_buf())).wrap_err() };
    tokio::fs::create_dir_all(parent)
        .await
        .context_lazy(|| Error::CreateParentDir(dst.to_path_buf()))?;
    tokio::fs::rename(src, dst).await.context(Error::IO)
}

/// Searches configured locations for the file with the provided name.
///
/// Locations searched:
/// - The [`data_root`] location.
/// - The [`working_dir`] location.
#[tracing::instrument]
pub async fn find(ctx: &AppContext, name: &str) -> Result<PathBuf, Report<Error>> {
    let ctx = ctx.to_owned();
    let name = name.to_string();
    run_background(move || sync::find(&ctx, &name)).await
}

/// Searches configured locations (see [`find`])
/// for one of several provided names, returning the first one that was found.
#[tracing::instrument]
pub async fn find_some(ctx: &AppContext, names: &[&str]) -> Result<PathBuf, Report<Error>> {
    let ctx = ctx.to_owned();
    let names = names
        .iter()
        .map(|name| name.to_string())
        .collect::<Vec<_>>();

    run_background(move || {
        let name_refs = names.iter().map(String::as_str).collect::<Vec<_>>();
        sync::find_some(&ctx, &name_refs)
    })
    .await
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
    E: error_stack::Context,
    F: FnOnce() -> Result<T, Report<E>> + Send + 'static,
{
    task::spawn_blocking(work)
        .await
        .context(Error::JoinWorker)
        .describe("Broker runs some IO actions in a background process, and that thread was unable to be synchronized with the main Broker process.")
        .help("This is unlikely to be resolvable by an end user, although it may be environmental; try restarting Broker.")?
        .change_context(Error::IO)
}

/// Run the provided blocking closure in the background.
///
/// The error returned by the function is wrapped inside a report
/// and set to this module's `Error::IO` context.
#[tracing::instrument(skip_all)]
pub async fn spawn_blocking_wrap<T, E, F>(work: F) -> Result<T, Report<Error>>
where
    T: Send + 'static,
    E: std::error::Error + Sync + Send + 'static,
    Report<E>: From<E>,
    F: FnOnce() -> Result<T, E> + Send + 'static,
{
    spawn_blocking(|| work().into_report()).await
}

/// Run the provided blocking closure in the background.
#[tracing::instrument(skip_all)]
pub async fn spawn_blocking<T, E, F>(work: F) -> Result<T, Report<Error>>
where
    T: Send + 'static,
    E: error_stack::Context,
    F: FnOnce() -> Result<T, Report<E>> + Send + 'static,
{
    run_background(work).await
}
