//! Types and functions for parsing v1 config files.

use std::path::PathBuf;

use error_stack::{report, IntoReport, Report, ResultExt};
use serde::Deserialize;

use crate::{
    api::{
        code_host::{self, git},
        fossa, http, ssh,
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
    endpoint: String,
    integration_key: String,
    logging: Logging,
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
    let debugging = debug::Config::try_from(config.logging).change_context(Error::Validate)?;
    let integrations = config
        .integrations
        .into_iter()
        .map(code_host::Integration::try_from)
        .collect::<Result<Vec<_>, Report<code_host::ValidationError>>>()
        .change_context(Error::Validate)
        .map(code_host::Config::new)?;

    Ok(super::Config::new(api, debugging, integrations))
}

#[derive(Debug, Deserialize)]
pub(super) struct Logging {
    location: PathBuf,
    retention: LoggingRetention,
}

impl TryFrom<Logging> for debug::Config {
    type Error = Report<debug::ValidationError>;

    fn try_from(value: Logging) -> Result<Self, Self::Error> {
        let root = debug::Root::from(value.location);
        let retention = debug::Retention::try_from(value.retention)?;
        Ok(Self::new(root, retention))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct LoggingRetention {
    duration: Option<String>,
    size: Option<u64>,
}

impl TryFrom<LoggingRetention> for debug::Retention {
    type Error = Report<debug::ValidationError>;

    fn try_from(value: LoggingRetention) -> Result<Self, Self::Error> {
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
    Git(GitIntegration),
}

impl TryFrom<Integration> for code_host::Integration {
    type Error = Report<code_host::ValidationError>;

    fn try_from(value: Integration) -> Result<Self, Self::Error> {
        match value {
            Integration::Git(git) => {
                let poll_interval = code_host::PollInterval::try_from(git.poll_interval)?;
                let endpoint = code_host::Endpoint::try_from(git.url)?;
                let protocol = match git.auth {
                    Some(auth) => match auth {
                        Auth::SshKeyFile(file) => {
                            let auth = ssh::Auth::KeyFile(file);
                            git::Transport::new_ssh(endpoint, Some(auth))
                        }
                        Auth::SshKey(key) => {
                            let secret = ComparableSecretString::from(key);
                            let auth = ssh::Auth::KeyValue(secret);
                            git::Transport::new_ssh(endpoint, Some(auth))
                        }
                        Auth::HttpHeader(header) => {
                            let secret = ComparableSecretString::from(header);
                            let auth = http::Auth::new_header(secret);
                            git::Transport::new_http(endpoint, Some(auth))
                        }
                        Auth::HttpBasic(AuthHttpBasic { username, password }) => {
                            let password = ComparableSecretString::from(password);
                            let auth = http::Auth::new_basic(username, password);
                            git::Transport::new_http(endpoint, Some(auth))
                        }
                    },
                    None => match endpoint.as_ref().scheme() {
                        "http" => Ok(git::Transport::new_http(endpoint, None)),
                        "ssh" => Ok(git::Transport::new_ssh(endpoint, None)),
                        scheme => Err(report!(code_host::ValidationError::ValidateEndpoint))
                            .help("supported protocols: 'ssh', 'http'")
                            .describe_lazy(|| {
                                format!("provided url: {endpoint} with protocol {scheme}")
                            }),
                    }?,
                };

                Ok(code_host::Integration::new(poll_interval, protocol.into()))
            }
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GitIntegration {
    poll_interval: String,
    url: String,
    auth: Option<Auth>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(super) enum Auth {
    #[serde(rename = "ssh_key_file")]
    SshKeyFile(PathBuf),
    #[serde(rename = "ssh_key")]
    SshKey(String),
    #[serde(rename = "http_header")]
    HttpHeader(String),
    #[serde(rename = "http_basic")]
    HttpBasic(AuthHttpBasic),
}

#[derive(Debug, Deserialize)]
pub(super) struct AuthHttpBasic {
    username: String,
    password: String,
}
