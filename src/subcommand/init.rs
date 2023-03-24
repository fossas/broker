//! Implementation for the `init` subcommand.
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::ext::{
    error_stack::{ErrorHelper, IntoContext},
    result::WrapOk,
};
use error_stack::Result;
use indoc::formatdoc;

/// Errors encountered during init.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A config file already exists
    #[error("config file exists")]
    ConfigFileExists,

    /// Creating the data root directory
    #[error("create data root")]
    CreateDataRoot(PathBuf),

    /// Writing the file did not work
    #[error("write config file")]
    WriteConfigFile(PathBuf),
}

/// generate the config and db files in the default location
#[tracing::instrument(skip_all)]
pub async fn main(data_root: PathBuf) -> Result<(), Error> {
    let default_already_exists = write_config(&data_root, "config.yml", false)?;
    write_config(&data_root, "config.example.yml", true)?;
    if default_already_exists {
        println!(
            "config file already exists at {:?}, so we have not overwritten it. We did, however, create a new example config file for you at {:?}",
            data_root.join("config.yml"),
            data_root.join("config.example.yml")
        );
    } else {
        println!(
            "writing config to {:?}. We also wrote the same contents to {:?} to serve as a reference for you in the future",
            data_root.join("config.yml"),
            data_root.join("config.example.yml")
        );
    }
    Ok(())
}

fn write_config(data_root: &PathBuf, filename: &str, force_write: bool) -> Result<bool, Error> {
    let config_file_path = data_root.join(filename);
    if config_file_path.try_exists().unwrap_or(false) && !force_write {
        return true.wrap_ok();
    }

    std::fs::create_dir_all(data_root)
        .context_lazy(|| Error::CreateDataRoot(data_root.clone()))
        .help_lazy(|| {
            formatdoc! {r#"
        We encountered an error while attempting to create the config directory {}.
        This can happen if you do not have permission to create the directory.
        Please ensure that you can create a directory at this location and try again
        "#, data_root.display()}
        })?;

    fs::write(&config_file_path, default_config_file(data_root.as_path()))
        .context_lazy(|| Error::WriteConfigFile(config_file_path.clone()))
        .help_lazy(|| {
            formatdoc! {r#"
        We encountered an error while attempting to write a sample config file to {}.
        This can happen if the you do not have permission to create files in that directory.
        Please ensure that you can create a file at this location and try again
        "#, config_file_path.display()}
        })?;
    false.wrap_ok()
}

fn default_config_file(data_root: &Path) -> String {
    let debugging_dir = data_root.join("debugging");
    let default_config_format_string = include_str!("config.default.yml");
    default_config_format_string.replace("{debugging_dir}", &debugging_dir.display().to_string())
}
