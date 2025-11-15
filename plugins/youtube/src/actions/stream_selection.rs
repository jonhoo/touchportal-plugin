use crate::Channel;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::background::video_metrics::StreamSelection;
use crate::notifications;

#[derive(Debug, Clone)]
pub enum BroadcastSelection {
    Latest,
    Specific(String),
}

impl BroadcastSelection {
    pub fn from_saved_id(id: &str) -> Option<Self> {
        if id == "latest" {
            Some(BroadcastSelection::Latest)
        } else if !id.is_empty() {
            Some(BroadcastSelection::Specific(id.to_string()))
        } else {
            None
        }
    }
}

/// Handle the complex stream selection logic for choosing a broadcast
pub async fn handle_select_stream(
    tp: &mut crate::plugin::TouchPortalHandle,
    yt: &Arc<Mutex<HashMap<String, Channel>>>,
    channel_selection: String,
    broadcast_selection: BroadcastSelection,
) -> eyre::Result<Option<(String, BroadcastSelection, StreamSelection)>> {
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
            notifications::channel_not_available(tp).await?;
            return Ok(None);
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
            tp.set_selected_broadcast_id("latest".to_string()).await;

            tp.update_ytl_current_stream_title("Waiting for active broadcast...")
                .await;

            Ok(Some((
                channel_id.to_string(),
                BroadcastSelection::Latest,
                selection,
            )))
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
                };
                let Some(channel) = channel else {
                    notifications::channel_not_available(tp).await?;
                    return Ok(None);
                };

                // Fetch broadcast details to get live chat ID
                match channel.yt.get_live_broadcast(&id).await {
                    Ok(broadcast) => {
                        match broadcast.snippet.live_chat_id {
                            Some(live_chat_id) => (id, Some(live_chat_id)),
                            None => {
                                tracing::warn!(
                                    broadcast = %id,
                                    "selected broadcast has no live chat; it may have ended"
                                );
                                // Clear the broadcast selection since it's not usable for chat
                                tp.set_selected_broadcast_id(String::new()).await;
                                notifications::selected_broadcast_not_available(tp).await?;
                                return Ok(None);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            broadcast = %id,
                            error = %e,
                            "failed to get broadcast details for selected broadcast; it may have been deleted"
                        );
                        // Clear the broadcast selection since it's not accessible
                        tp.set_selected_broadcast_id(String::new()).await;
                        notifications::selected_broadcast_not_available(tp).await?;
                        return Ok(None);
                    }
                }
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

            Ok(Some((
                channel_id.to_string(),
                BroadcastSelection::Specific(broadcast_id),
                selection,
            )))
        }
    }
}
