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

/// Process a single check for latest broadcast changes
async fn process_latest_broadcast_check(
    outgoing: &mut crate::plugin::TouchPortalHandle,
    channels: &Arc<Mutex<HashMap<String, Channel>>>,
    stream_selection_tx: &watch::Sender<StreamSelection>,
    channel_id: &str,
    broadcast_id: &str,
) {
    // Get channel from shared state
    let channel_opt = {
        let channels_guard = channels.lock().await;
        channels_guard.get(channel_id).cloned()
    };

    if let Some(channel) = channel_opt {
        match check_for_latest_broadcast_change(&channel, broadcast_id).await {
            Ok(Some((new_broadcast_id, new_live_chat_id))) => {
                // Latest broadcast changed - update selection
                if let Some(new_chat_id) = new_live_chat_id {
                    let new_selection = StreamSelection::ChannelAndBroadcast {
                        channel_id: channel_id.to_string(),
                        broadcast_id: new_broadcast_id.clone(),
                        live_chat_id: new_chat_id,
                    };

                    // Update TouchPortal settings
                    outgoing
                        .set_selected_broadcast_id(new_broadcast_id.clone())
                        .await;

                    // Get the new broadcast title for status update
                    match get_broadcast_title(&channel, &new_broadcast_id).await {
                        Ok(title) => {
                            // Update current stream title state so users see the change
                            outgoing.update_ytl_current_stream_title(&title).await;
                        }
                        Err(e) => {
                            tracing::warn!(
                                broadcast = %new_broadcast_id,
                                error = %e,
                                "failed to get title for new broadcast"
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
                            old_broadcast = %broadcast_id,
                            new_broadcast = %new_broadcast_id,
                            "automatically switched to latest broadcast"
                        );
                    }
                } else {
                    tracing::warn!(
                        new_broadcast = %new_broadcast_id,
                        "new latest broadcast has no live chat ID - cannot switch"
                    );
                }
            }
            Ok(None) => {
                // No change - continue monitoring
                tracing::debug!(
                    channel = %channel_id,
                    current_broadcast = %broadcast_id,
                    "latest broadcast unchanged"
                );
            }
            Err(e) => {
                tracing::warn!(
                    channel = %channel_id,
                    current_broadcast = %broadcast_id,
                    error = %e,
                    "failed to check for latest broadcast change"
                );
            }
        }
    }
}

/// Spawn the dedicated latest broadcast monitoring task
///
/// This task periodically checks if the "latest non-completed broadcast" has changed
/// when the user has selected that option. When a change is detected, it automatically
/// switches to the new latest broadcast.
pub async fn spawn_latest_monitor_task(
    mut outgoing: crate::plugin::TouchPortalHandle,
    channels: Arc<Mutex<HashMap<String, Channel>>>,
    stream_rx: watch::Receiver<StreamSelection>,
    stream_selection_tx: watch::Sender<StreamSelection>,
    monitor_interval_minutes: u64,
    monitor_interval_rx: watch::Receiver<u64>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut current_interval_minutes = monitor_interval_minutes;
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(
            current_interval_minutes * 60,
        ));
        let mut monitor_interval_rx = monitor_interval_rx;

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    // Time to check for latest broadcast changes
                    let selection = stream_rx.borrow().clone();

                    if let StreamSelection::ChannelAndBroadcast {
                        channel_id,
                        broadcast_id,
                        live_chat_id: _,
                    } = &selection {
                        // Process latest broadcast check
                        process_latest_broadcast_check(
                            &mut outgoing,
                            &channels,
                            &stream_selection_tx,
                            channel_id,
                            broadcast_id,
                        ).await;
                    } else {
                        // No specific broadcast selected - nothing to monitor
                        tracing::debug!("no broadcast selected - skipping latest monitor check");
                    }
                }
                Ok(()) = monitor_interval_rx.changed() => {
                    // Monitor interval changed - update our timer
                    let new_interval_minutes = *monitor_interval_rx.borrow();
                    if new_interval_minutes != current_interval_minutes {
                        tracing::debug!(
                            old_interval = current_interval_minutes,
                            new_interval = new_interval_minutes,
                            "updating latest monitor interval"
                        );
                        current_interval_minutes = new_interval_minutes;
                        interval = tokio::time::interval(std::time::Duration::from_secs(current_interval_minutes * 60));
                        continue; // Skip this iteration to reset timing
                    }
                }
            }
        }
    })
}
