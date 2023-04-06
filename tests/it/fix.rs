/// A logger that prints to stdout and also keeps track of what has been logged so that we can test it
struct TestLogger;

impl Logger for TestLogger {
    fn log(&self, content: &str) {
        println!("{content}");
    }
}
#[tokio::test]
async fn with_http_no_auth_integration() {
    let (_, conf) = load_config!(
        "testdata/config/basic-http-no-auth.yml",
        "testdata/database/empty.sqlite"
    )
    .await;
    broker::subcommand::fix::main(config, &TestLogger).expect("should run fix");
}
