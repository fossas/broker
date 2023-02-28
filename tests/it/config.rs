use broker::{
    api::{self, remote},
    config,
};
use bytesize::ByteSize;

use crate::{
    args::raw_base_args,
    helper::{gen, load_config},
};

#[test]
fn test_fossa_api_values() {
    let conf = load_config!();

    assert_eq!(conf.fossa_api().key(), &gen::fossa_api_key("abcd1234"),);
    assert_eq!(
        conf.fossa_api().endpoint(),
        &gen::fossa_api_endpoint("https://app.fossa.com"),
    );
}

#[test]
fn test_debug_values() {
    let conf = load_config!();

    assert_eq!(
        conf.debug().location(),
        &gen::debug_root("/home/me/.fossa/broker/debugging/"),
    );
    assert_eq!(
        conf.debug().retention().age(),
        &Some(gen::debug_artifact_max_age("7days")),
    );
    assert_eq!(
        conf.debug().retention().size(),
        &Some(gen::debug_artifact_max_size(ByteSize::b(1048576))),
    );
}

#[test]
fn test_one_integration() {
    let conf = load_config!();

    let mut integrations = conf.integrations().as_ref().iter();
    let Some(_) = integrations.next() else { panic!("must have parsed at least one integration") };
    let None = integrations.next() else { panic!("must have parsed exactly one integration") };
}

#[test]
fn test_integration_git_ssh_key_file() {
    let conf = load_config!();

    let Some(integration) = conf.integrations().as_ref().iter().next() else { panic!("must have parsed at least one integration") };
    assert_eq!(integration.poll_interval(), gen::code_poll_interval("1h"));

    let remote::Protocol::Git(remote::git::Transport::Ssh{ endpoint, auth }) = integration.protocol() else { panic!("must have parsed integration to git") };
    assert_eq!(
        endpoint,
        &gen::code_remote("git@github.com:fossas/broker.git")
    );

    let Some(api::ssh::Auth::KeyFile(file)) = auth else { panic!("must have parsed ssh key file auth") };
    assert_eq!(file, &gen::path_buf("/home/me/.ssh/id_rsa"));
}

#[test]
fn test_integration_git_ssh_key() {
    let conf = load_config!(
        "testdata/config/basic-ssh-key.yml",
        "testdata/database/empty.sqlite"
    );

    let Some(integration) = conf.integrations().as_ref().iter().next() else { panic!("must have parsed at least one integration") };
    assert_eq!(integration.poll_interval(), gen::code_poll_interval("1h"));

    let remote::Protocol::Git(remote::git::Transport::Ssh{ endpoint, auth }) = integration.protocol() else { panic!("must have parsed integration") };
    assert_eq!(
        endpoint,
        &gen::code_remote("git@github.com:fossas/broker.git")
    );

    let Some(api::ssh::Auth::KeyValue(key)) = auth else { panic!("must have parsed auth value") };
    assert_eq!(key, &gen::secret("efgh5678"));
}

#[test]
fn test_integration_git_ssh_no_auth() {
    let conf = load_config!(
        "testdata/config/basic-ssh-no-auth.yml",
        "testdata/database/empty.sqlite"
    );

    let Some(integration) = conf.integrations().as_ref().iter().next() else { panic!("must have parsed at least one integration") };
    assert_eq!(integration.poll_interval(), gen::code_poll_interval("1h"));

    let remote::Protocol::Git(remote::git::Transport::Ssh{ endpoint, auth }) = integration.protocol() else { panic!("must have parsed integration") };
    assert_eq!(
        endpoint,
        &gen::code_remote("git@github.com:fossas/broker.git")
    );

    let None = auth else { panic!("must have parsed no auth value") };
}

#[test]
fn test_integration_git_http_basic() {
    let conf = load_config!(
        "testdata/config/basic-http-basic.yml",
        "testdata/database/empty.sqlite"
    );

    let Some(integration) = conf.integrations().as_ref().iter().next() else { panic!("must have parsed at least one integration") };
    assert_eq!(integration.poll_interval(), gen::code_poll_interval("1h"));

    let remote::Protocol::Git(remote::git::Transport::Http{ endpoint, auth }) = integration.protocol() else { panic!("must have parsed integration") };
    assert_eq!(
        endpoint,
        &gen::code_remote("https://github.com/fossas/broker.git")
    );

    let Some(api::http::Auth::Basic { username, password }) = auth else { panic!("must have parsed auth value") };
    assert_eq!(username, &String::from("jssblck"));
    assert_eq!(password, &gen::secret("efgh5678"));
}

#[test]
fn test_integration_git_http_header() {
    let conf = load_config!(
        "testdata/config/basic-http-header.yml",
        "testdata/database/empty.sqlite"
    );

    let Some(integration) = conf.integrations().as_ref().iter().next() else { panic!("must have parsed at least one integration") };
    assert_eq!(integration.poll_interval(), gen::code_poll_interval("1h"));
}

#[test]
fn test_integration_git_http_no_auth() {
    let conf = load_config!(
        "testdata/config/basic-http-no-auth.yml",
        "testdata/database/empty.sqlite"
    );

    let Some(integration) = conf.integrations().as_ref().iter().next() else { panic!("must have parsed at least one integration") };
    assert_eq!(integration.poll_interval(), gen::code_poll_interval("1h"));

    let remote::Protocol::Git(remote::git::Transport::Http{ endpoint, auth }) = integration.protocol() else { panic!("must have parsed integration") };
    assert_eq!(
        endpoint,
        &gen::code_remote("https://github.com/fossas/broker.git")
    );

    let None = auth else { panic!("must have parsed no auth value") };
}
