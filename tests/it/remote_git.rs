//! Tests for git remotes

use std::path::PathBuf;
use tempfile::tempdir;

use broker::api::remote::{self, git, RemoteProvider};

#[test]
fn clone_public_repo_with_no_auth() {
    let endpoint =
        remote::Remote::try_from(String::from("http://github.com/spatten/slack-wifi-status"))
            .unwrap();

    let clone_dir = tempdir().unwrap();
    let clone_path = clone_dir.path().display().to_string();
    let repo = git::repository::Repository {
        directory: PathBuf::from(clone_path.clone()),
        checkout_type: git::repository::CheckoutType::None,
        transport: git::transport::Transport::Http {
            endpoint,
            auth: None,
        },
    };
    let res = repo.clone().unwrap();
    assert_eq!(PathBuf::from(clone_path), res);
}
