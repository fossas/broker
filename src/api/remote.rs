//! This module provides functionality for integrating with "remotes", which are external code hosts.
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

use derive_more::{AsRef, Display, From};
use derive_new::new;
use error_stack::{report, IntoReport, Report, ResultExt};
use getset::{CopyGetters, Getters};
use humantime::parse_duration;
use url;

use crate::ext::error_stack::{DescribeContext, ErrorHelper};

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
pub struct Config(Vec<Integration>);

/// Validated remote location for a code host.
#[derive(Debug, Clone, PartialEq, Eq, AsRef, Display, new)]
pub struct Remote(String);

impl Remote {
    /// parses a Remote and returns a Result<Url>
    pub fn parse(&self) -> Result<url::Url, url::ParseError> {
        url::Url::parse(&self.0)
    }
}

impl TryFrom<String> for Remote {
    type Error = Report<ValidationError>;

    fn try_from(input: String) -> Result<Self, Self::Error> {
        // Different remotes have different semantics-
        // we can't guarantee this is actually a well formatted URL.
        // Just validate that it's not empty.
        if input.is_empty() {
            Err(report!(ValidationError::ValueEmpty))
                .describe_lazy(|| format!("provided input: '{input}'"))
        } else {
            Ok(Remote(input))
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
    Git(git::Transport),
}

/// Specifies the maximum age for an observability artifact.
#[derive(Debug, Copy, Clone, PartialEq, Eq, AsRef, From, new)]
pub struct PollInterval(Duration);

impl TryFrom<String> for PollInterval {
    type Error = Report<ValidationError>;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        parse_duration(&value)
            .into_report()
            .change_context(ValidationError::PollInterval)
            .describe_lazy(|| format!("provided value: {value}"))
            .map(PollInterval)
    }
}
