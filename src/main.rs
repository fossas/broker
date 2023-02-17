//! The `broker` binary.

#![deny(clippy::unwrap_used)]
#![deny(unsafe_code)]
#![deny(missing_docs)]
#![warn(rust_2018_idioms)]

use broker::{config, ext::error_stack::ErrorHelper};
use clap::Parser;
use error_stack::{fmt::ColorMode, Report, Result, ResultExt};

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("validate arguments")]
    ValidateArgs,

    #[error("a fatal error occurred at runtime")]
    _Runtime,
}

fn main() -> Result<(), Error> {
    Report::set_color_mode(ColorMode::Color);

    let args = config::RawBaseArgs::parse();
    let args = config::BaseArgs::try_from(args)
        .change_context(Error::ValidateArgs)
        .help("detailed error messages are available below")
        .help("try running Broker with the '--help' argument to see available options and usage suggestions")?;

    println!("args: {args:?}");
    Ok(())
}
