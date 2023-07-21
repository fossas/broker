//! Implementation for the `init` subcommand.
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::ext::error_stack::{DescribeContext, ErrorHelper, IntoContext};
use error_stack::Result;
use indoc::formatdoc;
use indoc::indoc;

/// Errors encountered during init.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A config file already exists
    #[error("config file exists")]
    ConfigFileExists,

    /// Creating the data root directory
    #[error("create data root at {}", .0.display())]
    CreateDataRoot(PathBuf),

    /// Writing the file did not work
    #[error("write config file inside data root '{}', at '{}'", .data_root.display(), .path.display())]
    WriteConfigFile {
        /// The path to the config file
        path: PathBuf,
        /// The data_root directory
        data_root: PathBuf,
    },
}

/// generate the config and db files in the default location
#[tracing::instrument]
pub fn main(data_root: &Path) -> Result<(), Error> {
    let default_already_exists = write_config(data_root, "config.yml", false)?;
    write_config(data_root, "config.example.yml", true)?;
    if default_already_exists {
        let output = formatdoc! {r#"

        `broker init` detected a previously existing config file at {config} and left it as is.

        `broker init` did, however, create a new example config file for you at {example_config}.

        This example config file contains a detailed explanation of everything you need to do to get broker up and running.

        You can safely re-run `broker init` at any time to re-generate the "config.example.yml" file.
        "#,
            config = data_root.join("config.yml").display(),
            example_config = data_root.join("config.example.yml").display(),
        };
        println!("{output}");
    } else {
        let output = formatdoc! {r#"

        `broker init` created an example config in {config}.

        The config file contains a detailed explanation of everything you need to do to get broker up and running.

        The next step is to open {config} and follow the instructions to configure broker.

        We also wrote the same example file to {example_config} to serve as a reference for you in the future.

        You can safely re-run `broker init` at any time to re-generate the "config.example.yml" file.

        "#,
            config = data_root.join("config.yml").display(),
            example_config = data_root.join("config.example.yml").display(),
        };
        println!("{output}");
    }
    Ok(())
}

fn write_config(data_root: &Path, filename: &str, force_write: bool) -> Result<bool, Error> {
    let config_file_path = data_root.join(filename);
    if config_file_path.try_exists().unwrap_or(false) && !force_write {
        return Ok(true);
    }

    std::fs::create_dir_all(data_root)
        .context_lazy(|| Error::CreateDataRoot(data_root.to_owned()))
        .describe_lazy(|| indoc! {"
            Broker requires that the data root exists and is a directory in order to create config files.
            If the directory does not exist, Broker attempts to create it at a default location for your user.
        "})
        .help_lazy(|| indoc! {"
            This can happen if Broker did not have permission to create the directory.
            Try creating the directory yourself then running Broker again.
            Alternately, you may specify a different data root: run Broker with the `-h` argument to see how.
        "})?;

    fs::write(&config_file_path, default_config_file(data_root))
        .context_lazy(|| Error::WriteConfigFile {
            path: config_file_path.to_path_buf(),
            data_root: data_root.to_path_buf(),
        })
        .help_lazy(|| indoc! {"
            This can happen if Broker did not have permission to create files in the data root directory.
            Ensure that your current user in the operating system is allowed to create files in the data root;
            deleting it and re-creating it may resolve this issue.
            Alternately, you may specify a different data root: run Broker with the `-h` argument to see how.
        "})?;

    Ok(false)
}

fn default_config_file(data_root: &Path) -> String {
    let debugging_dir = data_root.join("debugging");
    let default_config_format_string = include_str!("config.example.yml");
    default_config_format_string.replace("{debugging_dir}", &debugging_dir.display().to_string())
}
