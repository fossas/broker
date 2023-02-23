use std::path::PathBuf;

use broker::config;
use insta::assert_debug_snapshot;

use crate::helper::set_snapshot_vars;

pub fn raw_base_args(config: &str, db: &str) -> config::RawBaseArgs {
    config::RawBaseArgs::new(Some(String::from(config)), Some(String::from(db)))
}

#[test]
fn validates_args() {
    set_snapshot_vars!();

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
    set_snapshot_vars!();

    let base = raw_base_args(
        "testdata/config/does_not_exist",
        "testdata/database/empty.sqlite",
    );

    insta::with_settings!({
        // Include the details of the args being validated.
        info => &base,
        // Don't fail the snapshot on source code location changes.
        filters => vec![(r"src.+:\d+:\d+", "{source location}")]
    }, {
        assert_debug_snapshot!(
            config::validate_args(base).expect_err("args must have failed validation")
        );
    });
}

#[test]
fn errors_on_nonexistent_database() {
    set_snapshot_vars!();

    let base = raw_base_args(
        "testdata/config/basic.yml",
        "testdata/database/does_not_exist",
    );

    insta::with_settings!({
        // Include the details of the args being validated.
        info => &base,
        // Don't fail the snapshot on source code location changes.
        filters => vec![(r"src.+:\d+:\d+", "{source location}")]
    }, {
        assert_debug_snapshot!(
            config::validate_args(base).expect_err("args must have failed validation")
        );
    });
}

#[test]
fn errors_on_nonexistent_both() {
    set_snapshot_vars!();

    let base = raw_base_args(
        "testdata/config/does_not_exist",
        "testdata/database/does_not_exist",
    );

    insta::with_settings!({
        // Include the details of the args being validated.
        info => &base,
        // Don't fail the snapshot on source code location changes.
        filters => vec![(r"src.+:\d+:\d+", "{source location}")]
    }, {
        assert_debug_snapshot!(
            config::validate_args(base).expect_err("args must have failed validation")
        );
    });
}
