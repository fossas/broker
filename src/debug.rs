//! Covers logging, tracing, metrics reporting, etc.
//!
//! # Artifacts
//!
//! Debug artifacts are generally similar to log files, but:
//! - They can contain more than just logs, for example traces.
//! - They can comprise other types of data, for example time series metrics snapshots.

use std::{path::PathBuf, time::Duration};

use bytesize::ByteSize;
use derive_more::{AsRef, Display, From};
use derive_new::new;
use error_stack::{report, IntoReport, Report, ResultExt};
use getset::Getters;
use humantime::parse_duration;

use crate::ext::{
    self,
    error_stack::{DescribeContext, ErrorHelper},
};

const MIN_RETENTION_SIZE_BYTES: u64 = 1000;
const MIN_RETENTION_AGE: Duration = Duration::from_secs(1);

/// Errors that are possibly surfaced during validation of config values.
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    /// Retention age is parsed from a user-provided string.
    #[error("validate retention age")]
    RetentionAge,

    /// Retention size is parsed from a user-provided string.
    #[error("validate retention size")]
    RetentionSize,

    /// Retentions must be above a minimum value.
    #[error("retention value is too small")]
    RetentionBelowMinimum,

    /// Retention is not a valid duration value
    #[error("parse duration")]
    ParseDuration,
}

/// Validated config values for observability.
#[derive(Debug, Clone, PartialEq, Eq, Getters, new)]
#[getset(get = "pub")]
pub struct Config {
    /// The location into which observability artifacts are stored.
    location: Root,

    /// The configured retention settings.
    retention: Retention,
}

/// Observability artifacts are stored on disk until requested by the FOSSA service.
/// This variable defines the root at which these artifacts are stored.
#[derive(Debug, Clone, PartialEq, Eq, AsRef, From, new)]
pub struct Root(PathBuf);

/// Since observability artifacts are stored on disk, we obviously want to clean them up.
/// These retention settings are used by a background process to keep artifact size in line.
#[derive(Debug, Clone, PartialEq, Eq, Getters, new)]
#[getset(get = "pub")]
pub struct Retention {
    /// The max age for an artifact.
    age: Option<ArtifactMaxAge>,
    /// The max size for an artifact.
    size: Option<ArtifactMaxSize>,
}

/// Specifies the maximum age for an observability artifact.
#[derive(Debug, Clone, PartialEq, Eq, AsRef, From, new)]
pub struct ArtifactMaxAge(Duration);

impl TryFrom<String> for ArtifactMaxAge {
    type Error = Report<ValidationError>;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        parse_duration(&value)
            .into_report()
            .change_context(ValidationError::ParseDuration)
            .and_then(|duration| {
                if duration < MIN_RETENTION_AGE {
                    Err(report!(ValidationError::RetentionBelowMinimum))
                        .help_lazy(|| format!("must be at least {MIN_RETENTION_AGE:?}"))
                } else {
                    Ok(duration)
                }
            })
            .change_context(ValidationError::RetentionAge)
            .describe_lazy(|| format!("provided value: {value}"))
            .map(ArtifactMaxAge)
    }
}

/// Specifies the maximum size for an observability artifact.
#[derive(Debug, Clone, PartialEq, Eq, AsRef, Display, From, new)]
pub struct ArtifactMaxSize(ByteSize);

impl TryFrom<u64> for ArtifactMaxSize {
    type Error = Report<ValidationError>;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        if value < MIN_RETENTION_SIZE_BYTES {
            return Err(report!(ValidationError::RetentionBelowMinimum))
                .change_context(ValidationError::RetentionSize)
                .help_lazy(|| format!("must be at least {MIN_RETENTION_SIZE_BYTES} bytes"))
                .describe_lazy(|| format!("provided value: {value}"));
        }

        let parsed = ext::bytesize::parse_bytes(value);
        Ok(Self(parsed))
    }
}
