//! Types and functions for parsing v1 config files.

use std::path::PathBuf;

use error_stack::{report, IntoReport, Report, ResultExt};
use serde::Deserialize;

use crate::{
    api::{
        fossa, http,
        remote::{self, git},
        ssh,
    },
    debug,
    ext::{
        error_stack::{DescribeContext, ErrorHelper},
        secrecy::ComparableSecretString,
    },
};

/// Errors surfaced parsing v1 config values.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("parse config file")]
    Parse,

    #[error("validate parsed config file values")]
    Validate,
}

/// Load the config at v1 for the application.
pub fn load(content: String) -> Result<super::Config, Report<Error>> {
    RawConfigV1::parse(content).and_then(validate)
}

/// Config values as parsed from disk.
/// The "Raw" prefix indicates that this is the initial parsed value before any validation.
///
/// Unlike `RawBaseArgs`, we don't have to leak this to consumers of the `config` module,
/// so we don't.
#[derive(Debug, Deserialize)]
struct RawConfigV1 {
    #[serde(rename = "fossa_endpoint")]
    endpoint: String,
    #[serde(rename = "fossa_integration_key")]
    integration_key: String,
    debugging: Debugging,
    integrations: Vec<Integration>,
}

impl RawConfigV1 {
    /// Parse config from the provided file on disk.
    pub fn parse(content: String) -> std::result::Result<Self, Report<Error>> {
        serde_yaml::from_str(&content)
            .into_report()
            .change_context(Error::Parse)
    }
}

fn validate(config: RawConfigV1) -> Result<super::Config, Report<Error>> {
    let endpoint = fossa::Endpoint::try_from(config.endpoint).change_context(Error::Validate)?;
    let key = fossa::Key::try_from(config.integration_key).change_context(Error::Validate)?;
    let api = fossa::Config::new(endpoint, key);
    let debugging = debug::Config::try_from(config.debugging).change_context(Error::Validate)?;
    let integrations = config
        .integrations
        .into_iter()
        .map(remote::Integration::try_from)
        .collect::<Result<Vec<_>, Report<remote::ValidationError>>>()
        .change_context(Error::Validate)
        .map(remote::Config::new)?;

    Ok(super::Config::new(api, debugging, integrations))
}

#[derive(Debug, Deserialize)]
pub(super) struct Debugging {
    location: PathBuf,
    retention: DebuggingRetention,
}

impl TryFrom<Debugging> for debug::Config {
    type Error = Report<debug::ValidationError>;

    fn try_from(value: Debugging) -> Result<Self, Self::Error> {
        let root = debug::Root::from(value.location);
        let retention = debug::Retention::try_from(value.retention)?;
        Ok(Self::new(root, retention))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct DebuggingRetention {
    duration: Option<String>,
    size: Option<u64>,
}

impl TryFrom<DebuggingRetention> for debug::Retention {
    type Error = Report<debug::ValidationError>;

    fn try_from(value: DebuggingRetention) -> Result<Self, Self::Error> {
        let age = value
            .duration
            .map(debug::ArtifactMaxAge::try_from)
            .transpose()?;
        let size = value
            .size
            .map(debug::ArtifactMaxSize::try_from)
            .transpose()?;
        Ok(debug::Retention::new(age, size))
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(super) enum Integration {
    #[serde(rename = "git")]
    Git {
        poll_interval: String,
        remote: String,
        auth: Auth,
    },
}

impl TryFrom<Integration> for remote::Integration {
    type Error = Report<remote::ValidationError>;

    fn try_from(value: Integration) -> Result<Self, Self::Error> {
        match value {
            Integration::Git {
                poll_interval,
                remote,
                auth,
            } => {
                let poll_interval = remote::PollInterval::try_from(poll_interval)?;
                let endpoint = remote::Remote::try_from(remote)?;
                let protocol = match auth {
                    Auth::SshKeyFile { path } => {
                        let auth = ssh::Auth::KeyFile(path);
                        git::Transport::new_ssh(endpoint, Some(auth))
                    }
                    Auth::SshKey { key } => {
                        let secret = ComparableSecretString::from(key);
                        let auth = ssh::Auth::KeyValue(secret);
                        git::Transport::new_ssh(endpoint, Some(auth))
                    }
                    Auth::HttpHeader { header } => {
                        let secret = ComparableSecretString::from(header);
                        let auth = http::Auth::new_header(secret);
                        git::Transport::new_http(endpoint, Some(auth))
                    }
                    Auth::HttpBasic { username, password } => {
                        let password = ComparableSecretString::from(password);
                        let auth = http::Auth::new_basic(username, password);
                        git::Transport::new_http(endpoint, Some(auth))
                    }
                    Auth::None { transport } => match transport.as_str() {
                        "ssh" => Ok(git::Transport::new_ssh(endpoint, None)),
                        "http" => Ok(git::Transport::new_http(endpoint, None)),
                        other => Err(report!(remote::ValidationError::Remote))
                            .help("transport must be 'ssh' or 'http'")
                            .describe_lazy(|| format!("provided transport: {other}")),
                    }?,
                };

                Ok(remote::Integration::new(poll_interval, protocol.into()))
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(super) enum Auth {
    #[serde(rename = "ssh_key_file")]
    SshKeyFile { path: PathBuf },

    #[serde(rename = "ssh_key")]
    SshKey { key: String },

    #[serde(rename = "http_header")]
    HttpHeader { header: String },

    #[serde(rename = "http_basic")]
    HttpBasic { username: String, password: String },

    #[serde(rename = "none")]
    None { transport: String },
}
