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
    () => {
        // During error stack snapshot testing, colors really mess with readability.
        // While colors are an important part of the overall error message story,
        // they're less important than structure; the thought is that by making structure easier to test
        // we can avoid most failures. Colors, by comparison, are harder to accidentally change.
        error_stack::Report::set_color_mode(error_stack::fmt::ColorMode::None);
        colored::control::set_override(false);
    };
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
    ($context:expr, $inner:expr) => {
        crate::helper::set_snapshot_vars!();
        insta::with_settings!({
            // The program state that led to this error.
            info => $context,
            // Don't fail the snapshot on source code location changes.
            filters => vec![(r"src.+:\d+:\d+", "{source location}")]
        }, {
            insta::assert_debug_snapshot!($inner);
        });
    };
}

pub(crate) use assert_error_stack_snapshot;
pub(crate) use set_snapshot_vars;
