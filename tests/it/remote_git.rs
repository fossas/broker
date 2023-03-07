//! Tests for git remotes

use crate::{args::raw_base_args, helper::load_config};
use broker::api::remote::RemoteProvider;
use std::path::PathBuf;

use tempfile::tempdir;

use broker::{self, api::remote::git, config};

#[tokio::test]
async fn update_clones_on_public_repo_with_no_auth() {
    let conf = load_config!(
        "testdata/config/fossa-one-http-no-auth.yml",
        "testdata/database/empty.sqlite"
    )
    .await;

    let mut integrations = conf.integrations().as_ref().iter();
    let integration = integrations
        .next()
        .expect("no integration loaded from config");
    let tmpdir = tempdir().expect("creating tmpdir");
    let path = PathBuf::from(tmpdir.path());
    let res = git::repository::Repository::update_clones(path, integration)
        .expect("no results returned from update_clones on a public repo!");
    let mut master_path = PathBuf::from(tmpdir.path());
    master_path.push(String::from("master"));
    let master_res = res
        .into_iter()
        .find(|p| p.as_path() == master_path.as_path())
        .expect("no master path found");
    assert_eq!(master_res, master_path);
}

#[tokio::test]
async fn clone_private_repo_with_no_auth() {
    let conf = load_config!(
        "testdata/config/private-repo-http-no-auth.yml",
        "testdata/database/empty.sqlite"
    )
    .await;

    let mut integrations = conf.integrations().as_ref().iter();
    let integration = integrations.next().unwrap();
    let tmpdir = tempdir().unwrap();
    let path = PathBuf::from(tmpdir.path());
    let res = git::repository::Repository::update_clones(path, integration);
    assert!(res.is_err());
}
