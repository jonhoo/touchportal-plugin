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
) -> eyre::Result<Option<(String, String)>> {
    let broadcasts = channel.yt.list_my_live_broadcasts();
    let mut broadcasts = std::pin::pin!(broadcasts);

    // Find the first (latest) non-completed broadcast
    while let Some(broadcast) = broadcasts.next().await {
        let broadcast = broadcast.context("fetch broadcast for latest check")?;
        match broadcast.status.life_cycle_status {
            BroadcastLifeCycleStatus::Ready
            | BroadcastLifeCycleStatus::Testing
            | BroadcastLifeCycleStatus::Live
            | BroadcastLifeCycleStatus::Created => {
                // Found the latest non-completed broadcast
                if broadcast.id != current_broadcast_id {
                    // It's different from what we're currently monitoring
                    tracing::info!(
                        current_broadcast = %current_broadcast_id,
                        new_broadcast = %broadcast.id,
                        new_title = %broadcast.snippet.title,
                        "latest broadcast changed"
                    );
                    let Some(live_chat_id) = broadcast.snippet.live_chat_id else {
                        eyre::bail!(
                            "active broadcast {} does not have live chat id",
                            broadcast.id
                        );
                    };
                    return Ok(Some((broadcast.id, live_chat_id)));
                } else {
                    // Same broadcast is active and latest - no change
                    return Ok(None);
                }
            }
            BroadcastLifeCycleStatus::Complete => {}
            BroadcastLifeCycleStatus::Revoked => {
                tracing::debug!(
                    broadcast = %broadcast.id,
                    "ignoring revoked broadcast"
                );
            }
        }
    }

    // No non-completed broadcasts found
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
        // Fixed 1-minute interval for latest broadcast monitoring
        const MONITOR_INTERVAL_MINUTES: u64 = 1;
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(
            MONITOR_INTERVAL_MINUTES * 60,
        ));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    // Time to check for latest broadcast changes
                    let selection = stream_rx.borrow().clone();

                    let StreamSelection::WaitForActiveBroadcast { channel_id } = &selection else {
                        // Not in wait mode - do nothing
                        continue;
                    };

                    // We're waiting for an active broadcast - check if one appeared
                    tracing::debug!(channel = %channel_id, "checking for new active broadcast");

                    // Get channel from shared state
                    let channel_opt = {
                        let channels_guard = channels.lock().await;
                        channels_guard.get(channel_id).cloned()
                    };

                    let Some(channel) = channel_opt else {
                        tracing::warn!(
                            channel = %channel_id,
                            "asked to look for latest channel, \
                            but it does not have an authenticated client"
                        );
                        continue;
                    };

                    match check_for_latest_broadcast_change(&channel, "").await {
                        Ok(Some((new_broadcast_id, new_live_chat_id))) => {
                            // Found an active broadcast - switch to monitoring it
                            let new_selection = StreamSelection::ChannelAndBroadcast {
                                channel_id: channel_id.clone(),
                                broadcast_id: new_broadcast_id.clone(),
                                live_chat_id: new_live_chat_id,
                                return_to_latest_on_completion: true,
                            };

                            // Note that we **don't** update the TouchPortal settings
                            // since they should remain set to "latest".

                            // Get the new broadcast title for status update
                            // TODO(claude): the broadcast title should be updated by the metrics background task, which already has access to this information, not here.
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
                                    "failed to send updated stream \
                                    selection to background tasks"
                                );
                            } else {
                                tracing::info!(
                                    channel = %channel_id,
                                    new_broadcast = %new_broadcast_id,
                                    "switching to active broadcast"
                                );
                            }
                        }
                        Ok(None) => {
                            tracing::trace!(
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
                }
            }
        }
    })
}
