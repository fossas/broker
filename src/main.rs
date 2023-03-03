//! The `broker` binary.

#![deny(clippy::unwrap_used)]
#![deny(unsafe_code)]
#![deny(missing_docs)]
#![warn(rust_2018_idioms)]

use std::path::PathBuf;

use broker::api::remote::{self, RemoteProvider};
use broker::{api::remote::git, config, ext::error_stack::ErrorHelper};
use broker::{
    config::Config,
    doc,
    ext::error_stack::{DescribeContext, ErrorDocReference, FatalErrorReport},
};
use clap::{Parser, Subcommand};
use error_stack::{bail, fmt::ColorMode, Report, Result, ResultExt};

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("determine effective configuration")]
    DetermineEffectiveConfig,

    #[error("git command")]
    GitWrapper,

    #[error("this subcommand is not implemented")]
    SubcommandUnimplemented,

    #[error("a fatal error occurred at runtime")]
    _Runtime,
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
    println!("conf: {conf:?}");
    bail!(Error::SubcommandUnimplemented)
}

/// Parse application args and then load effective config.
async fn load_config(args: config::RawBaseArgs) -> Result<Config, Error> {
    let args = config::validate_args(args)
        .await
        .change_context(Error::DetermineEffectiveConfig)
        .help("try running Broker with the '--help' argument to see available options and usage suggestions")?;

    // TODO: point the user towards the docs entrypoint for configuration.
    config::load(&args)
        .await
        .change_context(Error::DetermineEffectiveConfig)
        .documentation_lazy(doc::link::config_file_reference)
}

async fn main_clone(args: config::RawBaseArgs) -> Result<(), Error> {
    let conf = load_config(args).await?;
    let integration = &conf.integrations().as_ref()[0];
    let remote::Protocol::Git(transport) = integration.protocol().clone();
    let repo = git::repository::Repository {
        directory: PathBuf::from("/tmp/cloned"),
        checkout_type: git::repository::CheckoutType::None,
        transport,
    };
    let res = repo.clone();
    match res {
        Ok(_) => {
            println!("got OK() from git clone, directory = {:?}", res);
            Ok(())
        }
        Err(err) => {
            // println!("git clone err: {}", err);
            Err(err).change_context(Error::GitWrapper)
        }
    }
}
