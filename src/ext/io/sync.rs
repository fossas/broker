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
    env, fmt,
    fs::{self, File},
    iter,
    path::{Path, PathBuf},
};

use error_stack::{Report, ResultExt};
use itertools::Itertools;
use libflate::gzip;
use once_cell::sync::OnceCell;
use serde_json::Value;
use tempfile::NamedTempFile;
use tracing::debug;

use crate::{
    ext::{
        error_stack::{DescribeContext, ErrorHelper, IntoContext},
        iter::{AlternativeIter, ChainOnceWithIter},
        result::{WrapErr, WrapOk},
    },
    AppContext,
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

    /// The value was already set.
    #[error("value already set: {0}")]
    ValueAlreadySet(String),

    /// Generic IO error.
    #[error("underlying IO error")]
    IO,
}

/// Lists the contents of a directory.
/// Returns the file names without their path components.
#[tracing::instrument]
pub fn list_contents(dir: &Path) -> Result<Vec<String>, Report<Error>> {
    std::fs::read_dir(dir)
        .context(Error::IO)?
        .map_ok(|entry| entry.file_name().to_string_lossy().to_string())
        .collect::<Result<Vec<_>, _>>()
        .context(Error::IO)
}

/// Create a new temporary file.
///
/// The file will be created in the location returned by [`std::env::temp_dir()`].
///
/// # Security
///
/// This variant is secure/reliable in the presence of a pathological temporary file cleaner.
///
/// # Resource Leaking
///
/// The temporary file will be automatically removed by the OS when the last handle to it is closed.
/// This doesn't rely on Rust destructors being run, so will (almost) never fail to clean up the temporary file.
///
/// # Errors
///
/// If the file can not be created, `Err` is returned.
///
/// [`std::env::temp_dir()`]: https://doc.rust-lang.org/std/env/fn.temp_dir.html
//
// Documentation above taken from the [`tempfile::tempfile`] docs.
//
#[tracing::instrument]
pub fn tempfile() -> Result<File, Report<Error>> {
    tempfile::tempfile().context(Error::IO)
        .help("altering the temporary directory location may resolve this issue")
        .describe("temporary directory location uses $TMPDIR on Linux and macOS; for Windows it uses the 'GetTempPath' system call")
}

/// Copies a given file into a new temporary file, returning the temporary file.
///
/// The contents of the source file will have been written to the temp file and a best effort
/// is made to sync the contents to disk before this function returns.
#[tracing::instrument]
pub fn copy_temp<P>(file: P) -> Result<NamedTempFile, Report<Error>>
where
    P: AsRef<Path> + std::fmt::Debug,
{
    let mut source = File::open(file).context(Error::IO)?;
    let mut copy = NamedTempFile::new().context(Error::IO)?;
    std::io::copy(&mut source, &mut copy).context(Error::IO)?;
    copy.as_file().sync_all().context(Error::IO)?;
    Ok(copy)
}

/// Copies the given FOSSA CLI debug bundle into a new temporary file, returning the temporary file
/// and its new relative file name (to be used in the overall debug bundle).
///
/// If the file does not end with `.json.gz`, no decompression or formatting is performed,
/// and the `rel` argument is returned unmodified.
///
/// The contents of the source file will have been written to the temp file and a best effort
/// is made to sync the contents to disk before this function returns.
///
/// The debug bundle is decompressed and prettified during this operation.
#[tracing::instrument]
pub fn copy_debug_bundle<P, Q>(file: P, rel: Q) -> Result<(NamedTempFile, PathBuf), Report<Error>>
where
    P: AsRef<Path> + std::fmt::Debug,
    Q: AsRef<Path> + std::fmt::Debug,
{
    const EXT: &str = ".json.gz";
    if !file.as_ref().to_string_lossy().ends_with(&EXT) {
        let copy = copy_temp(file)?;
        return (copy, rel.as_ref().to_path_buf()).wrap_ok();
    }

    let source = File::open(file).context(Error::IO)?;
    let mut reader = gzip::Decoder::new(source).context(Error::IO)?;
    let data: Value = serde_json::from_reader(&mut reader).context(Error::IO)?;

    let mut copy = NamedTempFile::new().context(Error::IO)?;
    serde_json::to_writer_pretty(&mut copy, &data).context(Error::IO)?;
    copy.as_file().sync_all().context(Error::IO)?;

    let renamed = rel.as_ref().to_string_lossy().replace(EXT, ".json");
    (copy, PathBuf::from(renamed)).wrap_ok()
}

/// Searches configured locations for the file with the provided name.
///
/// Locations searched:
/// - The [`data_root`] location.
/// - The [`working_dir`] location.
#[tracing::instrument]
pub fn find(ctx: &AppContext, name: &str) -> Result<PathBuf, Report<Error>> {
    iter::once_with(|| working_dir().map(|d| d.join(name)).and_then(validate_file))
        .chain_once_with(|| validate_file(ctx.data_root().join(name)))
        .alternative_fold()
        .describe("searches the working directory and '{USER_DIR}/.config/fossa/broker'")
}

/// Searches configured locations (see [`find`])
/// for one of several provided names, returning the first one that was found.
#[tracing::instrument]
pub fn find_some(ctx: &AppContext, names: &[&str]) -> Result<PathBuf, Report<Error>> {
    names.iter().map(|name| find(ctx, name)).alternative_fold()
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
        .describe_lazy(|| format!("validate file: '{}'", path.display()))
        .help("validate that you have access to the file and that it exists")?;

    if meta.is_file() {
        path.wrap_ok()
    } else {
        Error::NotRegularFile
            .wrap_err()
            .map_err(Report::from)
            .attach_printable_lazy(|| format!("validate file: '{}'", path.display()))
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
        dirs::home_dir().ok_or(Error::LocateUserHome).map_err(Report::from)
            .describe("on macOS and Linux, this uses the $HOME environment variable or the system call 'getpwuid_r'")
            .describe("on Windows, this uses the Windows API call 'SHGetKnownFolderPath'")
            .describe("this is a very rare condition, and it's not likely that Broker will be able to resolve this issue")
    })
}
