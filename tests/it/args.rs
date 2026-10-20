use std::{ffi::OsString, path::PathBuf};

use broker::{
    api::fossa::{Endpoint, Key},
    config::{RawInitArgs, RawRunArgs, DISABLE_FILE_DISCOVERY_VAR},
};
use proptest::{prop_assert, prop_assert_eq};
use tokio::sync::{Mutex, MutexGuard};
use url::Url;

use crate::assert_error_stack_snapshot;
use test_strategy::proptest;

pub fn raw_base_args(config: &str, db: &str) -> RawRunArgs {
    RawRunArgs::new(Some(String::from(config)), Some(String::from(db)), None)
}

/// Sets or removes [`DISABLE_FILE_DISCOVERY_VAR`] for one test. Tests can share a process, so this
/// holds a lock until it drops, and restores the variable's prior value before releasing it.
struct DiscoveryVar {
    prior: Option<OsString>,
    _lock: MutexGuard<'static, ()>,
}

impl DiscoveryVar {
    async fn set(value: Option<&str>) -> Self {
        static LOCK: Mutex<()> = Mutex::const_new(());
        let lock = LOCK.lock().await;
        let prior = std::env::var_os(DISABLE_FILE_DISCOVERY_VAR);
        match value {
            Some(value) => std::env::set_var(DISABLE_FILE_DISCOVERY_VAR, value),
            None => std::env::remove_var(DISABLE_FILE_DISCOVERY_VAR),
        }
        Self { prior, _lock: lock }
    }
}

impl Drop for DiscoveryVar {
    fn drop(&mut self) {
        match &self.prior {
            Some(prior) => std::env::set_var(DISABLE_FILE_DISCOVERY_VAR, prior),
            None => std::env::remove_var(DISABLE_FILE_DISCOVERY_VAR),
        }
    }
}

#[tokio::test]
async fn validates_args() {
    let base = raw_base_args(
        "testdata/config/basic.yml",
        "testdata/database/empty.sqlite",
    );

    let validated = base.validate().await;
    let validated = validated.expect("args must have passed validation");
    assert_eq!(
        validated.config_path().path(),
        &PathBuf::from("testdata/config/basic.yml")
    );
    assert_eq!(
        validated.database_path().path(),
        &PathBuf::from("testdata/database/empty.sqlite")
    );
}

#[tokio::test]
async fn validates_init_args() {
    let base = RawInitArgs::new(Some(PathBuf::from("some/path")));
    let ctx = base.validate().await.expect("valid args");
    assert_eq!(ctx.data_root(), &PathBuf::from("some/path"));
}

#[tokio::test]
async fn infers_db_path() {
    let _discovery = DiscoveryVar::set(Some("1")).await;

    let base = RawRunArgs::new(Some(String::from("testdata/config/basic.yml")), None, None);
    let validated = base.validate().await;
    let validated = validated.expect("args must have passed validation");
    assert_eq!(
        validated.config_path().path(),
        &PathBuf::from("testdata/config/basic.yml")
    );

    // Inferse `db.sqlite` to be a sibling of the config file.
    assert_eq!(
        validated.database_path().path(),
        &PathBuf::from("testdata/config/db.sqlite")
    );
}

#[tokio::test]
async fn infers_db_path_failing_config() {
    let _discovery = DiscoveryVar::set(Some("1")).await;

    let base = RawRunArgs::new(Some(String::from("")), None, None);
    let validated = base.clone().validate().await;
    let err = validated.expect_err("must have errored");
    assert_error_stack_snapshot!(&base, err);
}

/// Unix only: the snapshot includes the OS's file-not-found message, which differs on Windows.
#[tokio::test]
#[cfg(target_family = "unix")]
async fn missing_config_suggests_init() {
    let _discovery = DiscoveryVar::set(None).await;

    let tmp = tempfile::tempdir().expect("must create tempdir");
    let base = RawRunArgs::new(None, None, Some(tmp.path().to_path_buf()));
    let validated = base.clone().validate().await;
    let err = validated.expect_err("must have errored");
    assert_error_stack_snapshot!(data_root => tmp.path(); &base, err);
}

#[proptest]
fn fossa_api_endpoint(#[strategy(r"\PC+")] user_input: String) {
    let canonical = Url::parse(&user_input);
    let validated = Endpoint::try_from(user_input.clone());

    match (canonical, validated) {
        (Ok(canonical), Ok(validated)) => prop_assert_eq!(
            validated.as_ref(),
            &canonical,
            "parsed URLs must be equal parsing input {}",
            user_input
        ),
        (Err(canonical), Err(validated)) => {
            let contains = format!("{validated:#}").contains(&canonical.to_string());
            prop_assert!(
                contains,
                "validation error must contain parser error, parsing input {}",
                user_input
            );
        }
        (Ok(canonical), Err(validated)) => {
            prop_assert_eq!(
                format!("{validated:#}"),
                canonical.as_str(),
                "parser and validator must not disagree parsing input {}",
                user_input
            )
        }
        (Err(canonical), Ok(validated)) => {
            prop_assert_eq!(
                validated.as_ref().as_str(),
                format!("{canonical:#}"),
                "parser and validator must not disagree parsing input {}",
                user_input
            )
        }
    }
}

#[proptest]
fn fossa_api_key(#[strategy(r"\PC+")] user_input: String) {
    match Key::try_from(user_input.clone()) {
        Ok(validated) => prop_assert_eq!(validated.expose_secret(), &user_input),
        Err(err) => prop_assert!(false, "unexpected parse error: {:#}", err),
    }
}

#[test]
fn fossa_api_key_empty() {
    let input = String::from("");
    assert_error_stack_snapshot!(
        &input,
        Key::try_from(input).expect_err("must have failed validation")
    )
}
