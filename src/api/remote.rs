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

use std::time::Duration;

use async_trait::async_trait;
use delegate::delegate;
use derive_more::{AsRef, Display, From};
use derive_new::new;
use error_stack::{report, Report, ResultExt};
use getset::{CopyGetters, Getters};
use humantime::parse_duration;
use tempfile::TempDir;

use crate::ext::{
    error_stack::{DescribeContext, ErrorHelper, IntoContext},
    result::{WrapErr, WrapOk},
};

/// Integrations for git repositories
pub mod git;

/// Errors that are possibly surfaced during validation of config values.
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    /// Poll interval is parsed from a user-provided string.
    #[error("validate poll interval")]
    PollInterval,

    /// The provided remote is not valid.
    #[error("validate remote location")]
    Remote,

    /// The provided value is empty.
    #[error("value is empty")]
    ValueEmpty,
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
#[derive(Debug, Clone, PartialEq, Eq, AsRef, Display, new)]
pub struct Remote(String);

impl Remote {
    /// Check whether the remote, rendered into string form, starts with a substring.
    pub fn starts_with(&self, test: &str) -> bool {
        self.0.starts_with(test)
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
#[derive(Debug, Clone, PartialEq, Eq, Getters, CopyGetters, new)]
pub struct Integration {
    /// The interval at which Broker should poll the remote code host for whether the code has changed.
    #[getset(get_copy = "pub")]
    poll_interval: PollInterval,

    /// The protocol Broker uses to communicate with the remote code host.
    #[getset(get = "pub")]
    protocol: Protocol,
}

/// Code is stored in many kinds of locations, from git repos to
/// random FTP sites to DevOps hosts like GitHub.
///
/// To handle this variety, Broker uses a predefined list
/// of supported protocols (this type),
/// which are specialized with configuration unique to those integrations.
#[derive(Debug, Clone, PartialEq, Eq, From, new)]
pub enum Protocol {
    /// Integration with a code host using the git protocol.
    Git(git::transport::Transport),
}

/// Specifies the maximum age for an observability artifact.
#[derive(Debug, Copy, Clone, PartialEq, Eq, AsRef, From, new)]
pub struct PollInterval(Duration);

impl PollInterval {
    /// The poll interval expressed as a [`Duration`].
    pub fn as_duration(&self) -> Duration {
        self.0
    }
}

impl TryFrom<String> for PollInterval {
    type Error = Report<ValidationError>;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        parse_duration(&value)
            .context(ValidationError::PollInterval)
            .describe_lazy(|| format!("provided value: {value}"))
            .map(PollInterval)
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
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum Reference {
    /// Git remotes have their own reference format.
    Git(git::Reference),
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
