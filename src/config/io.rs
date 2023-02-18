use std::{env, fs, iter, path::PathBuf};

use error_stack::{IntoReport, Report, ResultExt};

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
}

/// Validate that a file path exists and is a regular file.
pub fn validate_file(path: PathBuf) -> Result<PathBuf, Report<Error>> {
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

/// Searches configured locations for the file with the provided name.
///
/// Locations searched:
/// - The current working directory
/// - On Linux and macOS: `~/.fossa/broker/`
/// - On Windows: `%USERPROFILE%\.fossa\broker`
pub fn find(name: &str) -> Result<PathBuf, Report<Error>> {
    iter::once_with(|| check_cwd(name).and_then(validate_file))
        .chain_once_with(|| check_home(name).and_then(validate_file))
        .alternative_fold()
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
