//! Tools to ensure that the fossa-cli exists and download it if it does not
use bytes::Bytes;
use error_stack::IntoReport;
use error_stack::{Result, ResultExt};
use indoc::indoc;
use std::fs::{self};
use std::io::Cursor;
use std::io::{copy, Write};
use std::os::unix::prelude::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::{tempfile, NamedTempFile};

use crate::ext::error_stack::{DescribeContext, ErrorHelper, IntoContext};
use crate::ext::io;
use crate::ext::result::WrapErr;

/// Errors while downloading fossa-cli
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Errors while finding the config root
    #[error("find config root")]
    Config,

    /// Broker attempts to find the latest version of the CLI before downloading.
    /// It does this by checking the latest tag and parsing the redirect location.
    #[error("find latest FOSSA CLI version")]
    FindVersion,

    /// Broker parses the redirect location from the 'latest' psuedo-tag to determine
    /// the correct tag representing 'latest'. If that fails to parse, this error occurs.
    #[error("parse 'latest' psuedo-tag redirect: '{0}'")]
    ParseRedirect(String),

    /// If the determined tag doesn't start with 'v', something went wrong in the parse.
    #[error("expected tag to start with 'v', but got '{0}'")]
    DeterminedTagFormat(String),

    /// Once Broker determines the correct version, it downloads it from Github.
    #[error("download FOSSA CLI from github")]
    Download,

    /// Finally, once FOSSA CLI is downloaded, Broker must extract it from an archive.
    #[error("extract FOSSA CLI archive")]
    Extract,
}

/// Ensure that the fossa cli exists and return its path.
/// If we find it in config_dir/fossa, then return that
/// If we find `fossa` in your path, then just return "fossa"
/// Otherwise, download the latest release, put it in `config_dir/fossa` and return that
#[tracing::instrument]
pub async fn ensure_fossa_cli() -> Result<PathBuf, Error> {
    let command = command_name();
    let data_root = io::data_root().await.change_context(Error::Config)?;

    // default to fossa that lives in ~/.config/fossa/broker/fossa
    let command_in_config_dir = data_root.join(&command);
    if check_command_existence(&command_in_config_dir).await {
        return Ok(command_in_config_dir);
    }

    // if it does not exist in ~/.config/fossa/broker/fossa, then check to see if it is on the path
    if check_command_existence(&PathBuf::from(&command)).await {
        return Ok(PathBuf::from(command));
    };

    // if it is not in either location, then download it
    download(&data_root)
        .await
        .describe("fossa-cli not found in your path, attempting to download it")
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

/// Download the CLI into the config_dir
#[tracing::instrument]
async fn download(config_dir: &PathBuf) -> Result<PathBuf, Error> {
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
    let version = tag.trim_start_matches('v');

    // currently supported os/arch combos:
    // darwin/amd64
    // linux/amd64
    // windows/amd64
    //
    // We only support "amd64" right now, so no need to look at std::env::consts::ARCH
    let arch = "amd64";
    let extension = ".zip";
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "windows",
        _ => "linux",
    };

    // Example URLs:
    // https://github.com/fossas/fossa-cli/releases/download/v3.7.2/fossa_3.7.2_darwin_amd64.zip
    // https://github.com/fossas/fossa-cli/releases/download/v3.7.2/fossa_3.7.2_linux_amd64.zip
    let download_url = format!("https://github.com/fossas/fossa-cli/releases/download/v{version}/fossa_{version}_{os}_{arch}{extension}");
    let content = download_from_github(download_url).await?;

    let final_path = config_dir.join(command_name());
    let unzip_location = unzip_zip(content).await?;
    move_to_final_path(unzip_location, &final_path).await?;
    Ok(final_path)
}

#[tracing::instrument]
async fn download_from_github(download_url: String) -> Result<Cursor<Bytes>, Error> {
    let client = reqwest::Client::new();
    let response = client
        .get(&download_url)
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
async fn unzip_zip(content: Cursor<Bytes>) -> Result<PathBuf, Error> {
    let mut tmp_file = NamedTempFile::new()
        .context(Error::Extract)
        .describe("creating temp file to write unzipped file to")?;
    let mut archive = zip::ZipArchive::new(content)
        .context(Error::Extract)
        .describe("extracting zip file from downloaded fossa release")?;
    let mut file = match archive.by_name("fossa") {
        Ok(file) => file,
        Err(..) => {
            return Err(Error::Extract)
                .into_report()
                .describe("finding fossa executable in downloaded fossa release");
        }
    };

    copy(&mut file, &mut tmp_file)
        .into_report()
        .change_context(Error::Extract)
        .describe_lazy(|| format!("writing extracted zip file to {:?}", tmp_file))?;
    Ok(PathBuf::from(tmp_file.path()))
}

/// TODO: conditional function, depending on whether it's windows or Unix
/// On Windows, just copy to fossa.exe
/// On Unix, make it executable
async fn move_to_final_path(unzip_location: PathBuf, final_path: &PathBuf) -> Result<(), Error> {
    Ok(())
}
