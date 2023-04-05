//! Interactions and data types for the FOSSA API live here.

use std::fmt::Display;

use delegate::delegate;
use derive_more::{AsRef, Display, From};
use derive_new::new;
use error_stack::{report, Report, Result, ResultExt};
use getset::Getters;
use indoc::formatdoc;
use reqwest::{header::CONTENT_TYPE, Client, ClientBuilder, RequestBuilder};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use srclib::{Fetcher, Locator};
use thiserror::Error;
use url::Url;

use crate::{
    api::remote::git,
    ext::{
        error_stack::{DescribeContext, ErrorHelper, IntoContext},
        result::{WrapErr, WrapOk},
        secrecy::ComparableSecretString,
        tracing::span_record,
    },
    fossa_cli::{SourceUnits, Version},
};

use super::remote::{Integration, Reference};

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
    #[error("parse HTTP response body: '{0}'")]
    ParseResponseBody(String),

    /// The request body failed to serialize before we could even run the request.
    #[error("encode HTTP request body")]
    EncodeRequestBody,

    /// Uploading a scan failed.
    #[error("upload scan\n{metadata}")]
    UploadScan {
        /// Information about the scan that was uploaded.
        metadata: String,
    },

    /// If the FOSSA API rejects the uploaded scan, report this.
    #[error("the FOSSA API rejected the uploaded scan with the following message: {error}")]
    ValidateUploadedScan {
        /// The error the FOSSA API reported.
        error: String,
    },

    /// If the FOSSA API rejects the request, report it.
    #[error(r#"the FOSSA API rejected the request\n{error}"#)]
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

    fn parse_response_body(body: &[u8]) -> Self {
        Self::ParseResponseBody(String::from_utf8_lossy(body).to_string())
    }

    fn upload_scan(locator: &Locator, source_units: &SourceUnits) -> Self {
        let metadata = formatdoc! {r#"
        project locator: {locator}
        source units: {source_units}
        "#};
        Self::UploadScan { metadata }
    }

    fn fossa_api(err: ApiError) -> Self {
        let ApiError { name, message, .. } = err;
        let ApiError { code, uuid, .. } = err;
        let http_status_code = err.http_status_code;

        let name = if let Some(name) = name {
            format!("{name}:")
        } else {
            String::from("")
        };

        let error = formatdoc! {r#"
        {name} {message}

        code: {code}
        http status code: {http_status_code}
        
        If you report this issue to FOSSA Support,
        please include the Error ID in the request: '{uuid}'
        "#};

        Self::FossaApi { error }
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
    #[tracing::instrument]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectMetadata {
    name: String,
    branch: Option<String>,
    revision: String,
}

impl ProjectMetadata {
    /// Create metadata from the project information.
    pub fn new(integration: &Integration, reference: &Reference) -> Self {
        let name = integration.endpoint().to_string();
        match reference {
            Reference::Git(reference) => match reference {
                git::Reference::Branch { name: branch, head } => Self {
                    name,
                    branch: Some(branch.to_string()),
                    revision: head.to_string(),
                },
                git::Reference::Tag { name: tag, .. } => Self {
                    name,
                    branch: None,
                    revision: tag.to_string(),
                },
            },
        }
    }
}

impl Display for ProjectMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = &self.name;
        let revision = &self.revision;
        match &self.branch {
            Some(branch) => write!(f, "{name}@{revision} ({branch})"),
            None => write!(f, "{name}@{revision}"),
        }
    }
}

/// Metadata from FOSSA CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, new)]
pub struct CliMetadata {
    version: Version,
}

/// Upload the scan results for a project.
///
/// In the future we'd like to have this method be made available via trait so we can test.
/// I ran out of time this time around though.
#[tracing::instrument(skip(source_units))]
pub async fn upload_scan(
    opts: &Config,
    project: &ProjectMetadata,
    cli: &CliMetadata,
    source_units: SourceUnits,
) -> Result<Locator, Error> {
    let url = opts.endpoint().join("api/builds/custom")?;

    let locator = Locator::builder()
        .fetcher(Fetcher::Custom)
        .project(&project.name)
        .revision(&project.revision)
        .build();
    let package_locator = locator.clone().into_package();

    // We don't currently include metadata such as team/policies/etc.
    // We can add it to integration config as users request it though.
    let mut query = vec![
        ("title", package_locator.to_string()),
        ("locator", locator.to_string()),
        ("cliVersion", cli.version.to_string()),
        ("managedBuild", String::from("true")),
    ];
    if let Some(branch) = &project.branch {
        query.push(("branch", branch.to_string()));
    }

    let req = new_client()?
        .post(url)
        .bearer_auth(opts.key().expose_secret())
        .query(&query)
        .header(CONTENT_TYPE, "application/json")
        .body(source_units.to_string());

    run_request::<UploadResponse>(req)
        .await
        .change_context_lazy(|| Error::upload_scan(&locator, &source_units))?
        .into()
}

impl Endpoint {
    /// Make a GET request against the FOSSA server with the provided route,
    /// which is joined to the base.
    #[tracing::instrument]
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

#[tracing::instrument(skip_all, fields(url))]
async fn run_request<T: DeserializeOwned>(req: RequestBuilder) -> Result<T, Error> {
    let (client, req) = req.build_split();
    let req = req.context(Error::Request)?;
    span_record!(url, display req.url());

    let res = client.execute(req).await.context(Error::Request)?;
    let status = res.status();

    let body = res.bytes().await.context(Error::ReadResponse)?;
    if !status.is_success() {
        let err = serde_json::from_slice::<ApiError>(&body)
            .context_lazy(|| Error::parse_response_body(&body))?;
        report!(Error::fossa_api(err)).wrap_err()
    } else {
        serde_json::from_slice(&body).context_lazy(|| Error::parse_response_body(&body))
    }
}

/// The FOSSA API's organization info response. There's more here, but we don't care about it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OrganizationInfo {
    organization_id: usize,
}

/// After an otherwise successful upload, the build can fail due to validation; capture that here.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UploadResponse {
    locator: Locator,
    error: Option<String>,
}

/// FOSSA API reports errors in this formatted form.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiError {
    name: Option<String>,
    uuid: String,
    code: i32,
    http_status_code: i32,
    message: String,
}

impl From<UploadResponse> for Result<Locator, Error> {
    fn from(UploadResponse { error, locator }: UploadResponse) -> Self {
        match error {
            Some(error) => report!(Error::ValidateUploadedScan { error }).wrap_err(),
            None => Ok(locator),
        }
    }
}
