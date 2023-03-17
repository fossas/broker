//! Tools to ensure that the fossa-cli exists and download it if it does not

use bytes::Bytes;
use error_stack::IntoReport;
use error_stack::{Result, ResultExt};
use flate2::bufread;
use std::fs::{self};
use std::io::copy;
use std::io::Cursor;
use std::os::unix::prelude::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use tar::Archive;
use tracing::info;

use crate::ext::error_stack::{DescribeContext, IntoContext};

/// Errors while downloading fossa-cli
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Error
    #[error("a fatal error occurred during internal configuration")]
    InternalSetup,
}

/// Ensure that the fossa cli exists
/// If we find `fossa` in your path, then just return "fossa"
/// If we find it in config_dir/fossa, then return that
/// Otherwise, download the latest release, put it in `config_dir/fossa` and return that
#[tracing::instrument]
pub async fn ensure_fossa_cli(config_dir: &PathBuf) -> Result<PathBuf, Error> {
    let command = command_name();
    if check_command_existence(&PathBuf::from(&command)) {
        info!("using fossa found in path");
        return Ok(PathBuf::from(command));
    };

    let command_in_config_dir = config_dir.join(command);
    if check_command_existence(&command_in_config_dir) {
        info!("Using already existing fossa in config dir");
        return Ok(command_in_config_dir);
    }

    info!("downloading latest release of fossa");
    download(config_dir)
        .await
        .change_context(Error::InternalSetup)
        .describe("fossa-cli not found in your path, so we attempted to download it")
}

#[tracing::instrument]
fn check_command_existence(command_path: &PathBuf) -> bool {
    let output = Command::new(command_path)
        .arg("--version")
        .output()
        .context(Error::InternalSetup)
        .describe("Unable to find `fossa` binary in your path");

    match output {
        Ok(output) => {
            if !output.status.success() {
                return false;
            }
            return true;
        }
        Err(_) => {
            return false;
        }
    }
}

/// command_name is "fossa.exe" on windows and "fossa" on all other platforms
#[tracing::instrument]
fn command_name() -> String {
    match std::env::consts::OS {
        "windows" => "fossa.exe".to_string(),
        _ => "fossa".to_string(),
    }
}

/// Download the CLI into the same directory as the config_path
#[tracing::instrument]
async fn download(config_dir: &PathBuf) -> Result<PathBuf, Error> {
    let client = reqwest::Client::new();
    // This will follow the redirect, so latest_release_response.url().path() will be something like "/fossas/fossa-cli/releases/tag/v3.7.2"
    let latest_release_response = client
        .get("https://github.com/fossas/fossa-cli/releases/latest")
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .context(Error::InternalSetup)
        .describe("Getting URL for latest release of the fossa CLI")?;
    let path = latest_release_response.url().path();
    let tag = path
        .rsplit("/")
        .next()
        .ok_or(Error::InternalSetup)
        .into_report()
        .describe_lazy(|| format!("Parsing fossa-cli version from path {}", path))?;
    if !tag.starts_with("v") {
        return Err(Error::InternalSetup).into_report()?;
    }
    let version = tag.trim_start_matches("v");

    // currently supported os/arch combos:
    // darwin/amd64
    // linux/amd64
    // windows/amd64
    //
    // We only support "amd64" right now, so no need to look at std::env::consts::ARCH
    let arch = "amd64";
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "windows",
        _ => "linux",
    };

    let extension = match os {
        "darwin" => ".zip",
        "windows" => ".zip",
        _ => ".tar.gz",
    };

    let final_path = config_dir.join(command_name());

    let download_url = format!("https://github.com/fossas/fossa-cli/releases/download/v{version}/fossa_{version}_{os}_{arch}{extension}");
    // Example URLs:
    // https://github.com/fossas/fossa-cli/releases/download/v3.7.2/fossa_3.7.2_darwin_amd64.zip
    // https://github.com/fossas/fossa-cli/releases/download/v3.7.2/fossa_3.7.2_linux_amd64.tar.gz
    let content = download_from_github(download_url)
        .await
        .change_context(Error::InternalSetup)?;
    if extension == ".zip" {
        unzip_zip(content, &final_path).await?;
    } else {
        unzip_tar_gz(content, &final_path).await?;
    };
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
        .change_context(Error::InternalSetup)?;

    let content = response
        .bytes()
        .await
        .into_report()
        .change_context(Error::InternalSetup)?;
    let content = Cursor::new(content);
    Ok(content.clone())
}

#[tracing::instrument(skip(content))]
async fn unzip_tar_gz(content: Cursor<Bytes>, final_path: &Path) -> Result<(), Error> {
    let deflater = bufread::GzDecoder::new(content);
    let mut tar_archive = Archive::new(deflater);
    let mut entries = tar_archive
        .entries()
        .into_report()
        .change_context(Error::InternalSetup)?;
    let entry_result = entries.next().ok_or(Error::InternalSetup)?;
    let mut entry = entry_result
        .into_report()
        .change_context(Error::InternalSetup)?;

    let mut final_file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .mode(0o770)
        .open(&final_path)
        .into_report()
        .change_context(Error::InternalSetup)?;
    copy(&mut entry, &mut final_file)
        .into_report()
        .change_context(Error::InternalSetup)?;

    Ok(())
}

#[tracing::instrument(skip(content))]
async fn unzip_zip(content: Cursor<Bytes>, final_path: &Path) -> Result<(), Error> {
    // With zip
    let mut archive = zip::ZipArchive::new(content).unwrap();
    let mut file = match archive.by_name("fossa") {
        Ok(file) => file,
        Err(..) => {
            return Err(Error::InternalSetup).into_report();
        }
    };

    let mut final_file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .mode(0o770)
        .open(&final_path)
        .into_report()
        .change_context(Error::InternalSetup)?;
    copy(&mut file, &mut final_file)
        .into_report()
        .change_context(Error::InternalSetup)?;
    Ok(())
}
