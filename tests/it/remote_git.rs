//! Tests for git remotes
use crate::helper::assert_error_stack_snapshot;
use crate::{args::raw_base_args, helper::load_config};
use broker::api::remote::{Reference, RemoteProvider};

use broker::{self, api::remote::git, config};

#[tokio::test]
async fn references_on_public_repo_with_no_auth() {
    let conf = load_config!(
        "testdata/config/fossa-one-http-no-auth.yml",
        "testdata/database/empty.sqlite"
    )
    .await;

    let mut integrations = conf.integrations().as_ref().iter();
    let integration = integrations
        .next()
        .expect("no integration loaded from config");
    let references = integration
        .references()
        .expect("no results returned from get_references_that_need_scanning on a public repo!");
    let expected_empty_vec: Vec<Reference> = Vec::new();
    assert_eq!(expected_empty_vec, references);
}

#[tokio::test]
async fn references_on_private_repo_with_no_auth() {
    let conf = load_config!(
        "testdata/config/private-repo-http-no-auth.yml",
        "testdata/database/empty.sqlite"
    )
    .await;
    let mut integrations = conf.integrations().as_ref().iter();
    let integration = integrations
        .next()
        .expect("no integration loaded from config");

    let context = String::from("references on private repo with bad auth");
    assert_error_stack_snapshot!(
        &context,
        integration.references().expect_err(
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

    let reference = Reference::Git(git::Reference::new_tag(
        "master".to_string(),
        "onetwothree".to_string(),
    ));
    integration
        .clone_reference(&reference)
        .expect("no path returned from clone_branch_or_tag on a public repo!");
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
    let reference = Reference::Git(git::Reference::new_tag(
        "main".to_string(),
        "onetwothree".to_string(),
    ));
    let context = String::from("cloning private repo with bad auth");
    assert_error_stack_snapshot!(
        &context,
        integration
            .clone_reference(&reference)
            .expect_err("Could not read from remote repository")
    );
}
