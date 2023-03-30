//! Implementation for the fix command

use colored::Colorize;
use core::result::Result;
use error_stack::{IntoReport, Report};
use std::time::Duration;

use crate::{
    api::remote::{git::repository, Integration, Protocol, Remote},
    config::Config,
    ext::{error_stack::IntoContext, result::WrapErr, tracing::span_record},
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
    #[error("check fossa connection: {}", msg)]
    CheckFossaGet {
        /// The error message when checking GET from FOSSA with no auth
        msg: String,
    },

    /// Creating a full URL from the provided endpoint
    #[error("create FOSSA URL from endpoint {}/{}", remote, path)]
    CreateFullFossaUrl {
        /// The configured fossa URL
        remote: url::Url,
        /// The path on Fossa
        path: String,
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
                Error::CheckFossaGet { msg } => {
                    println!("❌ Error checking connection to FOSSA: {}", msg);
                }
                Error::CreateFullFossaUrl { remote, path } => {
                    println!(
                        "❌ Creating a full URL from your remote of {} and path = {}",
                        remote, path
                    )
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
    let title = "\nDiagnosing connections to configured repositories\n"
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
    let title = "\nDiagnosing connection to FOSSA\n"
        .bold()
        .blue()
        .to_string();
    println!("{}", title);
    let mut errors = Vec::new();

    let get_with_no_auth = check_fossa_get_with_no_auth(ctx).await;
    match get_with_no_auth {
        Ok(()) => {
            println!("✅ check fossa API connection with no auth required");
        }
        Err(err) => {
            println!("❌ check fossa API connection with no auth required");
            println!("report: {}", err);
            errors.push(Error::CheckFossaGet {
                msg: err.to_string(),
            })
        }
    }
    let get_with_auth = check_fossa_get_with_auth(ctx).await;
    match get_with_auth {
        Ok(()) => {
            println!("✅ check fossa API connection with auth required");
        }
        Err(err) => {
            println!("❌ check fossa API connection with auth required");
            errors.push(Error::CheckFossaGet {
                msg: err.to_string(),
            });
        }
    }

    errors
}

async fn check_fossa_get_with_no_auth(ctx: &CmdContext) -> Result<(), Report<Error>> {
    let endpoint = ctx.config.fossa_api().endpoint().as_ref();
    let path = "/api/cli/organization";
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(1))
        .build()
        .context(Error::CreateFullFossaUrl {
            remote: endpoint.clone(),
            path: path.to_string(),
        })?;
    let url = endpoint
        .join("/health")
        .context(Error::CreateFullFossaUrl {
            remote: endpoint.clone(),
            path: path.to_string(),
        })?;
    let health_check_response = client
        .get(url.as_str())
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await;

    match health_check_response {
        Err(health_check_err) => err_from_request(health_check_err).wrap_err().into_report(),
        Ok(health_check_ok) => {
            if let Err(status_err) = health_check_ok.error_for_status() {
                return err_from_request(status_err).wrap_err().into_report();
            }
            Ok(())
        }
    }
}

async fn check_fossa_get_with_auth(ctx: &CmdContext) -> Result<(), Report<Error>> {
    let endpoint = ctx.config.fossa_api().endpoint().as_ref();
    let path = "/api/cli/organization";
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(1))
        .build()
        .context(Error::CreateFullFossaUrl {
            remote: endpoint.clone(),
            path: path.to_string(),
        })?;
    let url = endpoint.join(path).context(Error::CreateFullFossaUrl {
        remote: endpoint.clone(),
        path: path.to_string(),
    })?;
    let org_endpoint_response = client
        .get(url.as_str())
        .header(reqwest::header::ACCEPT, "application/json")
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", ctx.config.fossa_api().key().expose_secret()),
        )
        .send()
        .await;

    match org_endpoint_response {
        Err(org_endpoint_err) => err_from_request(org_endpoint_err).wrap_err().into_report(),
        Ok(org_endpoint_ok) => {
            if let Err(status_err) = org_endpoint_ok.error_for_status() {
                return err_from_request(status_err).wrap_err().into_report();
            }
            Ok(())
        }
    }
}

fn err_from_request(err: reqwest::Error) -> Error {
    if err.is_timeout() {
        Error::CheckFossaGet {
            msg: "timeout".to_string(),
        }
    } else if let Some(status) = err.status() {
        Error::CheckFossaGet {
            msg: format!("status error, status = {}, err = {}", status, err),
        }
    } else {
        Error::CheckFossaGet {
            msg: "something".to_string(),
        }
    }
}
