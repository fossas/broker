//! Interactions and data types for the FOSSA API live here.

use derive_more::AsRef;
use error_stack::{IntoReport, Report, ResultExt};
use url::Url;

use crate::ext::error_stack::{DescribeContext, ErrorHelper};

/// Errors that are possibly surfaced during validation of config values.
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    /// The provided FOSSA URL is not valid.
    #[error("validate FOSSA endpoint URL")]
    ValidateFossaEndpoint,

    /// The provided FOSSA API key is not valid.
    #[error("validate FOSSA API key")]
    ValidateFossaApiKey,

    /// The value provided to parse is empty.
    #[error("provided value is empty")]
    ValueEmpty,
}

/// The URL to the FOSSA endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FossaEndpoint(Url);

impl TryFrom<String> for FossaEndpoint {
    type Error = Report<ValidationError>;

    fn try_from(input: String) -> Result<Self, Self::Error> {
        Url::parse(&input)
            .into_report()
            .describe_lazy(|| format!("provided input: '{input}'"))
            .help("the url provided must be absolute and must contain the protocol, for example 'https://app.fossa.com'")
            .change_context(ValidationError::ValidateFossaEndpoint)
            .map(FossaEndpoint)
    }
}

/// The FOSSA API key.
#[derive(Debug, Clone, PartialEq, Eq, AsRef)]
pub struct FossaApiKey(String);

impl TryFrom<String> for FossaApiKey {
    type Error = Report<ValidationError>;

    fn try_from(input: String) -> Result<Self, Self::Error> {
        if input.is_empty() {
            Err(Report::new(ValidationError::ValueEmpty))
                .describe_lazy(|| format!("provided input: '{input}'"))
                .help("use an API key from FOSSA here: https://app.fossa.com/account/settings/integrations/api_tokens")
                .change_context(ValidationError::ValidateFossaApiKey)
        } else {
            Ok(FossaApiKey(input))
        }
    }
}
