//! The `broker` binary.

#![deny(clippy::unwrap_used)]
#![deny(unsafe_code)]
#![deny(missing_docs)]
#![warn(rust_2018_idioms)]

use std::env::temp_dir;
use std::fs::File;
use std::io::copy;
use std::path::PathBuf;
use std::process::Command;

use broker::api::remote::RemoteProvider;
use broker::ext::error_stack::IntoContext;
use broker::{config, ext::error_stack::ErrorHelper};
use broker::{
    config::Config,
    doc,
    ext::error_stack::{DescribeContext, ErrorDocReference, FatalErrorReport},
};
use clap::{Parser, Subcommand};
use error_stack::IntoReport;
use error_stack::{bail, fmt::ColorMode, Report, Result, ResultExt};
use tracing::info;

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("determine effective configuration")]
    DetermineEffectiveConfig,

    #[error("this subcommand is not implemented")]
    SubcommandUnimplemented,

    #[error("a fatal error occurred during internal configuration")]
    InternalSetup,

    #[error("a fatal error occurred at runtime")]
    Runtime,
}

#[derive(Debug, Parser)]
#[clap(version)]
struct Opts {
    /// Broker can run a number of subcommands.
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Initialize Broker configuration.
    Init,

    /// Guided setup.
    Setup,

    /// Guided configuration changes.
    Config(config::RawBaseArgs),

    /// Automatically detect problems with Broker and fix them.
    Fix(config::RawBaseArgs),

    /// Back up or restore Broker's current config and database.
    Backup(config::RawBaseArgs),

    /// Run Broker with the current config.
    Run(config::RawBaseArgs),

    /// Attempt to do a git clone.
    #[clap(hide = true)]
    Clone(config::RawBaseArgs),
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    // App-wide setup goes here.
    Report::set_color_mode(ColorMode::Color);

    // Record global information to display with any error.
    let version = env!("CARGO_PKG_VERSION");

    // Subcommand routing.
    let Opts { command } = Opts::parse();
    match command {
        Commands::Init => main_init().await,
        Commands::Setup => main_setup().await,
        Commands::Config(args) => main_config(args).await,
        Commands::Fix(args) => main_fix(args).await,
        Commands::Backup(args) => main_backup(args).await,
        Commands::Run(args) => main_run(args).await,
        Commands::Clone(args) => main_clone(args).await,
    }
    .request_support()
    .describe_lazy(|| format!("broker version: {version}"))
}

/// Initialize Broker configuration.
async fn main_init() -> Result<(), Error> {
    bail!(Error::SubcommandUnimplemented)
}

/// Guided interactive setup.
async fn main_setup() -> Result<(), Error> {
    bail!(Error::SubcommandUnimplemented)
}

/// Guided interactive configuration changes.
async fn main_config(args: config::RawBaseArgs) -> Result<(), Error> {
    let conf = load_config(args).await?;
    println!("conf: {conf:?}");
    bail!(Error::SubcommandUnimplemented)
}

/// Automatically detect problems with Broker and fix them.
/// If they can't be fixed, generate a debug bundle.
async fn main_fix(args: config::RawBaseArgs) -> Result<(), Error> {
    let conf = load_config(args).await?;
    println!("conf: {conf:?}");
    bail!(Error::SubcommandUnimplemented)
}

/// Back up or restore Broker's current config and database.
async fn main_backup(args: config::RawBaseArgs) -> Result<(), Error> {
    let conf = load_config(args).await?;
    println!("conf: {conf:?}");
    bail!(Error::SubcommandUnimplemented)
}

/// Run Broker with the current config.
async fn main_run(args: config::RawBaseArgs) -> Result<(), Error> {
    let conf = load_config(args).await?;
    let _tracing_guard = conf
        .debug()
        .run_tracing_sink()
        .change_context(Error::InternalSetup)?;

    info!("Loaded {conf:?}");

    ensure_fossa_cli(conf.path()).await?;

    broker::subcommand::run::main(conf)
        .await
        .change_context(Error::Runtime)
}

async fn ensure_fossa_cli(config_path: &PathBuf) -> Result<(), Error> {
    let output = Command::new("fossa")
        .arg("--help")
        .output()
        .context(Error::InternalSetup)
        .describe("Unable to find `fossa` binary in your path");

    match output {
        Ok(output) => {
            if !output.status.success() {
                return Err(Error::InternalSetup).into_report().describe(
                    "Error when running `fossa -h` on the fossa binary found in your path",
                );
            }
        }
        Err(_) => {
            return download_fossa_cli(config_path)
                .await
                .change_context(Error::InternalSetup)
                .describe("fossa-cli not found in your path, so we attempted to download it");
        }
    }

    Ok(())
}

async fn download_fossa_cli(config_path: &PathBuf) -> Result<(), Error> {
    let config_dir = config_path
        .parent()
        .ok_or(Error::InternalSetup)
        .into_report()
        .describe("Finding location to download fossa-cli to")?;
    let client = reqwest::Client::new();
    let latest_release_response = client
        .get("https://github.com/fossas/fossa-cli/releases/latest")
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .context(Error::InternalSetup)
        .describe("Getting URL for latest release of the fossa CLI")?;
    // latest_release_response.url().path() will be something like "/fossas/fossa-cli/releases/tag/v3.7.2"
    let path = latest_release_response.url().path();
    let tag = path
        .rsplit("/")
        .next()
        .ok_or(Error::InternalSetup)
        .into_report()
        .describe_lazy(|| format!("Parsing fossa-cli version from path {}", path))?;
    println!(
        "latest_release_response.url().path(): {:?}, version = {}",
        path, tag
    );
    if !tag.starts_with("v") {
        return Err(Error::InternalSetup).into_report()?;
    }
    let version = tag.trim_start_matches("v");

    // TODO: Convert these into the options we support and blow up if unsupported
    // Supported os/arch combos:
    // windows/amd64
    // darwin/amd64
    // darwin/arm64
    // linux/amd64
    let mut arch = match std::env::consts::ARCH {
        "x86" | "x86_64" => "amd64",
        "aarch64" | "arm" => "arm64",
        _ => "amd64",
    };
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "windows",
        _ => "linux",
    };

    if os == "darwin" && arch == "arm64" {
        arch = "amd64";
    }

    let extension = match os {
        "darwin" => ".zip",
        "windows" => ".zip",
        _ => ".tar.gz",
    };
    let download_url = format!("https://github.com/fossas/fossa-cli/releases/download/v{version}/fossa_{version}_{os}_{arch}{extension}");
    println!("Downloading from {}", download_url);
    // Download into a tmp dir so that we can unzip and untar it
    let tmpdir = temp_dir();
    let response = client
        .get(&download_url)
        .send()
        .await
        .context(Error::InternalSetup)
        .describe_lazy(|| format!("downloading fossa-cli from {}", download_url))?;
    let download_path = tmpdir.as_path().join(format!("fossa.{}", extension));
    let mut download_file = File::create(&download_path)
        .context(Error::InternalSetup)
        .describe("Creating temp file to download fossa-cli into")?;
    let content = response
        .bytes()
        .await
        .context(Error::InternalSetup)
        .describe("converting downloaded fossa-cli into bytes")?;
    let mut decoder = libflate::deflate::Decoder::new(content.as_ref());
    copy(&mut decoder, &mut download_file)
        .context(Error::InternalSetup)
        .describe("writing downloaded fossa-cli to disk")?;

    let final_location = config_dir.join(format!("fossa{}", extension));
    std::fs::rename(&download_path, &final_location)
        .context(Error::InternalSetup)
        .describe_lazy(|| {
            format!(
                "Copying fossa-cli to final destination of {:?}",
                final_location
            )
        })?;
    println!("fossa-cli should now be in {:?}", &final_location);
    Ok(())
}

/// Parse application args and then load effective config.
async fn load_config(args: config::RawBaseArgs) -> Result<Config, Error> {
    let args = config::validate_args(args)
        .await
        .change_context(Error::DetermineEffectiveConfig)
        .help("try running Broker with the '--help' argument to see available options and usage suggestions")?;

    config::load(&args)
        .await
        .change_context(Error::DetermineEffectiveConfig)
        .documentation_lazy(doc::link::config_file_reference)
}

/// Workflow:
/// 1. get a list of remotes
/// 2. For each remote, clone it into a directory and check out the tag or branch
async fn main_clone(args: config::RawBaseArgs) -> Result<(), Error> {
    let conf = load_config(args).await?;
    let integration = &conf.integrations().as_ref()[0];
    let mut references = integration.references()
        .change_context(Error::Runtime)
        .help("try running Broker with the '--help' argument to see available options and usage suggestions")?;

    // clone the first 5 references that need to be scanned
    references.truncate(5);
    for reference in references {
        integration
            .clone_reference(&reference)
            .change_context(Error::Runtime)?;
    }
    Ok(())
}
