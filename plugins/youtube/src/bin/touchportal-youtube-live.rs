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

    touchportal_youtube_live::plugin::Plugin::run_dynamic("127.0.0.1:12136").await?;

    Ok(())
}
