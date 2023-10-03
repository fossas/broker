//! Types and functions for parsing & validating config files.
//!
//! # Versions
//!
//! Different config file versions are specified by the `version` field.
//! Externally, this module only publishes the unversioned `Config` struct; this always represents the latest "version".
//! This forces parsers for older versions of the config file to choose how to represent new or incompatible entries
//! when a new version is added, without making consumers of this module track multiple config file versions.
//!
//! When at all possible we should strive to make older versions work, but sometimes this may not be possible.
//! If a change is made to the config format that renders an older version completely incompatible,
//! its version-specific parser should be removed and replaced with `fail(Error::Incompatible)`
//! (this module is already written to do this for versions <1, which never existed).
//!
//! Different version parsers are in submodules for nicer organization.
//!
//! # Type organization
//!
//! [`v1`] contains several types that look very similar to the application types elsewhere
//! (for example, [`crate::api::code_host::Integration`] is very similar to [`v1::Integration`]).
//!
//! This similarity is largely because this is just the first version of the config file;
//! I expect much of this to diverge more if we have more config file versions.
//!
//! # Validation vs Parsing
//!
//! This module contains types for _parsing_: for example, [`v1::RawConfigV1`] and child structs/enums.
//! These are not meant to live beyond the [`v1::load`] lifetime; actual "application types"
//! are owned in other modules and contain all validation for the parsed values.
//!
//! Having the "parsing" and "validation" types be separate is going to be critical,
//! as it'll allow different version parsers to eventually lead to the same validated types.
//!
//! Validations are expressed as `From<String>` or `TryFrom<String>` implementations.

use std::path::Path;

use derive_new::new;
use error_stack::{report, Report, ResultExt};
use getset::Getters;
use serde::Deserialize;

use crate::{
    api::{self},
    debug,
    ext::{
        error_stack::{DescribeContext, ErrorHelper, IntoContext},
        result::WrapErr,
    },
};

use crate::ext::io;

mod v1;

/// Errors that are possibly surfaced during validation of config values.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("config file is not compatible with this version of the application")]
    Incompatible,

    #[error("config file version is not supported by this version of the application")]
    Unsupported,

    #[error("read config file on disk")]
    ReadFile,

    #[error("parse config file version")]
    ParseVersion,

    #[error("parse config file in v1 format")]
    ParseV1,
}

/// Validated config values to use during the program runtime.
#[derive(Debug, Clone, PartialEq, Eq, Getters, new)]
#[getset(get = "pub")]
pub struct Config {
    /// Configuration related to the FOSSA API.
    fossa_api: api::fossa::Config,

    /// Configuration related to observability.
    debug: debug::Config,

    /// Configured integration points.
    integrations: api::remote::Integrations,
}

impl Config {
    /// Load the config for the application.
    pub async fn load(path: &Path) -> Result<Self, Report<Error>> {
        // Parsing the config file at least twice; just load it into memory since it's small.
        let content = io::read_to_string(path)
            .await
            .change_context(Error::ReadFile)
            .describe_lazy(|| format!("read config file at '{}'", path.display()))
            .help("ensure you have access to the file and that it exists")?;

        // Parsing just the version allows us to then choose the correct parser to use.
        let RawConfigVersion { version } = serde_yaml::from_str(&content)
            .context(Error::ParseVersion)
            .describe("prior to parsing the config file, Broker checks just the 'version' field to select the correct parser")?;

        match version {
            1 => v1::load(content).await.change_context(Error::ParseV1),
            0 => fail(Error::Incompatible, 0).help("update the config file to a newer format"),
            v => fail(Error::Unsupported, v).help("ensure that Broker is at the latest version"),
        }
    }
}

/// Fail the config load process with the provided file.
fn fail(error: Error, version: usize) -> Result<Config, Report<Error>> {
    report!(error)
        .wrap_err()
        .describe_lazy(|| format!("config file specifies version: {version}"))
}

/// Parse just the version from the config file, allowing branching parsing of other options.
///
/// We could do something fancy with using unparsed `Value` types
/// (in the form of https://lexi-lambda.github.io/blog/2019/11/05/parse-don-t-validate/),
/// but since config files are small it's simpler to just parse the version and then parse the correct version struct.
#[derive(Debug, Deserialize)]
struct RawConfigVersion {
    version: usize,
}
