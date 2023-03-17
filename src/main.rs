//! The `broker` binary.

#![deny(clippy::unwrap_used)]
#![deny(unsafe_code)]
#![deny(missing_docs)]
#![warn(rust_2018_idioms)]

use broker::api::remote::RemoteProvider;
use broker::db;
use broker::doc::crate_version;
use broker::download_fossa_cli;
use broker::{config, ext::error_stack::ErrorHelper};
use broker::{
    doc,
    ext::error_stack::{DescribeContext, ErrorDocReference, FatalErrorReport},
};
use clap::{Parser, Subcommand};
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
    let version = crate_version();

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
async fn main_config(_args: config::RawBaseArgs) -> Result<(), Error> {
    bail!(Error::SubcommandUnimplemented)
}

/// Automatically detect problems with Broker and fix them.
/// If they can't be fixed, generate a debug bundle.
async fn main_fix(_args: config::RawBaseArgs) -> Result<(), Error> {
    bail!(Error::SubcommandUnimplemented)
}

/// Back up or restore Broker's current config and database.
async fn main_backup(_args: config::RawBaseArgs) -> Result<(), Error> {
    bail!(Error::SubcommandUnimplemented)
}

/// Run Broker with the current config.
async fn main_run(args: config::RawBaseArgs) -> Result<(), Error> {
    let args = config::validate_args(args)
        .await
        .change_context(Error::DetermineEffectiveConfig)
        .help("try running Broker with the '--help' argument to see available options and usage suggestions")?;

    let conf = config::load(&args)
        .await
        .change_context(Error::DetermineEffectiveConfig)
        .documentation_lazy(doc::link::config_file_reference)?;

    let _tracing_guard = conf
        .debug()
        .run_tracing_sink()
        .change_context(Error::InternalSetup)?;

    let db = db::connect_sqlite(args.database_path().path())
        .await
        .change_context(Error::InternalSetup)?;

    let fossa_path = download_fossa_cli::ensure_fossa_cli(conf.directory())
        .await
        .change_context(Error::InternalSetup)?;
    info!("fossa path: {:?}", fossa_path);

    info!("Loaded {conf:?}");
    broker::subcommand::run::main(conf, db)
        .await
        .change_context(Error::Runtime)
}

/// Workflow:
/// 1. get a list of remotes
/// 2. For each remote, clone it into a directory and check out the tag or branch
async fn main_clone(args: config::RawBaseArgs) -> Result<(), Error> {
    let args = config::validate_args(args)
        .await
        .change_context(Error::DetermineEffectiveConfig)
        .help("try running Broker with the '--help' argument to see available options and usage suggestions")?;

    let conf = config::load(&args)
        .await
        .change_context(Error::DetermineEffectiveConfig)
        .documentation_lazy(doc::link::config_file_reference)?;

    let _tracing_guard = conf
        .debug()
        .run_tracing_sink()
        .change_context(Error::InternalSetup)?;

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
