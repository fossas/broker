//! Tests for git remotes

use crate::helper::assert_error_stack_snapshot;
use std::{
    fs::{self},
    path::PathBuf,
};
use tempfile::tempdir;

use broker::api::remote::{self, git, RemoteProvider};

#[test]
fn clone_public_repo_with_no_auth() {
    let endpoint = remote::Remote::try_from(String::from("https://github.com/fossas/one")).unwrap();

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
    assert_eq!(PathBuf::from(clone_path.clone()), res);
    let mut paths: Vec<String> = fs::read_dir(clone_path)
        .unwrap()
        .map(|file| String::from(file.unwrap().path().file_name().unwrap().to_str().unwrap()))
        .collect();
    paths.sort();
    let mut expected = vec![String::from("LICENSE"), String::from(".git")];
    expected.sort();
    assert_eq!(expected, paths);
}

#[test]
fn clone_private_repo_with_no_auth() {
    let endpoint = remote::Remote::try_from(String::from("git@github.com:fossas/basis")).unwrap();

    let clone_dir = tempdir().unwrap();
    let clone_path = clone_dir.path().display().to_string();
    let repo = git::repository::Repository {
        directory: PathBuf::from(clone_path.clone()),
        checkout_type: git::repository::CheckoutType::None,
        transport: git::transport::Transport::Ssh {
            endpoint,
            auth: None,
        },
    };

    let context = String::from("cloning private repo with bad auth");
    assert_error_stack_snapshot!(
        &context,
        repo.clone()
            .expect_err("Could not read from remote repository")
    );
}
