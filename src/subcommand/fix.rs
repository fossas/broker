//! Implementation for the fix command

use colored::Colorize;
use core::result::Result;
use error_stack::Report;
use indoc::formatdoc;
use std::time::Duration;

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
            errors.push(err);
        }
    }

    errors
}

async fn check_fossa_get_with_no_auth(ctx: &CmdContext) -> Result<(), Error> {
    let endpoint = ctx.config.fossa_api().endpoint().as_ref();
    let path = "/api/cli/organization";
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(1))
        .build()
        .map_err(|_| Error::CreateFullFossaUrl {
            remote: endpoint.clone(),
            path: path.to_string(),
        })?;
    let url = endpoint
        .join("/health")
        .map_err(|_| Error::CreateFullFossaUrl {
            remote: endpoint.clone(),
            path: path.to_string(),
        })?;
    let health_check_response = client
        .get(url.as_str())
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await;

    describe_fossa_request(
        health_check_response,
        "GET to fossa endpoint with no authentication required",
        url.as_ref(),
        &format!("curl {}", url),
    )
}

async fn check_fossa_get_with_auth(ctx: &CmdContext) -> Result<(), Error> {
    let endpoint = ctx.config.fossa_api().endpoint().as_ref();
    let path = "/api/cli/organization";
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(1))
        .build()
        .map_err(|_| Error::CreateFullFossaUrl {
            remote: endpoint.clone(),
            path: path.to_string(),
        })?;
    let url = endpoint.join(path).map_err(|_| Error::CreateFullFossaUrl {
        remote: endpoint.clone(),
        path: path.to_string(),
    })?;
    let org_endpoint_response = client
        .get(url.as_str())
        .header(reqwest::header::ACCEPT, "application/json")
        .bearer_auth(ctx.config.fossa_api().key().expose_secret())
        .send()
        .await;
    describe_fossa_request(
        org_endpoint_response,
        "GET to fossa endpoint with authentication required",
        url.as_ref(),
        &format!(
            r#"curl -H "Authorization: Bearer <your fossa api key>" {}"#,
            url
        ),
    )
}

fn describe_fossa_request(
    response: Result<reqwest::Response, reqwest::Error>,
    prefix: &str,
    url: &str,
    example_command: &str,
) -> Result<(), Error> {
    match response {
        Ok(result_ok) => {
            if let Err(status_err) = result_ok.error_for_status() {
                if let Some(status) = status_err.status() {
                    return match status {
                        reqwest::StatusCode::UNAUTHORIZED => {
                        Error::CheckFossaGet {
                            msg: formatdoc!(r#"{}
                                We received an "Unauthorized" status from FOSSA.

                                This can mean that the fossa_integration_key configured in your config.yml file is not correct.

                                The URL we attempted to connect to was {}.

                                Please make sure you can make a request to that URL. For example, try this curl command:

                                {}

                                You can obtain a FOSSA API key at {}/account/settings/integrations/api_tokens.

                                Full error message: {}
                                "#,
                                prefix, url, example_command, url, status_err
                            )
                        }
                        .wrap_err()
                    },
                    status => Error::CheckFossaGet {
                            msg: formatdoc!(r#"{}
                                We received a {} status in the response from FOSSA.

                                The URL we attempted to connect to was {}.

                                Please make sure you can make a request to that URL. For example, try this curl command:

                                {}

                                Full error message: {}
                                "#,
                                prefix, status, url, example_command, status_err
                            ),
                        }
                        .wrap_err(),
                    };
                }
                Ok(())
            } else {
                Ok(())
            }
        }
        Err(err) => {
            if err.is_timeout() {
                Error::CheckFossaGet {
                    msg: formatdoc!(
                        "{}
                        We received a timeout error while attempting to connect to FOSSA.
                        This can happen if we are unable to connect to FOSSA due to various reasons.

                        The URL we attempted to connect to was {}.

                        Please make sure you can make a request to that URL. For example, try this curl command:

                        {}

                        Full error message: {}
                        ",
                        prefix, url, example_command, err
                    ),
                }
                .wrap_err()
            } else {
                Error::CheckFossaGet {
                    msg: formatdoc!(
                        "{}
                        an error occurred while attempting to connect to FOSSA.

                        The URL we attempted to connect to was {}.

                        Please make sure you can make a request to that URL. For example, try this curl command:

                        {}

                        Full error message: {}
                        ",
                        prefix, url, example_command, err
                    ),
                }
                .wrap_err()
            }
        }
    }
}
