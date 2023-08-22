use broker::api::{self, remote};

use crate::{assert_error_stack_snapshot, helper::gen, load_config, load_config_err};

#[tokio::test]
async fn test_fossa_api_values() {
    let (_, conf) = load_config!().await;

    assert_eq!(conf.fossa_api().key(), &gen::fossa_api_key("abcd1234"),);
    assert_eq!(
        conf.fossa_api().endpoint(),
        &gen::fossa_api_endpoint("https://app.fossa.com"),
    );
}

#[tokio::test]
async fn test_debug_values() {
    let (_, conf) = load_config!().await;

    assert_eq!(
        conf.debug().location(),
        &gen::debug_root("/home/me/.config/fossa/broker/debugging/"),
    );
    assert_eq!(
        conf.debug().retention().days(),
        gen::debug_artifact_retention_count(3),
    );
}

#[tokio::test]
async fn test_debug_values_default_retention() {
    let (_, conf) = load_config!(
        "testdata/config/basic-retention-default.yml",
        "testdata/database/empty.sqlite"
    )
    .await;

    assert_eq!(
        conf.debug().location(),
        &gen::debug_root("/home/me/.fossa/broker/debugging/"),
    );
    assert_eq!(
        conf.debug().retention().days(),
        gen::debug_artifact_retention_count(7),
    );
}

#[tokio::test]
async fn test_debug_values_retention_malformed() {
    let (config_path, err) = load_config_err!(
        "testdata/config/basic-retention-malformed.yml",
        "testdata/database/empty.sqlite"
    )
    .await;
    assert_error_stack_snapshot!(&config_path, err);
}

#[tokio::test]
async fn test_debug_values_retention_invalid() {
    let (config_file_path, err) = load_config_err!(
        "testdata/config/basic-retention-invalid.yml",
        "testdata/database/empty.sqlite"
    )
    .await;
    assert_error_stack_snapshot!(&config_file_path, err);
}

#[tokio::test]
async fn test_one_integration() {
    let (_, conf) = load_config!().await;

    let mut integrations = conf.integrations().as_ref().iter();
    let Some(_) = integrations.next() else { panic!("must have parsed at least one integration") };
    let None = integrations.next() else { panic!("must have parsed exactly one integration") };
}

#[tokio::test]
async fn test_integration_git_ssh_key_file() {
    let (_, conf) = load_config!().await;

    let Some(integration) = conf.integrations().as_ref().iter().next() else { panic!("must have parsed at least one integration") };
    assert_eq!(integration.poll_interval(), gen::code_poll_interval("1h"));

    let remote::Protocol::Git(remote::git::transport::Transport::Ssh{ endpoint, auth }) = integration.protocol() else { panic!("must have parsed integration to git") };
    assert_eq!(
        endpoint,
        &gen::code_remote("git@github.com:fossas/broker.git")
    );

    let api::ssh::Auth::KeyFile(file) = auth else { panic!("must have parsed ssh key file auth") };
    assert_eq!(file, &gen::path_buf("/home/me/.ssh/id_rsa"));
}

#[tokio::test]
async fn test_integration_git_ssh_key() {
    let (_, conf) = load_config!(
        "testdata/config/basic-ssh-key.yml",
        "testdata/database/empty.sqlite"
    )
    .await;

    let Some(integration) = conf.integrations().as_ref().iter().next() else { panic!("must have parsed at least one integration") };
    assert_eq!(integration.poll_interval(), gen::code_poll_interval("1h"));

    let remote::Protocol::Git(remote::git::transport::Transport::Ssh{ endpoint, auth }) = integration.protocol() else { panic!("must have parsed integration") };
    assert_eq!(
        endpoint,
        &gen::code_remote("git@github.com:fossas/broker.git")
    );

    let api::ssh::Auth::KeyValue(key) = auth else { panic!("must have parsed auth value") };
    assert_eq!(key, &gen::secret("efgh5678"));
}

#[tokio::test]
async fn test_integration_git_http_basic() {
    let (_, conf) = load_config!(
        "testdata/config/basic-http-basic.yml",
        "testdata/database/empty.sqlite"
    )
    .await;

    let Some(integration) = conf.integrations().as_ref().iter().next() else { panic!("must have parsed at least one integration") };
    assert_eq!(integration.poll_interval(), gen::code_poll_interval("1h"));

    let remote::Protocol::Git(remote::git::transport::Transport::Http{ endpoint, auth }) = integration.protocol() else { panic!("must have parsed integration") };
    assert_eq!(
        endpoint,
        &gen::code_remote("https://github.com/fossas/broker.git")
    );

    let Some(api::http::Auth::Basic { username, password }) = auth else { panic!("must have parsed auth value") };
    assert_eq!(username, &String::from("jssblck"));
    assert_eq!(password, &gen::secret("efgh5678"));
}

#[tokio::test]
async fn test_integration_git_http_basic_malformed_auth() {
    let (config_file_path, err) = load_config_err!(
        "testdata/config/basic-http-basic-malformed-auth.yml",
        "testdata/database/empty.sqlite"
    )
    .await;
    assert_error_stack_snapshot!(&config_file_path, err);
}

#[tokio::test]
async fn test_integration_git_http_basic_malformed_debugging() {
    let (config_file_path, err) = load_config_err!(
        "testdata/config/basic-http-basic-malformed-debugging.yml",
        "testdata/database/empty.sqlite"
    )
    .await;
    assert_error_stack_snapshot!(&config_file_path, err);
}

#[tokio::test]
async fn test_integration_git_http_header() {
    let (_, conf) = load_config!(
        "testdata/config/basic-http-header.yml",
        "testdata/database/empty.sqlite"
    )
    .await;

    let Some(integration) = conf.integrations().as_ref().iter().next() else { panic!("must have parsed at least one integration") };
    assert_eq!(integration.poll_interval(), gen::code_poll_interval("1h"));
}

#[tokio::test]
async fn test_integration_git_http_no_auth() {
    let (_, conf) = load_config!(
        "testdata/config/basic-http-no-auth.yml",
        "testdata/database/empty.sqlite"
    )
    .await;

    let Some(integration) = conf.integrations().as_ref().iter().next() else { panic!("must have parsed at least one integration") };
    assert_eq!(integration.poll_interval(), gen::code_poll_interval("1h"));

    let remote::Protocol::Git(remote::git::transport::Transport::Http{ endpoint, auth }) = integration.protocol() else { panic!("must have parsed integration") };
    assert_eq!(
        endpoint,
        &gen::code_remote("https://github.com/fossas/broker.git")
    );

    let None = auth else { panic!("must have parsed no auth value") };
}

#[tokio::test]
async fn test_integration_short_poll_interval() {
    let (config_file_path, err) = load_config_err!(
        "testdata/config/short-poll-interval.yml",
        "testdata/database/empty.sqlite"
    )
    .await;
    assert_error_stack_snapshot!(&config_file_path, err);
}
