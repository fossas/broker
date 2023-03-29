//! Implementation for the fix command

use colored::Colorize;
use core::result::Result;
use error_stack::Report;

use crate::{
    api::remote::{git::repository, Integration, Protocol, Remote},
    config::Config,
    ext::{result::WrapErr, tracing::span_record},
    AppContext,
};

/// Errors encountered when running the fix command.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Check the integration
    #[error("check integration for {}\n{}", remote, error)]
    CheckIntegration {
        /// the remote that the integration check failed for
        remote: Remote,
        /// the error returned by the integration check
        error: String,
    },
}

/// Similar to [`AppContext`], but scoped for this subcommand.
#[derive(Debug)]
struct CmdContext {
    /// The application context.
    // app: AppContext,

    /// The application configuration.
    config: Config,
}

/// The primary entrypoint for the fix command.
#[tracing::instrument(skip_all, fields(subcommand = "fix", cmd_context))]
pub async fn main(_ctx: &AppContext, config: Config) -> Result<(), Report<Error>> {
    let ctx = CmdContext {
        // app: ctx.clone(),
        config,
    };
    span_record!(cmd_context, debug ctx);
    let integration_errors = check_integrations(&ctx).await;
    if !integration_errors.is_empty() {
        println!(
            "\n{}\n",
            "Errors found while checking integrations".bold().red()
        );
        for err in integration_errors {
            match err {
                Error::CheckIntegration { remote, error } => {
                    println!("❌ {}\n{}", remote.to_string().red(), error);
                }
            }
        }
    }
    Ok(())
}

async fn check_integrations(ctx: &CmdContext) -> Vec<Error> {
    let title = "Diagnosing connections to configured repositories\n"
        .bold()
        .blue()
        .to_string();
    println!("{}", title);
    let integrations = ctx.config.integrations();
    let mut errors = Vec::new();
    for integration in integrations.iter() {
        match check_integration(integration).await {
            Ok(()) => {
                println!("✅ {}", integration.remote())
            }
            Err(err) => {
                println!("❌ {}", integration.remote());
                errors.push(err);
            }
        }
    }
    errors
}

#[tracing::instrument]
async fn check_integration(integration: &Integration) -> Result<(), Error> {
    let Protocol::Git(transport) = integration.protocol();
    repository::ls_remote(transport).or_else(|err| {
        Error::CheckIntegration {
            remote: integration.remote().clone(),
            error: err.to_string(),
        }
        .wrap_err()
    })?;
    Ok(())
}
