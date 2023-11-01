//! Module to download and interact with FOSSA CLI.

use bytes::Bytes;
use cached::proc_macro::cached;
use error_stack::{bail, report};
use error_stack::{Result, ResultExt};
use futures::future::try_join3;
use indoc::formatdoc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt::Debug;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use tempfile::tempdir;
use tokio::fs;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tracing::{debug, warn};

use crate::ext::command::{Command, CommandDescriber, OutputProvider};
use crate::ext::error_stack::{DescribeContext, ErrorHelper, IntoContext};
use crate::ext::io::{spawn_blocking, spawn_blocking_wrap};
use crate::ext::result::DiscardResult;
use crate::ext::result::{WrapErr, WrapOk};
use crate::ext::tracing::span_record;
use crate::{debug, AppContext};

/// Errors while downloading fossa-cli
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Broker attempts to find the latest version of the CLI before downloading.
    /// It does this by checking the latest tag and parsing the redirect location.
    #[error("find latest FOSSA CLI version")]
    FindVersion,

    /// When running FOSSA CLI, we create a temporary directory to hold the debug bundle.
    /// If creating this directory fails, this error is returned.
    #[error("create temporary directory for debug bundle in {}", .0.display())]
    CreateTempDir(PathBuf),

    /// This module shells out to FOSSA CLI, and that failed.
    #[error("run FOSSA CLI: {}", .0.trim())]
    Execution(String),

    /// Broker parses the redirect location from the 'latest' pseudo-tag to determine
    /// the correct tag representing 'latest'. If that fails to parse, this error occurs.
    #[error("parse 'latest' pseudo-tag redirect: '{0}'")]
    ParseRedirect(String),

    /// If we find a local fossa, then we run `fossa --version` and parse the output to
    /// find the current version
    #[error("parse FOSSA CLI version")]
    ParseVersion,

    /// The output of `fossa --version` isn't in the expected format.
    /// We expect the format 'fossa-cli version 3.7.1 (revision 3014137291f9 compiled with ghc-9.0)',
    /// and look for the '3.7.1' in that string.
    #[error("FOSSA CLI version format unexpected")]
    VersionOutputFormat,

    /// If the determined tag doesn't start with 'v', something went wrong in the parse.
    #[error("expected tag to start with 'v', but got '{0}'")]
    DeterminedTagFormat(String),

    /// Once Broker determines the correct version, it downloads it from Github.
    #[error("download FOSSA CLI from github")]
    Download,

    /// Once FOSSA CLI is downloaded, Broker must extract it from an archive into a tmpfile
    #[error("extract FOSSA CLI archive")]
    Extract,

    /// The final step is to copy the file from the tmpfile into its final location
    #[error("copy FOSSA CLI to final location '{0}'")]
    FinalCopy(String),

    /// Encountered if there are errors reading the CLI output.
    ///
    /// This error is distinct from `ParseOutput` in that this error is related to
    /// specifically IO errors when _reading_ the output.
    #[error("read FOSSA CLI output")]
    ReadOutput,

    /// Encountered if there are errors parsing the CLI output.
    ///
    /// This error is distinct from `ReadOutput` in that this error is related to
    /// specifically parse errors after output has been fully read.
    #[error("parse FOSSA CLI output: '{0}'")]
    ParseOutput(String),
}

impl Error {
    fn create_temp_dir() -> Self {
        Self::CreateTempDir(std::env::temp_dir())
    }

    fn running_cli<D: CommandDescriber>(describer: D) -> Self {
        Self::Execution(describer.describe().to_string())
    }
}

/// Which version of the fossa-cli you want to download.
/// Currently, this is always the latest version
#[derive(Debug, Clone)]
pub enum DesiredVersion {
    /// The latest version of the fossa-cli
    Latest,
    // In the future...
    // Tagged(String), // Tag name
}

/// The result of running an analysis with FOSSA CLI.
#[derive(Debug, serde::Deserialize)]
struct AnalysisResult {
    /// The source unit output of the analysis.
    #[serde(rename = "sourceUnits")]
    source_units: Value,
}

impl From<AnalysisResult> for SourceUnits {
    fn from(value: AnalysisResult) -> Self {
        Self(value.source_units)
    }
}

/// FOSSA CLI outputs "source units".
///
/// Each source unit is a dependency graph in a specific format.
/// Broker doesn't actually inspect these units, it just passes them through.
#[derive(Debug, derive_more::Display)]
pub struct SourceUnits(Value);

impl SourceUnits {
    /// Check whether the source units were empty.
    pub fn is_empty(&self) -> bool {
        self.0.as_array().map(|arr| arr.is_empty()).unwrap_or(true)
    }
}

impl<'de> Deserialize<'de> for SourceUnits {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Value::deserialize(deserializer).map(Self)
    }
}

impl Serialize for SourceUnits {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

/// The FOSSA CLI version.
#[derive(Debug, Clone, derive_more::Display, PartialEq, Eq)]
pub struct Version(semver::Version);

impl Version {
    /// Parse the version from the output of `fossa --version`.
    fn parse(raw_version: String) -> Result<Self, Error> {
        raw_version
            .strip_prefix("fossa-cli version ")
            .and_then(|version| version.split(' ').next())
            .ok_or_else(|| report!(Error::VersionOutputFormat))
            .and_then(|version| semver::Version::parse(version).context(Error::ParseVersion))
            .map(Self)
    }
}

impl<'de> Deserialize<'de> for Version {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        semver::Version::parse(&raw)
            .map_err(serde::de::Error::custom)
            .map(Self)
    }
}

impl Serialize for Version {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

/// A reference to the FOSSA CLI binary and other
/// information used at runtime, for example where to store debug bundles.
///
/// Allows easy disambiguation between another arbitrary path variable,
/// and allows easy methods to run the CLI.
#[derive(Debug)]
pub struct Location {
    cli: PathBuf,
    artifacts: debug::Root,
}

impl Location {
    /// Create a new instance with the CLI asserted at the provided path,
    /// storing debug artifacts in the provided debug root.
    ///
    /// It is recommended that most users use `find_or_download` instead.
    #[tracing::instrument]
    pub fn new(path: PathBuf, artifact_root: &debug::Root) -> Self {
        Self {
            cli: path,
            artifacts: artifact_root.to_owned(),
        }
    }

    /// Report the version of FOSSA CLI.
    #[tracing::instrument]
    pub async fn version(&self) -> Result<Version, Error> {
        local_version(&self.cli).await
    }

    /// Analyze a project with FOSSA CLI, returning the unparsed `sourceUnits` output.
    ///
    /// FOSSA CLI log output is streamed into the traces for this function as `trace` logs.
    /// It also automatically places the debug bundle in the appropriate location for the scan.
    #[tracing::instrument]
    pub async fn analyze(&self, scan_id: &str, project: &Path) -> Result<SourceUnits, Error> {
        let tmp = tempdir().context_lazy(Error::create_temp_dir)?;

        // Set the CLI to run in the temporary directory so that it creates the debug bundle there,
        // but pass it the location of the project to analyze.
        //
        // Use spawn instead of output so that we can stream the output;
        // this way trace events are recorded at the time the CLI actually logs them
        // instead of all at once at the end.
        // The hope is that this will improve debugging, as we'll be able to see timings and partial output.
        //
        // We clear the env so that dynamic analysis strategies don't run.
        // This is intended to make Broker more predictable: users aren't surprised by
        // the presence or absence of build tools on their local system.
        // In the future FOSSA CLI may have an option to enable this more gracefully;
        // in such case we'll switch to that.
        let cmd = Command::new(&self.cli)
            .env_remove("PATH")
            .current_dir(tmp.path())
            .arg_plain("analyze")
            .arg_plain("--debug")
            .arg_plain("--output")
            .arg_plain(project.to_string_lossy());
        let mut stream = cmd.stream().context_lazy(|| Error::running_cli(&cmd))?;
        let redacter = stream.redacter();

        // We need to parse stdout, so just pipe that into a buffer.
        let mut stdout = stream.take_stdout();
        let stdout_reader = async {
            let mut buf = String::new();
            stdout
                .read_to_string(&mut buf)
                .await
                .context_lazy(|| Error::running_cli(&cmd))
                .change_context(Error::ReadOutput)?;
            redacter.redact_str(&buf).wrap_ok()
        };

        // Read stderr of the child process and log it as trace events.
        // Additionally buffer it so we can report it in the case of an error.
        let stderr = stream.take_stderr();
        let stderr_reader = async {
            let mut buf = String::new();
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();

            while let Some(line) = lines
                .next_line()
                .await
                .context_lazy(|| Error::running_cli(&cmd))
                .change_context(Error::ReadOutput)?
            {
                let line = redacter.redact_str(&line);
                buf.push_str(&line);
                buf.push('\n');
                tracing::trace!(message = %line, cmd = "fossa-cli", cmd_context = "stderr");
            }
            Ok(buf)
        };

        // Wait for all three futures to complete: both readers and the child process itself.
        let waiter = async {
            stream
                .wait()
                .await
                .context_lazy(|| Error::running_cli(&cmd))
        };
        let (stdout, stderr, status) = try_join3(stdout_reader, stderr_reader, waiter).await?;

        // If the child process exited with a non-zero status, then return the error.
        if !status.success() {
            let description = stream.describe().with_stderr(stderr).with_stdout(stdout);
            let description = match status.code() {
                Some(code) => description.with_status(code),
                None => description,
            };

            bail!(Error::Execution(description.to_string()));
        }

        // Copy the debug bundle to the correct location.
        // Don't error the process if this fails, as it's not critical to the scan process.
        // We're copying instead of moving because on Linux, it's likely these are at different mount points.
        let debug_bundle = tmp.path().join("fossa.debug.json.gz");
        let destination = self.artifacts.debug_bundle(scan_id);
        if let Err(err) = fs::create_dir_all(self.artifacts.as_path()).await {
            warn!(
                "failed to create FOSSA CLI debug bundle location {:?}: {err:#}",
                self.artifacts.as_path()
            );
        }

        match fs::copy(&debug_bundle, &destination).await {
            Ok(_) => debug!("stored FOSSA CLI debug bundle at {destination:?}"),
            Err(err) => {
                warn!("failed to store FOSSA CLI debug bundle at {destination:?}: {err:#}")
            }
        }

        // Parse the output. We only care about source units.
        serde_json::from_str::<AnalysisResult>(&stdout)
            .context_lazy(|| Error::running_cli(&cmd))
            .change_context_lazy(|| Error::ParseOutput(stdout.clone()))
            .map(SourceUnits::from)
    }
}

/// Find the location of the FOSSA CLI, downloading it if it doesn't exist or is outdated.
/// If it is downloaded, it is placed in the data root of the provided [`AppContext`].
///
/// When the CLI is run, debug bundles are automatically placed in the provided `artifact_root`,
/// based on the provided `scan_id` at the time of running FOSSA CLI.
/// For this reason, it is recommended to use methods on the returned [`Location`] to run the CLI
/// instead of trying to run the CLI by path directly.
#[tracing::instrument]
pub async fn find_or_download(
    ctx: &AppContext,
    artifact_root: &debug::Root,
    desired_version: DesiredVersion,
) -> Result<Location, Error> {
    // default to fossa that lives in the data root
    // if it does not exist in the data root, then check to see if it is on the path
    let command = command_name();
    let command_in_config_dir = ctx.data_root().join(command);
    let current_path: Option<PathBuf> = if check_command_existence(&command_in_config_dir).await {
        Some(command_in_config_dir)
    } else {
        match find_in_path(command).await {
            Ok(path) => Some(path),
            Err(err) => {
                debug!("failed to find {command} in path: {err:?}");
                None
            }
        }
    };

    // If the CLI isn't already local, download it.
    let Some(current_path) = current_path else {
        return download(ctx, artifact_root, desired_version).await;
    };

    // Now we know the CLI exists locally, check if it matches the desired version.
    // If so, use its path. If not, download the desired version and use it.
    let resolved_version = resolve_version(&desired_version).await?;
    match local_version(&current_path).await {
        Ok(local_version) if local_version.to_string() == resolved_version => {
            debug!(
                "local version of fossa-cli at {} matches desired version of {}",
                current_path.display(),
                resolved_version
            );
            Location::new(current_path, artifact_root).wrap_ok()
        }
        Ok(local_version) => {
            debug!(
                "local version of fossa-cli at {} has version of {}, which does not match desired version of {}. Downloading new version.",
                current_path.display(),
                local_version,
                resolved_version,
            );
            download(ctx, artifact_root, desired_version).await
        }
        Err(err) => {
            debug!(
                "Error while getting version from local fossa-cli at {}: {err:#}. Downloading new version",
                current_path.display()
            );
            download(ctx, artifact_root, desired_version).await
        }
    }
}

/// Download FOSSA CLI, placing it in the data root of the provided [`AppContext`].
///
/// When the CLI is run, debug bundles are automatically placed in the provided `artifact_root`,
/// based on the provided `scan_id` at the time of running FOSSA CLI.
/// For this reason, it is recommended to use methods on the returned [`Location`] to run the CLI
/// instead of trying to run the CLI by path directly.
#[tracing::instrument]
pub async fn download(
    ctx: &AppContext,
    artifact_root: &debug::Root,
    desired_version: DesiredVersion,
) -> Result<Location, Error> {
    let resolved_version = resolve_version(&desired_version).await?;
    let path = download_tag(ctx, &resolved_version).await?;
    Location::new(path, artifact_root).wrap_ok()
}

/// Resolve a [`DesiredVersion`] to a concrete version.
async fn resolve_version(desired_version: &DesiredVersion) -> Result<String, Error> {
    match desired_version {
        DesiredVersion::Latest => latest_release_version().await,
    }
}

#[tracing::instrument(fields(fossa_version_output))]
async fn local_version(current_path: &PathBuf) -> Result<Version, Error> {
    let cmd = Command::new(current_path).arg_plain("--version");
    let output = cmd
        .output()
        .await
        .context_lazy(|| Error::running_cli(&cmd))?;
    if !output.status().success() {
        bail!(Error::running_cli(&output));
    }

    // the output will look something like "fossa-cli version 3.7.2 (revision 49a37c0147dc compiled with ghc-9.0)"
    let raw_version = output.stdout_string_lossy();
    span_record!(fossa_version_output, debug raw_version);

    Version::parse(raw_version).change_context_lazy(|| Error::running_cli(&output))
}

/// Find the path to the command in `$PATH`.
async fn find_in_path(command: &str) -> Result<PathBuf, Error> {
    let command = command.to_string();
    spawn_blocking_wrap(|| which::which(command))
        .await
        .change_context(Error::FindVersion)
}

/// Given a path to a possible fossa executable, return whether or not it successfully runs
/// "fossa --version"
#[tracing::instrument]
async fn check_command_existence(command_path: &PathBuf) -> bool {
    Command::new(command_path)
        .arg_plain("--version")
        .output()
        .await
        .map(|out| out.status().success())
        .unwrap_or(false)
}

/// "fossa.exe" on windows and "fossa" on all other platforms
#[cfg(target_family = "windows")]
fn command_name() -> &'static str {
    "fossa.exe"
}

/// "fossa.exe" on windows and "fossa" on all other platforms
#[cfg(target_family = "unix")]
fn command_name() -> &'static str {
    "fossa"
}

/// Get the version of the latest release on GitHub.
#[tracing::instrument]
#[cached(time = 3600, sync_writes = true, result = true)]
pub async fn latest_release_version() -> Result<String, Error> {
    let client = reqwest::Client::new();
    // This will follow the redirect, so latest_release_response.url().path() will be something like "/fossas/fossa-cli/releases/tag/v3.7.2"
    let latest_release_response = client
        .get("https://github.com/fossas/fossa-cli/releases/latest")
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .context(Error::FindVersion)
        .describe("uses Github's 'latest' pseudo-tag to determine the latest release")?;
    let path = latest_release_response.url().path();

    let tag = path
        .rsplit('/')
        .next()
        .ok_or_else(|| Error::ParseRedirect(String::from(path)))
        .context(Error::FindVersion)
        .describe("uses the 'latest' pseudo-tag on Github to determine the tag representing the latest release")?;

    if !tag.starts_with('v') {
        return Error::DeterminedTagFormat(String::from(tag))
            .wrap_err()
            .context(Error::FindVersion);
    }
    tag.trim_start_matches('v').to_string().wrap_ok()
}

/// Download the CLI into the config_dir
#[tracing::instrument]
async fn download_tag(ctx: &AppContext, version: &str) -> Result<PathBuf, Error> {
    let content = download_from_github(version).await?;

    let final_path = ctx.data_root().join(command_name());
    spawn_blocking(move || unzip_zip(content, final_path))
        .await
        .change_context(Error::Download)
}

// currently supported os/arch combos:
// darwin/amd64
// linux/amd64
// windows/amd64
//
// We only support "amd64" right now, so no need to look at target_arch
// Example URLs:
// https://github.com/fossas/fossa-cli/releases/download/v3.7.2/fossa_3.7.2_windows_amd64.zip
// https://github.com/fossas/fossa-cli/releases/download/v3.7.2/fossa_3.7.2_darwin_amd64.zip
// https://github.com/fossas/fossa-cli/releases/download/v3.7.2/fossa_3.7.2_linux_amd64.zip
#[cfg(target_os = "windows")]
fn download_url(version: &str) -> String {
    format!("https://github.com/fossas/fossa-cli/releases/download/v{version}/fossa_{version}_windows_amd64.zip")
}

#[cfg(target_os = "macos")]
fn download_url(version: &str) -> String {
    format!("https://github.com/fossas/fossa-cli/releases/download/v{version}/fossa_{version}_darwin_amd64.zip")
}

#[cfg(target_os = "linux")]
fn download_url(version: &str) -> String {
    format!("https://github.com/fossas/fossa-cli/releases/download/v{version}/fossa_{version}_linux_amd64.zip")
}

#[tracing::instrument]
async fn download_from_github(version: &str) -> Result<Cursor<Bytes>, Error> {
    let download_url = download_url(version);
    let client = reqwest::Client::new();
    let response = client
        .get(&download_url)
        .send()
        .await
        .context(Error::Download)
        .help_lazy(|| formatdoc!{"
            Try downloading FOSSA CLI from '{download_url}' to determine if this is an issue with the local network.
            You also may be able to work around this issue by using the installation script for FOSSA CLI,
            located at https://github.com/fossas/fossa-cli#installation
            "}
        )?;

    let content = response
        .bytes()
        .await
        .context(Error::Download)
        .help_lazy(|| formatdoc!{"
            Try downloading FOSSA CLI from '{download_url}' to determine if this is an issue with the local network.
            You also may be able to work around this issue by using the installation script for FOSSA CLI,
            located at https://github.com/fossas/fossa-cli#installation
            "}
        )?;
    let content = Cursor::new(content);
    Ok(content)
}

#[tracing::instrument(skip(content))]
fn unzip_zip(content: Cursor<Bytes>, final_path: PathBuf) -> Result<PathBuf, Error> {
    let mut archive = zip::ZipArchive::new(content)
        .context(Error::Extract)
        .describe("extracting zip file from downloaded fossa release")?;
    let zip_file = match archive.by_name(command_name()) {
        Ok(file) => file,
        Err(..) => {
            return report!(Error::Extract)
                .wrap_err()
                .describe("finding fossa executable in downloaded fossa release");
        }
    };

    write_zip_to_final_file(zip_file, &final_path)
        .change_context(Error::Extract)
        .map(|_| final_path)
}

/// On unix we need to set the mode to 0o770 so that it is executable
/// On windows we cannot (and do not need to) do this
#[tracing::instrument(skip(zip_file))]
#[cfg(target_family = "unix")]
fn write_zip_to_final_file<R>(mut zip_file: R, final_path: &PathBuf) -> Result<(), Error>
where
    R: Read,
{
    use std::os::unix::fs::OpenOptionsExt;
    let final_path_string = final_path.to_str().unwrap_or("").to_string();
    let mut final_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .mode(0o770)
        .open(final_path)
        .context_lazy(|| Error::FinalCopy(final_path_string.clone()))?;

    std::io::copy(&mut zip_file, &mut final_file)
        .context_lazy(|| Error::FinalCopy(final_path_string.clone()))
        .discard_ok()
}

#[tracing::instrument(skip(zip_file))]
#[cfg(target_family = "windows")]
fn write_zip_to_final_file<R>(mut zip_file: R, final_path: &PathBuf) -> Result<(), Error>
where
    R: Read,
{
    let final_path_string = final_path.to_str().unwrap_or("").to_string();
    let mut final_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(final_path)
        .context_lazy(|| Error::FinalCopy(final_path_string.clone()))?;

    std::io::copy(&mut zip_file, &mut final_file)
        .context_lazy(|| Error::FinalCopy(final_path_string.clone()))
        .discard_ok()
}
