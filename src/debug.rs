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
use tracing_appender::rolling;
use tracing_subscriber::{filter, fmt::format::FmtSpan, prelude::*, Registry};

pub mod retention;

use crate::ext::{
    self,
    error_stack::{DescribeContext, ErrorHelper},
};

/// The minimum size for a debugging artifact in bytes.
pub const MIN_RETENTION_SIZE_BYTES: u64 = 1000;

/// The minimum age for a debugging artifact.
pub const MIN_RETENTION_AGE: Duration = Duration::from_secs(1);

/// Errors that are possibly surfaced when running debugging operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// When the trace sink is initialized, it is initialized as a global singleton.
    /// Future attempts to initialize it result in this error.
    /// This is a program logic error ("a bug"), and cannot be resolved by users.
    #[error("trace sink was configured again after being configured once")]
    TraceSinkReconfigured,
}

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

impl Config {
    /// Initialize debugging singletons.
    ///
    /// If the configured log rotation root doesn't exist, it's created.
    /// Until this method is run, traces are not output anywhere and are lost forever;
    /// run it as soon as possible.
    pub fn initialize(&self) -> Result<(), Report<Error>> {
        self.initialize_tracing_sink()
    }

    /// The path to the directory containing trace files.
    fn tracing_root(&self) -> PathBuf {
        self.location().as_ref().join("trace")
    }

    /// Initialize tracing sinks:
    /// - Hourly rotating sink of all raw traces in JSON format to disk.
    /// - Pretty sink of INFO-level traces to stdout.
    fn initialize_tracing_sink(&self) -> Result<(), Report<Error>> {
        let sink = rolling::hourly(self.tracing_root(), "broker.trace");
        let subscriber = Registry::default()
            // log pretty info traces to terminal
            .with(
                tracing_subscriber::fmt::layer()
                    .pretty()
                    .with_filter(filter::LevelFilter::INFO),
            )
            // log all traces to file in json format
            .with(
                tracing_subscriber::fmt::layer()
                    .json()
                    .with_span_events(FmtSpan::FULL)
                    .with_writer(sink),
            );

        tracing::subscriber::set_global_default(subscriber)
            .into_report()
            .change_context(Error::TraceSinkReconfigured)
            .help("if you're a user and you're seeing this, please report this as a defect to FOSSA support")
            .describe("this is a program bug and is not something that users can fix")
    }
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

impl ArtifactMaxAge {
    /// Check whether an artifact with the provided age is older than the max age.
    pub fn is_violated_by(&self, age: Duration) -> bool {
        self.0 < age
    }
}

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

impl ArtifactMaxSize {
    /// Check whether the provided size is larger than the max size.
    pub fn is_violated_by(&self, size: ByteSize) -> bool {
        self.0 < size
    }
}

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
