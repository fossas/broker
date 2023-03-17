//! Tools to ensure that the fossa-cli exists and download it if it does not

use error_stack::IntoReport;
use error_stack::{Result, ResultExt};
use flate2::bufread;
use std::env::temp_dir;
use std::fs::{self};
use std::io::copy;
use std::io::Cursor;
use std::os::unix::prelude::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use tar::Archive;

use crate::ext::error_stack::{DescribeContext, IntoContext};

/// Errors while downloading fossa-cli
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Error
    #[error("a fatal error occurred during internal configuration")]
    InternalSetup,
}

/// Ensure that the fossa cli exists
pub async fn ensure_fossa_cli(config_path: &PathBuf) -> Result<PathBuf, Error> {
    let output = Command::new("fossa")
        .arg("--help")
        .output()
        .context(Error::InternalSetup)
        .describe("Unable to find `fossa` binary in your path");

    match output {
        Ok(output) => {
            if !output.status.success() {
                return Err(Error::InternalSetup).into_report().describe(
                    "Error when running `fossa -h` on the fossa binary found in your path",
                );
            }
        }
        Err(_) => {
            return download(config_path)
                .await
                .change_context(Error::InternalSetup)
                .describe("fossa-cli not found in your path, so we attempted to download it");
        }
    }

    Ok(PathBuf::from("fossa"))
}

/// Download the CLI into the same directory as the config_path
async fn download(config_path: &PathBuf) -> Result<PathBuf, Error> {
    let config_dir = config_path
        .parent()
        .ok_or(Error::InternalSetup)
        .into_report()?;
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
    println!(
        "latest_release_response.url().path(): {:?}, version = {}",
        path, tag
    );
    if !tag.starts_with("v") {
        return Err(Error::InternalSetup).into_report()?;
    }
    let version = tag.trim_start_matches("v");

    // TODO: Convert these into the options we support and blow up if unsupported
    // Supported os/arch combos:
    // windows/amd64
    // darwin/amd64
    // darwin/arm64
    // linux/amd64
    let mut arch = match std::env::consts::ARCH {
        "x86" | "x86_64" => "amd64",
        "aarch64" | "arm" => "arm64",
        _ => "amd64",
    };
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "windows",
        _ => "linux",
    };

    if os == "darwin" && arch == "arm64" {
        arch = "amd64";
    }

    let extension = match os {
        "darwin" => ".zip",
        "windows" => ".zip",
        _ => ".tar.gz",
    };
    let download_url = format!("https://github.com/fossas/fossa-cli/releases/download/v{version}/fossa_{version}_{os}_{arch}{extension}");
    println!("Downloading from {}", download_url);
    // let download_url = format!("https://github.com/fossas/fossa-cli/releases/download/v{version}/fossa_{version}_{os}_{arch}{extension}");
    // let download_url =
    //     "https://github.com/fossas/fossa-cli/releases/download/v3.7.2/fossa_3.7.2_darwin_amd64.zip"
    //         .to_string();
    // let download_url =
    //     "https://github.com/fossas/fossa-cli/releases/download/v3.7.2/fossa_3.7.2_linux_amd64.tar.gz".to_string();
    println!("Downloading from {}", download_url);
    let final_location = if download_url.ends_with(".zip") {
        unzip_zip(download_url, config_dir).await
    } else {
        unzip_targz(download_url, config_dir).await
    };
    final_location
}

async fn unzip_targz(download_url: String, config_dir: &Path) -> Result<PathBuf, Error> {
    let client = reqwest::Client::new();
    // Download into a tmp dir so that we can unzip and untar it
    let tmpdir = temp_dir();
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

    let download_path = tmpdir.as_path().join("fossa");
    let mut download_file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .mode(0o770)
        .open(&download_path)
        .into_report()
        .change_context(Error::InternalSetup)?;
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

    copy(&mut entry, &mut download_file)
        .into_report()
        .change_context(Error::InternalSetup)?;

    let final_location = config_dir.join("fossa");
    std::fs::rename(&download_path, &final_location)
        .into_report()
        .change_context(Error::InternalSetup)?;

    Ok(final_location)
}

async fn unzip_zip(download_url: String, config_dir: &Path) -> Result<PathBuf, Error> {
    let client = reqwest::Client::new();
    // Download into a tmp dir so that we can unzip and untar it
    let tmpdir = temp_dir();
    let response = client
        .get(&download_url)
        .send()
        .await
        .into_report()
        .change_context(Error::InternalSetup)?;

    let download_path = tmpdir.as_path().join("fossa");
    let mut download_file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .mode(0o770)
        .open(&download_path)
        .into_report()
        .change_context(Error::InternalSetup)?;
    let content = response
        .bytes()
        .await
        .into_report()
        .change_context(Error::InternalSetup)?;
    let content = Cursor::new(content);

    // With zip
    let mut archive = zip::ZipArchive::new(content).unwrap();
    let mut file = match archive.by_name("fossa") {
        Ok(file) => file,
        Err(..) => {
            println!("File test/lorem_ipsum.txt not found");
            return Err(Error::InternalSetup).into_report();
        }
    };
    println!("file found!");
    copy(&mut file, &mut download_file)
        .into_report()
        .change_context(Error::InternalSetup)?;
    let final_location = config_dir.join("fossa");

    std::fs::rename(&download_path, &final_location)
        .into_report()
        .change_context(Error::InternalSetup)?;

    Ok(final_location)
}
