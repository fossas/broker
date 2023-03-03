//! Helper macros/functions for testing.
//!
//! Note: Rust macros are expanded in place as if the generated code was written in that file;
//! as such each macro in this file must be independent of location.
//! Mostly this just means "if the macro calls something else, it needs to reference it by fully qualified path".

pub mod duration;
pub mod gen;

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
            filters => vec![
                // Rust source locations (`at /some/path/to/src/internal/foo.rs:81:82`)
                (r"at .*src.+:\d+:\d+", "at {source location}"),
                // Unix-style abs file paths inside common delimiters (`'/Users/jessica/.config/fossa/broker/queue/Echo'`)
                (r#"['"`](?:/[^/\pC]+)+['"`]"#, "{file path}"),
                // Windows-style abs file paths inside common delimiters (`'C:\Users\jessica\.config\fossa\broker\queue\Echo'`)
                (r#"['"`]\PC:(?:\\[^\\\pC]+)+['"`]"#, "{file path}"),
            ]
        }, {
            insta::assert_debug_snapshot!($inner);
        });
    }};
}

// (?:["'`])(?:(?:[A-Z]:(?:\\[^\\\pC]+)+)|(?:[\pC]?(?:\/[^\/\pC]+)+))(?:["'`])

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
    ($config_path:expr, $db_path:expr) => {
        async {
            let base = crate::args::raw_base_args($config_path, $db_path);
            let args = broker::config::validate_args(base)
                .await
                .expect("must have validated");
            let config = broker::config::load(&args)
                .await
                .expect("must have loaded config");
            ($config_path, config)
        }
    };
}

/// Convenience macro to load a failing config inline with the test function (so errors are properly attributed).
macro_rules! load_config_err {
    ($config_path:expr, $db_path:expr) => {
        async {
            let base = crate::args::raw_base_args($config_path, $db_path);
            let args = broker::config::validate_args(base)
                .await
                .expect("must have validated args");
            let err = broker::config::load(&args)
                .await
                .expect_err("must have failed to validate config");
            ($config_path, err)
        }
    };
}

pub(crate) use assert_error_stack_snapshot;
pub(crate) use load_config;
pub(crate) use load_config_err;
pub(crate) use set_snapshot_vars;
