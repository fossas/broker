use std::path::PathBuf;

use broker::fossa_cli::{self, DesiredVersion, Location};
use tracing_test::traced_test;
use uuid::Uuid;

use crate::{assert_error_stack_snapshot, guard_integration_test, temp_config};

#[tokio::test]
async fn downloads_latest_cli() {
    guard_integration_test!();

    let (_tmp, config, ctx) = temp_config!(load);

    println!("Downloading CLI");
    let location = fossa_cli::download(&ctx, config.debug().location(), DesiredVersion::Latest)
        .await
        .expect("must download CLI");

    println!("Checking versions");
    let (downloaded, latest) =
        tokio::try_join!(location.version(), fossa_cli::latest_release_version())
            .expect("must fetch version information");

    assert_eq!(
        downloaded.to_string(),
        latest,
        "version downloaded must be latest"
    );
}

#[tokio::test]
#[traced_test]
async fn analyze_runs() {
    guard_integration_test!();

    let (_tmp, config, ctx) = temp_config!(load);
    let scan_id = Uuid::new_v4().to_string();
    let project = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("testdata")
        .join("fossa-analyze");

    println!("Downloading CLI");
    let location = fossa_cli::download(&ctx, config.debug().location(), DesiredVersion::Latest)
        .await
        .expect("must download CLI");

    // Scan our vendored node project to speed up tests.
    println!("Analyzing '{}' with scan id '{scan_id}'", project.display());
    let source_units = location
        .analyze(&scan_id, &project)
        .await
        .expect("must analyze");

    // The debug bundle should be in the correct location: '{DEBUG_ROOT}/{SCAN_ID}.fossa.debug.json.gz'.
    let debug_bundle_location = config.debug().location().debug_bundle(&scan_id);
    assert!(
        debug_bundle_location.exists(),
        "must have moved debug bundle to correct location ({debug_bundle_location:?})",
    );

    // The CLI should have found source units, and its logs should have been captured in the traces.
    assert!(!source_units.is_empty(), "must have source units");
    assert!(
        logs_contain("[DEBUG] [TASK"),
        "must have traced CLI debug logs"
    );
    assert!(logs_contain("Scan Summary"), "must have traced CLI logs");
}

#[tokio::test]
async fn analyze_fails() {
    guard_integration_test!();

    let (_tmp, config, ctx) = temp_config!(load);
    let scan_id = Uuid::new_v4().to_string();

    // Provide a path that doesn't exist, so that analysis fails.
    let project = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("testdata")
        .join("fossa-analyze-does-not-exist");

    println!("Downloading CLI");
    let location = fossa_cli::download(&ctx, config.debug().location(), DesiredVersion::Latest)
        .await
        .expect("must download CLI");

    // Scan our path that does not exist.
    println!("Analyzing '{}' with scan id '{scan_id}'", project.display());
    let err = location
        .analyze(&scan_id, &project)
        .await
        .expect_err("must fail to analyze");

    // Snapshot the error message.
    assert_error_stack_snapshot!(
        fossa_cli;
        data_root => ctx.data_root();
        &ctx.data_root().to_string_lossy().to_string(),
        err
    );
}

/// Analysis of a dynamic-only project should fail, since dynamic strategies are disabled.
/// Rust is dynamic only, so analyze Broker itself.
#[tokio::test]
async fn analyze_fails_dynamic() {
    guard_integration_test!();

    let (_tmp, config, ctx) = temp_config!(load);
    let scan_id = Uuid::new_v4().to_string();

    // Provide a path that doesn't exist, so that analysis fails.
    let project = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    println!("Downloading CLI");
    let location = fossa_cli::download(&ctx, config.debug().location(), DesiredVersion::Latest)
        .await
        .expect("must download CLI");

    // Scan our project.
    println!("Analyzing '{}' with scan id '{scan_id}'", project.display());
    let analysis_results = location
        .analyze(&scan_id, &project)
        .await
        .expect("Must successfully run");

    assert!(analysis_results.is_empty());
}

#[tokio::test]
async fn parse_version_fails() {
    guard_integration_test!();

    let (_tmp, config, ctx) = temp_config!(load);

    // Pretend Broker is FOSSA CLI
    println!("Copying broker into data root as if it was FOSSA CLI");
    let broker_path = PathBuf::from(env!("CARGO_BIN_EXE_broker"));
    let cli_path = ctx.data_root().join("fossa");
    tokio::fs::copy(&broker_path, &cli_path)
        .await
        .expect("must copy broker");

    // Try to parse the version, and snapshot the error.
    let cli = Location::new(cli_path, config.debug().location());
    let err = cli.version().await.expect_err("must fail to parse version");
    assert_error_stack_snapshot!(
        fossa_cli;
        data_root => ctx.data_root();
        &ctx.data_root().to_string_lossy().to_string(),
        err
    );
}
