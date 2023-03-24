use std::path::PathBuf;

use broker::{
    config::RawBaseArgs,
    fossa_cli::{self, DesiredVersion},
};
use tracing_test::traced_test;
use uuid::Uuid;

use crate::helper::temp_config;

#[tokio::test]
async fn downloads_latest_cli() {
    let (_tmp, config, ctx) = temp_config!(load);

    println!("Downloading CLI");
    let location = fossa_cli::download(&ctx, config.debug().location(), DesiredVersion::Latest)
        .await
        .expect("must download CLI");

    println!("Checking versions");
    let (downloaded, latest) =
        tokio::try_join!(location.version(), fossa_cli::latest_release_version())
            .expect("must fetch version information");
    assert_eq!(downloaded, latest, "version downloaded must be latest");
}

#[tokio::test]
#[traced_test]
async fn analyze_runs() {
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
    assert!(
        logs_contain("[ INFO] Scan Summary"),
        "must have traced CLI logs"
    );
}
