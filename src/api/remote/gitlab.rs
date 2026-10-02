//! Discovery of repositories belonging to a GitLab group.
//!
//! Broker's `git` integration requires the user to enumerate every repository they
//! wish to scan. For large GitLab groups this is impractical, so this module asks
//! GitLab which repositories exist and expands them into `git` integrations.
//!
//! Discovery uses the same credential that clones the repositories, so a single
//! GitLab group access token is sufficient. Because Broker polls rather than
//! receiving webhooks, that token only needs read access.
//!
//! Discovery uses GitLab's GraphQL API rather than its REST API because only GraphQL
//! reports whether a project's repository exists (`repository.exists`). GitLab lets a
//! project exist without a repository, for example after a failed import; to git such a
//! project is indistinguishable from a URL that doesn't exist (both are an HTTP 404),
//! so it must be filtered here, before Broker tries to poll it.

use error_stack::{report, Report};
use reqwest::{header::CONTENT_TYPE, Client, ClientBuilder, RequestBuilder};
use serde::Deserialize;
use serde_json::json;
use tracing::{debug, info, warn};
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

/// The largest page size the GitLab GraphQL API accepts.
const PER_PAGE: usize = 100;

/// Upper bound on pages walked during discovery.
///
/// GitLab removes projects the token cannot read from each page after paginating, so
/// pages are frequently shorter than [`PER_PAGE`]; this still allows well over any real
/// group while guaranteeing termination if the remote misreports pagination.
const MAX_PAGES: usize = 5000;

/// Lists the projects in a group, one page at a time.
///
/// Archived projects are filtered after fetching rather than with `includeArchived`,
/// because that argument is recent and older self-managed GitLab instances reject
/// queries that use it.
const PROJECTS_QUERY: &str = r#"
query($group: ID!, $includeSubgroups: Boolean!, $first: Int!, $after: String) {
  group(fullPath: $group) {
    projects(includeSubgroups: $includeSubgroups, first: $first, after: $after) {
      pageInfo { hasNextPage endCursor }
      nodes {
        fullPath
        httpUrlToRepo
        archived
        repository { exists rootRef }
      }
    }
  }
}
"#;

/// Errors surfaced while discovering repositories in a GitLab group.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The HTTP client could not be constructed.
    #[error("construct HTTP client")]
    ConstructClient,

    /// The discovery URL could not be built from the configured host.
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

    /// GitLab rejected the query.
    #[error("GitLab rejected the discovery query: {0}")]
    Query(String),

    /// The group does not exist, or the credential cannot read it.
    #[error("GitLab group not found")]
    GroupNotFound,

    /// Discovery needs a credential it can send as an HTTP header.
    #[error("authentication method is not supported for GitLab group discovery")]
    UnsupportedAuth,

    /// The group contained no repositories Broker could scan.
    #[error("no repositories found in GitLab group")]
    NoProjects,
}

/// A repository discovered in a GitLab group.
#[derive(Debug, Clone)]
pub struct Project {
    /// The full path of the project, including any parent groups.
    ///
    /// For example `my-org/platform/api`.
    pub path_with_namespace: String,

    /// The URL Broker clones the repository from.
    pub http_url_to_repo: String,

    /// The repository's default branch.
    ///
    /// `None` for repositories with no commits, which Broker cannot scan.
    pub default_branch: Option<String>,

    /// Whether the project is archived in GitLab.
    pub archived: bool,

    /// Whether the project has a repository at all.
    pub has_repository: bool,
}

impl From<ProjectNode> for Project {
    fn from(node: ProjectNode) -> Self {
        let (has_repository, default_branch) = match node.repository {
            Some(repository) => (repository.exists, repository.root_ref),
            // GitLab omits the repository when the credential can't read the code,
            // in which case Broker can't clone it either.
            None => (false, None),
        };
        Self {
            path_with_namespace: node.full_path,
            http_url_to_repo: node.http_url_to_repo,
            default_branch,
            archived: node.archived,
            has_repository,
        }
    }
}

#[derive(Debug, Deserialize)]
struct Response {
    data: Option<ResponseData>,
    #[serde(default)]
    errors: Vec<ResponseError>,
}

#[derive(Debug, Deserialize)]
struct ResponseError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct ResponseData {
    group: Option<GroupNode>,
}

#[derive(Debug, Deserialize)]
struct GroupNode {
    projects: ProjectConnection,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectConnection {
    page_info: PageInfo,
    nodes: Vec<Option<ProjectNode>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectNode {
    full_path: String,
    http_url_to_repo: String,
    #[serde(default)]
    archived: bool,
    repository: Option<RepositoryNode>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepositoryNode {
    exists: bool,
    root_ref: Option<String>,
}

/// List every repository in `group`.
///
/// `host` is the base URL of the GitLab instance (for example `https://gitlab.com`).
/// `group` is the group's full path, which may itself be a subgroup (`parent/child`).
///
/// Only repositories the group owns are listed, not those shared into it, so results are
/// predictable. Archived projects, projects with no repository, repositories with no
/// commits, and repositories under a path in `excluded_paths` are skipped, with a log line
/// naming each one.
#[tracing::instrument(skip(auth))]
pub async fn discover_projects(
    host: &str,
    group: &str,
    include_subgroups: bool,
    excluded_paths: &[String],
    auth: &http::Auth,
) -> Result<Vec<Project>, Report<Error>> {
    let client = new_client()?;
    let url = graphql_url(host)?;

    let mut discovered = Vec::new();
    let mut after: Option<String> = None;
    for page in 1..=MAX_PAGES {
        let body = json!({
            "query": PROJECTS_QUERY,
            "variables": {
                "group": group,
                "includeSubgroups": include_subgroups,
                "first": PER_PAGE,
                "after": after,
            },
        });

        let req = client
            .post(url.clone())
            .header(CONTENT_TYPE, "application/json")
            .body(body.to_string());
        let req = authenticate(req, auth)?;
        let connection = run_request(req).await?;

        let count = connection.nodes.len();
        discovered.extend(connection.nodes.into_iter().flatten().map(Project::from));
        debug!(
            page,
            count,
            total = discovered.len(),
            "discovered page of GitLab projects"
        );

        match connection.page_info {
            PageInfo {
                has_next_page: true,
                end_cursor: Some(cursor),
            } => after = Some(cursor),
            _ => return finalize(group, discovered, excluded_paths),
        }
    }

    warn!(
        group,
        max_pages = MAX_PAGES,
        "stopped GitLab discovery at the page limit; some repositories may not have been imported"
    );
    finalize(group, discovered, excluded_paths)
}

/// Drop repositories Broker cannot or should not scan, and reject an empty result.
///
/// An empty result is an error rather than an empty integration list because it
/// almost always means the token cannot see the group, which would otherwise
/// present as a successful run that scans nothing.
fn finalize(
    group: &str,
    projects: Vec<Project>,
    excluded_paths: &[String],
) -> Result<Vec<Project>, Report<Error>> {
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
            if !project.has_repository {
                info!(
                    project = project.path_with_namespace,
                    "skipping GitLab project with no repository"
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
            if let Some(excluded) =
                excluded_path_prefix(&project.path_with_namespace, excluded_paths)
            {
                debug!(
                    project = project.path_with_namespace,
                    excluded_path = excluded,
                    "skipping GitLab project under an excluded path"
                );
                return false;
            }
            true
        })
        .collect::<Vec<_>>();

    if scannable.is_empty() {
        return report!(Error::NoProjects)
            .wrap_err()
            .help("verify the token can read the group, that the group contains repositories, and that 'excluded_paths' does not exclude all of them")
            .describe_lazy(|| format!("configured group: '{group}'"));
    }

    scannable.wrap_ok()
}

/// Returns the excluded path that matches `project_path`, if any.
///
/// A project is excluded if its path is exactly one of `excluded_paths`, or is nested
/// under one of them (a subgroup or project path prefix, split on `/`). Matching is
/// segment-aware so that excluding `parent/archive` does not also exclude an unrelated
/// `parent/archive-2` project.
fn excluded_path_prefix<'a>(project_path: &str, excluded_paths: &'a [String]) -> Option<&'a str> {
    excluded_paths
        .iter()
        .find(|excluded| {
            let excluded = excluded.trim_matches('/');
            project_path == excluded || project_path.starts_with(&format!("{excluded}/"))
        })
        .map(String::as_str)
}

/// Build the GraphQL endpoint URL for a GitLab host.
fn graphql_url(host: &str) -> Result<Url, Report<Error>> {
    let mut url = Url::parse(host)
        .context(Error::ConstructUrl)
        .describe_lazy(|| format!("provided host: '{host}'"))
        .help("the host must be an absolute URL, for example 'https://gitlab.com'")?;

    url.path_segments_mut()
        .map_err(|_| report!(Error::ConstructUrl))
        .describe_lazy(|| format!("provided host: '{host}'"))
        .help("the host must be an absolute URL, for example 'https://gitlab.com'")?
        .pop_if_empty()
        .extend(["api", "graphql"]);

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
async fn run_request(req: RequestBuilder) -> Result<ProjectConnection, Report<Error>> {
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

    parse_page(&body)
}

/// Parse one page of the projects query.
///
/// GraphQL reports query errors with a success status, so they are checked here.
fn parse_page(body: &[u8]) -> Result<ProjectConnection, Report<Error>> {
    let response = serde_json::from_slice::<Response>(body)
        .context(Error::ParseResponse)
        .describe_lazy(|| format!("response body: '{}'", String::from_utf8_lossy(body)))?;

    if !response.errors.is_empty() {
        let messages = response
            .errors
            .into_iter()
            .map(|error| error.message)
            .collect::<Vec<_>>()
            .join("; ");
        return report!(Error::Query(messages))
            .wrap_err()
            .help("verify the configured host and token; discovery requires a token with the 'read_api' scope");
    }

    match response.data.and_then(|data| data.group) {
        Some(group) => group.projects.wrap_ok(),
        None => report!(Error::GroupNotFound)
            .wrap_err()
            .help("verify the configured group path, and that the token can read the group"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ext::secrecy::ComparableSecretString;

    #[test]
    fn builds_graphql_url() {
        let url = graphql_url("https://gitlab.com").expect("must build url");
        assert_eq!(url.as_str(), "https://gitlab.com/api/graphql");
    }

    #[test]
    fn builds_graphql_url_for_self_managed_host_with_trailing_slash() {
        let url = graphql_url("https://gitlab.example.com/").expect("must build url");
        assert_eq!(url.as_str(), "https://gitlab.example.com/api/graphql");
    }

    #[test]
    fn builds_graphql_url_for_host_under_a_path() {
        let url = graphql_url("https://example.com/gitlab").expect("must build url");
        assert_eq!(url.as_str(), "https://example.com/gitlab/api/graphql");
    }

    #[test]
    fn rejects_host_that_is_not_a_url() {
        let _ = graphql_url("not a url").expect_err("must reject non-url host");
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
    fn skips_archived_empty_and_repositoryless_projects() {
        let projects = vec![
            project("group/live"),
            Project {
                archived: true,
                ..project("group/archived")
            },
            Project {
                default_branch: None,
                ..project("group/empty")
            },
            Project {
                has_repository: false,
                ..project("group/no-repository")
            },
        ];

        let scannable = finalize("group", projects, &[]).expect("must retain the live project");
        assert_eq!(scannable.len(), 1);
        assert_eq!(scannable[0].path_with_namespace, "group/live");
    }

    #[test]
    fn errors_when_no_projects_are_scannable() {
        let _ = finalize("group", Vec::new(), &[]).expect_err("must reject an empty group");
    }

    fn project(path: &str) -> Project {
        Project {
            path_with_namespace: String::from(path),
            http_url_to_repo: format!("https://gitlab.com/{path}.git"),
            default_branch: Some(String::from("main")),
            archived: false,
            has_repository: true,
        }
    }

    #[test]
    fn excludes_project_at_excluded_path() {
        let projects = vec![project("group/live"), project("group/archive")];
        let excluded = vec![String::from("group/archive")];

        let scannable = finalize("group", projects, &excluded).expect("must retain live project");
        assert_eq!(scannable.len(), 1);
        assert_eq!(scannable[0].path_with_namespace, "group/live");
    }

    #[test]
    fn excludes_projects_nested_under_excluded_path() {
        let projects = vec![
            project("group/live"),
            project("group/archive/team-a/repo"),
            project("group/archive/team-b/repo"),
        ];
        let excluded = vec![String::from("group/archive")];

        let scannable = finalize("group", projects, &excluded).expect("must retain live project");
        assert_eq!(scannable.len(), 1);
        assert_eq!(scannable[0].path_with_namespace, "group/live");
    }

    #[test]
    fn excluded_path_match_is_segment_aware() {
        // "group/archive-2" is not under "group/archive", so it must not be excluded.
        let projects = vec![project("group/archive"), project("group/archive-2")];
        let excluded = vec![String::from("group/archive")];

        let scannable = finalize("group", projects, &excluded).expect("must retain project");
        assert_eq!(scannable.len(), 1);
        assert_eq!(scannable[0].path_with_namespace, "group/archive-2");
    }

    #[test]
    fn excluded_path_ignores_surrounding_slashes() {
        let projects = vec![project("group/live"), project("group/archive/repo")];
        let excluded = vec![String::from("/group/archive/")];

        let scannable = finalize("group", projects, &excluded).expect("must retain live project");
        assert_eq!(scannable.len(), 1);
        assert_eq!(scannable[0].path_with_namespace, "group/live");
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
    /// `GITLAB_HOST` may be set for self-managed instances, and `GITLAB_EXCLUDED_PATHS`
    /// to a comma-separated list of paths to exclude.
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

        let excluded_paths = std::env::var("GITLAB_EXCLUDED_PATHS")
            .map(|paths| paths.split(',').map(String::from).collect::<Vec<_>>())
            .unwrap_or_default();

        let started = std::time::Instant::now();
        let projects = discover_projects(&host, &group, true, &excluded_paths, &auth)
            .await
            .expect("must discover projects");

        println!(
            "discovered {} repositories in '{group}' in {:?} (excluded: {excluded_paths:?})",
            projects.len(),
            started.elapsed()
        );
        for excluded in &excluded_paths {
            assert!(
                projects.iter().all(|p| excluded_path_prefix(
                    &p.path_with_namespace,
                    std::slice::from_ref(excluded)
                )
                .is_none()),
                "no project under excluded path '{excluded}' may be discovered"
            );
        }
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
    fn parses_page_of_projects() {
        let body = br#"{
            "data": {
                "group": {
                    "projects": {
                        "pageInfo": { "hasNextPage": true, "endCursor": "abc" },
                        "nodes": [
                            {
                                "fullPath": "group/repo",
                                "httpUrlToRepo": "https://gitlab.com/group/repo.git",
                                "archived": false,
                                "repository": { "exists": true, "rootRef": "main" }
                            },
                            {
                                "fullPath": "group/empty",
                                "httpUrlToRepo": "https://gitlab.com/group/empty.git",
                                "archived": false,
                                "repository": { "exists": true, "rootRef": null }
                            },
                            {
                                "fullPath": "group/no-repository",
                                "httpUrlToRepo": "https://gitlab.com/group/no-repository.git",
                                "archived": false,
                                "repository": { "exists": false, "rootRef": null }
                            },
                            {
                                "fullPath": "group/no-code-access",
                                "httpUrlToRepo": "https://gitlab.com/group/no-code-access.git",
                                "archived": false,
                                "repository": null
                            },
                            null
                        ]
                    }
                }
            }
        }"#;

        let page = parse_page(body).expect("must parse");
        assert!(page.page_info.has_next_page);
        assert_eq!(page.page_info.end_cursor.as_deref(), Some("abc"));

        let projects = page
            .nodes
            .into_iter()
            .flatten()
            .map(Project::from)
            .map(|p| (p.path_with_namespace, p.has_repository, p.default_branch))
            .collect::<Vec<_>>();
        assert_eq!(
            projects,
            vec![
                (String::from("group/repo"), true, Some(String::from("main"))),
                (String::from("group/empty"), true, None),
                (String::from("group/no-repository"), false, None),
                (String::from("group/no-code-access"), false, None),
            ]
        );
    }

    #[test]
    fn reports_missing_group() {
        let body = br#"{ "data": { "group": null } }"#;
        let err = parse_page(body).expect_err("must reject missing group");
        assert!(matches!(err.current_context(), Error::GroupNotFound));
    }

    #[test]
    fn reports_query_errors() {
        let body =
            br#"{ "errors": [ { "message": "Field 'projects' doesn't accept argument 'foo'" } ] }"#;
        let err = parse_page(body).expect_err("must reject query errors");
        assert!(
            matches!(err.current_context(), Error::Query(message) if message.contains("doesn't accept argument"))
        );
    }
}
