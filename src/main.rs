//! The `broker` binary.

#![deny(clippy::unwrap_used)]
#![deny(unsafe_code)]
#![deny(missing_docs)]
#![warn(rust_2018_idioms)]

use atty::Stream;
use broker::api::remote::RemoteProvider;
use broker::db;
use broker::doc::crate_version;
use broker::ext::error_stack::IntoContext;
use broker::fossa_cli;
use broker::{config, ext::error_stack::ErrorHelper};
use broker::{
    doc,
    ext::error_stack::{DescribeContext, ErrorDocReference, FatalErrorReport},
};
use clap::{Parser, Subcommand};
use error_stack::{bail, fmt::ColorMode, Report, Result, ResultExt};
use tap::TapFallible;
use tracing::debug;

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
    Init(config::RawInitArgs),

    /// Guided setup.
    Setup,

    /// Guided configuration changes.
    Config(config::RawRunArgs),

    /// Automatically detect problems with Broker and fix them.
    Fix(config::RawRunArgs),

    /// Back up or restore Broker's current config and database.
    Backup(config::RawRunArgs),

    /// Run Broker with the current config.
    Run(config::RawRunArgs),

    /// Attempt to do a git clone.
    #[clap(hide = true)]
    Clone(config::RawRunArgs),
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    // App-wide setup that doesn't depend on config or subcommand goes here.
    let version = crate_version();
    if atty::is(Stream::Stdout) {
        Report::set_color_mode(ColorMode::Color);
    } else {
        Report::set_color_mode(ColorMode::None);
    }

    // Subcommand routing.
    let Opts { command } = Opts::parse();
    let subcommand = || async {
        match command {
            Commands::Init(args) => main_init(args).await,
            Commands::Setup => main_setup().await,
            Commands::Config(args) => main_config(args).await,
            Commands::Fix(args) => main_fix(args).await,
            Commands::Backup(args) => main_backup(args).await,
            Commands::Run(args) => main_run(args).await,
            Commands::Clone(args) => main_clone(args).await,
        }
    };

    // Run the subcommand, but also listen for ctrl+c.
    // If ctrl+c is fired, we exit; this drops any futures currently running.
    // In Rust, this is the appropriate way to cancel futures.
    tokio::select! {
        // We want to handle signals first, regardless of how often the subcommand
        // is ready to be polled.
        biased;

        // If the signal fires, log that we're shutting down and return.
        result = tokio::signal::ctrl_c() => {
            // Only log this on success.
            //
            // Write directly to stderr because tracing may already be shut down,
            // or may not ever have been started, by the time this runs.
            result.tap_ok(|_| eprintln!("Shut down at due to OS signal"))
            // If this errors, it'll do so immediately before anything else runs,
            // so it's definitely part of internal setup.
            .context(Error::InternalSetup)
        },

        // Otherwise, run the subcommand to completion.
        result = subcommand() => {
            result
        }
    }
    // Decorate any error message with top level diagnostics and debugging help.
    .request_support()
    .describe_lazy(|| format!("broker version: {version}"))
}

/// Initialize Broker configuration.
async fn main_init(args: config::RawInitArgs) -> Result<(), Error> {
    let ctx = args
        .validate()
        .await
        .change_context(Error::DetermineEffectiveConfig)?;
    broker::subcommand::init::main(ctx.data_root()).change_context(Error::Runtime)
}

/// Guided interactive setup.
async fn main_setup() -> Result<(), Error> {
    bail!(Error::SubcommandUnimplemented)
}

/// Guided interactive configuration changes.
async fn main_config(_args: config::RawRunArgs) -> Result<(), Error> {
    bail!(Error::SubcommandUnimplemented)
}

/// Automatically detect problems with Broker and fix them.
/// If they can't be fixed, generate a debug bundle.
async fn main_fix(_args: config::RawRunArgs) -> Result<(), Error> {
    bail!(Error::SubcommandUnimplemented)
}

/// Back up or restore Broker's current config and database.
async fn main_backup(_args: config::RawRunArgs) -> Result<(), Error> {
    bail!(Error::SubcommandUnimplemented)
}

/// Run Broker with the current config.
async fn main_run(args: config::RawRunArgs) -> Result<(), Error> {
    let args = args.validate()
        .await
        .change_context(Error::DetermineEffectiveConfig)
        .help("try running Broker with the '--help' argument to see available options and usage suggestions")?;

    let conf = config::load(&args)
        .await
        .change_context(Error::DetermineEffectiveConfig)
        .documentation_lazy(doc::link::config_file_reference)?;
    debug!("Loaded {conf:?}");

    let _tracing_guard = conf
        .debug()
        .run_tracing_sink()
        .change_context(Error::InternalSetup)?;

    let db = db::connect_sqlite(args.database_path().path())
        .await
        .change_context(Error::InternalSetup)?;

    let fossa_path = fossa_cli::find_or_download(args.context(), fossa_cli::DesiredVersion::Latest)
        .await
        .change_context(Error::InternalSetup)?;
    debug!("fossa path: {:?}", fossa_path);

    broker::subcommand::run::main(args.context(), conf, db)
        .await
        .change_context(Error::Runtime)
}

/// Workflow:
/// 1. get a list of remotes
/// 2. For each remote, clone it into a directory and check out the tag or branch
async fn main_clone(args: config::RawRunArgs) -> Result<(), Error> {
    let args = args.validate()
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
        .await
        .change_context(Error::Runtime)
        .help("try running Broker with the '--help' argument to see available options and usage suggestions")?;

    // clone the first 5 references that need to be scanned
    references.truncate(5);
    for reference in references {
        integration
            .clone_reference(&reference)
            .await
            .change_context(Error::Runtime)?;
    }
    Ok(())
}
