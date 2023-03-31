//! Interactions and data types for the FOSSA API live here.

use std::fmt::Display;

use delegate::delegate;
use derive_more::{AsRef, Display, From};
use derive_new::new;
use error_stack::{report, Report, Result, ResultExt};
use getset::Getters;
use reqwest::{Client, ClientBuilder, RequestBuilder};
use serde::{de::DeserializeOwned, Deserialize};
use srclib::{Fetcher, Locator};
use thiserror::Error;
use url::Url;

use crate::ext::{
    error_stack::{DescribeContext, ErrorHelper, IntoContext},
    result::{WrapErr, WrapOk},
    secrecy::ComparableSecretString,
};

/// Errors encountered using this module.
#[derive(Debug, Error)]
pub enum Error {
    /// Looking up the organization for the user failed.
    #[error("look up organization for user")]
    LookupOrgId,

    /// When making requests, we have to construct a URL from the base and a new route.
    /// If that fails, this error occurs.
    #[error("construct request URL from '{base}' and '{route}'")]
    ConstructUrl {
        /// The base URL used.
        base: String,

        /// The relative route used.
        route: String,
    },

    /// If initializing the client fails, this error occurs.
    #[error("construct HTTP client")]
    ConstructClient,

    /// If running a request fails, this error occurs.
    #[error("run HTTP request")]
    Request,

    /// The request was successfully sent, and a response received,
    /// but the client was unable to download the response.
    #[error("download HTTP response")]
    ReadResponse,

    /// The request was successfully sent, and the response body was downloaded,
    /// but the response body did not successfully parse into the destination type.
    #[error("parse HTTP response body")]
    ParseResponseBody,

    /// The request body failed to serialize before we could even run the request.
    #[error("encode HTTP request body")]
    EncodeRequestBody,

    /// If the FOSSA API responds with an error, report this.
    #[error("the FOSSA API reported an error: {error}")]
    FossaApi {
        /// The error the FOSSA API reported.
        error: String,
    },
}

impl Error {
    fn construct_url(base: &Endpoint, route: &str) -> Self {
        Self::ConstructUrl {
            base: base.to_string(),
            route: route.to_string(),
        }
    }
}

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

    fn try_from(input: String) -> std::result::Result<Self, Self::Error> {
        Url::parse(&input)
            .context(ValidationError::Endpoint)
            .describe_lazy(|| format!("provided input: '{input}'"))
            .help("the url provided must be absolute and must contain the protocol, for example 'https://app.fossa.com'")
            .map(Endpoint)
    }
}

/// The FOSSA API key.
#[derive(Debug, Clone, PartialEq, Eq, From, new)]
pub struct Key(ComparableSecretString);

impl Key {
    delegate! {
        to self.0 {
            /// Expose the key, viewing it as a standard string.
            pub fn expose_secret(&self) -> &str;
        }
    }
}

impl Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TryFrom<String> for Key {
    type Error = Report<ValidationError>;

    fn try_from(input: String) -> std::result::Result<Self, Self::Error> {
        if input.is_empty() {
            Report::new(ValidationError::ValueEmpty)
                .wrap_err()
                .describe_lazy(|| format!("provided input: '{input}'"))
                .help("use an API key from FOSSA here: https://app.fossa.com/account/settings/integrations/api_tokens")
                .change_context(ValidationError::ApiKey)
        } else {
            Key(ComparableSecretString::from(input)).wrap_ok()
        }
    }
}

/// Validated config values for the FOSSA API populated with the org of the current user.
#[derive(Debug, Clone, PartialEq, Eq, Getters, new)]
#[getset(get = "pub")]
pub struct OrgConfig {
    /// The endpoint for the FOSSA backend.
    endpoint: Endpoint,

    /// The key used when interacting with the FOSSA backend.
    key: Key,

    /// The ID of the organization to which the API key is registered.
    organization_id: usize,
}

impl OrgConfig {
    /// Lookup the organization for the provided config.
    pub async fn lookup(config: &Config) -> Result<Self, Error> {
        let OrganizationInfo { organization_id } = config
            .endpoint()
            .get::<OrganizationInfo>("/api/cli/organization", config.key())
            .await
            .change_context(Error::LookupOrgId)?;

        let Config { endpoint, key } = config.clone();
        Ok(Self {
            endpoint,
            key,
            organization_id,
        })
    }
}

/// The metadata for a project to upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectMetadata {
    name: String,
    branch: String,
    revision: String,
}

/// Metadata from FOSSA CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliMetadata {
    version: String,
}

/// Upload the scan results for a project.
///
/// Currently Broker doesn't inspect source units, so this is just a string;
/// be careful that this is the proper shape.
pub async fn upload_scan(
    opts: &Config,
    project: &ProjectMetadata,
    cli: &CliMetadata,
    source_units: String,
) -> Result<Locator, Error> {
    // Get the org info each time, in case the org for the token has changed since Broker started.
    let opts = OrgConfig::lookup(opts).await?;
    let url = opts.endpoint().join("api/builds/custom")?;

    let locator = Locator::builder()
        .fetcher(Fetcher::Custom)
        .project(&project.name)
        .revision(&project.revision)
        .org_id(opts.organization_id)
        .build()
        .to_string();

    let req = new_client()?
        .post(url)
        .bearer_auth(opts.key().expose_secret())
        .query(&[
            // We don't currently include metadata such as team/policies/title/etc.
            // We can add it to integration config as users request it though.
            ("locator", locator.as_str()),
            ("branch", project.branch.as_str()),
            ("cliVersion", cli.version.as_str()),
            ("managedBuild", "true"),
        ])
        .body(source_units);

    run_request::<UploadResponse>(req).await?.into()
}

impl Endpoint {
    /// Make a GET request against the FOSSA server with the provided route,
    /// which is joined to the base.
    async fn get<T: DeserializeOwned>(&self, route: &str, token: &Key) -> Result<T, Error> {
        let full_url = self.join(route)?;
        let req = new_client()?
            .get(full_url)
            .bearer_auth(token.expose_secret());

        run_request(req).await
    }

    /// Parse a string as an URL, with this URL as the base URL.
    ///
    /// Note: a trailing slash is significant.
    /// Without it, the last path component is considered to be a “file” name to be
    /// removed to get at the “directory” that is used as the base.
    fn join(&self, route: &str) -> Result<Url, Error> {
        self.0
            .join(route)
            .context_lazy(|| Error::construct_url(self, route))
    }
}

fn new_client() -> Result<Client, Error> {
    static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);
    ClientBuilder::new()
        .user_agent(APP_USER_AGENT)
        .build()
        .context(Error::ConstructClient)
}

async fn run_request<T: DeserializeOwned>(req: RequestBuilder) -> Result<T, Error> {
    let res = req.send().await.context(Error::Request)?;
    let response_body = res.bytes().await.context(Error::ReadResponse)?;
    serde_json::from_slice(&response_body).context(Error::ParseResponseBody)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct OrganizationInfo {
    organization_id: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct UploadResponse {
    upload_locator: Locator,
    upload_error: Option<String>,
}

impl From<UploadResponse> for Result<Locator, Error> {
    fn from(value: UploadResponse) -> Self {
        if let Some(error) = value.upload_error {
            Err(report!(Error::FossaApi { error }))
        } else {
            Ok(value.upload_locator)
        }
    }
}
