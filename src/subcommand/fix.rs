//! Implementation for the fix command

use error_stack::{Report, ResultExt};

use crate::{
    api::remote::{git::repository, Integration, Protocol},
    config::Config,
    ext::tracing::span_record,
    AppContext,
};

/// Errors encountered when running the fix command.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Check the integration
    #[error("check integration")]
    CheckIntegration,
}

/// Similar to [`AppContext`], but scoped for this subcommand.
#[derive(Debug)]
struct CmdContext {
    /// The application context.
    app: AppContext,

    /// The application configuration.
    config: Config,
}

/// The primary entrypoint for the fix command.
#[tracing::instrument(skip_all, fields(subcommand = "fix", cmd_context))]
pub async fn main(ctx: &AppContext, config: Config) -> Result<(), Report<Error>> {
    let ctx = CmdContext {
        app: ctx.clone(),
        config,
    };
    span_record!(cmd_context, debug ctx);
    check_integrations(&ctx).await?;
    Ok(())
}

async fn check_integrations(ctx: &CmdContext) -> Result<(), Report<Error>> {
    println!("Diagnosing connections to configured repositories");
    let integrations = ctx.config.integrations();
    for integration in integrations.iter() {
        check_integration(integration).await;
    }
    Ok(())
}

#[tracing::instrument]
async fn check_integration(integration: &Integration) {
    let Protocol::Git(transport) = integration.protocol();
    let result = repository::ls_remote(transport);
    match result {
        Ok(_) => {
            println!("✅ {}\n", integration.remote());
        }
        Err(err) => {
            println!("❌ {}:\n{}\n", integration.remote(), err);
        }
    }
}
