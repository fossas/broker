//! Tests for debugging functionality.

use broker::debug::{ArtifactMaxAge, ArtifactMaxSize, MIN_RETENTION_AGE, MIN_RETENTION_SIZE_BYTES};
use bytesize::ByteSize;
use proptest::prelude::*;
use test_strategy::proptest;

use crate::helper::{assert_error_stack_snapshot, duration::DurationInput};

#[proptest]
fn validate_artifact_max_age(
    #[by_ref]
    #[filter(#input.expected_duration() > MIN_RETENTION_AGE)]
    input: DurationInput,
) {
    let user_input = input.to_string();
    match ArtifactMaxAge::try_from(user_input.clone()) {
        Ok(validated) => prop_assert_eq!(
            validated.as_ref(),
            &input.expected_duration(),
            "tested input: {:?}",
            input
        ),
        Err(err) => prop_assert!(
            false,
            "unexpected parsing error '{err:#}' for input '{user_input}'"
        ),
    }
}

#[test]
fn validate_artifact_max_age_empty() {
    let input = String::from("");
    assert_error_stack_snapshot!(
        &input,
        ArtifactMaxAge::try_from(input).expect_err("must have failed validation")
    )
}

#[test]
fn validate_artifact_max_age_below_min() {
    let input = String::from("1ms");
    assert_error_stack_snapshot!(
        &input,
        ArtifactMaxAge::try_from(input).expect_err("must have failed validation")
    )
}

#[proptest]
fn validate_artifact_max_size(
    #[filter(#user_input as u64 > MIN_RETENTION_SIZE_BYTES)] user_input: u16,
) {
    let user_input = user_input as u64;
    match ArtifactMaxSize::try_from(user_input) {
        Ok(validated) => prop_assert_eq!(
            validated.as_ref(),
            &ByteSize::b(user_input),
            "tested input: {:?}",
            input
        ),
        Err(err) => prop_assert!(
            false,
            "unexpected parsing error '{err:#}' for input '{user_input}'"
        ),
    }
}

#[test]
fn validate_artifact_max_size_below_min() {
    let input = 100;
    assert_error_stack_snapshot!(
        &input,
        ArtifactMaxSize::try_from(input).expect_err("must have failed validation")
    )
}
