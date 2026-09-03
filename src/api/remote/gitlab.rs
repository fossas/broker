//! Discovery of repositories belonging to a GitLab group.
//!
//! Broker's `git` integration requires the user to enumerate every repository they
//! wish to scan. For large GitLab groups this is impractical, so this module asks
//! GitLab which repositories exist and expands them into `git` integrations.
//!
//! Discovery uses the same credential that clones the repositories, so a single
//! GitLab group access token is sufficient. Because Broker polls rather than
//! receiving webhooks, that token only needs read access.

use error_stack::{report, Report};
use reqwest::{Client, ClientBuilder, RequestBuilder};
use serde::Deserialize;
use tracing::{debug, warn};
use url::Url;

use crate::{
    api::http,
    ext::{
        error_stack::{DescribeContext, ErrorHelper, IntoContext},
        result::{WrapErr, WrapOk},
    },
};

/// The GitLab SaaS host, used when the integration does not specify one.
pub const DEFAULT_HOST: &str = "https://gitlab.com";

/// The largest page size the GitLab API accepts.
const PER_PAGE: usize = 100;

/// Upper bound on pages walked during discovery.
///
/// At [`PER_PAGE`] this allows 100,000 repositories, comfortably above any real group,
/// while still guaranteeing termination if the remote misreports pagination.
const MAX_PAGES: usize = 1000;

/// Errors surfaced while discovering repositories in a GitLab group.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The HTTP client could not be constructed.
    #[error("construct HTTP client")]
    ConstructClient,

    /// The discovery URL could not be built from the configured host and group.
    #[error("construct GitLab API url")]
    ConstructUrl,

    /// The request to GitLab could not be completed.
    #[error("send request to GitLab")]
    Request,

    /// The response body could not be read.
    #[error("read response from GitLab")]
    ReadResponse,

    /// The response body was not the shape Broker expected.
    #[error("parse response from GitLab")]
    ParseResponse,

    /// GitLab responded with a non-success status.
    #[error("GitLab responded with status {0}")]
    Status(u16),

    /// Discovery needs a credential it can send as an HTTP header.
    #[error("authentication method is not supported for GitLab group discovery")]
    UnsupportedAuth,

    /// The group contained no repositories Broker could scan.
    #[error("no repositories found in GitLab group")]
    NoProjects,
}

/// A repository discovered in a GitLab group.
#[derive(Debug, Clone, Deserialize)]
pub struct Project {
    /// The full path of the project, including any parent groups.
    ///
    /// For example `countryfinancial/platform/api`.
    pub path_with_namespace: String,

    /// The URL Broker clones the repository from.
    pub http_url_to_repo: String,

    /// The repository's default branch.
    ///
    /// `None` for repositories with no commits, which Broker cannot scan.
    #[serde(default)]
    pub default_branch: Option<String>,

    /// Whether the project is archived in GitLab.
    #[serde(default)]
    pub archived: bool,
}

/// List every repository in `group`.
///
/// `host` is the base URL of the GitLab instance (for example `https://gitlab.com`).
/// `group` is the group's full path, which may itself be a subgroup (`parent/child`).
///
/// Repositories shared into the group from elsewhere are excluded, so results are
/// limited to repositories the group actually owns. Archived repositories and
/// repositories with no commits are skipped, with a log line naming each one.
#[tracing::instrument(skip(auth))]
pub async fn discover_projects(
    host: &str,
    group: &str,
    include_subgroups: bool,
    auth: &http::Auth,
) -> Result<Vec<Project>, Report<Error>> {
    let client = new_client()?;
    let base = projects_url(host, group)?;

    let mut discovered = Vec::new();
    for page in 1..=MAX_PAGES {
        let mut url = base.clone();
        url.query_pairs_mut()
            .append_pair("per_page", &PER_PAGE.to_string())
            .append_pair("page", &page.to_string())
            .append_pair("include_subgroups", &include_subgroups.to_string())
            // Limit results to repositories the group owns, rather than repositories
            // shared into it, so that the set of scanned repositories is predictable.
            .append_pair("with_shared", "false");

        let req = authenticate(client.get(url), auth)?;
        let page_projects = run_request(req).await?;

        let count = page_projects.len();
        discovered.extend(page_projects);
        debug!(
            page,
            count,
            total = discovered.len(),
            "discovered page of GitLab projects"
        );

        // A short page means there are no further pages to walk.
        if count < PER_PAGE {
            return finalize(group, discovered);
        }
    }

    warn!(
        group,
        max_pages = MAX_PAGES,
        "stopped GitLab discovery at the page limit; some repositories may not have been imported"
    );
    finalize(group, discovered)
}

/// Drop repositories Broker cannot scan, and reject an empty result.
///
/// An empty result is an error rather than an empty integration list because it
/// almost always means the token cannot see the group, which would otherwise
/// present as a successful run that scans nothing.
fn finalize(group: &str, projects: Vec<Project>) -> Result<Vec<Project>, Report<Error>> {
    let scannable = projects
        .into_iter()
        .filter(|project| {
            if project.archived {
                debug!(
                    project = project.path_with_namespace,
                    "skipping archived GitLab project"
                );
                return false;
            }
            if project.default_branch.is_none() {
                warn!(
                    project = project.path_with_namespace,
                    "skipping GitLab project with no default branch; it likely has no commits"
                );
                return false;
            }
            true
        })
        .collect::<Vec<_>>();

    if scannable.is_empty() {
        return report!(Error::NoProjects)
            .wrap_err()
            .help("verify the token can read the group, and that the group contains repositories")
            .describe_lazy(|| format!("configured group: '{group}'"));
    }

    scannable.wrap_ok()
}

/// Build the projects URL for a group.
///
/// The group path is pushed as a single path segment so that subgroup paths such as
/// `parent/child` are percent-encoded, as the GitLab API requires.
fn projects_url(host: &str, group: &str) -> Result<Url, Report<Error>> {
    let mut url = Url::parse(host)
        .context(Error::ConstructUrl)
        .describe_lazy(|| format!("provided host: '{host}'"))
        .help("the host must be an absolute URL, for example 'https://gitlab.com'")?;

    url.path_segments_mut()
        .map_err(|_| report!(Error::ConstructUrl))
        .describe_lazy(|| format!("provided host: '{host}'"))
        .help("the host must be an absolute URL, for example 'https://gitlab.com'")?
        .pop_if_empty()
        .extend(["api", "v4", "groups"])
        .push(group)
        .push("projects");

    url.wrap_ok()
}

/// Attach the integration's credential to a request.
///
/// A `http_basic` credential carries the token in its password, which GitLab accepts
/// as a `PRIVATE-TOKEN`. A `http_header` credential is forwarded as configured, which
/// supports `Authorization: Bearer <token>` among others.
fn authenticate(req: RequestBuilder, auth: &http::Auth) -> Result<RequestBuilder, Report<Error>> {
    match auth {
        http::Auth::Basic { password, .. } => req.header("PRIVATE-TOKEN", password.expose_secret()),
        http::Auth::Header(header) => {
            let raw = header.expose_secret();
            let (name, value) = raw.split_once(':').ok_or_else(|| {
                report!(Error::UnsupportedAuth)
            })
            .help("the header must be formatted as 'Name: Value', for example 'Authorization: Bearer abcd1234'")
            .describe_lazy(|| "provided header could not be split into a name and value".to_string())?;
            req.header(name.trim(), value.trim())
        }
    }
    .wrap_ok()
}

fn new_client() -> Result<Client, Report<Error>> {
    static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
    ClientBuilder::new()
        .user_agent(APP_USER_AGENT)
        .build()
        .context(Error::ConstructClient)
}

#[tracing::instrument(skip_all)]
async fn run_request(req: RequestBuilder) -> Result<Vec<Project>, Report<Error>> {
    let (client, req) = req.build_split();
    let req = req.context(Error::Request)?;
    let res = client.execute(req).await.context(Error::Request)?;

    let status = res.status();
    let body = res.bytes().await.context(Error::ReadResponse)?;
    if !status.is_success() {
        return report!(Error::Status(status.as_u16()))
            .wrap_err()
            .help("verify the configured host, group, and token")
            .describe_lazy(|| format!("response body: '{}'", String::from_utf8_lossy(&body)));
    }

    serde_json::from_slice::<Vec<Project>>(&body)
        .context(Error::ParseResponse)
        .describe_lazy(|| format!("response body: '{}'", String::from_utf8_lossy(&body)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ext::secrecy::ComparableSecretString;

    #[test]
    fn builds_projects_url_for_top_level_group() {
        let url = projects_url("https://gitlab.com", "countryfinancial").expect("must build url");
        assert_eq!(
            url.as_str(),
            "https://gitlab.com/api/v4/groups/countryfinancial/projects"
        );
    }

    #[test]
    fn encodes_subgroup_path_as_single_segment() {
        let url = projects_url("https://gitlab.com", "countryfinancial/platform")
            .expect("must build url");
        assert_eq!(
            url.as_str(),
            "https://gitlab.com/api/v4/groups/countryfinancial%2Fplatform/projects"
        );
    }

    #[test]
    fn builds_projects_url_for_self_managed_host_with_trailing_slash() {
        let url = projects_url("https://gitlab.example.com/", "group").expect("must build url");
        assert_eq!(
            url.as_str(),
            "https://gitlab.example.com/api/v4/groups/group/projects"
        );
    }

    #[test]
    fn rejects_host_that_is_not_a_url() {
        let _ = projects_url("not a url", "group").expect_err("must reject non-url host");
    }

    #[test]
    fn rejects_header_without_a_separator() {
        let auth = http::Auth::Header(ComparableSecretString::from(String::from("nocolon")));
        let client = new_client().expect("must build client");
        let _ = authenticate(client.get("https://gitlab.com"), &auth)
            .expect_err("must reject header with no separator");
    }

    #[test]
    fn accepts_well_formed_header() {
        let auth = http::Auth::Header(ComparableSecretString::from(String::from(
            "Authorization: Bearer abcd1234",
        )));
        let client = new_client().expect("must build client");
        let _ = authenticate(client.get("https://gitlab.com"), &auth)
            .expect("must accept well formed header");
    }

    #[test]
    fn skips_archived_and_empty_projects() {
        let projects = vec![
            Project {
                path_with_namespace: String::from("group/live"),
                http_url_to_repo: String::from("https://gitlab.com/group/live.git"),
                default_branch: Some(String::from("main")),
                archived: false,
            },
            Project {
                path_with_namespace: String::from("group/archived"),
                http_url_to_repo: String::from("https://gitlab.com/group/archived.git"),
                default_branch: Some(String::from("main")),
                archived: true,
            },
            Project {
                path_with_namespace: String::from("group/empty"),
                http_url_to_repo: String::from("https://gitlab.com/group/empty.git"),
                default_branch: None,
                archived: false,
            },
        ];

        let scannable = finalize("group", projects).expect("must retain the live project");
        assert_eq!(scannable.len(), 1);
        assert_eq!(scannable[0].path_with_namespace, "group/live");
    }

    #[test]
    fn errors_when_no_projects_are_scannable() {
        let _ = finalize("group", Vec::new()).expect_err("must reject an empty group");
    }

    /// Discovers repositories against a real GitLab instance.
    ///
    /// Ignored by default because it requires network access and a credential.
    /// Run it against your own group with:
    ///
    /// ```not_rust
    /// GITLAB_GROUP=your-group \
    /// GITLAB_TOKEN=glpat-xxxx \
    ///   cargo test --lib discovers_projects_against_real_gitlab -- --ignored --nocapture
    /// ```
    ///
    /// `GITLAB_HOST` may be set for self-managed instances.
    #[tokio::test]
    #[ignore]
    async fn discovers_projects_against_real_gitlab() {
        let host = std::env::var("GITLAB_HOST").unwrap_or_else(|_| DEFAULT_HOST.to_string());
        let group = std::env::var("GITLAB_GROUP").expect("set GITLAB_GROUP");
        let auth = match std::env::var("GITLAB_TOKEN") {
            Ok(token) => http::Auth::new_basic(
                String::from("fossa-broker"),
                ComparableSecretString::from(token),
            ),
            // Public groups are readable without a credential; send a header GitLab ignores.
            Err(_) => http::Auth::Header(ComparableSecretString::from(String::from(
                "X-Broker-Discovery-Test: 1",
            ))),
        };

        let projects = discover_projects(&host, &group, true, &auth)
            .await
            .expect("must discover projects");

        println!("discovered {} repositories in '{group}'", projects.len());
        for project in projects.iter().take(10) {
            println!(
                "  {} -> {} (default branch: {})",
                project.path_with_namespace,
                project.http_url_to_repo,
                project.default_branch.as_deref().unwrap_or("<none>"),
            );
        }
        if projects.len() > 10 {
            println!("  ... and {} more", projects.len() - 10);
        }

        assert!(
            !projects.is_empty(),
            "must discover at least one repository"
        );
        for project in &projects {
            assert!(
                project.http_url_to_repo.starts_with("http"),
                "clone url must be http(s): {}",
                project.http_url_to_repo
            );
            assert!(
                project.default_branch.is_some(),
                "unscannable repository must have been filtered: {}",
                project.path_with_namespace
            );
        }
    }

    #[test]
    fn parses_project_list_ignoring_unknown_fields() {
        let body = br#"[
            {
                "id": 1,
                "path_with_namespace": "group/repo",
                "http_url_to_repo": "https://gitlab.com/group/repo.git",
                "default_branch": "main",
                "archived": false,
                "some_field_broker_does_not_model": true
            }
        ]"#;

        let parsed = serde_json::from_slice::<Vec<Project>>(body).expect("must parse");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].path_with_namespace, "group/repo");
        assert_eq!(parsed[0].default_branch.as_deref(), Some("main"));
    }
}
