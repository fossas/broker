//! Helper macros/functions for testing.
//!
//! Note: Rust macros are expanded in place as if the generated code was written in that file;
//! as such each macro in this file must be independent of location.
//! Mostly this just means "if the macro calls something else, it needs to reference it by fully qualified path".

pub mod duration;
pub mod gen;

/// Create a context in a temporary directory.
macro_rules! temp_ctx {
    () => {{
        let tmp = tempfile::tempdir().expect("must create tempdir");
        let root = tmp.path().to_path_buf();
        (tmp, broker::AppContext::new(root))
    }};
}

/// Create a config file inside a temporary directory `{TMP_DIR}`.
///
/// This config file:
/// - Has no configured integrations.
/// - Writes the config file to `{TMP_DIR}/config.yml`.
/// - Specifies `{TMP_DIR}/debug` as the debug root.
/// - Returns the `{TMP_DIR}`, along with the path to the config file.
///
/// If `load` is specified as an argument to this macro, the config is then also
/// parsed, and loaded, and returned. The data root in the config is set to `{TMP_DIR}`.
macro_rules! temp_config {
    () => {{
        let tmp = tempfile::tempdir().expect("must create tempdir");
        let dir = tmp.path().join("debug");
        let content = indoc::formatdoc! {r#"
        fossa_endpoint: https://app.fossa.com
        fossa_integration_key: abcd1234
        version: 1
        debugging:
          location: {dir:?}
          retention:
            days: 1
        integrations:
        "#};

        let path = tmp.path().join("config.yml");
        std::fs::write(&path, content).expect("must write config file");

        println!(
            "wrote config to {path:?}: {}",
            std::fs::read_to_string(&path).expect("must read config")
        );

        (tmp, path)
    }};
    (load) => {{
        let (tmp, config_file_path) = temp_config!();
        let raw_args = broker::config::RawRunArgs::new(
            Some(config_file_path.to_string_lossy().to_string()),
            None, // Infer the DB path to be a sibling of the config file.
            Some(tmp.path().to_path_buf()),
        );

        let args = raw_args.validate().await.expect("must have validated");
        let config = broker::config::load(&args).await.expect("must load config");
        (tmp, config, args.context().clone())
    }};
}

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
/// Automatically redacts the items in the error stack that we don't need to keep stable across tests.
/// Additionally sets the standard snapshot vars.
///
/// `context` should describe the program state that led to this error. Examples:
/// - When validating a config, `context` is the raw config struct.
/// - When parsing a config, `context` is the string being parsed.
///
/// # Usage
///
/// `([<modifier>]; context, err)`
///
/// Modifiers:
/// - `fossa_cli`: Set FOSSA CLI specific redactions, mostly around file paths.
/// - `data_root => {data_root}`: Redact the data root set at runtime.
///
/// Modifiers are separated by semicolon and must come before `context` and `err`.
/// They also must be listed in the order above if present.
///
/// Examples:
/// ```ignore
/// assert_error_stack_snapshot!(fossa_cli; data_root => ctx.data_root(); &config, err);
/// assert_error_stack_snapshot!(data_root => ctx.data_root(); &config, err);
/// ```
///
/// This is invalid, as they're in the wrong order:
/// ```ignore
/// assert_error_stack_snapshot!(data_root => ctx.data_root(); fossa_cli; &config, err);
/// ```
///
/// The ordering requirement is a quirk of how this macro is written in an attempt to keep it simple;
/// if we get 1-2 more modifiers to this macro we'll refactor to make it more generic.
//
// Implementation note
//
// If we need to make this more smart in the future, we should either:
// - Split into multiple macros.
// - Utilize 'push down accumulation' in combination with making this an 'incremental tt muncher'.
//   - https://danielkeep.github.io/tlborm/book/pat-push-down-accumulation.html
//   - https://danielkeep.github.io/tlborm/book/pat-incremental-tt-munchers.html
//
// This obviously makes the macro WAY more complicated, but is required to make it smarter:
// - Handling many modifiers in arbitrary order requires incremental tt munching.
// - Building the vector of filters during that incremental process requires push down accumulation.
//
macro_rules! assert_error_stack_snapshot {
    // Handle the `fossa_cli` case.
    (fossa_cli; $context:expr, $inner:expr) => {{
        let extra_filters = assert_error_stack_snapshot!(@cli_filters);
        assert_error_stack_snapshot!(@run extra_filters => $context, $inner);
    }};
    // Handle the `fossa_cli` + `data_root` case.
    (fossa_cli; data_root => $data_root:expr; $context:expr, $inner:expr) => {{
        let default_filters = assert_error_stack_snapshot!(@default_filters);
        let extra_filters = assert_error_stack_snapshot!(@cli_filters);
        let filters = assert_error_stack_snapshot!(@combine_filters default_filters, extra_filters);
        assert_error_stack_snapshot!(@run filters; data_root => $data_root; $context, $inner);
    }};
    // Handle the `data_root` case.
    (data_root => $data_root:expr; $context:expr, $inner:expr) => {{
        let filters = assert_error_stack_snapshot!(@default_filters);
        assert_error_stack_snapshot!(@run filters; data_root => $data_root; $context, $inner);
    }};
    // Handle with no modifiers.
    ($context:expr, $inner:expr) => {{
        let filters = assert_error_stack_snapshot!(@default_filters);
        assert_error_stack_snapshot!(@run filters; $context, $inner);
    }};
    // Build default filters.
    (@default_filters) => {{
        // The intent is to filter output that changes per test environment
        // or is inconsequential to the human understanding of the error message.
        vec![
            // Some tests output Broker's version, but it should be abstracted.
            (env!("CARGO_PKG_VERSION"), "{current broker version}"),
            // github gives different errors depending on whether you are logged in or not
            (r"(git@github.com|ERROR): (Permission denied \(publickey\)|Repository not found)", "{permission denied}"),
            // Rust source locations (`at /some/path/to/src/internal/foo.rs:81:82`)
            (r"at .*src.+:\d+:\d+", "at {source location}"),
            // Unix-style abs file paths inside common delimiters (`'/Users/jessica/.config/fossa/broker/queue/Echo'`)
            (r#"['"`](?:/[^/\pC]+)+['"`]"#, "{file path}"),
            // Windows-style abs file paths inside common delimiters (`'C:\Users\jessica\.config\fossa\broker\queue\Echo'`)
            (r#"['"`]\PC:(?:\\[^\\\pC]+)+['"`]"#, "{file path}"),
            // Some tests are based on the current workspace directory,
            // but this path should be abstracted.
            // Ensure this is below other file path filters, they should take precendence;
            // this is only a stopgap for if paths are not properly redacted.
            (env!("CARGO_MANIFEST_DIR"), "{cargo dir}"),
            // Paths to binaries are also platform dependent.
            // Just strip .exe off any path.
            (r"\.exe", ""),
        ]
    }};
    // Build FOSSA CLI filters.
    (@cli_filters) => {{
        // standardize some CLI path output, since FOSSA CLI doesn't enclose some paths in quotes.
        vec![
            (r"Directory does not exist: [^\n]+", "Directory does not exist: {directory}"),
            // This path is in quotes, but it (incorrectly) doubles the backslashes, which we don't want to add to the
            // usual path filters because we want to catch incorrectly escaped backslashes.
            (r#"\[DEBUG\] Loading configuration file from ".+""#, r#"[DEBUG] Loading configuration file from "{config path}""#)
        ]
    }};
    // Combine multiple filter vecs into one.
    (@combine_filters $($filter_group:expr),*) => {{
        let mut combined = vec![];
        $(
            for extra_filter in $filter_group {
                combined.push(extra_filter);
            }
        )*
        combined
    }};
    // Run with the provided filters.
    (@run $filters:expr; $context:expr, $inner:expr) => {{
        crate::helper::set_snapshot_vars!();
        insta::with_settings!({
            info => $context,
            filters => $filters,
        }, {
            insta::assert_debug_snapshot!($inner);
        });
    }};
    // Run with filters and data root.
    // Data root can't be integrated into filters since it relies on the `as_str` method of the `Regex` type,
    // which doesn't live long enough otherwise.
    (@run $filters:expr; data_root => $data_root:expr; $context:expr, $inner:expr) => {{
        crate::helper::set_snapshot_vars!();

        // Filter the data root.
        // Additionally normalize trailing slash/backslashes to slash, since these are platform dependent.
        let mut filters = $filters;
        let data_root = PathBuf::from($data_root).to_string_lossy().to_string();
        let data_root = regex::escape(&data_root);

        // This will turn into a regular expression, so escape the backslash.
        let with_trailing_slash = regex::Regex::new(&format!("{}(/|\\\\)", data_root)).expect("must create valid regex");
        filters.push((with_trailing_slash.as_str(), "{data root}/"));
        filters.push((data_root.as_str(), "{data root}"));

        insta::with_settings!({
            info => $context,
            filters => filters,
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
            let base = crate::args::raw_base_args($config_path, $db_path);
            let args = base.validate().await.expect("must have validated");
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
            let args = base.validate().await.expect("must have validated args");
            let err = broker::config::load(&args)
                .await
                .expect_err("must have failed to validate config");
            ($config_path, err)
        }
    };
}

use std::{
    fs::{self, File},
    path::Path,
};

pub(crate) use assert_error_stack_snapshot;
use libflate::gzip;
pub(crate) use load_config;
pub(crate) use load_config_err;
pub(crate) use set_snapshot_vars;
pub(crate) use temp_config;
pub(crate) use temp_ctx;
use tempfile::TempDir;

#[track_caller]
pub fn copy_recursive<P, Q>(source: P, dest: Q)
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    let source = source.as_ref();
    let dest = dest.as_ref();

    for entry in walkdir::WalkDir::new(source).follow_links(false) {
        let entry = entry.expect("must walk source dir");
        let rel = entry
            .path()
            .strip_prefix(source)
            .expect("walked content must be children of source dir");

        let target = dest.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(target).expect("must create destination dir");
        } else if entry.file_type().is_file() {
            std::fs::copy(entry.path(), target).expect("must copy file to destination dir");
        }
    }
}

#[track_caller]
pub fn assert_equal_contents<P, Q>(source: P, dest: Q)
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    let source = source.as_ref();
    let dest = dest.as_ref();

    for entry in walkdir::WalkDir::new(source).follow_links(false) {
        let entry = entry.expect("must walk source dir");
        if !entry.file_type().is_file() {
            continue;
        }

        let rel = entry
            .path()
            .strip_prefix(source)
            .expect("walked content must be children of source dir");

        let a = fs::read_to_string(entry.path()).expect("must read source file");
        let b = fs::read_to_string(dest.join(rel).as_path()).expect("must read dest file");

        // Compare content with normalized line endings
        // so that tests don't get tripped up over platform differences.
        assert_eq!(
            normalize_line_endings(a),
            normalize_line_endings(b),
            "file contents must be equivalent for '{}'",
            entry.path().display()
        );
    }
}

#[track_caller]
pub fn expand_debug_bundle<P: AsRef<Path>>(bundle: P) -> TempDir {
    let handle = File::open(bundle).expect("must open file");
    let decompressed = gzip::Decoder::new(handle).expect("must be a gzip file");
    let mut archive = tar::Archive::new(decompressed);

    let dir = TempDir::new().expect("must create temp dir");
    archive.unpack(dir.path()).expect("must unpack archive");

    dir
}

#[track_caller]
fn normalize_line_endings(input: String) -> String {
    input.replace("\r\n", "\n")
}
