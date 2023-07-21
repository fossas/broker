//! Tests for `api::code` functionality.

use broker::api::remote::{PollInterval, Remote};
use proptest::{prop_assert, prop_assert_eq};
use test_strategy::proptest;

use crate::{assert_error_stack_snapshot, helper::duration::DurationInput};

#[proptest]
fn validate_poll_interval(input: DurationInput) {
    let user_input = input.to_string();
    match PollInterval::try_from(user_input.clone()) {
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
fn validate_poll_interval_empty() {
    let input = String::from("");
    assert_error_stack_snapshot!(
        &input,
        PollInterval::try_from(input).expect_err("must have failed validation")
    )
}

#[proptest]
fn validate_remote(#[strategy("\\PC+")] input: String) {
    match Remote::try_from(input.clone()) {
        Ok(validated) => prop_assert_eq!(validated.as_ref(), &input, "tested input: {:?}", input),
        Err(err) => prop_assert!(
            false,
            "unexpected parsing error '{err:#}' for input '{input}'"
        ),
    }
}

#[test]
fn validate_remote_empty() {
    let input = String::from("");
    assert_error_stack_snapshot!(
        &input,
        Remote::try_from(input).expect_err("must have failed validation")
    )
}
