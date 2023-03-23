//! Tools to ensure that the fossa-cli exists and download it if it does not
use bytes::Bytes;
use error_stack::{bail, IntoReport};
use error_stack::{Result, ResultExt};
use futures::future::try_join3;
use indoc::indoc;
use semver::Version;
use std::fmt::Debug;
use std::fs;
use std::io::copy;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tempfile::tempdir;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;
use tracing::debug;

use crate::ext::command::DescribeCommand;
use crate::ext::error_stack::{DescribeContext, ErrorHelper, IntoContext};
use crate::ext::result::{WrapErr, WrapOk};
use crate::ext::tracing::span_record;
use crate::AppContext;

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
    #[error("run command: {}", str::trim(.0))]
    Execution(String),

    /// Broker parses the redirect location from the 'latest' pseudo-tag to determine
    /// the correct tag representing 'latest'. If that fails to parse, this error occurs.
    #[error("parse 'latest' pseudo-tag redirect: '{0}'")]
    ParseRedirect(String),

    /// If we find a local fossa, then we run `fossa --version`.
    /// This error is returned if that fails
    #[error("run local fossa --version")]
    RunLocalFossaVersion,

    /// If we find a local fossa, then we run `fossa --version` and parse the output to
    /// find the current version
    #[error("parse local fossa --version")]
    ParseLocalFossaVersion(String),

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
    fn running_cli(cmd: &Command) -> Self {
        Self::Execution(cmd.describe().to_string())
    }

    fn create_temp_dir() -> Self {
        Self::CreateTempDir(std::env::temp_dir())
    }
}

/// Which version of the fossa-cli you want to download.
/// Currently, this is always the latest version
#[derive(Debug)]
pub enum DesiredVersion {
    /// The latest version of the fossa-cli
    Latest,
    // In the future...
    // Tagged(String), // Tag name
}

/// Ensure that the fossa cli exists and return its path, preferring fossa in config_dir/fossa over fossa in your path.
/// If we find it in config_dir/fossa, then return that.
/// If we find `fossa` in your path, then just return "fossa"
/// Otherwise, download the latest release, put it in `config_dir/fossa` and return that
// #[tracing::instrument]
// pub async fn find_or_download(ctx: &AppContext) -> Result<Location, Error> {
//     let command = command_name();

//     // default to fossa that lives directly in the data root.
//     let command_in_config_dir = ctx.data_root().join(command);
//     if check_command_existence(&command_in_config_dir).await {
//         return Ok(command_in_config_dir.into());
//     }

//     // if it does not exist in the data root, then check to see if it is on the path.
//     if check_command_existence(&PathBuf::from(&command)).await {
//         return Ok(PathBuf::from(command).into());
//     };

//     // if it is not in either location, then download it
//     download(ctx.data_root())
//         .await
//         .describe("fossa-cli not found in your path, attempting to download it")
//         .map(Location::new)
// }

/// The result of running an analysis with FOSSA CLI.
#[derive(Debug, serde::Deserialize)]
struct AnalysisResult {
    /// The source unit output of the analysis.
    #[serde(rename = "sourceUnits")]
    source_units: String,
}

/// A reference to the FOSSA CLI binary.
///
/// Allows easy disambiguation between another arbitrary path variable,
/// and allows easy methods to run the CLI.
#[derive(Debug)]
pub struct Location {
    path: PathBuf,
}

impl Location {
    /// Report the version of FOSSA CLI.
    #[tracing::instrument]
    pub async fn version(&self) -> Result<String, Error> {
        let mut cmd = self.cmd();
        cmd.arg("--version");
        let output = cmd
            .output()
            .await
            .context_lazy(|| Error::running_cli(&cmd))?;

        String::from_utf8_lossy(&output.stdout)
            .to_string()
            .wrap_ok()
    }

    /// Analyze a project with FOSSA CLI, returning the unparsed `sourceUnits` output.
    ///
    /// FOSSA CLI log output is streamed into the traces for this function as `trace` logs.
    /// It also automatically places the debug bundle in the appropriate location for the scan.
    #[tracing::instrument]
    pub async fn analyze(&self, scan_id: &str, project: &Path) -> Result<String, Error> {
        let tmp = tempdir().context_lazy(Error::create_temp_dir)?;

        // Set the CLI to run in the temporary directory so that it creates the debug bundle there,
        // but pass it the location of the project to analyze.
        //
        // Use spawn instead of output so that we can stream the output;
        // this way trace events are recorded at the time the CLI actually logs them
        // instead of all at once at the end.
        // The hope is that this will improve debugging, as we'll be able to see timings and partial output.
        let mut cmd = self.cmd();
        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(tmp.path())
            .arg("analyze")
            .arg("--debug")
            .arg("--output")
            .arg(project);
        let mut child = cmd.spawn().context_lazy(|| Error::running_cli(&cmd))?;

        // We need to parse stdout, so just pipe that into a buffer.
        let Some(mut stdout) = child.stdout.take() else { panic!("stdout must be piped") };
        let stdout_reader = async {
            let mut buf = String::new();
            stdout
                .read_to_string(&mut buf)
                .await
                .context(Error::ReadOutput)?;
            Ok(buf)
        };

        // Read stderr of the child process and log it as trace events.
        // Additionally buffer it so we can report it in the case of an error.
        let Some(stderr) = child.stderr.take() else { panic!("stderr must be piped") };
        let stderr_reader = async {
            let mut buf = String::new();
            let stream = BufReader::new(stderr);
            let mut lines = stream.lines();
            while let Some(line) = lines.next_line().await.context(Error::ReadOutput)? {
                buf.push_str(&line);
                buf.push('\n');
                tracing::trace!(message = %line, cmd = "fossa-cli", cmd_context = "stderr");
            }
            Ok(buf)
        };

        // Wait for all three futures to complete: both readers and the child process itself.
        let waiter = async { child.wait().await.context_lazy(|| Error::running_cli(&cmd)) };
        let (stdout, stderr, status) = try_join3(stdout_reader, stderr_reader, waiter).await?;

        // If the child process exited with a non-zero status, then return the error.
        if !status.success() {
            let description = cmd.describe().with_stderr(stderr).with_stdout(stdout);
            let description = match status.code() {
                Some(code) => description.with_status(code),
                None => description,
            };

            bail!(Error::Execution(description.to_string()));
        }

        // Parse the output. We only care about source units.
        serde_json::from_str::<AnalysisResult>(&stdout)
            .context_lazy(|| Error::ParseOutput(stdout.clone()))
            .map(|result| result.source_units)
    }

    fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn cmd(&self) -> Command {
        // If we drop the future driving the command, there's no reason to keep the command running.
        let mut cmd = Command::new(&self.path);
        cmd.kill_on_drop(true);
        cmd
    }
}

impl From<PathBuf> for Location {
    fn from(val: PathBuf) -> Self {
        Location::new(val)
    }
}

pub async fn find_or_download(
    ctx: &AppContext,
    desired_version: DesiredVersion,
) -> Result<PathBuf, Error> {
    let command = command_name();

    // default to fossa that lives in the data root
    // if it does not exist in the data root, then check to see if it is on the path
    let command_in_config_dir = ctx.data_root().join(command);
    let current_path: Option<PathBuf> = if check_command_existence(&command_in_config_dir).await {
        Some(command_in_config_dir)
    } else if check_command_existence(&PathBuf::from(&command)).await {
        Some(PathBuf::from(command))
    } else {
        None
    };

    let desired_version = latest_release_version().await?;
    match current_path {
        Some(current_path) => download_if_old(ctx, current_path, &desired_version).await,
        None => download(ctx, &desired_version).await,
    }
}

/// If the cli exists locally, then compare the version of the local CLI to the latest release,
/// and download it if it is different.
/// If there are any errors while finding the local version, then just download it.
#[tracing::instrument]
async fn download_if_old(
    ctx: &AppContext,
    current_path: PathBuf,
    desired_version: &str,
) -> Result<PathBuf, Error> {
    if let Ok(local_version) = local_version(&current_path).await {
        if local_version == desired_version {
            debug!(
                "local version of fossa-cli at {} matches desired version of {}",
                current_path.display(),
                desired_version
            );
            Ok(current_path)
        } else {
            debug!(
                "local version of fossa-cli at {} has version of {}, which does not match desired version of {}. Downloading new version.",
                current_path.display(),
                local_version,
                desired_version,
            );
            download(ctx, desired_version).await
        }
    } else {
        debug!(
            "Error while getting version from local fossa-cli at {}. Downloading new version",
            current_path.display()
        );
        download(ctx, desired_version).await
    }
}

#[tracing::instrument(fields(fossa_version_output))]
async fn local_version(current_path: &PathBuf) -> Result<String, Error> {
    let output = Command::new(current_path)
        .arg("--version")
        .output()
        .context(Error::RunLocalFossaVersion)?;
    if !output.status.success() {
        return Error::RunLocalFossaVersion.wrap_err().into_report();
    }

    // the output will look something like "fossa-cli version 3.7.2 (revision 49a37c0147dc compiled with ghc-9.0)"
    let output = String::from_utf8(output.stdout).context(Error::RunLocalFossaVersion)?;
    span_record!(fossa_version_output, debug output);

    let version = output
        .strip_prefix("fossa-cli version ")
        .ok_or_else(|| Error::ParseLocalFossaVersion(output.clone()))?;
    let version = version
        .split(' ')
        .next()
        .ok_or_else(|| Error::ParseLocalFossaVersion(output.clone()))?;

    // The string we found should be a valid version
    Version::parse(version)
        .context_lazy(|| Error::ParseLocalFossaVersion(output.clone()))
        .map(|version| version.to_string())
}

/// Given a path to a possible fossa executable, return whether or not it successfully runs
/// "fossa --version"
#[tracing::instrument]
async fn check_command_existence(command_path: &PathBuf) -> bool {
    Command::new(command_path)
        // If we drop the future driving the command, there's no reason to keep the command running.
        .kill_on_drop(true)
        .arg("--version")
        .output()
        .await
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// command_name is "fossa.exe" on windows and "fossa" on all other platforms
#[tracing::instrument]
#[cfg(target_family = "windows")]
fn command_name() -> &'static str {
    "fossa.exe"
}

#[tracing::instrument]
#[cfg(target_family = "unix")]
fn command_name() -> &'static str {
    "fossa"
}

/// Get the version of the latest release on GitHub
#[tracing::instrument]
async fn latest_release_version() -> Result<String, Error> {
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
async fn download(ctx: &AppContext, version: &str) -> Result<PathBuf, Error> {
    let content = download_from_github(version).await?;

    let final_path = ctx.data_root().join(command_name());
    unzip_zip(content, &final_path).await?;
    Ok(final_path)
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
    let client = reqwest::Client::new();
    let response = client
        .get(download_url(version))
        .send()
        .await
        .into_report()
        .change_context(Error::Download)
        .help_lazy(|| indoc!{"
            Try downloading FOSSA CLI from '{download_url}' to determine if this is an issue with the local network.
            You also may be able to work around this issue by using the installation script for FOSSA CLI,
            located at https://github.com/fossas/fossa-cli#installation
            "}
        )?;

    let content = response
        .bytes()
        .await
        .into_report()
        .change_context(Error::Download)
        .help_lazy(|| indoc!{"
            Try downloading FOSSA CLI from '{download_url}' to determine if this is an issue with the local network.
            You also may be able to work around this issue by using the installation script for FOSSA CLI,
            located at https://github.com/fossas/fossa-cli#installation
            "}
        )?;
    let content = Cursor::new(content);
    Ok(content)
}

#[tracing::instrument(skip(content))]
async fn unzip_zip(content: Cursor<Bytes>, final_path: &PathBuf) -> Result<(), Error> {
    let mut archive = zip::ZipArchive::new(content)
        .context(Error::Extract)
        .describe("extracting zip file from downloaded fossa release")?;
    let zip_file = match archive.by_name(command_name()) {
        Ok(file) => file,
        Err(..) => {
            return Error::Extract
                .wrap_err()
                .into_report()
                .describe("finding fossa executable in downloaded fossa release");
        }
    };

    write_zip_to_final_file(zip_file, final_path)?;
    Ok(())
}

#[tracing::instrument(skip(zip_file))]
#[cfg(target_family = "windows")]
fn write_zip_to_final_file<R>(mut zip_file: R, final_path: &PathBuf) -> Result<(), Error>
where
    R: Read,
{
    let final_path_string = final_path.to_str().unwrap_or("").to_string();
    let mut final_file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(final_path)
        .into_report()
        .change_context_lazy(|| Error::FinalCopy(final_path_string.clone()))?;
    copy(&mut zip_file, &mut final_file)
        .into_report()
        .change_context_lazy(|| Error::FinalCopy(final_path_string))?;
    Ok(())
}

/// On unix we need to set the mode to 0o770 so that it is executable
/// On windows we cannot (and do not need to) do this
#[tracing::instrument(skip(zip_file))]
#[cfg(target_family = "unix")]
fn write_zip_to_final_file<R>(mut zip_file: R, final_path: &PathBuf) -> Result<(), Error>
where
    R: Read,
{
    use std::os::unix::prelude::OpenOptionsExt;
    let final_path_string = final_path.to_str().unwrap_or("").to_string();
    let mut final_file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .mode(0o770)
        .open(final_path)
        .into_report()
        .change_context_lazy(|| Error::FinalCopy(final_path_string.clone()))?;
    copy(&mut zip_file, &mut final_file)
        .into_report()
        .change_context_lazy(|| Error::FinalCopy(final_path_string))?;
    Ok(())
}
