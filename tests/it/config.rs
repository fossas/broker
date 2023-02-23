use std::{path::PathBuf, time::Duration};

use broker::{
    api::{self, code},
    config, debug,
    ext::secrecy::ComparableSecretString,
};
use bytesize::ByteSize;
use url::Url;

use crate::args::raw_base_args;

/// Convenience macro to load the config inline with the test function (so errors are properly attributed).
///
/// Default paths are:
/// - Config: "testdata/config/basic.yml"
/// - Database: "testdata/database/empty.sqlite"
///
/// Leave args unspecified to use the defaults.
macro_rules! load_config {
    () => {
        load_config!(
            "testdata/config/basic.yml",
            "testdata/database/empty.sqlite"
        )
    };
    ($config_path:expr, $db_path:expr) => {{
        let base = raw_base_args($config_path, $db_path);
        let args = config::validate_args(base).expect("must have validated");
        config::load(&args).expect("must have loaded config")
    }};
}

#[test]
fn test_fossa_api_values() {
    let conf = load_config!();

    assert_eq!(conf.fossa_api().key(), &test_fossa_api_key("abcd1234"),);
    assert_eq!(
        conf.fossa_api().endpoint(),
        &test_fossa_api_endpoint("https://app.fossa.com"),
    );
}

#[test]
fn test_debug_values() {
    let conf = load_config!();

    assert_eq!(
        conf.debug().location(),
        &test_debug_root("/home/me/.fossa/broker/debugging/"),
    );
    assert_eq!(
        conf.debug().retention().age(),
        &Some(test_debug_retention_age(Duration::from_secs(604800))),
    );
    assert_eq!(
        conf.debug().retention().size(),
        &Some(test_debug_retention_size(ByteSize::b(1048576))),
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
fn test_integration_git_sshkey() {
    let conf = load_config!();

    let Some(integration) = conf.integrations().as_ref().iter().next() else { panic!("must have parsed at least one integration") };
    assert_eq!(
        integration.poll_interval(),
        test_integration_poll_interval(Duration::from_secs(3600))
    );

    let code::Protocol::Git(code::git::Transport::Ssh{ endpoint, auth }) = integration.protocol() else { panic!("must have parsed integration to git") };
    assert_eq!(
        endpoint,
        &test_host_endpoint("git@github.com:fossas/broker.git")
    );

    let Some(api::ssh::Auth::KeyFile(file)) = auth else { panic!("must have parsed ssh key file auth") };
    assert_eq!(file, &test_path_buf("/home/me/.ssh/id_rsa"));
}

fn test_fossa_api_key(val: &str) -> api::fossa::Key {
    api::fossa::Key::new(ComparableSecretString::from(String::from(val)))
}

fn test_fossa_api_endpoint(val: &str) -> api::fossa::Endpoint {
    api::fossa::Endpoint::new(Url::parse(val).unwrap_or_else(|_| panic!("must parse {val}")))
}

fn test_debug_root(val: &str) -> debug::Root {
    debug::Root::new(PathBuf::from(String::from(val)))
}

fn test_debug_retention_age(val: Duration) -> debug::ArtifactMaxAge {
    debug::ArtifactMaxAge::from(val)
}

fn test_debug_retention_size(val: ByteSize) -> debug::ArtifactMaxSize {
    debug::ArtifactMaxSize::from(val)
}

fn test_integration_poll_interval(val: Duration) -> code::PollInterval {
    code::PollInterval::from(val)
}

fn test_host_endpoint(val: &str) -> api::code::Remote {
    api::code::Remote::new(String::from(val))
}

fn test_path_buf(val: &str) -> PathBuf {
    PathBuf::from(String::from(val))
}
