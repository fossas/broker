use crate::helper::load_config;
use broker::subcommand::fix::Logger;

/// A logger that prints to stdout and also keeps track of what has been logged so that we can test it
struct TestLogger {
    output: String,
}

impl TestLogger {
    fn output(&self) -> String {
        self.output.clone()
    }

    fn new() -> Self {
        TestLogger {
            output: "".to_string(),
        }
    }
}

impl Logger for TestLogger {
    fn log(&mut self, content: &str) {
        println!("{content}");
        self.output.push_str(content);
    }
}

#[tokio::test]
async fn with_http_no_auth_integration() {
    let (_, conf) = load_config!(
        "testdata/config/basic-http-no-auth.yml",
        "testdata/database/empty.sqlite"
    )
    .await;
    // let output: String = "".to_string();
    let mut logger = TestLogger::new();
    broker::subcommand::fix::main(&conf, &mut logger)
        .await
        .expect("should run fix");
    println!("output after:\n{:?}", logger.output());
    assert!(false);
}
