//! Types and functions for parsing & validating CLI arguments.

use std::{iter, path::PathBuf};

use clap::Parser;
use error_stack::{Report, ResultExt};
use getset::{CopyGetters, Getters};

use crate::ext::{
    error_stack::{DescribeContext, ErrorHelper},
    iter::{AlternativeIter, ChainOnceWithIter},
};

use super::io;

/// Errors that are possibly surfaced during validation of config values.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The config file location provided is not valid, or was not able to be located.
    #[error("validate config file location")]
    ConfigFileLocation,

    /// The DB file location provided is not valid, or was not able to be located.
    #[error("validate database file location")]
    DbFileLocation,

    /// Failed to locate the file in any search location.
    #[error("failed to locate file in any known location")]
    LocateFile,
}

/// Base arguments, used in most Broker subcommands.
/// The "Raw" prefix indicates that this is the initial parsed value before any validation.
///
/// There is no exported function in `config` that parses these args; instead these are
/// parsed automatically by `clap` since they implement `Parser` and are included in the
/// top-level subcommand configuration sent to `clap`.
///
/// Unlike with the config file, there's not really a concept of these args "failing to parse",
/// as `clap` steps in and shows the user errors in this case. By the time `clap` hands
/// us this structure, it's been successfully parsed.
///
/// This odd dichotomy is why we have to leak the `RawBaseArgs` implementation to the package consumer,
/// because the consumer (`main`) needs to be able to give this type to `clap` for it to be parsed.
#[derive(Debug, Parser)]
#[command(version, about)]
pub struct RawBaseArgs {
    /// The path to the Broker config file.
    ///
    /// If unset, Broker searches (in order) for `config.yml` or `config.yaml` in
    /// the current working directory, then (on Linux and macOS) `~/.fossa/broker/`,
    /// or (on Windows) `%USERPROFILE%\.fossa\broker`.
    #[arg(short = 'c', long)]
    config_file_path: Option<String>,

    /// The path to the Broker database file.
    ///
    /// If unset, Broker searches (in order) for `db.sqlite` in
    /// the current working directory, then (on Linux and macOS) `~/.fossa/broker/`,
    /// or (on Windows) `%USERPROFILE%\.fossa\broker`.
    #[arg(short = 'd', long)]
    database_file_path: Option<String>,
}

/// Base arguments, used in most Broker subcommands.
#[derive(Debug, Clone, PartialEq, Eq, Getters)]
#[getset(get = "pub")]
pub struct BaseArgs {
    /// The path to the config file on disk.
    config_path: ConfigFilePath,

    /// The path to the database file on disk.
    database_path: DatabaseFilePath,
}

impl TryFrom<RawBaseArgs> for BaseArgs {
    type Error = Report<Error>;

    fn try_from(raw: RawBaseArgs) -> Result<Self, Self::Error> {
        let discovering_config = raw.config_file_path.is_none();
        let config_path = raw
            .config_file_path
            .map(ConfigFilePath::try_from)
            .unwrap_or_else(ConfigFilePath::discover)
            .change_context(Error::ConfigFileLocation)
            .describe_if(
                discovering_config,
                "searches the working directory and '{USER_DIR}/.fossa/broker' for 'config.yml' or 'config.yaml'"
            )
            .help_if(
                discovering_config,
                "consider providing an explicit argument instead"
            );

        let discovering_db = raw.database_file_path.is_none();
        let database_path = raw
            .database_file_path
            .map(DatabaseFilePath::try_from)
            .unwrap_or_else(DatabaseFilePath::discover)
            .change_context(Error::DbFileLocation)
            .describe_if(
                discovering_db,
                "searches the working directory and '{USER_DIR}/.fossa/broker' for 'db.sqlite'",
            )
            .help_if(
                discovering_db,
                "consider providing an explicit argument instead",
            );

        // `error_stack` supports stacking multiple errors together so
        // they can all be reported at the same time.
        // We use this elsewhere, for example in `alternative_fold`,
        // and we use this manually here.
        //
        // A stacked error can trivially be created by folding `Vec<Result<T, Report<E>>>`
        // into `Result<Vec<T>, Report<E>>` using `Report::extend_one`.
        //
        // Unfortunately, since our `Result<T, E>` types have heterogenous `T`'s,
        // we can't store them in a `Vec` or `Iterator`.
        // To explain, `Result<T1, E>` and `Result<T2, E>` can be reduced down to `T1, T2`.
        // It's not possible to store `Vec<T1, T2>` or `Iterator<Item = T1, T2>`,
        // because both of these containers require homogenous data types.
        //
        // I think the most elegant way around this is likely using https://docs.rs/tuple_list/latest/tuple_list/
        // because a "heterogenous vec" is ~a tuple, we just need some extra syntax to be able to
        // recursively work with tuples. I think this way would probably result in the least boilerplate
        // with purely compile time validation.
        //
        // We could also use a macro to automatically create `match` statements like the below at arbitrary arity,
        // in other words simply automate creation of the boilerplate. This is probably the less elegant but
        // more practical (and faster) approach.
        //
        // For now I'm tabling either route in favor of manually writing the match; we only have two things to validate.
        // If we start adding too many more, we should really consider making this better.
        // Seeing the below, you can imagine how unweidly this'll get with 3 or 4 errors.
        match (config_path, database_path) {
            (Ok(config_path), Ok(database_path)) => Ok(Self {
                config_path,
                database_path,
            }),
            (Ok(_), Err(err)) => Err(err),
            (Err(err), Ok(_)) => Err(err),
            (Err(mut first), Err(second)) => {
                first.extend_one(second);
                Err(first)
            }
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
    fn discover() -> Result<Self, Report<Error>> {
        iter::once_with(|| io::find("config.yml"))
            .chain_once_with(|| io::find("config.yaml"))
            .alternative_fold()
            .change_context(Error::LocateFile)
            .map(|path| Self {
                path,
                provided: false,
            })
    }
}

impl TryFrom<String> for ConfigFilePath {
    type Error = Report<Error>;

    fn try_from(input: String) -> Result<Self, Self::Error> {
        io::validate_file(PathBuf::from(input))
            .change_context(Error::LocateFile)
            .map(|path| Self {
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
    fn discover() -> Result<Self, Report<Error>> {
        io::find("db.sqlite")
            .change_context(Error::LocateFile)
            .map(|path| Self {
                path,
                provided: false,
            })
    }
}

impl TryFrom<String> for DatabaseFilePath {
    type Error = Report<Error>;

    fn try_from(input: String) -> Result<Self, Self::Error> {
        io::validate_file(PathBuf::from(input))
            .change_context(Error::LocateFile)
            .map(|path| Self {
                path,
                provided: true,
            })
    }
}
