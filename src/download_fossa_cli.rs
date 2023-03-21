//! Tools to ensure that the fossa-cli exists and download it if it does not
use bytes::Bytes;
use error_stack::IntoReport;
use error_stack::{Result, ResultExt};
use std::fs::{self};
use std::io::copy;
use std::io::Cursor;
use std::os::unix::prelude::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::ext::error_stack::{DescribeContext, IntoContext};
use crate::ext::io;

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
fn command_name() -> String {
    match std::env::consts::OS {
        "windows" => "fossa.exe".to_string(),
        _ => "fossa".to_string(),
    }
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
        .describe("Getting URL for latest release of the fossa CLI")?;
    let path = latest_release_response.url().path();

    let tag = path
        .rsplit('/')
        .next()
        .ok_or(Error::FindVersion)
        .into_report()
        .describe_lazy(|| format!("Parsing fossa-cli version from path {}", path))?;

    if !tag.starts_with('v') {
        return Err(Error::FindVersion)
            .into_report()
            .describe_lazy(|| format!(r#"Expected tag to start with v, but found "{tag}""#))?;
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
    unzip_zip(content, &final_path).await?;
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
        .describe_lazy(|| {
            format!("Error while downloading latest fossa release from {download_url}")
        })?;

    let content = response
        .bytes()
        .await
        .into_report()
        .change_context(Error::Download)
        .describe_lazy(|| {
            format!(
                "Error while converting download of fossa release from {download_url} into bytes"
            )
        })?;
    let content = Cursor::new(content);
    Ok(content)
}

#[tracing::instrument(skip(content))]
async fn unzip_zip(content: Cursor<Bytes>, final_path: &Path) -> Result<(), Error> {
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

    let mut final_file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .mode(0o770)
        .open(final_path)
        .into_report()
        .change_context(Error::Extract)
        .describe_lazy(|| {
            format!(
                "creating final file to write extracted zip to at {:?}",
                final_path
            )
        })?;
    copy(&mut file, &mut final_file)
        .into_report()
        .change_context(Error::Extract)
        .describe_lazy(|| format!("writing extracted zip file to {:?}", final_path))?;
    Ok(())
}
