use std::path::PathBuf;

use broker::config;

use crate::helper::assert_error_stack_snapshot;

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
