use eyre::Context;
use touchportal_youtube_live::plugin::Plugin;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::prelude::*;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    // Set up tracing with reload capability
    let (env_filter, reload_handle) = tracing_subscriber::reload::Layer::new(
        EnvFilter::builder()
            .with_default_directive(LevelFilter::INFO.into())
            .from_env_lossy(),
    );

    tracing_subscriber::registry()
        .with(env_filter)
        .with(
            tracing_subscriber::fmt::layer()
                .without_time() // done by TouchPortal's logs
                .with_ansi(false), // not supported by TouchPortal's log output
        )
        .init();

    Plugin::run_dynamic_with("127.0.0.1:12136", async move |setting, outgoing, info| {
        Plugin::new(setting, outgoing, info, reload_handle)
            .await
            .context("Plugin::new")
    })
    .await?;

    Ok(())
}
