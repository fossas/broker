//! Covers logging, tracing, metrics reporting, etc.
//!
//! # Artifacts
//!
//! Debug artifacts are generally similar to log files, but:
//! - They can contain more than just logs, for example traces.
//! - They can comprise other types of data, for example time series metrics snapshots.

use std::path::{Path, PathBuf};

use derive_more::{AsRef, From, Into};
use derive_new::new;
use error_stack::{report, Report, ResultExt};
use getset::{CopyGetters, Getters};
use rolling_file::{BasicRollingFileAppender, RollingConditionBasic};
use tracing::{info, Metadata};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{
    filter, fmt::format::FmtSpan, layer::Context, prelude::*, Layer, Registry,
};

use crate::ext::{
    error_stack::{DescribeContext, ErrorHelper, IntoContext},
    result::WrapErr,
};

use self::bundler::Bundler;

mod bundle;
pub mod bundler;

/// Errors that are possibly surfaced when running debugging operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// When the trace sink is initialized, it is initialized as a global singleton.
    /// Future attempts to initialize it result in this error.
    /// This is a program logic error ("a bug"), and cannot be resolved by users.
    #[error("trace sink was configured again after being configured once")]
    TraceSinkReconfigured,

    /// When configuring tracing log output, it's possible for the rolling appender to fail.
    #[error("failed to configure tracing output location")]
    TraceConfig,

    /// When configuring tracing, we ensure that the tracing root directory exists.
    /// If it didn't exist and can't be created, this error is returned.
    #[error("failed to create tracing output location")]
    EnsureTraceRoot,

    /// It's unfortunately possible for collecting a debug bundle to fail.
    #[error("collecting debug bundle")]
    CollectDebugBundle,
}

/// Errors that are possibly surfaced during validation of config values.
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    /// Retentions must be above a minimum value.
    #[error("retention value is too small")]
    RetentionBelowMinimum,
}

/// Export mode for the debug bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleExport {
    /// Bundle export is disabled entirely.
    Disable,

    /// It's up to Broker to determine whether it's appropriate to export.
    Auto,

    /// Exporting the debug bundle is toggled on by the customer.
    Always,
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
    /// Until this method is run, traces are not output anywhere and are lost forever;
    /// run it as soon as possible.
    #[must_use = "This guard must be stored in a variable that is retained; if it is dropped the tracing sink will stop running"]
    pub fn run_tracing_sink(&self) -> Result<WorkerGuard, Report<Error>> {
        self.ensure_tracing_root_exists()?;
        self.initialize_tracing_sink()
    }

    /// The path to the directory containing trace files.
    fn tracing_root(&self) -> PathBuf {
        self.location().as_ref().join("trace")
    }

    /// Ensure the tracing root exists.
    fn ensure_tracing_root_exists(&self) -> Result<(), Report<Error>> {
        let root = self.tracing_root();
        std::fs::create_dir_all(&root)
            .context(Error::EnsureTraceRoot)
            .help("this location is set in the config file")
            .describe_lazy(|| {
                format!(
                    "debug info is configured to be stored in '{}'",
                    root.display()
                )
            })
    }

    /// Initialize tracing sinks:
    /// - Hourly rotating sink of all raw traces in JSON format to disk.
    /// - Pretty sink of INFO-level traces to stdout.
    fn initialize_tracing_sink(&self) -> Result<WorkerGuard, Report<Error>> {
        let target = self.tracing_root().join("broker.trace");
        let file = self.retention().sink(&target)?;
        let (sink, guard) = tracing_appender::non_blocking(file);

        let subscriber = Registry::default()
            // log pretty info traces to terminal
            .with(
                tracing_subscriber::fmt::layer()
                    .compact()
                    .with_file(false)
                    .with_level(true)
                    .with_line_number(false)
                    .with_target(false)
                    .with_writer(std::io::stderr)
                    .with_ansi(atty::is(atty::Stream::Stderr))
                    .with_filter(filter::dynamic_filter_fn(filter_to_events))
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
            .context(Error::TraceSinkReconfigured)
            .help("if you're a user and you're seeing this, please report this as a defect to FOSSA support")
            .describe("this is a program bug and is not something that users can fix")?;

        info!(
            "Debug artifacts being stored in '{}'",
            self.tracing_root().display()
        );
        Ok(guard)
    }
}

/// When logging trace output, we're talking to FOSSA users, not developers.
/// Users don't care about the vast majority of what traces contain, things like:
/// - Line numbers
/// - Call stack context
/// - Module names
///
/// Instead, just limit it to literally only events. No span metadata.
fn filter_to_events(metadata: &Metadata<'_>, ctx: &Context<'_, Registry>) -> bool {
    if metadata.is_event() {
        return true;
    }

    if let Some(current) = ctx.lookup_current() {
        return current.metadata().is_event();
    }

    false
}

/// Observability artifacts are stored on disk until requested by the FOSSA service.
/// This variable defines the root at which these artifacts are stored.
#[derive(Debug, Clone, PartialEq, Eq, AsRef, From, new)]
pub struct Root(PathBuf);

impl Root {
    /// The location of the root as a path.
    pub fn as_path(&self) -> &Path {
        self.as_ref()
    }

    /// The location for the debug bundle for a given scan ID.
    pub fn debug_bundle(&self, scan_id: &str) -> PathBuf {
        let file_name = format!("{}.fossa.debug.json.gz", scan_id);
        self.as_ref().join(file_name)
    }
}

/// Since observability artifacts are stored on disk, we obviously want to clean them up.
/// These retention settings are used by a background process to keep artifact size in line.
#[derive(Debug, Clone, PartialEq, CopyGetters, Eq, new)]
#[getset(get_copy = "pub")]
pub struct Retention {
    /// The number of days to retain.
    days: ArtifactRetentionCount,
}

impl Retention {
    fn sink(&self, target: &Path) -> Result<BasicRollingFileAppender, Report<Error>> {
        let roll_condition = RollingConditionBasic::new().daily();
        BasicRollingFileAppender::new(target, roll_condition, self.days.into())
            .context(Error::TraceConfig)
            .help("ensure that the parent directory exists and you have access to it")
            .describe_lazy(|| format!("initialize sink to '{}'", target.display()))
    }
}

/// Specifies the number of rotated artifacts that are kept.
#[derive(Debug, Clone, Copy, PartialEq, Into, Eq, new)]
pub struct ArtifactRetentionCount(usize);

impl Default for ArtifactRetentionCount {
    /// Defaults to seven days.
    fn default() -> Self {
        Self(7)
    }
}

impl PartialEq<usize> for ArtifactRetentionCount {
    fn eq(&self, other: &usize) -> bool {
        self.0 == *other
    }
}

impl TryFrom<usize> for ArtifactRetentionCount {
    type Error = Report<ValidationError>;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        if value == 0 {
            report!(ValidationError::RetentionBelowMinimum)
                .wrap_err()
                .help("must specify at least '1'")
                .describe_lazy(|| format!("provided value: {value}"))
        } else {
            Ok(Self(value))
        }
    }
}

/// Intended to provide FOSSA employees with all the information required to debug
/// Broker in any environment, without sharing sensitive information.
#[derive(Debug, Clone, Getters, Default)]
#[getset(get = "pub")]
pub struct Bundle {
    /// The location to the debug bundle that was persisted.
    location: PathBuf,
}

impl Bundle {
    /// Collect a debug bundle from the working environment.
    pub fn collect<B, P>(conf: &Config, bundler: B, path: P) -> Result<Self, Report<Error>>
    where
        P: AsRef<Path>,
        B: Bundler,
        B::Error: error_stack::Context,
    {
        bundle::generate(conf, bundler, path).change_context(Error::CollectDebugBundle)
    }

    /// Whether the debug bundle is is empty.
    /// Empty debug bundles do not actually exist on disk.
    pub fn is_empty(&self) -> bool {
        self.location.as_os_str().is_empty()
    }
}
