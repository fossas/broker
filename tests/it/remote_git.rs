//! Tests for git remotes
use crate::helper::assert_error_stack_snapshot;
use crate::{args::raw_base_args, helper::load_config};
use broker::api::remote::{Commit, Reference, ReferenceType, RemoteProvider, RemoteReference};
use std::path::PathBuf;

use tempfile::tempdir;

use broker::{self, api::remote::git, config};

#[tokio::test]
async fn get_references_that_need_scanning_on_public_repo_with_no_auth() {
    let conf = load_config!(
        "testdata/config/fossa-one-http-no-auth.yml",
        "testdata/database/empty.sqlite"
    )
    .await;

    let mut integrations = conf.integrations().as_ref().iter();
    let integration = integrations
        .next()
        .expect("no integration loaded from config");
    let references = git::repository::Repository::get_references_that_need_scanning(integration)
        .expect("no results returned from get_references_that_need_scanning on a public repo!");
    let expected_empty_vec: Vec<RemoteReference> = Vec::new();
    assert_eq!(expected_empty_vec, references);
}

#[tokio::test]
async fn get_references_that_need_scanning_on_private_repo_with_no_auth() {
    let conf = load_config!(
        "testdata/config/private-repo-http-no-auth.yml",
        "testdata/database/empty.sqlite"
    )
    .await;
    let mut integrations = conf.integrations().as_ref().iter();
    let integration = integrations
        .next()
        .expect("no integration loaded from config");

    let context = String::from("cloning private repo with bad auth");
    assert_error_stack_snapshot!(
        &context,
        git::repository::Repository::get_references_that_need_scanning(integration).expect_err(
            "no results returned from get_references_that_need_scanning on a private repo"
        )
    );
}

#[tokio::test]
async fn clone_public_repo_with_no_auth() {
    let conf = load_config!(
        "testdata/config/fossa-one-http-no-auth.yml",
        "testdata/database/empty.sqlite"
    )
    .await;

    let mut integrations = conf.integrations().as_ref().iter();
    let integration = integrations
        .next()
        .expect("no integration loaded from config");

    let tmpdir = tempdir().unwrap();
    let path = PathBuf::from(tmpdir.path());
    let remote_reference = RemoteReference::new(
        ReferenceType::Tag,
        Commit(String::from("onetwothree")),
        Reference(String::from("master")),
    );
    let cloned_path = git::repository::Repository::clone_branch_or_tag(
        integration,
        path.clone(),
        &remote_reference,
    )
    .expect("no path returned from clone_branch_or_tag on a public repo!");
    let mut expected_path = PathBuf::from(path.as_path());
    expected_path.push("master");
    assert_eq!(cloned_path, expected_path);
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
    let remote_reference = RemoteReference::new(
        ReferenceType::Tag,
        Commit(String::from("onetwothree")),
        Reference(String::from("main")),
    );
    let context = String::from("cloning private repo with bad auth");
    assert_error_stack_snapshot!(
        &context,
        git::repository::Repository::clone_branch_or_tag(integration, path, &remote_reference)
            .expect_err("Could not read from remote repository")
    );
}
