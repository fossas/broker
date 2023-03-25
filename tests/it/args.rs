use std::path::PathBuf;

use broker::{
    api::fossa::{Endpoint, Key},
    config::{self, RawBaseArgs},
};
use proptest::{prop_assert, prop_assert_eq};
use url::Url;

use crate::helper::assert_error_stack_snapshot;
use test_strategy::proptest;

pub fn raw_base_args(config: &str, db: &str) -> RawBaseArgs {
    RawBaseArgs::new(Some(String::from(config)), Some(String::from(db)), None)
}

#[tokio::test]
async fn validates_args() {
    let base = raw_base_args(
        "testdata/config/basic.yml",
        "testdata/database/empty.sqlite",
    );

    let validated = config::validate_args(base).await;
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
    let base = RawBaseArgs::new(None, None, Some(PathBuf::from("some/path")));
    let ctx = config::validate_init_args(base).await.expect("valid args");
    assert_eq!(ctx.data_root(), &PathBuf::from("some/path"));
}

#[tokio::test]
async fn infers_db_path() {
    std::env::set_var(broker::config::DISABLE_FILE_DISCOVERY_VAR, "1");

    let base = RawBaseArgs::new(Some(String::from("testdata/config/basic.yml")), None, None);
    let validated = config::validate_args(base).await;
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
    std::env::set_var(broker::config::DISABLE_FILE_DISCOVERY_VAR, "1");

    let base = RawBaseArgs::new(Some(String::from("")), None, None);
    let validated = config::validate_args(base.clone()).await;
    let err = validated.expect_err("must have errored");
    assert_error_stack_snapshot!(&base, err);
}

#[proptest]
fn fossa_api_endpoint(#[strategy(r#"\PC+"#)] user_input: String) {
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
fn fossa_api_key(#[strategy(r#"\PC+"#)] user_input: String) {
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
