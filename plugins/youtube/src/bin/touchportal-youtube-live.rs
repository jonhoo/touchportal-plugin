use touchportal_youtube_live::plugin::Plugin;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::TRACE.into())
                .from_env_lossy(),
        )
        .without_time() // done by TouchPortal's logs
        .with_ansi(false) // not supported by TouchPortal's log output
        .init();

    Plugin::run_dynamic_with("127.0.0.1:12136", Plugin::new).await?;

    Ok(())
}
