//! Interactions and data types for the FOSSA API live here.

use derive_more::{AsRef, Display, From};
use derive_new::new;
use error_stack::{IntoReport, Report, ResultExt};
use getset::Getters;
use url::Url;

use crate::ext::{
    error_stack::{DescribeContext, ErrorHelper},
    secrecy::ComparableSecretString,
};

/// Errors that are possibly surfaced during validation of config values.
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    /// The provided URL is not valid.
    #[error("validate endpoint URL")]
    Endpoint,

    /// The provided API key is not valid.
    #[error("validate API key")]
    ApiKey,

    /// The value provided to parse is empty.
    #[error("provided value is empty")]
    ValueEmpty,
}

/// Validated config values for the FOSSA API.
#[derive(Debug, Clone, PartialEq, Eq, Getters, new)]
#[getset(get = "pub")]
pub struct Config {
    /// The endpoint for the FOSSA backend.
    endpoint: Endpoint,

    /// The key used when interacting with the FOSSA backend.
    key: Key,
}

/// The URL to the FOSSA endpoint.
#[derive(Debug, Clone, PartialEq, Eq, AsRef, Display, From, new)]
pub struct Endpoint(Url);

impl TryFrom<String> for Endpoint {
    type Error = Report<ValidationError>;

    fn try_from(input: String) -> Result<Self, Self::Error> {
        Url::parse(&input)
            .into_report()
            .describe_lazy(|| format!("provided input: '{input}'"))
            .help("the url provided must be absolute and must contain the protocol, for example 'https://app.fossa.com'")
            .change_context(ValidationError::Endpoint)
            .map(Endpoint)
    }
}

/// The FOSSA API key.
#[derive(Debug, Clone, PartialEq, Eq, AsRef, From, new)]
pub struct Key(ComparableSecretString);

impl TryFrom<String> for Key {
    type Error = Report<ValidationError>;

    fn try_from(input: String) -> Result<Self, Self::Error> {
        if input.is_empty() {
            Err(Report::new(ValidationError::ValueEmpty))
                .describe_lazy(|| format!("provided input: '{input}'"))
                .help("use an API key from FOSSA here: https://app.fossa.com/account/settings/integrations/api_tokens")
                .change_context(ValidationError::ApiKey)
        } else {
            let secret = ComparableSecretString::from(input);
            Ok(Key(secret))
        }
    }
}
