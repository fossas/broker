//! Helpers for generating test values.

use std::{path::PathBuf, time::Duration};

use broker::{
    api::{self, code},
    debug,
    ext::secrecy::ComparableSecretString,
};
use bytesize::ByteSize;
use humantime::parse_duration;
use url::Url;

#[track_caller]
pub(crate) fn fossa_api_key(val: &str) -> api::fossa::Key {
    api::fossa::Key::new(ComparableSecretString::from(String::from(val)))
}

#[track_caller]
pub(crate) fn fossa_api_endpoint(val: &str) -> api::fossa::Endpoint {
    api::fossa::Endpoint::new(Url::parse(val).unwrap_or_else(|_| panic!("must parse {val}")))
}

#[track_caller]
pub(crate) fn debug_root(val: &str) -> debug::Root {
    debug::Root::new(PathBuf::from(String::from(val)))
}

#[track_caller]
pub(crate) fn debug_artifact_max_age(val: &str) -> debug::ArtifactMaxAge {
    debug::ArtifactMaxAge::from(duration(val))
}

#[track_caller]
pub(crate) fn debug_artifact_max_size(val: ByteSize) -> debug::ArtifactMaxSize {
    debug::ArtifactMaxSize::from(val)
}

#[track_caller]
pub(crate) fn code_poll_interval(val: &str) -> code::PollInterval {
    code::PollInterval::from(duration(val))
}

#[track_caller]
pub(crate) fn code_remote(val: &str) -> api::code::Remote {
    api::code::Remote::new(String::from(val))
}

#[track_caller]
pub(crate) fn path_buf(val: &str) -> PathBuf {
    PathBuf::from(String::from(val))
}

#[track_caller]
pub(crate) fn secret(val: &str) -> ComparableSecretString {
    ComparableSecretString::from(String::from(val))
}

#[track_caller]
pub(crate) fn duration(val: &str) -> Duration {
    parse_duration(val).expect("must have parsed test duration")
}
