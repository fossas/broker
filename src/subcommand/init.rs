//! Implementation for the `init` subcommand.
use std::{fs, path::PathBuf};

use crate::{
    config,
    ext::error_stack::{ErrorHelper, IntoContext},
};
use error_stack::{Result, ResultExt};
use indoc::{formatdoc, indoc};

/// Errors encountered during init.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Finding the default root
    #[error("find default root")]
    FindDefaultRoot,

    /// A config file already exists
    #[error("config file exists")]
    ConfigFileExists,

    /// Writing the file did not work
    #[error("write file to default path")]
    WriteConfigFile(String),
}

/// generate the config and db files in the default location
#[tracing::instrument(skip_all)]
pub async fn main() -> Result<(), Error> {
    let data_root = config::default_data_root()
        .await
        .change_context(Error::FindDefaultRoot)?;
    let config_file_path = data_root.join("config.yml");
    println!("writing config to {:?}", config_file_path);
    write_default_config(config_file_path).await?;
    Ok(())
}

async fn write_default_config(config_file_path: PathBuf) -> Result<(), Error> {
    fs::write(&config_file_path, default_config_file())
        .context_lazy(|| Error::WriteConfigFile(config_file_path.display().to_string()))
        .help_lazy(|| formatdoc!{r#"
        We encountered an error while attempting to write a sample config file to {}.
        This can happen if the directory does not exist or you do not have permission to write to it.
        Please ensure that you can create a file at this location and try again
        "#, config_file_path.display()})?;
    Ok(())
}

fn default_config_file() -> &'static str {
    indoc! {r#"

fossa_endpoint: https://app.fossa.com
fossa_integration_key: abcd1234
version: 1

debugging:
  location: /Users/scott/.config/fossa/broker/debugging/
  retention:
    days: 7

integrations:
  - type: git
    poll_interval: 1h
    remote: https://github.com/fossas/broker.git
    auth:
      type: http_basic
      username: "pat"
      password: "your personal access token"
  "#}
}
