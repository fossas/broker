//! Types and functions for parsing & validating CLI arguments.
//!
//! `Raw` prefixes indicate that the type is the initial parsed value before any validation;
//! once validated they turn into the same name without the `Raw` prefix.
//! For example, `RawRunArgs -> RunArgs`.
//!
//! # Background
//!
//! There is no exported function in `config` that parses raw args; instead these are
//! parsed automatically by `clap` since they implement `Parser` and are included in the
//! top-level subcommand configuration sent to `clap`.
//!
//! Unlike with the config file, there's not really a concept of these args "failing to parse",
//! as `clap` steps in and shows the user errors in this case. By the time `clap` hands
//! us this structure, it's been successfully parsed.
//!
//! Meanwhile if we make validation part of parsing (e.g. in the style of "parse, don't validate"),
//! we can't show a formatted error message with all our help and context, because `clap` takes over
//! and renders the error however it thinks is best.
//!
//! This odd dichotomy is why we have to leak the `Raw*` implementations to the package consumer,
//! because the consumer (`main`) needs to be able to give this type to `clap` for it to be parsed.

use std::path::PathBuf;

use clap::Parser;
use derive_new::new;
use error_stack::{report, Report, ResultExt};
use getset::{CopyGetters, Getters};
use indoc::indoc;
use serde::Serialize;

use crate::{
    debug::BundleExport,
    ext::{
        error_stack::{merge_error_stacks, DescribeContext, ErrorHelper},
        io,
        result::{WrapErr, WrapOk},
    },
    AppContext,
};

/// The variable used to control whether Broker attempts to discover files.
pub const DISABLE_FILE_DISCOVERY_VAR: &str = "DISABLE_FILE_DISCOVERY";

/// Errors that are possibly surfaced during validation of config values.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The config file was not able to be located.
    #[error("locate config file")]
    ConfigFileLocation,

    /// The DB file was not able to be located.
    #[error("locate database file")]
    DbFileLocation,

    /// The data root was not able to be determined.
    #[error("determine data root")]
    DataRoot,
}

/// Arguments used by the "fix" command.
#[derive(Debug, Clone, Parser, Serialize, new)]
#[command(version, about)]
pub struct RawFixArgs {
    /// Include all the same args as used with `run`.
    ///
    /// These are flattened into the args, so they appear to the user
    /// as though they were in this struct directly.
    #[clap(flatten)]
    runtime: RawRunArgs,

    /// Always save the debug bundle.
    ///
    /// `broker fix` automatically saves the debug bundle if it is not able
    /// to resolve the issue, but this option causes the debug bundle to always be saved.
    #[arg(long)]
    export_bundle: bool,
}

impl RawFixArgs {
    /// Validate the raw args provided.
    ///
    /// In practice, if the user provided a path to the db and config file, the validation is straightforward.
    /// If the user did not provide one or both, this function discovers their location on disk
    /// or errors if they are not able to be found.
    ///
    /// In the case of the database file, if one was not provided _and_ not found,
    /// it is assumed to be a sibling to the config file.
    /// Database implementations then create it if it does not exist.
    #[tracing::instrument]
    pub async fn validate(self) -> Result<FixArgs, Report<Error>> {
        let runtime = self.runtime.validate().await?;
        let export_bundle = if self.export_bundle {
            BundleExport::Always
        } else {
            BundleExport::Auto
        };

        Ok(FixArgs {
            runtime,
            export_bundle,
        })
    }
}

/// Arguments used by the "run" command.
#[derive(Debug, Clone, PartialEq, Eq, Getters, CopyGetters)]
pub struct FixArgs {
    /// Runtime config options, like those used in `run`.
    #[getset(get = "pub")]
    runtime: RunArgs,

    /// How to export the debug bundle.
    #[getset(get_copy = "pub")]
    export_bundle: BundleExport,
}

/// Arguments used by the "run" command.
#[derive(Debug, Clone, Parser, Serialize, new)]
#[command(version, about)]
pub struct RawRunArgs {
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

    /// The root data directory for Broker.
    /// Broker uses this directory to store working state and to read configuration information.
    ///
    /// - On Linux and macOS: `~/.config/fossa/broker/`
    /// - On Windows: `%USERPROFILE%\.config\fossa\broker`
    #[arg(short = 'r', long)]
    data_root: Option<PathBuf>,
}

impl RawRunArgs {
    /// Validate the raw args provided.
    ///
    /// In practice, if the user provided a path to the db and config file, the validation is straightforward.
    /// If the user did not provide one or both, this function discovers their location on disk
    /// or errors if they are not able to be found.
    ///
    /// In the case of the database file, if one was not provided _and_ not found,
    /// it is assumed to be a sibling to the config file.
    /// Database implementations then create it if it does not exist.
    #[tracing::instrument]
    pub async fn validate(self) -> Result<RunArgs, Report<Error>> {
        let data_root = match self.data_root {
            Some(data_root) => data_root,
            None => default_data_root().await?,
        };
        let ctx = AppContext::new(data_root);

        let config_path = if let Some(provided_path) = self.config_file_path {
            ConfigFilePath::from(provided_path).wrap_ok()
        } else if discovery_enabled() {
            ConfigFilePath::discover(&ctx)
                .await
                .change_context(Error::ConfigFileLocation)
        } else {
            report!(Error::ConfigFileLocation).wrap_err().help_lazy(|| {
                format!("discovery is disabled via '{DISABLE_FILE_DISCOVERY_VAR}' env var")
            })
        };

        let database_path = if let Some(provided_path) = self.database_file_path {
            DatabaseFilePath::from(provided_path).wrap_ok()
        } else if discovery_enabled() {
            DatabaseFilePath::discover(&ctx)
                .await
                .change_context(Error::DbFileLocation)
        } else {
            report!(Error::DbFileLocation).wrap_err().help_lazy(|| {
                format!("discovery is disabled via '{DISABLE_FILE_DISCOVERY_VAR}' env var")
            })
        };

        // If the DB path couldn't be found, but we have a config path, try to set the DB
        // to be a sibling of the config path. If that can't be done, augment the error
        // to explain why for debugging.
        let database_path = match database_path {
            Ok(database_path) => Ok(database_path),
            Err(err) => {
                match &config_path {
                    Ok(config_path) => {
                        match config_path.path.parent() {
                            Some(parent) => Ok(DatabaseFilePath { path: parent.join("db.sqlite"), provided: false }),
                            None => Err(err).describe(indoc! {"
                            Usually in this case Broker assumes the DB path to be next to the config file path,
                            but the config path's parent could not be determined and so the db path was unable
                            to be constructed.
                            "}),
                        }
                    },
                    Err(_) => Err(err).describe(indoc! {"
                    Usually in this case Broker assumes the DB path to be next to the config file path,
                    but since the config file path was not able to be determined that's not possible here.
                    "}),
                }
            },
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
            (Ok(config_path), Ok(database_path)) => Ok(RunArgs {
                config_path,
                database_path,
                context: ctx,
            }),
            (Ok(_), Err(err)) => Err(err),
            (Err(err), Ok(_)) => Err(err),
            (Err(first), Err(second)) => Err(merge_error_stacks!(first, second)),
        }
    }
}

/// Arguments used by the "run" command.
#[derive(Debug, Clone, PartialEq, Eq, Getters)]
#[getset(get = "pub")]
pub struct RunArgs {
    /// The path to the config file on disk.
    config_path: ConfigFilePath,

    /// The path to the database file on disk.
    database_path: DatabaseFilePath,

    /// The configured application context.
    context: AppContext,
}

/// Arguments used by the "init" command.
#[derive(Debug, Clone, Parser, Serialize, new)]
#[command(version, about)]
pub struct RawInitArgs {
    /// The root data directory for Broker.
    /// Broker uses this directory to store working state and to read configuration information.
    ///
    /// - On Linux and macOS: `~/.config/fossa/broker/`
    /// - On Windows: `%USERPROFILE%\.config\fossa\broker`
    #[arg(short = 'r', long)]
    data_root: Option<PathBuf>,
}

impl RawInitArgs {
    /// validate the args for the init subcommand
    #[tracing::instrument]
    pub async fn validate(self) -> Result<AppContext, Report<Error>> {
        let data_root = match self.data_root {
            Some(data_root) => data_root,
            None => default_data_root().await?,
        };

        AppContext::new(data_root).wrap_ok()
    }
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
    async fn discover(ctx: &AppContext) -> Result<Self, Report<io::Error>> {
        io::find_some(ctx, &["config.yml", "config.yaml"])
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
    async fn discover(ctx: &AppContext) -> Result<Self, Report<io::Error>> {
        io::find(ctx, "db.sqlite")
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

fn discovery_enabled() -> bool {
    std::env::var(DISABLE_FILE_DISCOVERY_VAR)
        .map(|value| ["true", "1"].contains(&value.as_str()))
        .map(|value| !value)
        .unwrap_or(true)
}

async fn default_data_root() -> Result<PathBuf, Report<Error>> {
    io::home_dir()
        .await
        .map(|home| home.join(".config").join("fossa").join("broker"))
        .change_context(Error::DataRoot)
}
