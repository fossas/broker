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
    /// Check an integration
    #[error("check integration for {}\n{}", remote, error)]
    CheckIntegration {
        /// the remote that the integration check failed for
        remote: Remote,
        /// the error returned by the integration check
        error: String,
    },

    /// Make a GET request to a fossa endpoint that does not require authentication
    #[error("check fossa connection with no authentication required")]
    CheckFossaGetWithNoAuth(String),

    /// Make a GET Request to a fossa endpoint that requires authentication
    #[error("check fossa connection with authentication")]
    CheckFossaGetWithAuth(String),
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
    let fossa_connection_errors = check_fossa_connection(&ctx).await;
    print_errors(
        "Errors found while checking integrations"
            .bold()
            .red()
            .to_string(),
        integration_errors,
    );
    print_errors(
        "Errors found while checking connection to FOSSA"
            .bold()
            .red()
            .to_string(),
        fossa_connection_errors,
    );
    Ok(())
}

#[tracing::instrument]
fn print_errors(msg: String, errors: Vec<Error>) {
    if !errors.is_empty() {
        println!("\n{}\n", msg,);
        for err in errors {
            match err {
                Error::CheckIntegration { remote, error } => {
                    println!("❌ {}\n{}", remote.to_string().red(), error);
                }
                Error::CheckFossaGetWithNoAuth(msg) => {
                    println!("❌ Error checking connection to FOSSA for endpoint with no auth required: {}", msg);
                }
                Error::CheckFossaGetWithAuth(msg) => {
                    println!(
                        "❌ Error checking connection to FOSSA for endpoint with auth required: {}",
                        msg
                    );
                }
            }
        }
    }
}

/// Check that we can connect to the integrations
/// This is currently done by running `git ls-remote <remote>` using the authentication
/// info from the transport.
#[tracing::instrument(skip(ctx))]
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

#[tracing::instrument(skip(ctx))]
async fn check_fossa_connection(ctx: &CmdContext) -> Vec<Error> {
    let mut errors = Vec::new();

    let get_with_no_auth = check_fossa_get_with_no_auth(ctx).await;
    match get_with_no_auth {
        Ok(()) => {
            println!("✅ check fossa API connection with no auth required");
        }
        Err(err) => {
            println!("❌ check fossa API connection with no auth required");
            errors.push(err)
        }
    }
    let get_with_auth = check_fossa_get_with_auth(ctx).await;
    match get_with_auth {
        Ok(()) => {
            println!("✅ check fossa API connection with auth required");
        }
        Err(err) => {
            println!("❌ check fossa API connection with auth required");
            errors.push(err)
        }
    }

    errors
}

async fn check_fossa_get_with_no_auth(ctx: &CmdContext) -> Result<(), Error> {
    Error::CheckFossaGetWithNoAuth("Some error".to_string()).wrap_err()
}

async fn check_fossa_get_with_auth(ctx: &CmdContext) -> Result<(), Error> {
    Error::CheckFossaGetWithAuth("Some error".to_string()).wrap_err()
}
