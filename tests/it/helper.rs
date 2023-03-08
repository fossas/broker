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
                (r"src.+:\d+:\d+", "{source location}"),
                ("/var/folders[a-zA-Z0-9/_.]+", "{tmpdir}"), // Macos tmp folders
                ("/tmp/.[a-zA-Z-0-9/_.]+", "{tmpdir}"), // Unix tmp folders
                (r"(git@github.com|Error): (Permission denied \(publickey\)|Repository not found)", "{permission denied}") // github gives different errors depending on whether you are logged in or not
            ]
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
    ($config_path:expr, $db_path:expr) => {
        async {
            let base = raw_base_args($config_path, $db_path);
            let args = config::validate_args(base)
                .await
                .expect("must have validated");
            config::load(&args).await.expect("must have loaded config")
        }
    };
}

pub(crate) use assert_error_stack_snapshot;
pub(crate) use load_config;
pub(crate) use set_snapshot_vars;
