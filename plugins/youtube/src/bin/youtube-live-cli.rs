use crate::youtube_client::BroadcastLifeCycleStatus;
use eyre::Context;
use std::io::IsTerminal;
use tokio_stream::StreamExt;
use touchportal_youtube_live::{setup_youtube_clients, youtube_client, Channel};
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
        .with_ansi(std::io::stdout().is_terminal())
        .init();

    let mut tokens = String::new();
    if tokio::fs::try_exists("tokens.json").await.unwrap() {
        tokens = tokio::fs::read_to_string("tokens.json").await.unwrap();
    }

    // Use shared token setup logic (no notifications needed for CLI)
    let (client_by_channel, refreshed_tokens) =
        setup_youtube_clients(&tokens, async |_, _, _| {}).await?;

    // for testing
    for (id, Channel { name, yt }) in &client_by_channel {
        eprintln!("==> {name} ({id})");
        let broadcasts = yt.list_my_live_broadcasts();
        let mut stream = std::pin::pin!(broadcasts);
        while let Some(broadcast) = stream.next().await {
            let broadcast = broadcast.context("fetch broadcast")?;
            match broadcast.status.life_cycle_status {
                BroadcastLifeCycleStatus::Ready | BroadcastLifeCycleStatus::Created => {
                    eprintln!("upcoming : {broadcast:?}");
                }
                BroadcastLifeCycleStatus::Live | BroadcastLifeCycleStatus::Testing => {
                    eprintln!("active   : {broadcast:?}");
                }
                BroadcastLifeCycleStatus::Complete | BroadcastLifeCycleStatus::Revoked => {
                    // assume that results are returned in reverse chronological order
                    eprintln!("complete : {broadcast:?}");
                    break;
                }
            }
        }
    }

    // Save refreshed tokens to file
    let json = serde_json::to_string(&refreshed_tokens).unwrap();
    tokio::fs::write("tokens.json", &json).await.unwrap();

    Ok(())
}
