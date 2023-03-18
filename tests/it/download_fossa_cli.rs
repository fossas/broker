//! Tests for download_fossa_cli
use std::env;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

// use crate::helper::assert_error_stack_snapshot;
use crate::helper::load_config;
use broker::download_fossa_cli;

#[tokio::test]
async fn download_fossa_cli() {
    let (_, conf) = load_config!(
        "testdata/config/fossa-one-http-no-auth.yml",
        "testdata/database/empty.sqlite"
    )
    .await;

    let command_name = match std::env::consts::OS {
        "windows" => "fossa.exe".to_string(),
        _ => "fossa".to_string(),
    };

    // Setup for case when no fossa exists in the path or in the config dir
    env::set_var("PATH", "");
    // remove_file will return an Err if expected_path does not exist, but that's ok()
    let fossa_in_config_path = Path::join(conf.directory(), &command_name);
    fs::remove_file(&fossa_in_config_path).ok();
    let actual_path = download_fossa_cli::ensure_fossa_cli(conf.directory())
        .await
        .expect("download from github failed");

    // assertions
    assert_eq!(fossa_in_config_path, actual_path);
    assert!(fossa_in_config_path.exists());

    // cleanup
    fs::remove_file(&fossa_in_config_path).ok();

    // setup for case where fossa already exists in the path
    env::set_var("PATH", "/Users/scott/fossa/broker/testdata/fakebins");
    println!("PATH env var = {:?}", env::var("PATH"));
    // there is a fake fossa in the testdata/fake_bins path
    let actual_path = download_fossa_cli::ensure_fossa_cli(conf.directory())
        .await
        .expect("download from github failed");

    let expected_path = PathBuf::from(&command_name);
    let expected_path = expected_path.as_path();

    assert_eq!(expected_path, actual_path);
    assert!(!fossa_in_config_path.exists());
}
