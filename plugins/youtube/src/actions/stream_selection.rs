use std::collections::HashMap;
use eyre::Context;
use tokio_stream::StreamExt;
use crate::youtube_api::broadcasts::BroadcastLifeCycleStatus;
use crate::Channel;

use crate::background::metrics::StreamSelection;

/// Handle the complex stream selection logic for choosing a broadcast
pub async fn handle_select_stream(
    tp: &mut crate::plugin::TouchPortalHandle,
    yt: &HashMap<String, Channel>,
    channel_selection: String,
    broadcast_selection: String,
) -> eyre::Result<(String, String, StreamSelection)> {
    // Extract channel ID from the selected channel
    let channel_id = channel_selection
        .rsplit_once(" - ")
        .map(|(_, id)| id)
        .ok_or_else(|| eyre::eyre!("Invalid channel selection format"))?;

    // Extract broadcast ID and live chat ID or handle "latest" selection
    let (broadcast_id, live_chat_id) = if broadcast_selection == "Latest non-completed broadcast" {
        // Find the latest non-completed broadcast for this channel
        let channel = yt
            .get(channel_id)
            .ok_or_else(|| eyre::eyre!("Selected channel not found"))?;

        let broadcasts = channel.yt.list_my_live_broadcasts();
        let mut broadcasts = std::pin::pin!(broadcasts);

        let mut found_broadcast = None;
        while let Some(broadcast) = broadcasts.next().await {
            let broadcast = broadcast.context("fetch broadcast for latest selection")?;
            if broadcast.status.life_cycle_status != BroadcastLifeCycleStatus::Complete {
                let chat_id = broadcast.snippet.live_chat_id.clone();
                found_broadcast = Some((broadcast.id, chat_id));
                break;
            }
        }
        found_broadcast.ok_or_else(|| eyre::eyre!("No non-completed broadcast found"))?
    } else {
        let id = broadcast_selection
            .rsplit_once(" - ")
            .map(|(_, id)| id.to_string())
            .ok_or_else(|| eyre::eyre!("Invalid broadcast selection format"))?;
        (id, None) // We don't have the live chat ID for manually selected broadcasts
    };

    // Create stream selection object
    let selection = StreamSelection {
        channel_id: Some(channel_id.to_string()),
        broadcast_id: Some(broadcast_id.clone()),
        live_chat_id: live_chat_id.clone(),
    };

    // Update settings for persistence
    tp.set_selected_channel_id(channel_id.to_string()).await;
    tp.set_selected_broadcast_id(broadcast_id.clone()).await;

    // Update states
    if let Some(channel) = yt.get(channel_id) {
        tp.update_ytl_selected_channel_name(&channel.name).await;
    }

    tracing::info!(
        channel = %channel_id,
        broadcast = %broadcast_id,
        "stream selected"
    );

    Ok((channel_id.to_string(), broadcast_id, selection))
}