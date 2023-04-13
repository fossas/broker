use std::sync::RwLock;

use crate::helper::{load_config, set_snapshot_vars};
use broker::subcommand::fix::Logger;
use insta::assert_snapshot;

/// A logger that prints to stdout and also keeps track of what has been logged so that we can test it
#[derive(Default)]
struct TestLogger {
    output: RwLock<Vec<String>>,
}

impl TestLogger {
    fn output(&self) -> String {
        self.output
            .read()
            .expect("read lock must not be poisoned")
            .join("\n")
    }

    fn new() -> Self {
        Default::default()
    }
}

impl Logger for TestLogger {
    fn log(&self, content: &str) {
        self.output
            .write()
            .expect("write lock must not be poisoned")
            .push(content.to_string());
    }
}

/// git gives slightly different output in CI and locally. These filters hide that difference.
fn fix_output_filters() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "fatal: could not read Username for 'https://github.com': terminal prompts disabled",
            "{git authentication or missing repo error}",
        ),
        (
            r"remote: Repository not found.\s*fatal: repository '[^']*' not found",
            "{git authentication or missing repo error}",
        ),
    ]
}

#[tokio::test]
async fn with_successful_http_no_auth_integration() {
    set_snapshot_vars!();
    let (_, conf) = load_config!(
        "testdata/config/basic-http-no-auth.yml",
        "testdata/database/empty.sqlite"
    )
    .await;

    let logger = TestLogger::new();
    broker::subcommand::fix::main(&conf, &logger)
        .await
        .expect("should run fix");

    insta::with_settings!({ filters => fix_output_filters()},
       {
        assert_snapshot!(logger.output());
       }
    );
}

#[tokio::test]
async fn with_failing_http_basic_auth_integration() {
    set_snapshot_vars!();
    let (_, conf) = load_config!(
        "testdata/config/basic-http-basic-bad-repo-name.yml",
        "testdata/database/empty.sqlite"
    )
    .await;
    let logger = TestLogger::new();
    broker::subcommand::fix::main(&conf, &logger)
        .await
        .expect("should run fix");

    insta::with_settings!({ filters => fix_output_filters()},
       {
        assert_snapshot!(logger.output());
       }
    );
}

#[tokio::test]
async fn with_failing_http_no_auth_integration() {
    set_snapshot_vars!();
    let (_, conf) = load_config!(
        "testdata/config/private-repo-http-no-auth.yml",
        "testdata/database/empty.sqlite"
    )
    .await;

    let logger = TestLogger::new();
    broker::subcommand::fix::main(&conf, &logger)
        .await
        .expect("should run fix");

    insta::with_settings!({ filters => fix_output_filters()},
       {
        assert_snapshot!(logger.output());
       }
    );
}
