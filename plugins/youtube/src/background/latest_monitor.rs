use crate::Channel;
use crate::youtube_api::broadcasts::BroadcastLifeCycleStatus;
use eyre::Context;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, watch};
use tokio_stream::StreamExt;

use crate::background::metrics::StreamSelection;

/// Get the title of a specific broadcast
async fn get_broadcast_title(channel: &Channel, broadcast_id: &str) -> eyre::Result<String> {
    let broadcasts = channel.yt.list_my_live_broadcasts();
    let mut broadcasts = std::pin::pin!(broadcasts);

    while let Some(broadcast) = broadcasts.next().await {
        let broadcast = broadcast.context("fetch broadcast for title")?;
        if broadcast.id == broadcast_id {
            return Ok(broadcast.snippet.title);
        }
    }

    eyre::bail!("Broadcast {} not found", broadcast_id)
}

/// Check if the latest non-completed broadcast has changed for a given channel
///
/// Returns Some((new_broadcast_id, new_live_chat_id)) if the latest broadcast
/// is different from the current one, None if unchanged or no broadcasts available.
pub async fn check_for_latest_broadcast_change(
    channel: &Channel,
    current_broadcast_id: &str,
) -> eyre::Result<Option<(String, Option<String>)>> {
    let broadcasts = channel.yt.list_my_live_broadcasts();
    let mut broadcasts = std::pin::pin!(broadcasts);

    // Find the first (latest) non-completed broadcast
    while let Some(broadcast) = broadcasts.next().await {
        let broadcast = broadcast.context("fetch broadcast for latest check")?;
        if broadcast.status.life_cycle_status != BroadcastLifeCycleStatus::Complete {
            // Found the latest non-completed broadcast
            if broadcast.id != current_broadcast_id {
                // It's different from what we're currently monitoring
                tracing::info!(
                    current_broadcast = %current_broadcast_id,
                    new_broadcast = %broadcast.id,
                    new_title = %broadcast.snippet.title,
                    "latest broadcast changed"
                );
                return Ok(Some((broadcast.id, broadcast.snippet.live_chat_id.clone())));
            } else {
                // Same broadcast - no change
                return Ok(None);
            }
        }
    }

    // No non-completed broadcasts found
    tracing::debug!(
        current_broadcast = %current_broadcast_id,
        "no non-completed broadcasts found during latest check"
    );
    Ok(None)
}

/// Spawn the dedicated latest broadcast monitoring task
///
/// This task periodically checks if the "latest non-completed broadcast" has changed
/// when the user has selected WaitForActiveBroadcast mode. When a change is detected,
/// it automatically switches to the new latest broadcast.
/// Uses a fixed 5-minute interval for monitoring.
pub async fn spawn_latest_monitor_task(
    mut outgoing: crate::plugin::TouchPortalHandle,
    channels: Arc<Mutex<HashMap<String, Channel>>>,
    stream_rx: watch::Receiver<StreamSelection>,
    stream_selection_tx: watch::Sender<StreamSelection>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // Fixed 5-minute interval for latest broadcast monitoring
        const MONITOR_INTERVAL_MINUTES: u64 = 5;
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(
            MONITOR_INTERVAL_MINUTES * 60,
        ));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    // Time to check for latest broadcast changes
                    let selection = stream_rx.borrow().clone();

                    if let StreamSelection::WaitForActiveBroadcast { channel_id } = &selection {
                        // We're waiting for an active broadcast - check if one appeared
                        tracing::debug!(channel = %channel_id, "checking for new active broadcast");

                        // Get channel from shared state
                        let channel_opt = {
                            let channels_guard = channels.lock().await;
                            channels_guard.get(channel_id).cloned()
                        };

                        if let Some(channel) = channel_opt {
                            match check_for_latest_broadcast_change(&channel, "").await {
                                Ok(Some((new_broadcast_id, Some(new_live_chat_id)))) => {
                                    // Found an active broadcast - switch to monitoring it
                                    let new_selection = StreamSelection::ChannelAndBroadcast {
                                        channel_id: channel_id.clone(),
                                        broadcast_id: new_broadcast_id.clone(),
                                        live_chat_id: new_live_chat_id,
                                        return_to_latest_on_completion: true, // Found from wait mode
                                    };

                                    // Update TouchPortal settings
                                    outgoing
                                        .set_selected_broadcast_id(new_broadcast_id.clone())
                                        .await;

                                    // Get the new broadcast title for status update
                                    match get_broadcast_title(&channel, &new_broadcast_id).await {
                                        Ok(title) => {
                                            outgoing.update_ytl_current_stream_title(&title).await;
                                        }
                                        Err(e) => {
                                            tracing::warn!(
                                                broadcast = %new_broadcast_id,
                                                error = %e,
                                                "failed to get title for new active broadcast"
                                            );
                                        }
                                    }

                                    // Send updated selection to background tasks
                                    if let Err(e) = stream_selection_tx.send(new_selection) {
                                        tracing::warn!(
                                            error = %e,
                                            new_broadcast = %new_broadcast_id,
                                            "failed to send updated stream selection to background tasks"
                                        );
                                    } else {
                                        tracing::info!(
                                            channel = %channel_id,
                                            new_broadcast = %new_broadcast_id,
                                            "found active broadcast - switching from wait mode"
                                        );
                                    }
                                }
                                Ok(Some((new_broadcast_id, None))) => {
                                    tracing::warn!(
                                        broadcast = %new_broadcast_id,
                                        channel = %channel_id,
                                        "found broadcast but no live chat ID - continuing to wait"
                                    );
                                }
                                Ok(None) => {
                                    tracing::debug!(
                                        channel = %channel_id,
                                        "no active broadcast found yet - continuing to wait"
                                    );
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        channel = %channel_id,
                                        error = %e,
                                        "failed to check for active broadcast"
                                    );
                                }
                            }
                        } else {
                            tracing::debug!(
                                channel = %channel_id,
                                "channel not found in shared state - skipping check"
                            );
                        }
                    } else {
                        // Not in wait mode - do nothing
                        tracing::debug!(
                            selection = ?selection,
                            "not in WaitForActiveBroadcast mode - skipping latest monitor check"
                        );
                    }
                }
            }
        }
    })
}
