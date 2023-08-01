//! Tests for debugging functionality.

use broker::debug::ArtifactRetentionCount;
use proptest::{prop_assert, prop_assert_eq};
use test_strategy::proptest;

use crate::assert_error_stack_snapshot;

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
