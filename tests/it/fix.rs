use crate::helper::{load_config, set_snapshot_vars};
use broker::subcommand::fix::Logger;
use insta::assert_debug_snapshot;

/// A logger that prints to stdout and also keeps track of what has been logged so that we can test it
struct TestLogger {
    output: Vec<String>,
}

impl TestLogger {
    fn output(&self) -> String {
        self.output.join("")
    }

    fn new() -> Self {
        TestLogger { output: vec![] }
    }
}

impl Logger for TestLogger {
    fn log(&mut self, content: &str) {
        println!("{content}");
        self.output.push(content.to_string());
    }
}

#[tokio::test]
async fn with_successful_http_no_auth_integration() {
    set_snapshot_vars!();
    let (_, conf) = load_config!(
        "testdata/config/basic-http-no-auth.yml",
        "testdata/database/empty.sqlite"
    )
    .await;

    let mut logger = TestLogger::new();
    broker::subcommand::fix::main(&conf, &mut logger)
        .await
        .expect("should run fix");
    assert_debug_snapshot!(logger.output());
}

#[tokio::test]
async fn with_failing_http_basic_auth_integration() {
    set_snapshot_vars!();
    let (_, conf) = load_config!(
        "testdata/config/basic-http-basic.yml",
        "testdata/database/empty.sqlite"
    )
    .await;

    let mut logger = TestLogger::new();
    broker::subcommand::fix::main(&conf, &mut logger)
        .await
        .expect("should run fix");
    assert_debug_snapshot!(logger.output());
}

#[tokio::test]
async fn with_failing_http_no_auth_integration() {
    set_snapshot_vars!();
    let (_, conf) = load_config!(
        "testdata/config/private-repo-http-no-auth.yml",
        "testdata/database/empty.sqlite"
    )
    .await;

    let mut logger = TestLogger::new();
    broker::subcommand::fix::main(&conf, &mut logger)
        .await
        .expect("should run fix");
    assert_debug_snapshot!(logger.output());
}

#[tokio::test]
async fn with_failing_ssh_keyfile_integration() {
    set_snapshot_vars!();
    let (_, conf) = load_config!(
        "testdata/config/basic-ssh-key.yml",
        "testdata/database/empty.sqlite"
    )
    .await;

    let mut logger = TestLogger::new();
    broker::subcommand::fix::main(&conf, &mut logger)
        .await
        .expect("should run fix");
    assert_debug_snapshot!(logger.output());
}
