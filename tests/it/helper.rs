//! Helper macros/functions for testing.
//!
//! Note: Rust macros are expanded in place as if the generated code was written in that file;
//! as such each macro in this file must be independent of location.
//! Mostly this just means "if the macro calls something else, it needs to reference it by fully qualified path".

/// Tests are run independently by cargo nextest, so this macro configures settings used in snapshot tests.
///
/// If using `assert_error_stack_snapshot`, there's no need to run this, as it is run automatically.
/// This macro is still exported for tests using `insta` directly.
macro_rules! set_snapshot_vars {
    () => {{
        // During error stack snapshot testing, colors really mess with readability.
        // While colors are an important part of the overall error message story,
        // they're less important than structure; the thought is that by making structure easier to test
        // we can avoid most failures. Colors, by comparison, are harder to accidentally change.
        error_stack::Report::set_color_mode(error_stack::fmt::ColorMode::None);
        colored::control::set_override(false);
    }};
}

/// Run an error stack snapshot.
///
/// Automatically redacts the source code location in the error stack since that's
/// not something we care about keeping stable.
/// Additionally sets the standard snapshot vars.
///
/// `context` should describe the program state that led to this error. Examples:
/// - When validating a config, `context` is the raw config struct.
/// - When parsing a config, `context` is the string being parsed.
macro_rules! assert_error_stack_snapshot {
    ($context:expr, $inner:expr) => {{
        crate::helper::set_snapshot_vars!();
        insta::with_settings!({
            // The program state that led to this error.
            info => $context,
            // Don't fail the snapshot on source code location changes.
            filters => vec![(r"src.+:\d+:\d+", "{source location}")]
        }, {
            insta::assert_debug_snapshot!($inner);
        });
    }};
}

/// Convenience macro to load the config inline with the test function (so errors are properly attributed).
///
/// Default paths are:
/// - Config: "testdata/config/basic.yml"
/// - Database: "testdata/database/empty.sqlite"
///
/// Leave args unspecified to use the defaults.
macro_rules! load_config {
    () => {
        load_config!(
            "testdata/config/basic.yml",
            "testdata/database/empty.sqlite"
        )
    };
    ($config_path:expr, $db_path:expr) => {{
        let base = raw_base_args($config_path, $db_path);
        let args = config::validate_args(base).expect("must have validated");
        config::load(&args).expect("must have loaded config")
    }};
}

pub(crate) use assert_error_stack_snapshot;
pub(crate) use load_config;
pub(crate) use set_snapshot_vars;

/// Helpers for generating test values
pub mod gen {
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
}
