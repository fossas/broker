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
    formatdoc! {r#"
# fossa_endpoint sets the endpoint that broker will send requests to.
# This field should only be modified if your FOSSA account lives on a different server than app.fossa.com.
# This is most commonly needed with on-premise instances of FOSSA.
# In most cases this will be https://app.fossa.com.
fossa_endpoint: https://app.fossa.com

# fossa_api_key is the API key for your FOSSA account.
# You can obtain a fossa API key at https://app.fossa.com/account/settings/integrations/api_tokens.
# A push-only token will suffice, but you can use a full token as well if you wish.
fossa_integration_key: abcd1234

# The version of the config file format. "1" is the only currently supported version.
version: 1

# The debugging section can probably remain as is.
debugging:
  # The location where your debug traces are written to. This can probably remain as is,
  # but you can change it to an existing directory if you would like to write them to a different location.
  location: {}
  # How many days we retain the debug trace files for. The suggested default is 7 days.
  retention:
    days: 7

# The integrations section is where you configure the integrations that broker will analyze.
# You will want to create one integration for every repository that you want broker to analyze.
# The following integrations give examples for all supported auth types.
integrations:
  # "git" is the only type currently supported.
  - type: git
    # "poll_interval" is the interval at which we poll the remote for new data. Some example intervals are:
    # 1h: one hour
    # 30m: 30 minutes
    # 1d: one day
    # 1w: one week
    poll_interval: 1h
    # "remote" is the remote URL for the git repository. This can be an http or ssh URL. The auth section below must match the type of URL.
    # An http URL will start with 'http://' or 'https://'.
    # An ssh URL will start with 'ssh://' or 'git@'.
    remote: https://github.com/fossas/broker.git
    # auth is the authentication information for the remote. It must match the type of the remote URL.
    # https or http URLs can have types of "none", "http_header" or "http_basic".
    # ssh URLs can have auth types of "ssh_key" or "ssh_key_file".
    # There are examples of all these combinations below.
    auth:
      type: none
      transport: http

  # This is an example of using an auth type of "none" with an HTTP URL
  # This can be used for public repositories on github, gitlab, etc.
  - type: git
    poll_interval: 1h
    remote: https://github.com/fossas/broker.git
    auth:
      type: none
      transport: http

  # This is an example of using http basic auth with a github access token.
  - type: git
    poll_interval: 1h
    remote: https://github.com/fossas/private.git
    auth:
      type: http_basic
      # The username and password for the remote. These are the credentials that you would use to clone the repository.
      # When using a github access token, set the username to "pat" and the password to your github access token
      # The github access token must have read permission for the repository.
      username: pat
      password: <ghp_the_rest_of_your_github_token>

  # This is an example of using http basic auth using a GitLab access token.
  # The username can be any non-empty string. The password is the GitLab access token.
  # The access token must have read_repository access and must have a role of at least reporter.
  # You can generate a GitLab access token for your project by going to the project settings page and clicking on "Access Tokens".
  - type: git
    poll_interval: 1h
    remote: https://gitlab.com/fossas/private_repository
    auth:
      type: http_basic
      username: pat
      password: glpat-the-rest-of-your-gitlab-token

  # This is an example of using http basic auth on bitbucket with a repository access token.
  # You can create a repository access token by going to the repository settings page and clicking on "Access Tokens".
  # The access token must have read access for the repository.
  - type: git
    poll_interval: 1h
    remote: https://bitbucket.org/fossas/private_repository.git
    auth:
      type: http_basic
      # The username and password for the remote. For bitbucket repository access tokens, the username should be x-token-auth.
      # The password is a bitbucket access token with repo read access.
      username: x-token-auth
      password: <bitbucket access token>

  # This is an example of using an http header for authentication.
  # The header will be passed to git like this: `git -c http.extraheader="<header>" clone <remote>`
  # The header should like like this, where B64_BASIC_AUTH is a base64 encoded string of the format "username:password":
  # "AUTHORIZATION: BASIC B64_BASIC_AUTH"
  # You can generate B64_BASIC_AUTH with the command `echo -n "<username>:<password>" | base64`
  # When using a GitHub access token, the username should be "pat" and the password should be the access token.
  # When using a GitLab access token, the username can be any non-empty string and the password should be the access token.
  # When using a Bitbucket repository access token, the username should be "x-token-auth" and the password should be the access token.
  - type: git
    poll_interval: 1h
    remote: https://github.com/fossas/private.git
    auth:
      type: http_header
      # header: "AUTHORIZATION: BASIC eAXR10...=="
      header: "AUTHORIZATION: BASIC B64_BASIC_AUTH"

  # This is an example of using an ssh key file for authentication.
  # The path field is the path to the private ssh key file.
  # The private key file must have permissions of 0600.
  - type: git
    poll_interval: 1h
    remote: git@github.com:fossas/private.git
    auth:
      type: ssh_key_file
      path: "/Users/me/.ssh/id_ed25519"

  # This is an example of using an ssh key for authentication.
  # The ssh key field is the full contents of your private ssh key file.
  # We will write this key to a temporary file and use it to clone the repository.
  # The `|` means that the next line is the start of the ssh key, that newlines will be respected and that a single newline at the end of the field will be kept
  - type: git
    poll_interval: 1h
    remote: git@github.com:fossas/private.git
    auth:
      type: ssh_key
      key: |
        -----BEGIN OPENSSH PRIVATE KEY-----
        contents of your private key
        -----END OPENSSH PRIVATE KEY-----
  "#, debugging_dir.display()}
}
