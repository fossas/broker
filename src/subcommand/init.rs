//! Implementation for the `init` subcommand.
use std::{fs, path::PathBuf};

use crate::{
    config,
    ext::{
        error_stack::{ErrorHelper, IntoContext},
        result::WrapErr,
    },
};

use error_stack::IntoReport;
use error_stack::{Result, ResultExt};
use indoc::formatdoc;

/// Errors encountered during init.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Finding the default root
    #[error("find default root")]
    FindDefaultRoot,

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
pub async fn main() -> Result<(), Error> {
    let data_root = config::default_data_root()
        .await
        .change_context(Error::FindDefaultRoot)?;
    let config_file_path = data_root.join("config.yml");
    println!("writing config to {:?}", config_file_path);
    write_default_config(data_root).await?;
    Ok(())
}

async fn write_default_config(data_root: PathBuf) -> Result<(), Error> {
    let config_file_path = data_root.join("config.yml");
    if config_file_path.try_exists().unwrap_or(false) {
        return Error::ConfigFileExists.wrap_err().into_report()
            .help_lazy(|| formatdoc! {
              r#"
              A config file already exists at {}.
              To avoid deleting a valid config file, broker init will not overwrite this file.
              Please delete this file and run this command again if you would like to start with a fresh config file.
              "#, config_file_path.display()});
    }

    std::fs::create_dir_all(&data_root)
        .context_lazy(|| Error::CreateDataRoot(data_root.clone()))
        .help_lazy(|| {
            formatdoc! {r#"
        We encountered an error while attempting to create the config directory {}.
        This can happen if you do not have permission to create the directory.
        Please ensure that you can create a directory at this location and try again
        "#, data_root.display()}
        })?;
    fs::write(&config_file_path, default_config_file(data_root))
        .context_lazy(|| Error::WriteConfigFile(config_file_path.clone()))
        .help_lazy(|| formatdoc!{r#"
        We encountered an error while attempting to write a sample config file to {}.
        This can happen if the directory does not exist or you do not have permission to write to it.
        Please ensure that you can create a file at this location and try again
        "#, config_file_path.display()})?;
    Ok(())
}

fn default_config_file(data_root: PathBuf) -> String {
    let debugging_dir = data_root.join("debugging");
    formatdoc! {r#"

// The fossa_endpoint is the URL for the instance of fossa that you are uploading data to.
// In most cases this will be https://app.fossa.com.
fossa_endpoint: https://app.fossa.com

// You can obtain a fossa API key at https://app.fossa.com/account/settings/integrations/api_tokens.
// A push-only token will suffice, but you can use a full token as well if you wish.
fossa_integration_key: abcd1234

// The version of the config file format. This should remain as is.
version: 1

// The debugging section can probably remain as is.
debugging:
  // The location where your debug traces are written to. This can probably remain as is,
  // but you can change it to an existing directory if you would like to write them to a different location.
  location: {}
  // How many days we retain the debug trace files for. The suggested default is 7 days.
  retention:
    days: 7

integrations:
  - type: git
    poll_interval: 1h
    remote: https://github.com/fossas/broker.git
    auth:
      type: none
      transport: http
  "#, debugging_dir.display()}
}
