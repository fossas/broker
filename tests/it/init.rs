use std::{fs, path::PathBuf};

#[tokio::test]
async fn on_empty_dir_creates_config_and_example() {
    let tmpdir = tempfile::tempdir().unwrap();
    let tmpdir = PathBuf::from(tmpdir.path());
    broker::subcommand::init::main(&tmpdir).expect("should init");

    let config_file_path = tmpdir.join("config.yml");
    assert!(
      fs::read_to_string(config_file_path)
        .expect("should read config file")
        .starts_with("# This config file is read whenever broker starts, and contains all of the information that broker needs in order to work.")
    );

    let example_file_path = tmpdir.join("config.example.yml");
    assert!(
      fs::read_to_string(example_file_path)
        .expect("should read config.example file")
        .starts_with("# This config file is read whenever broker starts, and contains all of the information that broker needs in order to work.")
    );
}

#[tokio::test]
async fn when_config_files_exist_only_overwrites_example() {
    // setup: write config.yml and config.example.yml to the tempdir
    // init should overwrite config.example.yml but not config.yml
    let tmpdir = tempfile::tempdir().unwrap();
    let tmpdir = PathBuf::from(tmpdir.path());
    let config_file_path = tmpdir.join("config.yml");
    let example_file_path = tmpdir.join("config.example.yml");

    fs::write(&config_file_path, "hello").expect("should write config file");
    fs::write(&example_file_path, "hello").expect("should write config.example file");
    broker::subcommand::init::main(&tmpdir).expect("should init");

    assert_eq!(
        fs::read_to_string(&config_file_path).expect("should read config file"),
        "hello"
    );

    assert!(
      fs::read_to_string(&example_file_path)
        .expect("should read config.example file")
        .starts_with("# This config file is read whenever broker starts, and contains all of the information that broker needs in order to work.")
    );
}
