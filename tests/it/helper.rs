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
        let base_args = RawBaseArgs::new(
            Some(config_file_path.to_string_lossy().to_string()),
            None, // Infer the DB path to be a sibling of the config file.
            Some(tmp.path().to_path_buf()),
        );

        let args = broker::config::validate_args(base_args)
            .await
            .expect("must have validated");

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
/// Automatically redacts the source code location in the error stack since that's
/// not something we care about keeping stable.
/// Additionally sets the standard snapshot vars.
///
/// `context` should describe the program state that led to this error. Examples:
/// - When validating a config, `context` is the raw config struct.
/// - When parsing a config, `context` is the string being parsed.
macro_rules! assert_error_stack_snapshot {
    (@default_filters) => {{
        // The intent is to filter output that changes per test environment
        // or is inconsequential to the human understanding of the error message.
        vec![
            // Some tests are based on the current workspace directory,
            // but this path should be abstracted.
            (env!("CARGO_MANIFEST_DIR"), "{cargo dir}"),
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
        ]
    }};
    ($context:expr, $inner:expr) => {{
        crate::helper::set_snapshot_vars!();
        let filters = assert_error_stack_snapshot!(@default_filters);

        insta::with_settings!({
            info => $context,
            filters => filters,
        }, {
            insta::assert_debug_snapshot!($inner);
        });
    }};
    ($context:expr, $inner:expr, $data_root:expr) => {{
        crate::helper::set_snapshot_vars!();

        let mut filters = assert_error_stack_snapshot!(@default_filters);
        let data_root = $data_root.to_string_lossy().to_string();

        // Using this path as a regex input. Escape backslashes.
        let data_root = data_root.replace(r#"\"#, r#"\\"#);
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
pub(crate) use temp_config;
pub(crate) use temp_ctx;
