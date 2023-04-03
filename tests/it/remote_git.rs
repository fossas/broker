//! Tests for git remotes
use crate::helper::assert_error_stack_snapshot;
use crate::helper::load_config;
use broker::api::remote::{Reference, RemoteProvider};

use broker::ext::secrecy::REDACTION_LITERAL;
use broker::{self, api::remote::git};

#[tokio::test]
async fn references_on_public_repo_with_no_auth() {
    let (_, conf) = load_config!(
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
        .await
        .expect("no results returned from get_references_that_need_scanning on a public repo!");
    let expected_empty_vec: Vec<Reference> = Vec::new();
    assert_eq!(expected_empty_vec, references);
}

#[tokio::test]
async fn references_on_private_repo_with_no_auth() {
    let (_, conf) = load_config!(
        "testdata/config/private-repo-http-no-auth.yml",
        "testdata/database/empty.sqlite"
    )
    .await;

    let mut integrations = conf.integrations().as_ref().iter();
    let integration = integrations
        .next()
        .expect("no integration loaded from config");

    let context = String::from("references on private repo with bad auth");
    let err = integration
        .references()
        .await
        .expect_err("no results returned from get_references_that_need_scanning on a private repo");
    assert_error_stack_snapshot!(&context, err);
}

#[tokio::test]
async fn clone_public_repo_with_no_auth() {
    let (_, conf) = load_config!(
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
        .await
        .expect("no path returned from clone_branch_or_tag on a public repo!");
}

#[tokio::test]
async fn clone_private_repo_with_no_auth() {
    let (_, conf) = load_config!(
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
    let err = integration
        .clone_reference(&reference)
        .await
        .expect_err("Could not read from remote repository");
    assert_error_stack_snapshot!(&context, err);
}

#[tokio::test]
async fn redacts_auth_http_basic() {
    // Clone a repo that doesn't exist with some auth information.
    // Ensure that it is redacted.
    let (_, conf) = load_config!(
        "testdata/config/nonexistent-repo-bad-http-basic.yml",
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
    let err = integration
        .clone_reference(&reference)
        .await
        .expect_err("Could not read from remote repository");

    // Ensure it doesn't contain our auth values.
    let printed = format!("{err:#}");
    assert!(
        !printed.contains("some_password"),
        "error '{printed}' must not contain auth"
    );
    assert!(
        !printed.contains("some_user"),
        "error '{printed}' must not contain auth"
    );

    // Ensure it tried to print them but they were redacted.
    assert!(
        printed.contains(REDACTION_LITERAL),
        "error '{printed}' must have redacted auth"
    );

    // Finally, snapshot for stability.
    assert_error_stack_snapshot!(&context, err);
}

#[tokio::test]
async fn redacts_auth_http_header() {
    // Clone a repo that doesn't exist with some auth information.
    // Ensure that it is redacted.
    let (_, conf) = load_config!(
        "testdata/config/nonexistent-repo-bad-http-header.yml",
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
    let err = integration
        .clone_reference(&reference)
        .await
        .expect_err("Could not read from remote repository");

    // Ensure it doesn't contain our auth values.
    let printed = format!("{err:#}");
    assert!(
        !printed.contains("some_password"),
        "error '{printed}' must not contain auth"
    );
    assert!(
        !printed.contains("some_user"),
        "error '{printed}' must not contain auth"
    );

    // Ensure it tried to print them but they were redacted.
    assert!(
        printed.contains(REDACTION_LITERAL),
        "error '{printed}' must have redacted auth"
    );

    // Finally, snapshot for stability.
    assert_error_stack_snapshot!(&context, err);
}
