//! Interactions and data types for the Broker config file live here.

use std::{env, fs, iter, path::PathBuf};

use clap::Parser;
use derive_more::AsRef;
use error_stack::{IntoReport, Report, ResultExt};
use getset::{CopyGetters, Getters};
use url::Url;

use crate::ext::{
    error_stack::{DescribeContext, ErrorHelper},
    iter::{AlternativeIter, ChainOnceWithIter},
};

/// Base arguments, used in most Broker subcommands.
/// The "Raw" prefix indicates that this is the initial parsed value before any validation.
#[derive(Debug, Parser)]
#[command(version, about)]
pub struct RawBaseArgs {
    /// URL of FOSSA instance with which Broker should communicate.
    #[arg(short = 'e', long, default_value = "https://app.fossa.com")]
    endpoint: String,

    /// The API key to use when communicating with FOSSA.
    #[arg(short = 'k', long = "fossa-api-key", env = "FOSSA_API_KEY")]
    api_key: String,

    /// The path to the Broker config file.
    ///
    /// If unset, Broker searches (in order):
    /// - The current working directory
    /// - On Linux and macOS: `~/.fossa/broker/`
    /// - On Windows: `%USERPROFILE%\.fossa\broker`
    #[arg(short = 'c', long)]
    config_file_path: Option<String>,

    /// The path to the Broker database file.
    ///
    /// If unset, Broker searches (in order):
    /// - The current working directory
    /// - On Linux and macOS: `~/.fossa/broker/`
    /// - On Windows: `%USERPROFILE%\.fossa\broker`
    #[arg(short = 'd', long)]
    database_file_path: Option<String>,
}

/// Base arguments, used in most Broker subcommands.
#[derive(Debug, Clone, PartialEq, Eq, Getters)]
#[getset(get = "pub")]
pub struct BaseArgs {
    /// URL of FOSSA instance with which Broker should communicate.
    endpoint: FossaEndpoint,

    /// The API key to use when communicating with FOSSA.
    api_key: FossaApiKey,

    /// The path to the config file on disk.
    config_path: ConfigFilePath,

    /// The path to the database file on disk.
    database_path: DatabaseFilePath,
}

/// Errors that are possibly surfaced during validation of config values.
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    /// The FOSSA endpoint provided is not valid.
    #[error("validate FOSSA endpoint")]
    ValidateFossaEndpoint,

    /// The FOSSA API key provided is not valid.
    #[error("validate FOSSA api key")]
    ValidateFossaApiKey,

    /// The config file location provided is not valid, or was not able to be located.
    #[error("validate config file location")]
    ValidateConfigFileLocation,

    /// The DB file location provided is not valid, or was not able to be located.
    #[error("validate database file location")]
    ValidateDbFileLocation,

    /// The value provided is empty but should not be empty.
    #[error("value is empty, but a non-empty value is required")]
    ValueEmpty,

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

    /// Failed to locate the file in any search location.
    #[error("failed to locate file in any known location")]
    LocateFile,
}

impl TryFrom<RawBaseArgs> for BaseArgs {
    type Error = Report<ValidationError>;

    fn try_from(raw: RawBaseArgs) -> Result<Self, Self::Error> {
        // TODO:
        // `error_stack` supports stacking multiple errors together so
        // they can all be reported at the same time.
        // We use this elsewhere, for example in `alternative_fold`.
        //
        // It looks like this:
        //
        // ```not_rust
        // Error: validate arguments
        // ├╴at src/main.rs:26:49
        // │
        // ╰┬▶ validate FOSSA endpoint
        //  │  ├╴at src/config.rs:132:14
        //  │  │
        //  │  ╰─▶ relative URL without a base
        //  │      ├╴at src/config.rs:129:14
        //  │      ╰╴context: provided input: 'foo'
        //  │
        //  ╰▶ validate FOSSA api key
        //     ├╴at src/config.rs:149:18
        //     │
        //     ╰─▶ value is empty, but a non-empty value is required
        //         ├╴at src/config.rs:146:17
        //         ╰╴context: provided input: ''
        // ```
        //
        // It can trivially be created by folding `Vec<Result<T, Report<E>>>`
        // into `Result<Vec<T>, Report<E>>` using `Report::extend_one`.
        //
        // Unfortunately, since our `Result<T, E>` types have heterogenous `T`'s,
        // so we can't store them in a `Vec` or `Iterator`.
        // To explain, `Result<T1, E>` and `Result<T2, E>` can be reduced down to `T1, T2`.
        // It's not possible to store `Vec<T1, T2>` or `Iterator<Item = T1, T2>`,
        // because both of these containers require homogenous data types.
        //
        // I think the best way around this is likely using https://docs.rs/tuple_list/latest/tuple_list/
        // because a "heterogenous vec" is ~a tuple, we just need some extra syntax to be able to
        // recursively work with tuples. I think this way would probably result in the least boilerplate
        // with purely compile time validation.
        //
        // Alternately, it may be more boilerplate and will probably have to resort to runtime validation,
        // but we could store an enum of valid types or use `Box<Any>` into a `Vec`,
        // and perform the error fold that way.
        //
        // For now I'm tabling this in favor of simply reporting the first error we come across,
        // but it's much better UX long term if we can report all the errors at once.

        let endpoint = FossaEndpoint::try_from(raw.endpoint)?;
        let api_key = FossaApiKey::try_from(raw.api_key)?;

        let discovering_config = raw.config_file_path.is_none();
        let config_path = raw
            .config_file_path
            .map(ConfigFilePath::try_from)
            .unwrap_or_else(ConfigFilePath::discover)
            .change_context(ValidationError::ValidateConfigFileLocation)
            .describe_if(
                discovering_config, 
                "searches the working directory and '{USER_DIR}/.fossa/broker' for 'config.yml' or 'config.json'"
            )
            .help_if(
                discovering_config, 
                "consider providing an explicit argument instead"
            )?;

        let discovering_db = raw.database_file_path.is_none();
        let database_path = raw
            .database_file_path
            .map(DatabaseFilePath::try_from)
            .unwrap_or_else(DatabaseFilePath::discover)
            .change_context(ValidationError::ValidateDbFileLocation)
            .describe_if(
                discovering_db,
                "searches the working directory and '{USER_DIR}/.fossa/broker' for 'db.sqlite'",
            )
            .help_if(
                discovering_db,
                "consider providing an explicit argument instead",
            )?;

        Ok(Self {
            api_key,
            endpoint,
            config_path,
            database_path,
        })
    }
}

/// The URL to the FOSSA endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FossaEndpoint(Url);

impl TryFrom<String> for FossaEndpoint {
    type Error = Report<ValidationError>;

    fn try_from(input: String) -> Result<Self, Self::Error> {
        Url::parse(&input)
            .into_report()
            .describe_lazy(|| format!("provided input: '{input}'"))
            .help("the url provided must be absolute and must contain the protocol, for example 'https://app.fossa.com'")
            .change_context(ValidationError::ValidateFossaEndpoint)
            .map(FossaEndpoint)
    }
}

/// The FOSSA API key.
#[derive(Debug, Clone, PartialEq, Eq, AsRef)]
pub struct FossaApiKey(String);

impl TryFrom<String> for FossaApiKey {
    type Error = Report<ValidationError>;

    fn try_from(input: String) -> Result<Self, Self::Error> {
        if input.is_empty() {
            Err(Report::new(ValidationError::ValueEmpty))
                .describe_lazy(|| format!("provided input: '{input}'"))
                .help("use an API key from FOSSA here: https://app.fossa.com/account/settings/integrations/api_tokens")
                .change_context(ValidationError::ValidateFossaApiKey)
        } else {
            Ok(FossaApiKey(input))
        }
    }
}

/// The path to the config file, validated as existing on disk.
///
/// It is still required to handle possible access errors at the time of actually using the file,
/// since it is possible for the file to move or become otherwise inaccessible between validation time and access time.
#[derive(Debug, Clone, Eq, PartialEq, Getters, CopyGetters)]
pub struct ConfigFilePath {
    /// The path on disk for the file.
    #[getset(get = "pub")]
    path: PathBuf,

    /// Whether the path was provided by a user.
    /// If this is false, it was instead discovered during the validation process.
    #[getset(get_copy = "pub")]
    provided: bool,
}

impl ConfigFilePath {
    /// Discover the location for the config file on disk.
    fn discover() -> Result<Self, Report<ValidationError>> {
        iter::once_with(|| discover_file_named("config.yml"))
            .chain_once_with(|| discover_file_named("config.json"))
            .alternative_fold()
            .map(|path| Self {
                path,
                provided: false,
            })
    }
}

impl TryFrom<String> for ConfigFilePath {
    type Error = Report<ValidationError>;

    fn try_from(input: String) -> Result<Self, Self::Error> {
        parse_file_path(input).map(|path| Self {
            path,
            provided: true,
        })
    }
}

/// The path to the database file, validated as existing on disk.
///
/// It is still required to handle possible access errors at the time of actually using the file,
/// since it is possible for the file to move or become otherwise inaccessible between validation time and access time.
#[derive(Debug, Clone, Eq, PartialEq, Getters, CopyGetters)]
pub struct DatabaseFilePath {
    /// The path on disk for the file.
    #[getset(get = "pub")]
    path: PathBuf,

    /// Whether the path was provided by a user.
    /// If this is false, it was instead discovered during the validation process.
    #[getset(get_copy = "pub")]
    provided: bool,
}

impl DatabaseFilePath {
    /// Discover the location for the config file on disk.
    fn discover() -> Result<Self, Report<ValidationError>> {
        discover_file_named("db.sqlite").map(|path| Self {
            path,
            provided: false,
        })
    }
}

impl TryFrom<String> for DatabaseFilePath {
    type Error = Report<ValidationError>;

    fn try_from(input: String) -> Result<Self, Self::Error> {
        parse_file_path(input).map(|path| Self {
            path,
            provided: true,
        })
    }
}

fn parse_file_path(input: String) -> Result<PathBuf, Report<ValidationError>> {
    let path = PathBuf::from(&input);
    validate_file(path)
}

/// Searches configured locations for the file with the provided name.
///
/// Locations searched:
/// - The current working directory
/// - On Linux and macOS: `~/.fossa/broker/`
/// - On Windows: `%USERPROFILE%\.fossa\broker`
fn discover_file_named(name: &str) -> Result<PathBuf, Report<ValidationError>> {
    iter::once_with(|| check_cwd(name).and_then(validate_file))
        .chain_once_with(|| check_home(name).and_then(validate_file))
        .alternative_fold()
}

fn check_cwd(name: &str) -> Result<PathBuf, Report<ValidationError>> {
    let cwd = env::current_dir()
        .into_report()
        .change_context(ValidationError::LocateWorkingDirectory)
        .describe("on macOS and Linux, this uses the system call 'getcwd'")
        .describe("on Windows, this uses the Windows API call 'GetCurrentDirectoryW'")
        .describe("this kind of error is typically caused by the current user not having access to the working directory")?;
    Ok(cwd.join(name))
}

fn check_home(name: &str) -> Result<PathBuf, Report<ValidationError>> {
    let home = dirs::home_dir().ok_or(ValidationError::LocateUserHome).into_report()
        .describe("on macOS and Linux, this uses the $HOME environment variable or the system call 'getpwuid_r'")
        .describe("on Windows, this uses the Windows API call 'SHGetKnownFolderPath'")
        .describe("this is a very rare condition, and it's not likely that Broker will be able to resolve this issue")?;
    Ok(home.join(".fossa").join("broker").join(name))
}

fn validate_file(path: PathBuf) -> Result<PathBuf, Report<ValidationError>> {
    let meta = fs::metadata(&path)
        .into_report()
        .change_context(ValidationError::ValidatePath)
        .describe_lazy(|| format!("validate file: {path:?}"))
        .help("validate that you have access to the file and that it exists")?;

    if meta.is_file() {
        Ok(path)
    } else {
        Err(ValidationError::NotRegularFile)
            .into_report()
            .attach_printable_lazy(|| format!("validate file: {path:?}"))
    }
}
