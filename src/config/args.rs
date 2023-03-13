//! Types and functions for parsing & validating CLI arguments.

use std::path::PathBuf;

use clap::Parser;
use derive_new::new;
use error_stack::{Report, ResultExt};
use getset::{CopyGetters, Getters};
use serde::Serialize;

use crate::ext::{
    error_stack::{merge_error_stacks, DescribeContext, ErrorHelper},
    io,
    result::WrapOk,
};

/// Errors that are possibly surfaced during validation of config values.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The config file was not able to be located.
    #[error("locate config file")]
    ConfigFileLocation,

    /// The DB file was not able to be located.
    #[error("locate database file")]
    DbFileLocation,
}

/// Base arguments, used in most Broker subcommands.
/// The "Raw" prefix indicates that this is the initial parsed value before any validation.
///
/// # Background
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
#[derive(Debug, Clone, Parser, Serialize, new)]
#[command(version, about)]
pub struct RawBaseArgs {
    /// The path to the Broker config file.
    ///
    /// If unset, Broker searches (in order) for `config.yml` or `config.yaml` in
    /// the current working directory, then (on Linux and macOS) `~/.config/fossa/broker/`,
    /// or (on Windows) `%USERPROFILE%\.config\fossa\broker`.
    #[arg(short = 'c', long)]
    config_file_path: Option<String>,

    /// The path to the Broker database file.
    ///
    /// If unset, Broker searches (in order) for `db.sqlite` in
    /// the current working directory, then (on Linux and macOS) `~/.config/fossa/broker/`,
    /// or (on Windows) `%USERPROFILE%\.config\fossa\broker`.
    #[arg(short = 'd', long)]
    database_file_path: Option<String>,
}

impl RawBaseArgs {
    /// Validate the raw args provided.
    ///
    /// In practice, if the user provided a path to the db and config file, the validation is straightforward.
    /// If the user did not provide one or both, this function discovers their location on disk
    /// or errors if they are not able to be found.
    pub async fn validate(self) -> Result<BaseArgs, Report<Error>> {
        let config_path = if let Some(provided_path) = self.config_file_path {
            ConfigFilePath::from(provided_path).wrap_ok()
        } else {
            ConfigFilePath::discover()
                .await
                .change_context(Error::ConfigFileLocation)
        };

        let database_path = if let Some(provided_path) = self.database_file_path {
            DatabaseFilePath::from(provided_path).wrap_ok()
        } else {
            DatabaseFilePath::discover()
                .await
                .change_context(Error::DbFileLocation)
        };

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
            (Ok(config_path), Ok(database_path)) => Ok(BaseArgs {
                config_path,
                database_path,
            }),
            (Ok(_), Err(err)) => Err(err),
            (Err(err), Ok(_)) => Err(err),
            (Err(first), Err(second)) => Err(merge_error_stacks!(first, second)),
        }
    }
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

/// The path to the config file.
///
/// Note that this is validated as being correctly shaped; the file is not guaranteed to exist.
#[derive(Debug, Clone, Eq, PartialEq, Getters, CopyGetters)]
pub struct ConfigFilePath {
    /// The path on disk for the file.
    #[getset(get = "pub")]
    path: PathBuf,

    /// Whether the path was provided by a user.
    #[getset(get_copy = "pub")]
    provided: bool,
}

impl ConfigFilePath {
    /// Discover the location for the config file on disk.
    async fn discover() -> Result<Self, Report<io::Error>> {
        io::find_some(["config.yml", "config.yaml"])
            .await
            .describe("searches for 'config.yml' or 'config.yaml'")
            .help("consider providing an explicit argument instead")
            .map(|path| Self {
                path,
                provided: false,
            })
    }
}

impl From<String> for ConfigFilePath {
    fn from(value: String) -> Self {
        Self {
            path: PathBuf::from(value),
            provided: true,
        }
    }
}

/// The path to the database file.
///
/// Note that this is validated as being correctly shaped; the file is not guaranteed to exist.
#[derive(Debug, Clone, Eq, PartialEq, Getters, CopyGetters)]
pub struct DatabaseFilePath {
    /// The path on disk for the file.
    #[getset(get = "pub")]
    path: PathBuf,

    /// Whether the path was provided by a user.
    #[getset(get_copy = "pub")]
    provided: bool,
}

impl DatabaseFilePath {
    /// Discover the location for the config file on disk.
    async fn discover() -> Result<Self, Report<io::Error>> {
        io::find("db.sqlite")
            .await
            .describe("searches for 'db.sqlite'")
            .help("consider providing an explicit argument instead")
            .map(|path| Self {
                path,
                provided: false,
            })
    }
}

impl From<String> for DatabaseFilePath {
    fn from(value: String) -> Self {
        Self {
            path: PathBuf::from(value),
            provided: true,
        }
    }
}
