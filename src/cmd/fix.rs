//! Implementation for the fix command

use crate::{
    debug::{self, bundler, Bundle, BundleExport},
    ext::secrecy::REDACTION_LITERAL,
};
use colored::Colorize;
use core::result::Result;
use error_stack::{Report, ResultExt};
use indoc::formatdoc;
use std::time::Duration;

use crate::{
    api::{
        http,
        remote::{
            git::{
                repository,
                transport::{self, Transport},
            },
            Integration, Protocol, Remote,
        },
        ssh,
    },
    config::Config,
    ext::result::WrapErr,
};

/// Errors encountered when running the fix command.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Check an integration
    #[error("check integration for {remote}\n{error}")]
    CheckIntegration {
        /// the remote that the integration check failed for
        remote: Remote,
        /// the error returned by the integration check
        error: String,
        /// A message explaining how to fix this error
        msg: String,
    },

    /// Make a GET request to a fossa endpoint that does not require authentication
    #[error("check fossa connection: {msg}")]
    CheckFossaGet {
        /// An error message explaining how to fix this GET from FOSSA
        msg: String,
    },

    /// Creating a full URL from the provided endpoint
    #[error("create FOSSA URL from endpoint {remote}/{path}")]
    CreateFullFossaUrl {
        /// The configured fossa URL
        remote: url::Url,
        /// The path on Fossa
        path: String,
    },

    /// Generating an example command for a transport
    #[error("generate example command")]
    GenerateExampleCommand,

    /// Generating the debug bundle.
    #[error("generate debug bundle")]
    GenerateDebugBundle,
}

impl Error {
    fn fix_explanation(&self) -> String {
        match self {
            Error::CheckIntegration { remote, msg, .. } => {
                let remote = remote.to_string().red();
                format!("❌ {remote}\n\n{msg}")
            }
            Error::CheckFossaGet { msg } => {
                let error_string = "Error checking connection to FOSSA:".red();
                format!("❌ {error_string} {msg}")
            }
            Error::CreateFullFossaUrl { remote, path } => {
                format!("❌ Creating a full URL from your remote of '{remote}' and path = '{path}'")
            }
            Error::GenerateExampleCommand => {
                "❌ Generating an example command for a remote".to_string()
            }
            Error::GenerateDebugBundle => "❌ Generating the debug bundle".to_string(),
        }
    }

    fn integration_error(
        remote: &Remote,
        transport: &Transport,
        err: Report<repository::Error>,
    ) -> Self {
        let explanation = match Self::integration_connection_explanation(transport) {
            Err(err) => return err,
            Ok(exp) => exp,
        };

        let msg = formatdoc!(
            "
            Broker encountered an error while trying to connect to your git remote at '{remote}'.

            {explanation}

            Full error message from git:

            {err}

            "
        );
        Error::CheckIntegration {
            remote: remote.clone(),
            error: err.to_string(),
            msg,
        }
    }

    fn integration_connection_explanation(
        transport: &transport::Transport,
    ) -> Result<String, Error> {
        let shared_instructions = "Broker was unable to connect to this repository. Please make sure that the authentication info and the remote are set correctly in your config.yml file.";
        let base64_command = r#"echo -n "<username>:<password>" | base64"#.green();

        // Generate an example command. The basic command is `git ls-remote`, but there are other arguments and env variables added
        // depending on the auth used.
        // The resulting command should be an exact copy of the command used by broker, and should work when pasted into the terminal.
        let command = repository::pastable_ls_remote_command(transport)
            .or(Err(Error::GenerateExampleCommand))
            .map(|c| c.green())?;

        let specific_instructions = match transport {
            transport::Transport::Ssh {
                auth: ssh::Auth::KeyFile(_),
                ..
            } => {
                formatdoc!(
                    "You are using SSH keyfile authentication for this remote. This connects to your repository by setting the `GIT_SSH_COMMAND` environment variable with the path to the ssh key that you provided in your config file. Please make sure you can run the following command to verify the connection:

                    {command}"
                )
            }
            transport::Transport::Ssh {
                auth: ssh::Auth::KeyValue(_),
                ..
            } => {
                formatdoc!(
                    "You are using SSH key authentication for this remote. This method of authentication writes the SSH key that you provided in your config file to a temporary file, and then connects to your repository by setting the 'GIT_SSH_COMMAND' environment variable with the path to the temporary file. To debug this, please write the ssh key to a file and then make sure you can run the following command to verify the connection.

                    Note that the path to the SSH key in this example command is a path to a temporary file that will no longer exist. You will need to edit this command to change the path to point at the file you just created.

                    The path with the ssh key in it must have permissions of 0x660 on Linux and MacOS.

                    {command}"
                )
            }
            transport::Transport::Http {
                auth: Some(http::Auth::Basic { .. }),
                ..
            } => {
                formatdoc!(
                    r#"You are using HTTP basic authentication for this remote. This method of authentication encodes the username and password as a base64 string and then passes that to git using the "http.extraHeader" parameter. To debug this, please make sure that the following commands work.

                    You generate the base64 encoded username and password by joining them with a ":" and then base64 encoding them. If your username was "pat" and your password was "password123", then you would base64 encode "pat:password123". For example, you can use a command like this:

                    {base64_command}

                    Once you have the base64 encoded username and password, use them in a command like this, replacing {REDACTION_LITERAL} with your base64 encoded string:

                    {command}"#
                )
            }
            transport::Transport::Http {
                auth: Some(http::Auth::Header { .. }),
                ..
            } => {
                formatdoc!(
                    r#"You are using HTTP header authentication for this remote. This method of authentication passes the header that you have provided in your config file to git using the "http.extraHeader" parameter. To debug this, please make sure the following command works, after replacing {REDACTION_LITERAL} with the header from your config file:

                    {command}

                    You generate the header by making a string that looks like this:

                    Authorization: Basic <base64 encoded username:password>

                    If your username was "pat" and your password was "password123", then you would base64 encode "pat:password123". For example, you can use a command like this:

                    {base64_command}

                    The username you use depends on the git hosting platform you are authenticating to. For details on this, please see the 'config.example.yml' file in your broker config directory. You can re-generate this file at any time by running 'broker init'.
                    "#
                )
            }
            transport::Transport::Http { auth: None, .. } => {
                formatdoc!(
                    r#"You are using http transport with no authentication for this integration. To debug this, please make sure that the following command works:

                    {command}"#
                )
            }
        };

        Ok(format!("{shared_instructions}\n\n{specific_instructions}"))
    }

    fn fossa_integration_error(
        status: Option<reqwest::StatusCode>,
        err: reqwest::Error,
        description: &str,
        url: &str,
        example_command: &str,
    ) -> Self {
        match status {
            Some(reqwest::StatusCode::UNAUTHORIZED) => Error::CheckFossaGet {
                msg: Self::fossa_get_explanation(
                    description,
                    r#"Broker received an "Unauthorized" status response from FOSSA. This can mean that the fossa_integration_key configured in your config.yml file is not correct. You can obtain a FOSSA API key by going to Settings => Integrations => API in the FOSSA application."#,
                    url,
                    example_command,
                    err,
                ),
            },
            Some(status) => Error::CheckFossaGet {
                msg: Self::fossa_get_explanation(
                    description,
                    &format!("Broker received a {status} status response from FOSSA."),
                    url,
                    example_command,
                    err,
                ),
            },
            None => {
                if err.is_timeout() {
                    Error::CheckFossaGet {
                        msg: Self::fossa_get_explanation(
                            description,
                            "Broker received a timeout error while attempting to connect to FOSSA. This can happen if Broker is unable to connect to FOSSA due to various reasons.",
                            url,
                            example_command,
                            err,
                        ),
                    }
                } else {
                    Error::CheckFossaGet {
                        msg: Self::fossa_get_explanation(
                            description,
                            "An error occurred while attempting to connect to FOSSA.",
                            url,
                            example_command,
                            err,
                        ),
                    }
                }
            }
        }
    }

    fn fossa_get_explanation(
        description: &str,
        specific_error_message: &str,
        url: &str,
        example_command: &str,
        err: reqwest::Error,
    ) -> String {
        let description = description.red();
        let example_command = example_command.green();
        formatdoc!(
            "{description}

            {specific_error_message}

            The URL Broker attempted to connect to was '{url}'. Please make sure you can make a request to that URL. For example, try this curl command:

            {example_command}

            Full error message: {err}
            "
        )
    }
}

/// A logger. This is used to print the output to stdout
pub trait Logger {
    /// Log things
    fn log<S: AsRef<str>>(&self, content: S);
}

/// A logger that just prints to stdout
pub struct StdoutLogger;

impl Logger for StdoutLogger {
    fn log<S: AsRef<str>>(&self, content: S) {
        println!("{}", content.as_ref());
    }
}

macro_rules! log {
  ($logger:ident, $($arg:tt)*) => {{
    $logger.log(&format!($($arg)*));
  }}
}

/// The primary entrypoint for the fix command.
// #[tracing::instrument(skip(config, logger), fields(subcommand = "fix"))]
pub async fn main<L: Logger>(
    config: &Config,
    logger: &L,
    export: debug::BundleExport,
) -> Result<(), Report<Error>> {
    let integration_errors = check_integrations(logger, config).await;
    let fossa_connection_errors = check_fossa_connection(logger, config).await;
    let had_errors = !integration_errors.is_empty() || !fossa_connection_errors.is_empty();

    print_errors(
        logger,
        "\nErrors found while checking integrations",
        integration_errors,
    );
    print_errors(
        logger,
        "\nErrors found while checking connection to FOSSA",
        fossa_connection_errors,
    );

    log!(logger, "\n{}\n", "Collecting debug bundle".bold().blue());
    match export {
        BundleExport::Disable if had_errors => log!(logger, "❌ Debug bundle collection disabled."),
        BundleExport::Auto if had_errors => collect_bundle(config, logger)?,
        BundleExport::Disable | BundleExport::Auto => log!(logger, "✅ Debug bundle not needed."),
        BundleExport::Always => collect_bundle(config, logger)?,
    }

    Ok(())
}

fn collect_bundle<L: Logger>(config: &Config, logger: &L) -> Result<(), Report<Error>> {
    let bundler = bundler::TarGz::new().change_context(Error::GenerateDebugBundle)?;
    let bundle = Bundle::collect(config.debug(), bundler, "fossa.broker.debug.tar.gz")
        .change_context(Error::GenerateDebugBundle)?;

    log!(
        logger,
        "✅ Collected debug bundle at '{}'",
        bundle.location().display()
    );
    log!(
        logger,
        "Please include this debug bundle in any request to FOSSA Support."
    );

    Ok(())
}

// If there are errors, returns a string containing all of the error messages for a section.
// Sections are things like "checking integrations" or "checking fossa connection"
// If there are no errors, it returns None.
fn print_errors<L: Logger>(logger: &L, msg: &str, errors: Vec<Error>) {
    if !errors.is_empty() {
        log!(logger, "{}\n", msg.bold().red());
        for err in errors {
            logger.log(err.fix_explanation());
        }
    }
}

/// Check that Broker can connect to the integrations
/// This is currently done by running `git ls-remote <remote>` using the authentication
/// info from the transport.
#[tracing::instrument(skip(config, logger))]
async fn check_integrations<L: Logger>(logger: &L, config: &Config) -> Vec<Error> {
    let title = "\nDiagnosing connections to configured repositories\n"
        .bold()
        .blue()
        .to_string();
    logger.log(title);
    let integrations = config.integrations();
    let mut errors = Vec::new();
    for integration in integrations.iter() {
        let remote = integration.remote();
        match check_integration(integration).await {
            Ok(()) => log!(logger, "✅ {remote}"),
            Err(err) => {
                log!(logger, "❌ {remote}");
                errors.push(err);
            }
        }
    }
    errors
}

#[tracing::instrument]
async fn check_integration(integration: &Integration) -> Result<(), Error> {
    let Protocol::Git(transport) = integration.protocol();
    repository::ls_remote(transport)
        .await
        .or_else(|err| Error::integration_error(integration.remote(), transport, err).wrap_err())?;
    Ok(())
}

#[tracing::instrument(skip(config, logger))]
async fn check_fossa_connection<L: Logger>(logger: &L, config: &Config) -> Vec<Error> {
    let title = "\nDiagnosing connection to FOSSA\n"
        .bold()
        .blue()
        .to_string();
    logger.log(title);
    let mut errors = Vec::new();

    let get_with_no_auth = check_fossa_get_with_no_auth(config).await;
    match get_with_no_auth {
        Ok(_) => {
            logger.log("✅ check fossa API connection with no auth required");
        }
        Err(err) => {
            logger.log("❌ check fossa API connection with no auth required");
            errors.push(err);
        }
    }
    let get_with_auth = check_fossa_get_with_auth(config).await;
    match get_with_auth {
        Ok(_) => {
            logger.log("✅ check fossa API connection with auth required");
        }
        Err(err) => {
            logger.log("❌ check fossa API connection with auth required");
            errors.push(err);
        }
    }

    errors
}

const FOSSA_CONNECT_TIMEOUT_IN_SECONDS: u64 = 30;

#[tracing::instrument(skip(config))]
async fn check_fossa_get_with_no_auth(config: &Config) -> Result<(), Error> {
    let endpoint = config.fossa_api().endpoint().as_ref();
    let path = "/api/cli/organization";
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(FOSSA_CONNECT_TIMEOUT_IN_SECONDS))
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
        &format!("GET to fossa endpoint '{url}' with no authentication required"),
        url.as_ref(),
        &format!("curl {url}"),
    )
}

#[tracing::instrument(skip(config))]
async fn check_fossa_get_with_auth(config: &Config) -> Result<(), Error> {
    let endpoint = config.fossa_api().endpoint().as_ref();
    let path = "/api/cli/organization";
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(FOSSA_CONNECT_TIMEOUT_IN_SECONDS))
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
        .bearer_auth(config.fossa_api().key().expose_secret())
        .send()
        .await;
    describe_fossa_request(
        org_endpoint_response,
        &format!("GET to fossa endpoint '{url}' with authentication required"),
        url.as_ref(),
        &format!(r#"curl -H "Authorization: Bearer <your fossa api key>" {url}"#),
    )
}

#[tracing::instrument]
fn describe_fossa_request(
    response: Result<reqwest::Response, reqwest::Error>,
    description: &str,
    url: &str,
    example_command: &str,
) -> Result<(), Error> {
    match response {
        Ok(result_ok) => {
            if let Err(status_err) = result_ok.error_for_status() {
                Error::fossa_integration_error(
                    status_err.status(),
                    status_err,
                    description,
                    url,
                    example_command,
                )
                .wrap_err()
            } else {
                Ok(())
            }
        }
        Err(err) => {
            Error::fossa_integration_error(None, err, description, url, example_command).wrap_err()
        }
    }
}
