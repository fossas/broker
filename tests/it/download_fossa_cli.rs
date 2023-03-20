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
    let fossa_in_config_path = Path::join(conf.directory(), &command_name);
    fs::remove_file(&fossa_in_config_path).ok();
    temp_env::async_with_vars([("PATH", Some("aaaa"))], test_fn());

    // cleanup
    fs::remove_file(&fossa_in_config_path).ok();

    // setup for case where fossa already exists in the path
    println!("PATH env var = {:?}", env::var("PATH"));
    // there is a fake fossa in the testdata/fakebins path
    temp_env::async_with_vars(
        [("PATH", Some("/Users/scott/fossa/broker/testdata/fakebins"))],
        test_fn(),
    );

    assert!(!fossa_in_config_path.exists());
}

async fn test_fn() {
    let (_, conf) = load_config!(
        "testdata/config/fossa-one-http-no-auth.yml",
        "testdata/database/empty.sqlite"
    )
    .await;
    let actual_path = download_fossa_cli::ensure_fossa_cli(conf.directory())
        .await
        .expect("download from github failed");

    let command_name = match std::env::consts::OS {
        "windows" => "fossa.exe".to_string(),
        _ => "fossa".to_string(),
    };
    let expected_path = PathBuf::from(&command_name);
    let expected_path = expected_path.as_path();

    assert_eq!(expected_path, actual_path);
}
