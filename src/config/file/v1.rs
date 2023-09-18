//! Types and functions for parsing v1 config files.

use std::path::PathBuf;

use error_stack::{report, Report, ResultExt};
use futures::future::join_all;
use serde::Deserialize;

use crate::{
    api::{
        fossa, http,
        remote::{self, git},
        ssh,
    },
    debug,
    ext::{
        error_stack::{DescribeContext, ErrorHelper, IntoContext},
        result::{WrapErr, WrapOk},
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
pub async fn load(content: String) -> Result<super::Config, Report<Error>> {
    // RawConfigV1::parse(content).and_then(|val| async {
    // let validated_result = validate(val).await?;
    // })

    let parsed = RawConfigV1::parse(content)?;
    validate(parsed).await
}

/// Config values as parsed from disk.
/// The "Raw" prefix indicates that this is the initial parsed value before any validation.
///
/// Unlike `RawRunArgs`, we don't have to leak this to consumers of the `config` module,
/// so we don't.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfigV1 {
    #[serde(rename = "fossa_endpoint")]
    endpoint: String,

    #[serde(rename = "fossa_integration_key")]
    integration_key: String,

    #[serde(default)]
    integrations: Vec<Integration>,

    debugging: Debugging,

    #[serde(rename(deserialize = "version"))]
    _version: usize,
}

impl RawConfigV1 {
    /// Parse config from the provided file on disk.
    pub fn parse(content: String) -> std::result::Result<Self, Report<Error>> {
        serde_yaml::from_str(&content).context(Error::Parse)
    }
}

async fn validate(config: RawConfigV1) -> Result<super::Config, Report<Error>> {
    let endpoint = fossa::Endpoint::try_from(config.endpoint).change_context(Error::Validate)?;
    let key = fossa::Key::try_from(config.integration_key).change_context(Error::Validate)?;
    let api = fossa::Config::new(endpoint, key);
    let debugging = debug::Config::try_from(config.debugging).change_context(Error::Validate)?;
    let integrations = config
        .integrations
        .into_iter()
        .map(remote::Integration::try_from)
        .map(|res| async {
            match res {
                Ok(integration) => Ok(remote::Integration::fix_me(&integration).await),
                Err(report) => Err(report),
            }
        });
    //.collect::<Result<Vec<_>, Report<remote::ValidationError>>>()
    //.change_context(Error::Validate)
    //.map(remote::Integrations::new);

    let res = join_all(integrations).await;
    let new_integrations = res
        .into_iter()
        .map(|res| res.expect("test"))
        .collect::<Result<Vec<_>, Report<remote::ValidationError>>>()
        .change_context(Error::Validate)
        .map(remote::Integrations::new)?;

    super::Config::new(api, debugging, new_integrations).wrap_ok()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Debugging {
    location: PathBuf,

    #[serde(default)]
    retention: DebuggingRetention,
}

impl TryFrom<Debugging> for debug::Config {
    type Error = Report<debug::ValidationError>;

    fn try_from(value: Debugging) -> Result<Self, Self::Error> {
        let root = debug::Root::from(value.location);
        let retention = debug::Retention::try_from(value.retention)?;
        Self::new(root, retention).wrap_ok()
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DebuggingRetention {
    days: usize,
}

impl Default for DebuggingRetention {
    fn default() -> Self {
        Self {
            days: debug::ArtifactRetentionCount::default().into(),
        }
    }
}

impl TryFrom<DebuggingRetention> for debug::Retention {
    type Error = Report<debug::ValidationError>;

    fn try_from(value: DebuggingRetention) -> Result<Self, Self::Error> {
        value
            .days
            .try_into()
            .describe("validate 'retention.days'")
            .map(Self::new)
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub(super) enum Integration {
    #[serde(rename = "git")]
    Git {
        poll_interval: String,
        team: Option<String>,
        title: Option<String>,
        remote: String,
        auth: Auth,
        import_branches: Option<bool>,
        import_tags: Option<bool>,
        watched_branches: Option<Vec<String>>,
    },
}

impl TryFrom<Integration> for remote::Integration {
    type Error = Report<remote::ValidationError>;

    fn try_from(value: Integration) -> Result<Self, Self::Error> {
        match value {
            Integration::Git {
                poll_interval,
                remote,
                team,
                title,
                auth,
                import_branches,
                import_tags,
                watched_branches,
            } => {
                let poll_interval = remote::PollInterval::try_from(poll_interval)?;
                let endpoint = remote::Remote::try_from(remote)?;
                let import_branches = import_branches.unwrap_or(true);
                let import_tags = import_tags.unwrap_or(false);
                // .collect::<Vec<remote::WatchedBranch>>()
                let watched_branches = watched_branches
                    .unwrap_or_default()
                    .into_iter()
                    .map(remote::WatchedBranch::new)
                    .collect::<Vec<remote::WatchedBranch>>();
                //.map(|watched_branch| remote::WatchedBranches::new);

                if !import_branches && !watched_branches.is_empty() {
                    report!(remote::ValidationError::ImportBranches)
                        .wrap_err()
                        .help("import branches must be 'true' if watched branches are provided")
                        .describe_lazy(|| "import branches: 'false'".to_string())?
                }

                println!("the watched branches: {watched_branches:?}");
                //let watched_branches = watched_branches.into_iter().map(remote::Branch::try_from).collect<Result<Vec<_>, Report<remote::ValidationError>>>();
                let protocol = match auth {
                    Auth::SshKeyFile { path } => {
                        let auth = ssh::Auth::KeyFile(path);
                        git::transport::Transport::new_ssh(endpoint, auth)
                    }
                    Auth::SshKey { key } => {
                        let secret = ComparableSecretString::from(key);
                        let auth = ssh::Auth::KeyValue(secret);
                        git::transport::Transport::new_ssh(endpoint, auth)
                    }
                    Auth::HttpHeader { header } => {
                        let secret = ComparableSecretString::from(header);
                        let auth = http::Auth::new_header(secret);
                        git::transport::Transport::new_http(endpoint, Some(auth))
                    }
                    Auth::HttpBasic { username, password } => {
                        let password = ComparableSecretString::from(password);
                        let auth = http::Auth::new_basic(username, password);
                        git::transport::Transport::new_http(endpoint, Some(auth))
                    }
                    Auth::None { transport } => match transport.as_str() {
                        "ssh" => report!(remote::ValidationError::Remote)
                            .wrap_err()
                            .help("ssh must have an authentication method")
                            .describe_lazy(|| format!("provided transport: {transport}")),
                        "http" => git::transport::Transport::new_http(endpoint, None).wrap_ok(),
                        other => report!(remote::ValidationError::Remote)
                            .wrap_err()
                            .help("transport must be 'ssh' or 'http'")
                            .describe_lazy(|| format!("provided transport: {other}")),
                    }?,
                };

                remote::Integration::builder()
                    .poll_interval(poll_interval)
                    .team(team)
                    .title(title)
                    .protocol(protocol)
                    .import_branches(import_branches)
                    .import_tags(import_tags)
                    .watched_branches(watched_branches)
                    .build()
                    .wrap_ok()
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
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
