use crate::Channel;
use eyre::Context;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_stream::StreamExt;

use crate::background::metrics::StreamSelection;

#[derive(Debug, Clone)]
pub enum BroadcastSelection {
    Latest,
    Specific(String),
}

/// Handle the complex stream selection logic for choosing a broadcast
pub async fn handle_select_stream(
    tp: &mut crate::plugin::TouchPortalHandle,
    yt: &Arc<Mutex<HashMap<String, Channel>>>,
    channel_selection: String,
    broadcast_selection: BroadcastSelection,
) -> eyre::Result<(String, String, StreamSelection)> {
    // Extract channel ID from the selected channel
    let channel_id = channel_selection
        .rsplit_once(" - ")
        .map(|(_, id)| id)
        .ok_or_else(|| eyre::eyre!("Invalid channel selection format"))?;

    // Set the channel metadata state
    {
        let yt_guard = yt.lock().await;
        if let Some(channel) = yt_guard.get(channel_id) {
            tp.update_ytl_selected_channel_name(&channel.name).await;
        } else {
            // TODO(claude): this should give a channel_not_available notification and return
        }
    }
    // And persist it
    tp.set_selected_channel_id(channel_id.to_string()).await;

    match broadcast_selection {
        BroadcastSelection::Latest => {
            // Create WaitForActiveBroadcast selection - don't look for current broadcast
            let selection = StreamSelection::WaitForActiveBroadcast {
                channel_id: channel_id.to_string(),
            };

            // Update settings for persistence
            tp.set_selected_channel_id(channel_id.to_string()).await;
            // TODO(claude): here we need to set "latest" as the saved broadcast id, and then handle that again when we read settings back out.
            tp.set_selected_broadcast_id("".to_string()).await; // Clear broadcast ID

            tp.update_ytl_current_stream_title("Waiting for active broadcast...")
                .await;

            Ok((channel_id.to_string(), "latest".to_string(), selection))
        }
        BroadcastSelection::Specific(broadcast_selection) => {
            // Extract broadcast ID and live chat ID for specific broadcast selection
            let (broadcast_id, live_chat_id) = {
                // Manually selected broadcast - fetch its details to get the live chat ID
                let id = broadcast_selection
                    .rsplit_once(" - ")
                    .map(|(_, id)| id.to_string())
                    .ok_or_else(|| eyre::eyre!("Invalid broadcast selection format"))?;

                let channel = {
                    let yt_guard = yt.lock().await;
                    yt_guard.get(channel_id).cloned()
                }
                // TODO(claude): this should not error, but instead just give a channel_not_available notification as above
                .ok_or_else(|| eyre::eyre!("Selected channel not found"))?;

                // Fetch broadcast details to get live chat ID
                // TODO(claude): use liveStreamingDetails.activeLiveChatId from video get_metadata here so you don't need the helper. note that the stream may have completed and thus not have an activeLiveChatId. in which case we should just move to StreamSelection::ChannelOnly and alert the user that the selected stream is no longer live.
                let broadcasts = channel.yt.list_my_live_broadcasts();
                let mut broadcasts = std::pin::pin!(broadcasts);

                let mut live_chat_id = None;
                while let Some(broadcast) = broadcasts.next().await {
                    let broadcast = broadcast.context("fetch broadcast details")?;
                    if broadcast.id == id {
                        live_chat_id = broadcast.snippet.live_chat_id.clone();
                        break;
                    }
                }

                let chat_id = live_chat_id
                    .ok_or_else(|| eyre::eyre!("Live chat ID not found for broadcast {}", id))?;
                (id, Some(chat_id))
            };

            // Create stream selection object
            let selection = StreamSelection::ChannelAndBroadcast {
                channel_id: channel_id.to_string(),
                broadcast_id: broadcast_id.clone(),
                live_chat_id: live_chat_id
                    .expect("live_chat_id should always be present for broadcasts"),
                return_to_latest_on_completion: false, // Manually selected broadcast
            };

            // Update settings for persistence
            tp.set_selected_broadcast_id(broadcast_id.clone()).await;

            tracing::info!(
                channel = %channel_id,
                broadcast = %broadcast_id,
                "stream selected"
            );

            Ok((channel_id.to_string(), broadcast_id, selection))
        }
    }
}
