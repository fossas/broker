//! Tools to ensure that the fossa-cli exists and download it if it does not
use bytes::Bytes;
use error_stack::IntoReport;
use error_stack::{Result, ResultExt};
use indoc::indoc;
use std::fmt::Debug;
use std::fs::{self};
use std::io::copy;
use std::io::{Cursor, Read};
use std::path::PathBuf;
use std::process::Command;

use crate::ext::error_stack::{DescribeContext, ErrorHelper, IntoContext};
use crate::ext::result::WrapErr;
use crate::AppContext;

/// Errors while downloading fossa-cli
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Broker attempts to find the latest version of the CLI before downloading.
    /// It does this by checking the latest tag and parsing the redirect location.
    #[error("find latest FOSSA CLI version")]
    FindVersion,

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
}

/// Ensure that the fossa cli exists and return its path, preferring fossa in config_dir/fossa over fossa in your path.
/// If we find it in config_dir/fossa, then return that.
/// If we find `fossa` in your path, then just return "fossa"
/// Otherwise, download the latest release, put it in `config_dir/fossa` and return that
#[tracing::instrument]
pub async fn find_or_download(ctx: &AppContext) -> Result<PathBuf, Error> {
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

    let latest_release_version = latest_release_version().await?;
    match current_path {
        Some(current_path) => download_if_old(&ctx, current_path, latest_release_version).await,
        None => download(ctx, latest_release_version).await,
    }
}

/// If the cli exists locally, then compare the version of the local CLI to the latest release,
/// and download it if it is different.
/// If there are any errors while finding the local version, then just download it.
#[tracing::instrument]
async fn download_if_old(
    ctx: &AppContext,
    current_path: PathBuf,
    latest_release_version: String,
) -> Result<PathBuf, Error> {
    if let Ok(local_version) = local_version(&current_path).await {
        if local_version == latest_release_version {
            Ok(current_path)
        } else {
            download(ctx, latest_release_version).await
        }
    } else {
        download(ctx, latest_release_version).await
    }
}

#[tracing::instrument]
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
    println!("output from fossa --version: {}", output);

    let version = output
        .strip_prefix("fossa-cli version ")
        .ok_or(Error::ParseLocalFossaVersion(output.clone()))?;
    let version = version
        .split(' ')
        .next()
        .ok_or(Error::ParseLocalFossaVersion(output.clone()))?;

    Ok(version.to_string())
}

/// Given a path to a possible fossa executable, return whether or not it successfully runs
/// "fossa --version"
#[tracing::instrument]
async fn check_command_existence(command_path: &PathBuf) -> bool {
    Command::new(command_path)
        .arg("--version")
        .output()
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
        .ok_or(Error::ParseRedirect(String::from(path)))
        .context(Error::FindVersion)
        .describe("uses the 'latest' pseudo-tag on Github to determine the tag representing the latest release")?;

    if !tag.starts_with('v') {
        return Error::DeterminedTagFormat(String::from(tag))
            .wrap_err()
            .context(Error::FindVersion);
    }
    Ok(tag.trim_start_matches('v').to_string())
}

/// Download the CLI into the config_dir
#[tracing::instrument]
async fn download(ctx: &AppContext, version: String) -> Result<PathBuf, Error> {
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
fn download_url(version: String) -> String {
    format!("https://github.com/fossas/fossa-cli/releases/download/v{version}/fossa_{version}_windows_amd64.zip")
}

#[cfg(target_os = "macos")]
fn download_url(version: String) -> String {
    format!("https://github.com/fossas/fossa-cli/releases/download/v{version}/fossa_{version}_darwin_amd64.zip")
}

#[cfg(target_os = "linux")]
fn download_url(version: String) -> String {
    format!("https://github.com/fossas/fossa-cli/releases/download/v{version}/fossa_{version}_linux_amd64.zip")
}

#[tracing::instrument]
async fn download_from_github(version: String) -> Result<Cursor<Bytes>, Error> {
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
        .change_context(Error::FinalCopy(final_path_string.clone()))?;
    copy(&mut zip_file, &mut final_file)
        .into_report()
        .change_context(Error::FinalCopy(final_path_string))?;
    Ok(())
}
