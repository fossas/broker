use std::path::PathBuf;

use broker::{
    api::fossa::{Endpoint, Key},
    config,
};
use secrecy::ExposeSecret;
use url::Url;

use crate::helper::assert_error_stack_snapshot;
use proptest::prelude::*;
use test_strategy::proptest;

pub fn raw_base_args(config: &str, db: &str) -> config::RawBaseArgs {
    config::RawBaseArgs::new(Some(String::from(config)), Some(String::from(db)))
}

#[test]
fn validates_args() {
    let base = raw_base_args(
        "testdata/config/basic.yml",
        "testdata/database/empty.sqlite",
    );

    let validated = config::validate_args(base);
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

#[test]
fn errors_on_nonexistent_config() {
    let base = raw_base_args(
        "testdata/config/does_not_exist",
        "testdata/database/empty.sqlite",
    );

    assert_error_stack_snapshot!(
        &base,
        config::validate_args(base).expect_err("args must have failed validation")
    );
}

#[test]
fn errors_on_nonexistent_database() {
    let base = raw_base_args(
        "testdata/config/basic.yml",
        "testdata/database/does_not_exist",
    );

    assert_error_stack_snapshot!(
        &base,
        config::validate_args(base).expect_err("args must have failed validation")
    );
}

#[test]
fn errors_on_nonexistent_both() {
    let base = raw_base_args(
        "testdata/config/does_not_exist",
        "testdata/database/does_not_exist",
    );

    assert_error_stack_snapshot!(
        &base,
        config::validate_args(base).expect_err("args must have failed validation")
    );
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
        Ok(validated) => prop_assert_eq!(validated.as_ref().as_ref().expose_secret(), &user_input),
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
