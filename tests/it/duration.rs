//! Tests for duration validators.
//!
//! The idea is that we define a bunch of types representing the values we want to support
//! (these are the `TimeUnit` enums below).
//! Then we define an input struct, which is filled with arbitrary values and time units.
//!
//! Finally we define a property based test which tests those generated values.

use std::{fmt::Display, time::Duration};

use broker::{
    api::code::PollInterval,
    debug::{ArtifactMaxAge, MIN_RETENTION_AGE},
};
use proptest::prelude::*;
use strum::Display;
use test_strategy::{proptest, Arbitrary};

use crate::helper::assert_error_stack_snapshot;

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

// ---- Below this line are types used to generate test cases ----

/// Generates inputs for the validator.
///
/// These are all u8, not because we necessarily expect users to give only u8-sized values,
/// but more because `humantime` has its own tests and we're really just doing a bit of fuzzy testing on top
/// to double check that everything works.
///
/// The main complexity with using larger values is that the overall size can't overflow `Duration`
/// (which is a u64 of seconds + u32 of Nanoseconds), and since `humantime` has its own tests
/// it's not really worth our time to make our test case generator capable of generating cases
/// that are "up to but not over the limit" (especially since our durations aren't expected to be so large).
#[derive(Debug, Default, Eq, PartialEq, Arbitrary)]
#[filter(#self.is_valid())]
struct DurationInput {
    ns: Option<(u8, TimeUnitsNanoseconds)>,
    us: Option<(u8, TimeUnitsMicroseconds)>,
    ms: Option<(u8, TimeUnitsMilliseconds)>,
    sec: Option<(u8, TimeUnitsSeconds)>,
    min: Option<(u8, TimeUnitsMinutes)>,
    hr: Option<(u8, TimeUnitsHours)>,
    day: Option<(u8, TimeUnitsDays)>,
    wk: Option<(u8, TimeUnitsWeeks)>,
    mo: Option<(u8, TimeUnitsMonths)>,
    yr: Option<(u8, TimeUnitsYears)>,
}

impl DurationInput {
    fn builder(&self) -> DurationBuilder {
        DurationBuilder::new()
            .add(self.ns)
            .add(self.us)
            .add(self.ms)
            .add(self.sec)
            .add(self.min)
            .add(self.hr)
            .add(self.day)
            .add(self.wk)
            .add(self.mo)
            .add(self.yr)
    }

    fn expected_duration(&self) -> Duration {
        self.builder().sum()
    }

    fn is_valid(&self) -> bool {
        !self.is_empty()
    }

    fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

impl Display for DurationInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        macro_rules! print_option_pair {
            ($pair:expr) => {
                if let Some((ref a, ref b)) = $pair {
                    write!(f, "{a}{b}")?;
                }
            };
        }

        print_option_pair!(self.ns);
        print_option_pair!(self.us);
        print_option_pair!(self.ms);
        print_option_pair!(self.sec);
        print_option_pair!(self.min);
        print_option_pair!(self.hr);
        print_option_pair!(self.day);
        print_option_pair!(self.wk);
        print_option_pair!(self.mo);
        print_option_pair!(self.yr);

        Ok(())
    }
}

trait IntoDuration {
    fn into_duration(self, units: u8) -> Duration;
}

struct DurationBuilder {
    durations: Vec<Duration>,
}

impl DurationBuilder {
    fn new() -> Self {
        Self {
            durations: Vec::new(),
        }
    }

    fn add<T: IntoDuration>(mut self, next: Option<(u8, T)>) -> Self {
        if let Some((units, time_unit)) = next {
            self.durations.push(time_unit.into_duration(units));
        }
        self
    }

    fn sum(self) -> Duration {
        self.durations.into_iter().sum()
    }
}

#[derive(Debug, Clone, Copy, Arbitrary, Display, Eq, PartialEq)]
enum TimeUnitsNanoseconds {
    #[strum(serialize = "nsec")]
    Nsec,
    #[strum(serialize = "ns")]
    Ns,
}

impl IntoDuration for TimeUnitsNanoseconds {
    fn into_duration(self, units: u8) -> Duration {
        let units = units as u64;
        Duration::from_nanos(units)
    }
}

#[derive(Debug, Clone, Copy, Arbitrary, Display, Eq, PartialEq)]
enum TimeUnitsMicroseconds {
    #[strum(serialize = "usec")]
    Usec,
    #[strum(serialize = "us")]
    Us,
}

impl IntoDuration for TimeUnitsMicroseconds {
    fn into_duration(self, units: u8) -> Duration {
        let units = units as u64;
        Duration::from_micros(units)
    }
}

#[derive(Debug, Clone, Copy, Arbitrary, Display, Eq, PartialEq)]
enum TimeUnitsMilliseconds {
    #[strum(serialize = "msec")]
    Msec,
    #[strum(serialize = "ms")]
    Ms,
}

impl IntoDuration for TimeUnitsMilliseconds {
    fn into_duration(self, units: u8) -> Duration {
        let units = units as u64;
        Duration::from_millis(units)
    }
}

#[derive(Debug, Clone, Copy, Arbitrary, Display, Eq, PartialEq)]
enum TimeUnitsSeconds {
    #[strum(serialize = "seconds")]
    Seconds,
    #[strum(serialize = "second")]
    Second,
    #[strum(serialize = "sec")]
    Sec,
    #[strum(serialize = "s")]
    S,
}

impl IntoDuration for TimeUnitsSeconds {
    fn into_duration(self, units: u8) -> Duration {
        let units = units as u64;
        Duration::from_secs(units)
    }
}

#[derive(Debug, Clone, Copy, Arbitrary, Display, Eq, PartialEq)]
enum TimeUnitsMinutes {
    #[strum(serialize = "minutes")]
    Minutes,
    #[strum(serialize = "minute")]
    Minute,
    #[strum(serialize = "min")]
    Min,
    #[strum(serialize = "m")]
    M,
}

impl IntoDuration for TimeUnitsMinutes {
    fn into_duration(self, units: u8) -> Duration {
        let units = units as u64;
        Duration::from_secs(units * 60)
    }
}

#[derive(Debug, Clone, Copy, Arbitrary, Display, Eq, PartialEq)]
enum TimeUnitsHours {
    #[strum(serialize = "hours")]
    Hours,
    #[strum(serialize = "hour")]
    Hour,
    #[strum(serialize = "hr")]
    Hr,
    #[strum(serialize = "h")]
    H,
}

impl IntoDuration for TimeUnitsHours {
    fn into_duration(self, units: u8) -> Duration {
        let units = units as u64;
        Duration::from_secs(units * 3600)
    }
}

#[derive(Debug, Clone, Copy, Arbitrary, Display, Eq, PartialEq)]
enum TimeUnitsDays {
    #[strum(serialize = "days")]
    Days,
    #[strum(serialize = "day")]
    Day,
    #[strum(serialize = "d")]
    D,
}

impl IntoDuration for TimeUnitsDays {
    fn into_duration(self, units: u8) -> Duration {
        let units = units as u64;
        Duration::from_secs(units * 3600 * 24)
    }
}

#[derive(Debug, Clone, Copy, Arbitrary, Display, Eq, PartialEq)]
enum TimeUnitsWeeks {
    #[strum(serialize = "weeks")]
    Weeks,
    #[strum(serialize = "week")]
    Week,
    #[strum(serialize = "w")]
    W,
}

impl IntoDuration for TimeUnitsWeeks {
    fn into_duration(self, units: u8) -> Duration {
        let units = units as u64;
        Duration::from_secs(units * 3600 * 24 * 7)
    }
}

#[derive(Debug, Clone, Copy, Arbitrary, Display, Eq, PartialEq)]
enum TimeUnitsMonths {
    #[strum(serialize = "months")]
    Months,
    #[strum(serialize = "month")]
    Month,
    #[strum(serialize = "M")]
    M,
}

impl IntoDuration for TimeUnitsMonths {
    fn into_duration(self, units: u8) -> Duration {
        let units = units as f64;
        // Why 30.44? https://docs.rs/humantime/latest/humantime/fn.parse_duration.html
        Duration::from_secs_f64(units * 3600.0 * 24.0 * 30.44)
    }
}

#[derive(Debug, Clone, Copy, Arbitrary, Display, Eq, PartialEq)]
enum TimeUnitsYears {
    #[strum(serialize = "years")]
    Years,
    #[strum(serialize = "year")]
    Year,
    #[strum(serialize = "y")]
    Y,
}

impl IntoDuration for TimeUnitsYears {
    fn into_duration(self, units: u8) -> Duration {
        let units = units as f64;
        // Why 365.25? https://docs.rs/humantime/latest/humantime/fn.parse_duration.html
        Duration::from_secs_f64(units * 3600.0 * 24.0 * 365.25)
    }
}
