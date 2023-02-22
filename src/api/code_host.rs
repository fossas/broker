//! This module provides functionality for integrating with external code hosts.

use std::time::Duration;

use derive_more::{AsRef, Display, From};
use derive_new::new;
use error_stack::{IntoReport, Report, ResultExt};
use getset::{CopyGetters, Getters};
use url::Url;

use crate::ext::error_stack::{DescribeContext, ErrorHelper};

pub mod git;

/// Errors that are possibly surfaced during validation of config values.
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    /// Poll interval is parsed from a user-provided string.
    #[error("validate poll interval")]
    PollInterval,

    /// The provided URL is not valid.
    #[error("validate endpoint URL")]
    ValidateEndpoint,
}

/// Validated config values for external code host integrations.
#[derive(Debug, Clone, PartialEq, Eq, AsRef, From, new)]
pub struct Config(Vec<Integration>);

/// Validated endpoint for a code host.
#[derive(Debug, Clone, PartialEq, Eq, AsRef, From, Display, new)]
pub struct Endpoint(Url);

impl TryFrom<String> for Endpoint {
    type Error = Report<ValidationError>;

    fn try_from(input: String) -> Result<Self, Self::Error> {
        Url::parse(&input)
            .into_report()
            .describe_lazy(|| format!("provided input: '{input}'"))
            .help("the url provided must be absolute and must contain the protocol")
            .change_context(ValidationError::ValidateEndpoint)
            .map(Endpoint)
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
        parse_duration::parse(&value)
            .into_report()
            .change_context(ValidationError::PollInterval)
            .describe_lazy(|| format!("provided value: {value}"))
            .map(PollInterval)
    }
}
