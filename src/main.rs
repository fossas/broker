//! The `broker` binary.

#![deny(clippy::unwrap_used)]
#![deny(unsafe_code)]
#![deny(missing_docs)]
#![warn(rust_2018_idioms)]

use broker::{
    config::{self, Config},
    ext::error_stack::ErrorHelper,
};
use clap::{Parser, Subcommand};
use error_stack::{bail, fmt::ColorMode, Report, Result, ResultExt};

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("determine effective configuration")]
    DetermineEffectiveConfig,

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
}

fn main() -> Result<(), Error> {
    // App-wide setup goes here.
    Report::set_color_mode(ColorMode::Color);

    // Subcommand routing.
    let Opts { command } = Opts::parse();
    match command {
        Commands::Init => main_init(),
        Commands::Setup => main_setup(),
        Commands::Config(args) => main_config(args),
        Commands::Fix(args) => main_fix(args),
        Commands::Backup(args) => main_backup(args),
        Commands::Run(args) => main_run(args),
    }
}

/// Initialize Broker configuration.
fn main_init() -> Result<(), Error> {
    bail!(Error::SubcommandUnimplemented)
}

/// Guided interactive setup.
fn main_setup() -> Result<(), Error> {
    bail!(Error::SubcommandUnimplemented)
}

/// Guided interactive configuration changes.
fn main_config(args: config::RawBaseArgs) -> Result<(), Error> {
    let conf = load_config(args)?;
    println!("conf: {conf:?}");
    bail!(Error::SubcommandUnimplemented)
}

/// Automatically detect problems with Broker and fix them.
/// If they can't be fixed, generate a debug bundle.
fn main_fix(args: config::RawBaseArgs) -> Result<(), Error> {
    let conf = load_config(args)?;
    println!("conf: {conf:?}");
    bail!(Error::SubcommandUnimplemented)
}

/// Back up or restore Broker's current config and database.
fn main_backup(args: config::RawBaseArgs) -> Result<(), Error> {
    let conf = load_config(args)?;
    println!("conf: {conf:?}");
    bail!(Error::SubcommandUnimplemented)
}

/// Run Broker with the current config.
fn main_run(args: config::RawBaseArgs) -> Result<(), Error> {
    let conf = load_config(args)?;
    println!("conf: {conf:?}");
    bail!(Error::SubcommandUnimplemented)
}

/// Parse application args and then load effective config.
fn load_config(args: config::RawBaseArgs) -> Result<Config, Error> {
    let args = config::validate_args(args)
        .change_context(Error::DetermineEffectiveConfig)
        .help("try running Broker with the '--help' argument to see available options and usage suggestions")?;

    // TODO: point the user towards the docs entrypoint for configuration.
    config::load(&args).change_context(Error::DetermineEffectiveConfig)
}
