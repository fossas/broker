//! Tests for git remotes
use crate::helper::assert_error_stack_snapshot;
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
    let mut expected_clone_paths: Vec<PathBuf> = vec!["master", "other-branch", "1.1"]
        .into_iter()
        .map(|reference| {
            let mut path = PathBuf::from(tmpdir.path());
            path.push(String::from(reference));
            path
        })
        .collect();
    expected_clone_paths.sort();
    let mut clone_paths = git::repository::Repository::update_clones(path, integration)
        .expect("no results returned from update_clones on a public repo!");
    clone_paths.sort();
    assert_eq!(expected_clone_paths, clone_paths);
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
    let context = String::from("cloning private repo with bad auth");
    assert_error_stack_snapshot!(
        &context,
        git::repository::Repository::update_clones(path, integration)
            .expect_err("Could not read from remote repository")
    );
}
