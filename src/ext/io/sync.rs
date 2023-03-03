//! Types and functions for IO actions, wrapped in Broker errors and semantics.
//!
//! # Async
//!
//! Generally, prefer the variants in the parent module, as they are async.
//!
//! Functions in this module are the backing sync variants of those functions
//! and should only be used if inside a sync context.
//!
//! # Why backing sync
//!
//! Rust standard library IO operations are synchronous,
//! so in order to make them "async" we have to run these synchronous operations
//! in Tokio's backing thread pool.
//!
//! # Duplication
//!
//! At present, this sync module and its async parent are manually written
//! and contain a fair amount of duplication. Over time I'd like to unify them
//! using a macro to autogenerate the async versions,
//! or swapping to a backing FS implementation that is async native.

use std::{
    env, fmt, fs, iter,
    path::{Path, PathBuf},
};

use error_stack::{IntoReport, Report, ResultExt};
use once_cell::sync::OnceCell;
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
}

/// The root data directory for Broker.
/// Broker uses this directory to store working state and to read configuration information.
///
/// - On Linux and macOS: `~/.config/fossa/broker/`
/// - On Windows: `%USERPROFILE%\.config\fossa\broker`
#[tracing::instrument]
pub fn data_root() -> Result<PathBuf, Report<Error>> {
    home_dir().map(|home| home.join(".config").join("fossa").join("broker"))
}

/// Searches configured locations for the file with the provided name.
///
/// Locations searched:
/// - The [`data_root`] location.
/// - The [`working_dir`] location.
#[tracing::instrument]
pub fn find<S: AsRef<str> + fmt::Debug>(name: S) -> Result<PathBuf, Report<Error>> {
    let name = PathBuf::from(name.as_ref());
    iter::once_with(|| working_dir().map(|d| d.join(&name)).and_then(validate_file))
        .chain_once_with(|| data_root().map(|d| d.join(&name)).and_then(validate_file))
        .alternative_fold()
        .describe("searches the working directory and '{USER_DIR}/.config/fossa/broker'")
}

/// Searches configured locations (see [`find`])
/// for one of several provided names, returning the first one that was found.
#[tracing::instrument]
pub fn find_some<V, S>(names: V) -> Result<PathBuf, Report<Error>>
where
    S: AsRef<str> + fmt::Debug,
    V: IntoIterator<Item = S> + fmt::Debug,
{
    names.into_iter().map(find).alternative_fold()
}

/// Reads the provided file content to a string.
#[tracing::instrument]
pub fn read_to_string<P: AsRef<Path> + fmt::Debug>(file: P) -> Result<String, Report<Error>> {
    let file = file.as_ref().to_path_buf();
    fs::read_to_string(file)
        .context(Error::ReadFileContent)
        .help("validate that you have access to the file and that it exists")
}

/// Validate that a file path exists and is a regular file.
#[tracing::instrument]
pub fn validate_file(path: PathBuf) -> Result<PathBuf, Report<Error>> {
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
///
/// This function is lazy and memoized:
/// the lookup is performed the first time on demand
/// and (assuming no error was encountered)
/// that result is saved for future invocations.
#[tracing::instrument]
pub fn working_dir() -> Result<&'static PathBuf, Report<Error>> {
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
///
/// This function is lazy and memoized:
/// the lookup is performed the first time on demand
/// and (assuming no error was encountered)
/// that result is saved for future invocations.
#[tracing::instrument]
pub fn home_dir() -> Result<&'static PathBuf, Report<Error>> {
    static LAZY: OnceCell<PathBuf> = OnceCell::new();
    LAZY.get_or_try_init(|| {
        debug!("Performing uncached lookup of home directory");
        dirs::home_dir().ok_or(Error::LocateUserHome).into_report()
            .describe("on macOS and Linux, this uses the $HOME environment variable or the system call 'getpwuid_r'")
            .describe("on Windows, this uses the Windows API call 'SHGetKnownFolderPath'")
            .describe("this is a very rare condition, and it's not likely that Broker will be able to resolve this issue")
    })
}
