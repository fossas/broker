//! Types and functions for parsing & validating config files.

use std::path::Path;

use error_stack::Report;
use getset::Getters;

/// Errors that are possibly surfaced during validation of config values.
#[derive(Debug, thiserror::Error)]
pub enum Error {}

/// Config values as parsed from disk.
/// The "Raw" prefix indicates that this is the initial parsed value before any validation.
///
/// Unlike `RawBaseArgs`, we don't have to leak this to consumers of the `config` module,
/// so we don't.
#[derive(Debug)]
pub struct RawConfig {}

impl RawConfig {
    /// Parse config from the provided file on disk.
    pub fn parse(_location: &Path) -> Result<Self, Report<Error>> {
        todo!()
    }
}

/// Validated config values to use during the program runtime.
#[derive(Debug, Clone, PartialEq, Eq, Getters)]
#[getset(get = "pub")]
pub struct Config {}

impl TryFrom<RawConfig> for Config {
    type Error = Report<Error>;

    fn try_from(_raw: RawConfig) -> Result<Self, Self::Error> {
        todo!()
    }
}
