//! Responsible for cleaning up tracing files (and maybe other artifacts in the future).
//!
//! # Tracing files
//!
//! Stored in `self.location().join("trace")`. Each hour, a file is rotated;
//! these are stored in the format `broker.trace.yyyy-MM-dd-HH`,
//! where the timestamp is the UTC time.
//!
//! So we just need to find files with timestamps older than the configured timeout.

use std::{
    cmp::Ordering,
    fmt::{Debug, Display},
    fs::Metadata,
    ops::Add,
    os::unix::prelude::MetadataExt,
    path::Path,
    time::{Duration, SystemTime},
};

use async_stream::try_stream;
use bytesize::ByteSize;
use delegate::delegate;
use derive_more::From;
use derive_new::new;
use error_stack::{report, Report, ResultExt};
use futures::{Stream, StreamExt, TryStreamExt};
use tokio::{
    fs::{self, DirEntry},
    time::{self, Instant},
};
use tracing::{debug, info, warn};

use crate::ext::{
    error_stack::{merge_error_stacks, DescribeContext, ErrorHelper, IntoContext},
    result::InspectErr,
};

use super::Config;

const WORKER_CYCLE_DELAY: Duration = Duration::from_secs(60);

/// Errors that are possibly surfaced when running debugging operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Encountered cleaning up max age.
    #[error("retention by age")]
    ByAge,

    /// Encountered cleaning up max size.
    #[error("retention by size")]
    BySize,

    /// Encounted while enumerating files.
    #[error("enumerate files")]
    EnumerateFiles,

    /// Encountered while removing a file.
    #[error("remove file")]
    RemoveFile,
}

/// Run the tracing cleanup worker.
///
/// This runs as an async background task, meaning that it needs to be constantly driven.
/// If it is dropped at any point, the worker stops.
pub async fn run_worker(config: &Config) -> Result<(), Report<Error>> {
    loop {
        let artifacts = do_cleanup_cycle(config)
            .await
            .describe("perform cleanup cycle")?;

        if !artifacts.is_empty() {
            info!("Cleaned up artifacts:\n {artifacts}");
        }

        let next_cycle = Instant::now().add(WORKER_CYCLE_DELAY);
        debug!("Running next cleanup at {next_cycle:?}");
        time::sleep_until(next_cycle).await;
    }
}

/// The list of cleaned up artifacts in a cleanup cycle.
#[derive(Debug, Clone, PartialEq, Eq, From, Default, new)]
struct CleanedUpArtifacts(Vec<String>);

impl CleanedUpArtifacts {
    delegate! {
        to self.0 {
            /// The number of artifacts that were cleaned up.
            pub fn len(&self) -> usize;

            /// Check whether any artifacts were cleaned up.
            pub fn is_empty(&self) -> bool;

            /// Iterate over the artifacts that were cleaned up.
            pub fn iter(&self) -> impl Iterator<Item = &String>;
        }
    }
}

impl Add for CleanedUpArtifacts {
    type Output = CleanedUpArtifacts;

    fn add(mut self, rhs: Self) -> Self::Output {
        self.0.extend_from_slice(&rhs.0);
        self
    }
}

impl Display for CleanedUpArtifacts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let pretty = self
            .iter()
            .map(|artifact| format!("\t{artifact}"))
            .collect::<Vec<_>>()
            .join("\n");
        write!(f, "{pretty}")
    }
}

/// Run a cleanup cycle.
#[tracing::instrument]
async fn do_cleanup_cycle(config: &Config) -> Result<CleanedUpArtifacts, Report<Error>> {
    // These two cleanups should run serially so that they don't step on each other's toes.
    let age = do_cleanup_cycle_age(config)
        .await
        .change_context(Error::ByAge);
    let size = do_cleanup_cycle_size(config)
        .await
        .change_context(Error::BySize);

    match (age, size) {
        (Ok(age), Ok(size)) => Ok(age + size),
        (Err(age), Err(size)) => Err(merge_error_stacks!(age, size)),
        (Ok(age), Err(size)) => Err(report!(size)).help(format!(
            "age retention ran successfully, cleaning up {} items",
            age.len()
        )),
        (Err(age), Ok(size)) => Err(report!(age)).help(format!(
            "size retention ran successfully, cleaning up {} items",
            size.len()
        )),
    }
}

#[tracing::instrument]
async fn do_cleanup_cycle_age(config: &Config) -> Result<CleanedUpArtifacts, Report<Error>> {
    let Some(max_age) = config.retention().age() else { return Ok(CleanedUpArtifacts::default()) };

    list_contents(&config.tracing_root())
        .try_filter_map(|entry| async move {
            let meta = entry
                .metadata()
                .await
                .context(Error::EnumerateFiles)
                .describe_lazy(|| format!("list metadata for file {:?}", entry.path()))?;
            if meta.is_file() && max_age.is_violated_by(file_age(&meta)) {
                Ok(Some(entry.path()))
            } else {
                Ok(None)
            }
        })
        .and_then(|path| async move {
            fs::remove_file(&path)
                .await
                .context(Error::RemoveFile)
                .describe_lazy(|| format!("remove file {path:?}"))
                .map(|_| path.to_string_lossy().to_string())
        })
        .try_collect::<Vec<_>>()
        .await
        .map(CleanedUpArtifacts::from)
        .help("ensure the file system is not read only")
}

/// Clean up the oldest files that are causing the total file size to exceed the max retention size.
#[tracing::instrument]
async fn do_cleanup_cycle_size(config: &Config) -> Result<CleanedUpArtifacts, Report<Error>> {
    let Some(max_size) = config.retention().size() else { return Ok(CleanedUpArtifacts::default()) };

    // It's not efficient to sort streams, so collect into a vector to do the sorting.
    let mut files = list_contents(&config.tracing_root())
        .try_filter_map(|entry| async move {
            let meta = entry
                .metadata()
                .await
                .context(Error::EnumerateFiles)
                .describe_lazy(|| format!("list metadata for file {:?}", entry.path()))?;

            if meta.is_file() {
                let path = entry.path();
                let age = file_age(&meta);
                let size = ByteSize::b(meta.size());
                Ok(Some((age, size, path)))
            } else {
                Ok(None)
            }
        })
        .try_collect::<Vec<_>>()
        .await?;

    // If two files are the exact same age, use path to break the tie.
    files.sort_unstable_by(
        |(age_a, _, path_a), (age_b, _, path_b)| match age_a.cmp(age_b) {
            Ordering::Equal => path_a.cmp(path_b),
            other => other,
        },
    );

    // Now walk through the sorted vec, deleting the ones that cause the overall collection to go over the max size.
    let mut total_size = ByteSize::b(0);
    futures::stream::iter(files)
        .filter_map(|(_, size, path)| async move {
            total_size += size;
            if max_size.is_violated_by(total_size) {
                debug!("{path:?} is over overall size limit: {max_size}");
                Some(path)
            } else {
                None
            }
        })
        .then(|path| async move {
            fs::remove_file(&path)
                .await
                .context(Error::RemoveFile)
                .describe_lazy(|| format!("remove file {path:?}"))
                .map(|_| path.to_string_lossy().to_string())
        })
        .try_collect::<Vec<_>>()
        .await
        .map(CleanedUpArtifacts::from)
        .help("ensure the file system is not read only")
}

/// Create a stream which lists the contents of a single level of a directory.
fn list_contents(dir: &Path) -> impl Stream<Item = Result<DirEntry, Report<Error>>> {
    // Take ownership of the path (by copying it) so that we can send it across threads without having to manage external lifetimes.
    let dir = dir.to_path_buf();

    // Implements a stream (basically an async iterator) via `await` and `yield`.
    // Once this block exits, the stream is closed.
    try_stream! {
        let mut contents = fs::read_dir(&dir)
            .await
            .context(Error::EnumerateFiles)
            .help("ensure the current user has access to list files in the directory")
            .describe_lazy(|| format!("list contents of {dir:?}"))?;

        loop {
            // These error helpers are the same basically just because there's no useful additional information to give in this case.
            let file = contents.next_entry().await
                .context(Error::EnumerateFiles)
                .help("ensure the current user has access to list files in the directory")
                .describe_lazy(|| format!("list contents of {dir:?}"))?;

            // Yield files in the loop until the underlying iterator is done.
            let Some(file) = file else { break };
            yield file;
        }
    }
}

/// Returns the canonical age of a file.
///
/// Uses the file's last modified time (if possible, falling back to the creation time)
/// and then compares that to the current system time.
///
/// If anything in that chain fails, one or more warnings are emitted
/// and the file age is assumed to be "zero" (meaning it is assumed to have just been created).
///
/// # Failures
///
/// File modification time can fail if the system does not provide a file modification time API,
/// or if the file system does not support modification times.
///
/// File creation time can fail if the system does not provide a file modification time API,
/// or if the file system does not support creation times.
///
/// File age comparison fails if _both_ of the above fail, _or_ if the current system time
/// is earlier than the canonical modification time
/// (this usually is due to the system clock being messed with).
///
/// In all failure cases, the file's age is assumed to be zero.
fn file_age(metadata: &Metadata) -> Duration {
    metadata
        .modified()
        .ext_inspect_err(|err| warn!("file modified time not available on platform: {err}"))
        .or_else(|_| metadata.created())
        .ext_inspect_err(|err| warn!("file created time not available on platform: {err}"))
        .ok()
        .and_then(|modified| {
            let now = SystemTime::now();
            let elapsed = now.duration_since(modified);
            elapsed.ext_inspect_err(|err| warn!("unable to determine elapsed time between {modified:?} and {now:?} (possibly due to system clock drift): {err}")).ok()
        })
        .unwrap_or(Duration::ZERO)
}
