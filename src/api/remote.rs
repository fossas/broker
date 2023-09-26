//!
//! # "Remote" as a concept
//!
//! An example of a [`Remote`] is Github; it's a place where code is stored remotely.
//! [`Remote`] as a concept is abstracted over protocol implementation:
//! for example, a code host accessed via `ssh` or via `http` are both simply a [`Remote`].
//!
//! Their connection protocol (and any specifics, like authentication) is then specified via
//! [`Protocol`], which is usually wrapped inside an [`Integration`], forming the primary interaction
//! point for this module.

use std::{fmt::Display, time::Duration};

use async_trait::async_trait;
use delegate::delegate;
use derive_more::{AsRef, Display, From};
use derive_new::new;
use error_stack::{ensure, report, Report, ResultExt};
use getset::{CopyGetters, Getters};
use glob::Pattern;
use humantime::parse_duration;
use serde::{Deserialize, Serialize};
use tempfile::TempDir;
use typed_builder::TypedBuilder;

use crate::{
    db,
    ext::{
        error_stack::{DescribeContext, ErrorHelper, IntoContext},
        result::{WrapErr, WrapOk},
    },
};

/// Integrations for git repositories
pub mod git;

/// Errors that are possibly surfaced during validation of config values.
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    /// Poll interval is parsed from a user-provided string.
    #[error("validate poll interval")]
    PollInterval,

    /// Poll intervals must be at least a certain minimum.
    #[error("poll interval must be a minimum of {}", humantime::format_duration(MIN_POLL_INTERVAL).to_string())]
    MinPollInterval,

    /// The provided remote is not valid.
    #[error("validate remote location")]
    Remote,

    /// The provided value is empty.
    #[error("value is empty")]
    ValueEmpty,

    /// Invalid combination of import branches and watched branches
    #[error("validate import branches and watched branches")]
    ImportBranches,

    /// Unable to infer primary branch
    #[error("primary branch could not be inferred")]
    PrimaryBranch,
}

/// Validated config values for external code host integrations.
#[derive(Debug, Default, Clone, PartialEq, Eq, AsRef, From, new)]
pub struct Integrations(Vec<Integration>);

impl Integrations {
    delegate! {
        to self.0 {
            /// Iterate over configured integrations.
            pub fn iter(&self) -> impl Iterator<Item = &Integration>;
        }
    }
}

/// Validated remote location for a code host.
#[derive(Debug, Clone, PartialEq, Eq, AsRef, Display, Deserialize, Serialize, new)]
pub struct Remote(String);

impl Remote {
    /// Check whether the remote, rendered into string form, starts with a substring.
    pub fn starts_with(&self, test: &str) -> bool {
        self.0.starts_with(test)
    }

    /// Generate a representation for the remote suitable for use when
    /// creating a [`db::Coordinate`].
    pub fn for_coordinate(&self) -> String {
        // Distinct from the `Display` implementation so that the two can diverge.
        self.0.clone()
    }
}

impl TryFrom<String> for Remote {
    type Error = Report<ValidationError>;

    fn try_from(input: String) -> Result<Self, Self::Error> {
        // Different remotes have different semantics-
        // we can't guarantee this is actually a well formatted URL.
        // Just validate that it's not empty.
        if input.is_empty() {
            report!(ValidationError::ValueEmpty)
                .wrap_err()
                .describe_lazy(|| format!("provided input: '{input}'"))
        } else {
            Remote(input).wrap_ok()
        }
        .help("the remote location may not be empty")
        .change_context(ValidationError::Remote)
    }
}

/// Each integration has metadata (for example, how often it should be polled)
/// along with its protocol (which describes how to download the code so it can be analyzed).
///
/// This type stores this combination of data.
#[derive(
    Debug, Clone, PartialEq, Eq, Getters, CopyGetters, Deserialize, Serialize, TypedBuilder,
)]
pub struct Integration {
    /// The interval at which Broker should poll the remote code host for whether the code has changed.
    #[getset(get_copy = "pub")]
    poll_interval: PollInterval,

    /// The team to which this project should be assigned, if any.
    #[getset(get = "pub")]
    #[builder(default)]
    team: Option<String>,

    /// The title for this project.
    #[getset(get = "pub")]
    title: Option<String>,

    /// The protocol Broker uses to communicate with the remote code host.
    #[getset(get = "pub")]
    #[builder(setter(into))]
    protocol: Protocol,

    /// Specifies if we want to scan specific branches
    #[getset(get = "pub")]
    import_branches: BranchImportStrategy,

    /// Specifies if we want to scan tags
    #[getset(get = "pub")]
    import_tags: TagImportStrategy,

    /// The name of the branches we want to scan
    #[getset(get = "pub")]
    watched_branches: Vec<WatchedBranch>,
}

impl Display for Integration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.protocol())
    }
}

impl Integration {
    /// Get the configured remote for the integration, regardless of variant.
    pub fn remote(&self) -> &Remote {
        match &self.protocol {
            Protocol::Git(transport) => match transport {
                git::transport::Transport::Ssh { endpoint, .. } => endpoint,
                git::transport::Transport::Http { endpoint, .. } => endpoint,
            },
        }
    }

    /// The endpoint for the integration.
    pub fn endpoint(&self) -> &Remote {
        self.protocol().endpoint()
    }

    /// Checks if the reference branch should be scanned by comparing it to our watched branches
    pub fn should_scan_reference(&self, reference: &str) -> bool {
        let branches = self.watched_branches();
        for branch in branches {
            match Pattern::new(branch.name()) {
                Ok(p) => {
                    if p.matches(reference) {
                        return true;
                    }
                }
                // In the case of error continue on and have the function return false if there are no matches
                Err(_e) => continue,
            }
        }
        false
    }

    /// Mutable reference for watched branches
    pub fn add_watched_branch(&mut self, watched_branch: WatchedBranch) {
        self.watched_branches.push(watched_branch)
    }
}

/// Code is stored in many kinds of locations, from git repos to
/// random FTP sites to DevOps hosts like GitHub.
///
/// To handle this variety, Broker uses a predefined list
/// of supported protocols (this type),
/// which are specialized with configuration unique to those integrations.
#[derive(Debug, Clone, PartialEq, Eq, From, Deserialize, Serialize, new)]
pub enum Protocol {
    /// Integration with a code host using the git protocol.
    Git(git::transport::Transport),
}

impl Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Protocol::Git(transport) => write!(f, "git::{transport}"),
        }
    }
}

impl Protocol {
    /// The endpoint for the protocol.
    pub fn endpoint(&self) -> &Remote {
        match self {
            Protocol::Git(transport) => transport.endpoint(),
        }
    }
}

/// Specifies the maximum age for an observability artifact.
#[derive(Debug, Copy, Clone, PartialEq, Eq, AsRef, From, Deserialize, Serialize, new)]
pub struct PollInterval(Duration);

impl PollInterval {
    /// The poll interval expressed as a [`Duration`].
    pub fn as_duration(&self) -> Duration {
        self.0
    }
}

/// This is set because Broker is intended to bring eventual observability;
/// if users want faster polling than this it's probably because they want to make sure they don't miss revisions,
/// in such a case we recommend CI integration.
pub const MIN_POLL_INTERVAL: Duration = Duration::from_secs(60 * 60);

impl TryFrom<String> for PollInterval {
    type Error = Report<ValidationError>;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let parsed = parse_duration(&value)
            .context(ValidationError::PollInterval)
            .describe_lazy(|| format!("provided value: {value}"))?;

        ensure!(
            parsed >= MIN_POLL_INTERVAL,
            ValidationError::MinPollInterval
        );
        PollInterval(parsed).wrap_ok()
    }
}

/// Specificies if we want to scan branches
#[derive(Debug, Clone, PartialEq, Eq, Display, Deserialize, Serialize, new)]
pub enum BranchImportStrategy {
    /// Scanning branches is not allowed
    Disabled,
    /// Scanning branches is allowed
    Enabled,
}

impl From<Option<bool>> for BranchImportStrategy {
    fn from(val: Option<bool>) -> BranchImportStrategy {
        match val {
            Some(false) => BranchImportStrategy::Disabled,
            // True case maps to enabled and if it is None we default to enabled
            _ => BranchImportStrategy::Enabled,
        }
    }
}

/// Specifies if the we want to scan tags
#[derive(Debug, Clone, PartialEq, Eq, Display, Deserialize, Serialize, new)]
pub enum TagImportStrategy {
    /// Scanning tags is not allowed
    Disabled,
    /// Scanning tags is allowed
    Enabled,
}

impl From<Option<bool>> for TagImportStrategy {
    fn from(val: Option<bool>) -> TagImportStrategy {
        match val {
            Some(true) => TagImportStrategy::Enabled,
            // False case maps to disabled, and if it is None we default to disabled
            _ => TagImportStrategy::Disabled,
        }
    }
}

/// The integration's branch that you intend to scan
#[derive(Debug, Clone, PartialEq, Eq, AsRef, Display, Deserialize, Serialize, new)]
pub struct WatchedBranch(String);

impl WatchedBranch {
    /// The name of the watched branch
    pub fn name(&self) -> &str {
        &self.0
    }
}

/// Errors encountered while working with remotes
#[derive(Debug, thiserror::Error)]
pub enum RemoteProviderError {
    /// We encountered an error while shelling out to an external command
    #[error("run external command")]
    RunCommand,
}

/// Remotes can reference specific points in time on a remote unit of code.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub enum Reference {
    /// Git remotes have their own reference format.
    Git(git::Reference),
}

impl Reference {
    /// Given a remote, create a database coordinate from this reference.
    pub fn as_coordinate(&self, remote: &Remote) -> db::Coordinate {
        db::Coordinate::new(
            db::Namespace::Git,
            remote.for_coordinate(),
            match self {
                Reference::Git(reference) => format!("git:{}", reference.for_coordinate()),
            },
        )
    }

    /// Generate a canonical state for the reference.
    pub fn as_state(&self) -> &[u8] {
        match self {
            Reference::Git(git) => git.as_state(),
        }
    }

    /// The name of the reference's branch or tag
    pub fn name(&self) -> &str {
        match self {
            Reference::Git(git) => git.name().as_str(),
        }
    }
}

impl Display for Reference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Reference::Git(reference) => write!(f, "git::{reference}"),
        }
    }
}

/// RemoteProvider are code hosts that we get code from
#[async_trait]
pub trait RemoteProvider {
    /// The reference type used for this implementation.
    type Reference;

    /// Clone a [`Reference`] into a temporary directory.
    async fn clone_reference(
        &self,
        reference: &Self::Reference,
    ) -> Result<TempDir, Report<RemoteProviderError>>;

    /// List references that have been updated in the last 30 days.
    async fn references(&self) -> Result<Vec<Self::Reference>, Report<RemoteProviderError>>;
}

#[async_trait]
impl RemoteProvider for Integration {
    type Reference = Reference;

    async fn clone_reference(
        &self,
        reference: &Self::Reference,
    ) -> Result<TempDir, Report<RemoteProviderError>> {
        match self.protocol() {
            // This is a little awkward because these two types are _semantically related_,
            // but are not related in the code.
            // Right now we're considering this not worth fixing,
            // but as we add more protocols/references it's probably worth revisiting.
            Protocol::Git(transport) => match reference {
                Reference::Git(reference) => transport.clone_reference(reference).await,
            },
        }
    }

    async fn references(&self) -> Result<Vec<Self::Reference>, Report<RemoteProviderError>> {
        match self.protocol() {
            Protocol::Git(proto) => proto
                .references()
                .await
                .map(|refs| refs.into_iter().map(Reference::Git).collect()),
        }
    }
}
