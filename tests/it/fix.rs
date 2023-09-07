use std::sync::RwLock;

use crate::{
    guard_integration_test,
    helper::{assert_equal_contents, copy_recursive, expand_debug_bundle},
    load_config, set_snapshot_vars, temp_config,
};
use broker::{
    cmd::fix::Logger,
    debug::{bundler::TarGz, Bundle, BundleExport},
};
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
    fn log<S: AsRef<str>>(&self, content: S) {
        self.output
            .write()
            .expect("write lock must not be poisoned")
            .push(content.as_ref().to_string());
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
    guard_integration_test!();

    let (_tmp, _, ctx) = temp_config!(load);

    set_snapshot_vars!();
    let (_, conf) = load_config!(
        "testdata/config/basic-http-no-auth.yml",
        "testdata/database/empty.sqlite"
    )
    .await;

    let logger = TestLogger::new();
    broker::cmd::fix::main(&ctx, &conf, &logger, BundleExport::Disable)
        .await
        .expect("should run fix");

    insta::with_settings!({ filters => fix_output_filters() },
       {
        assert_snapshot!(logger.output());
       }
    );
}

#[tokio::test]
async fn with_failing_http_no_auth_integration_scan() {
    guard_integration_test!();

    let (_tmp, _, ctx) = temp_config!(load);

    set_snapshot_vars!();
    let (_, conf) = load_config!(
        "testdata/config/basic-http-no-auth-empty-repo.yml",
        "testdata/database/empty.sqlite"
    )
    .await;

    let logger = TestLogger::new();
    broker::cmd::fix::main(&ctx, &conf, &logger, BundleExport::Disable)
        .await
        .expect("should run fix");

    insta::with_settings!({ filters => fix_output_filters() },
       {
        assert_snapshot!(logger.output());
       }
    );
}

#[tokio::test]
async fn with_failing_http_no_auth_download_cli() {
    guard_integration_test!();

    let (_, _, ctx) = temp_config!(load);

    set_snapshot_vars!();
    let (_, conf) = load_config!(
        "testdata/config/basic-http-no-auth-empty-repo.yml",
        "testdata/database/empty.sqlite"
    )
    .await;

    let logger = TestLogger::new();
    broker::cmd::fix::main(&ctx, &conf, &logger, BundleExport::Disable)
        .await
        .expect("should run fix");

    insta::with_settings!({ filters => fix_output_filters() },
       {
        assert_snapshot!(logger.output());
       }
    );
}

#[tokio::test]
async fn with_failing_http_basic_auth_integration() {
    guard_integration_test!();

    let (_, _, ctx) = temp_config!(load);

    set_snapshot_vars!();
    let (_, conf) = load_config!(
        "testdata/config/basic-http-basic-bad-repo-name.yml",
        "testdata/database/empty.sqlite"
    )
    .await;
    let logger = TestLogger::new();
    broker::cmd::fix::main(&ctx, &conf, &logger, BundleExport::Disable)
        .await
        .expect("should run fix");

    insta::with_settings!({ filters => fix_output_filters() },
       {
        assert_snapshot!(logger.output());
       }
    );
}

#[tokio::test]
async fn with_failing_http_no_auth_integration() {
    guard_integration_test!();

    let (_, _, ctx) = temp_config!(load);

    set_snapshot_vars!();
    let (_, conf) = load_config!(
        "testdata/config/private-repo-http-no-auth.yml",
        "testdata/database/empty.sqlite"
    )
    .await;

    let logger = TestLogger::new();
    broker::cmd::fix::main(&ctx, &conf, &logger, BundleExport::Disable)
        .await
        .expect("should run fix");

    insta::with_settings!({ filters => fix_output_filters() },
       {
        assert_snapshot!(logger.output());
       }
    );
}

#[tokio::test]
async fn generates_debug_bundle() {
    guard_integration_test!();

    let (tmp, conf, _ctx) = temp_config!(load);
    copy_recursive(
        "testdata/fossa.broker.debug/raw",
        conf.debug().location().as_path(),
    );

    let bundle_target = tmp.path().join("fossa.broker.debug.tar.gz");
    let bundler = TarGz::new().expect("must create bundler");
    let bundle =
        Bundle::collect(conf.debug(), bundler, bundle_target).expect("must collect debug bundle");

    let unpacked = expand_debug_bundle(bundle.location());
    assert_equal_contents("testdata/fossa.broker.debug/bundled", unpacked.path());
}
