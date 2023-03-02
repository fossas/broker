//! Tests for debugging functionality.

use broker::debug::ArtifactRetentionCount;
use proptest::prelude::*;
use test_strategy::proptest;

use crate::helper::{assert_error_stack_snapshot, load_config};

#[proptest]
fn validate_artifact_retention_count(
    #[by_ref]
    #[filter(*#input > 0)]
    input: usize,
) {
    match ArtifactRetentionCount::try_from(input) {
        Ok(validated) => prop_assert_eq!(validated, input, "tested input: {:?}", input),
        Err(err) => prop_assert!(false, "unexpected parsing error '{err:#}' for '{input}'"),
    }
}

#[test]
fn validate_artifact_retention_count_min() {
    let input = 0;
    assert_error_stack_snapshot!(
        &input,
        ArtifactRetentionCount::try_from(input).expect_err("must have failed validation")
    )
}

#[test]
fn validate_artifact_retention_count_default() {
    assert_eq!(ArtifactRetentionCount::default(), 7);
}

#[tokio::test]
async fn test_debug_location_invalid() {
    let (config_path, config) = load_config!(
        "testdata/config/basic-location-invalid.yml",
        "testdata/database/empty.sqlite"
    )
    .await;

    let err = config
        .debug()
        .run_tracing_sink()
        .expect_err("must have errored");
    assert_error_stack_snapshot!(&config_path, err);
}
