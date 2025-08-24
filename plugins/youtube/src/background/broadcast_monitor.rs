use crate::Channel;
use crate::youtube_api::broadcasts::BroadcastLifeCycleStatus;
use eyre::Context;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, watch};
use tokio_stream::StreamExt;

use crate::background::metrics::StreamSelection;
use crate::plugin::{ChoicesForYtlBroadcast, TouchPortalHandle};

/// Result of broadcast monitoring that handles multiple responsibilities in one API call
#[derive(Debug)]
struct BroadcastMonitoringResult {
    /// List of broadcast choices for TouchPortal UI updates
    choices: Vec<String>,
    /// Information about the latest active broadcast, if any
    latest_broadcast: Option<LatestBroadcast>,
    /// Status of the currently monitored broadcast from YouTube API
    current_broadcast_status: Option<BroadcastLifeCycleStatus>,
}

#[derive(Debug)]
struct LatestBroadcast {
    id: String,
    live_chat_id: String,
    title: String,
}

/// Broadcast monitoring function that handles all broadcast-related concerns
/// in a single API call to maximize efficiency.
///
/// This function serves multiple purposes:
/// 1. **Choices Updates**: Maintains the list of available broadcasts for TouchPortal UI
/// 2. **Broadcast Detection**: Finds the latest non-completed broadcast for "latest" mode
/// 3. **Status Detection**: Gets the current broadcast status from YouTube API
///
/// This approach is much more API-efficient than separate calls for each concern.
async fn monitor_broadcasts(
    channel: &Channel,
    current_selection: &StreamSelection,
) -> eyre::Result<BroadcastMonitoringResult> {
    let broadcasts = channel.yt.list_my_live_broadcasts();
    let mut broadcasts = std::pin::pin!(broadcasts);

    let mut choices = vec![ChoicesForYtlBroadcast::LatestNonCompletedBroadcast.to_string()];
    let mut latest_broadcast: Option<LatestBroadcast> = None;
    let mut current_broadcast_status: Option<BroadcastLifeCycleStatus> = None;

    // Single pass through broadcasts to gather all needed information
    while let Some(broadcast) = broadcasts.next().await {
        let broadcast = broadcast.context("fetch broadcast for unified monitoring")?;

        // Add to choices list (all broadcasts, regardless of status)
        choices.push(format!("{} - {}", broadcast.snippet.title, broadcast.id));

        // Check if this is our current broadcast (for status detection)
        if let StreamSelection::ChannelAndBroadcast { broadcast_id, .. } = current_selection
            && broadcast.id == *broadcast_id
        {
            current_broadcast_status = Some(broadcast.status.life_cycle_status);
        }

        // Track the latest active broadcast for "latest" mode
        if latest_broadcast.is_none() {
            match broadcast.status.life_cycle_status {
                BroadcastLifeCycleStatus::Ready
                | BroadcastLifeCycleStatus::Testing
                | BroadcastLifeCycleStatus::Live
                | BroadcastLifeCycleStatus::Created => {
                    if let Some(live_chat_id) = broadcast.snippet.live_chat_id {
                        latest_broadcast = Some(LatestBroadcast {
                            id: broadcast.id,
                            live_chat_id,
                            title: broadcast.snippet.title,
                        });
                    } else {
                        tracing::warn!(
                            broadcast = %broadcast.id,
                            "found active broadcast without live chat id - skipping"
                        );
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
    }

    // Results are provided as-is for caller to decide what to do

    Ok(BroadcastMonitoringResult {
        choices,
        latest_broadcast,
        current_broadcast_status,
    })
}

/// Spawn the broadcast monitoring task
///
/// This task handles multiple broadcast-related responsibilities using a single API call
/// per monitoring cycle:
///
/// 1. **Broadcast Choices Updates**: Keeps the TouchPortal UI choices list current
/// 2. **Latest Broadcast Detection**: Finds new broadcasts when in "wait for active" mode
/// 3. **Completion Detection**: Detects when current broadcast ends (moved from metrics task)
///
/// ## Sticky Behavior for "Latest" Mode
///
/// **IMPORTANT DESIGN CHOICE**: Once "Latest non-completed broadcast" picks a specific
/// stream, that selection becomes "sticky" until the stream actually ends. This means:
///
/// - If a user selects "Latest non-completed broadcast" and it picks Stream A
/// - Even if Stream B becomes available later (newer than Stream A)
/// - We continue monitoring Stream A until it completes
/// - Only THEN do we switch to the newest available stream
///
/// **Rationale**: This prevents jarring mid-stream switches that would confuse users.
/// Imagine watching a stream and suddenly the plugin switches to a different stream
/// just because a newer one started - this would be extremely disruptive to the user
/// experience. The "latest" selection is meant to be "pick the latest at the time
/// of selection" not "always switch to whatever is newest."
///
/// **Implementation**: The `return_to_latest_on_completion` flag distinguishes between:
/// - Manually selected broadcasts (sticky until manually changed)
/// - "Latest" mode broadcasts (sticky until completion, then find new latest)
///
/// ## Dynamic Intervals
///
/// The monitoring frequency adapts to the current state:
/// - **1 minute**: `WaitForActiveBroadcast` (responsive for waiting users)
/// - **3 minutes**: `ChannelAndBroadcast` (completion detection + choices updates)
/// - **5 minutes**: Other states (background choices maintenance only)
pub async fn spawn_broadcast_monitor_task(
    channels: Arc<Mutex<HashMap<String, Channel>>>,
    mut stream_rx: watch::Receiver<StreamSelection>,
    stream_selection_tx: watch::Sender<StreamSelection>,
    mut tp: TouchPortalHandle,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // Dynamic interval based on current state
        let mut current_interval_minutes = 5u64; // Default
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(
            current_interval_minutes * 60,
        ));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let selection = stream_rx.borrow_and_update().clone();

                    // Update interval based on current state
                    let needed_interval_minutes = match &selection {
                        StreamSelection::WaitForActiveBroadcast { .. } => 1, // Responsive for waiting users
                        StreamSelection::ChannelAndBroadcast { .. } => 3,   // Completion detection + choices
                        _ => 5, // Background choices updates only
                    };

                    if needed_interval_minutes != current_interval_minutes {
                        current_interval_minutes = needed_interval_minutes;
                        interval = tokio::time::interval(std::time::Duration::from_secs(current_interval_minutes * 60));
                        tracing::debug!(
                            new_interval_minutes = current_interval_minutes,
                            "updated broadcast monitor interval"
                        );
                    }

                    // Get the channel we need to monitor
                    let channel_id = match &selection {
                        StreamSelection::WaitForActiveBroadcast { channel_id } => channel_id,
                        StreamSelection::ChannelAndBroadcast { channel_id, .. } => channel_id,
                        StreamSelection::ChannelOnly { channel_id } => channel_id,
                        StreamSelection::None => {
                            tracing::trace!("no channel selected - skipping broadcast monitoring");
                            continue;
                        }
                    };

                    // Get channel from shared state
                    let channel_opt = {
                        let channels_guard = channels.lock().await;
                        channels_guard.get(channel_id).cloned()
                    };

                    let Some(channel) = channel_opt else {
                        tracing::warn!(
                            channel = %channel_id,
                            "broadcast monitor cannot find authenticated client for channel"
                        );
                        continue;
                    };

                    // Perform broadcast monitoring
                    match monitor_broadcasts(&channel, &selection).await {
                        Ok(result) => {
                            // Update choices (always do this for any channel-based selection)
                            tp.update_choices_in_ytl_broadcast(result.choices.into_iter()).await;

                            // Handle broadcast state changes based on current mode
                            match &selection {
                                StreamSelection::WaitForActiveBroadcast { channel_id } => {
                                    // Check if we found an active broadcast to switch to
                                    if let Some(ref latest) = result.latest_broadcast {
                                        let new_selection = StreamSelection::ChannelAndBroadcast {
                                            channel_id: channel_id.clone(),
                                            broadcast_id: latest.id.clone(),
                                            live_chat_id: latest.live_chat_id.clone(),
                                            return_to_latest_on_completion: true,
                                        };

                                        tracing::info!(
                                            channel = %channel_id,
                                            new_broadcast = %latest.id,
                                            new_title = %latest.title,
                                            "found new active broadcast - switching to monitor it"
                                        );

                                        if let Err(e) = stream_selection_tx.send(new_selection) {
                                            tracing::warn!(
                                                error = %e,
                                                new_broadcast = %latest.id,
                                                "failed to send updated stream selection to background tasks"
                                            );
                                        }
                                    } else {
                                        tracing::trace!(
                                            channel = %channel_id,
                                            "no active broadcast found yet - continuing to wait"
                                        );
                                    }
                                }
                                StreamSelection::ChannelAndBroadcast { channel_id, broadcast_id, return_to_latest_on_completion, .. } => {
                                    // Check if current broadcast completed
                                    let current_completed = matches!(
                                        result.current_broadcast_status,
                                        Some(BroadcastLifeCycleStatus::Complete) | None // None means not found (likely deleted)
                                    );

                                    if current_completed {
                                        let new_selection = if *return_to_latest_on_completion {
                                            // Latest mode: try to switch to new latest active broadcast, or return to waiting
                                            if let Some(ref latest) = result.latest_broadcast {
                                                // Switch to new latest active broadcast
                                                tracing::info!(
                                                    channel = %channel_id,
                                                    old_broadcast = %broadcast_id,
                                                    new_broadcast = %latest.id,
                                                    new_title = %latest.title,
                                                    "current broadcast completed - switching to new latest active broadcast"
                                                );

                                                StreamSelection::ChannelAndBroadcast {
                                                    channel_id: channel_id.clone(),
                                                    broadcast_id: latest.id.clone(),
                                                    live_chat_id: latest.live_chat_id.clone(),
                                                    return_to_latest_on_completion: true,
                                                }
                                            } else {
                                                // No active broadcasts found - return to waiting mode
                                                tracing::info!(
                                                    channel = %channel_id,
                                                    old_broadcast = %broadcast_id,
                                                    "current broadcast completed with no active broadcasts available - returning to wait mode"
                                                );

                                                StreamSelection::WaitForActiveBroadcast {
                                                    channel_id: channel_id.clone(),
                                                }
                                            }
                                        } else {
                                            // Manual mode: deselect completed broadcast
                                            tracing::info!(
                                                channel = %channel_id,
                                                completed_broadcast = %broadcast_id,
                                                "manually selected broadcast completed - deselecting"
                                            );

                                            StreamSelection::ChannelOnly {
                                                channel_id: channel_id.clone(),
                                            }
                                        };

                                        if let Err(e) = stream_selection_tx.send(new_selection) {
                                            tracing::warn!(
                                                error = %e,
                                                channel = %channel_id,
                                                broadcast = %broadcast_id,
                                                "failed to send stream selection after broadcast completion"
                                            );
                                        }
                                    } else {
                                        let mode = if *return_to_latest_on_completion { "latest" } else { "manually selected" };
                                        tracing::trace!(
                                            channel = %channel_id,
                                            broadcast = %broadcast_id,
                                            mode = mode,
                                            status = ?result.current_broadcast_status,
                                            "broadcast still active"
                                        );
                                    }
                                }
                                _ => {
                                    // Not in a mode that requires automatic broadcast switching
                                }
                            }

                        }
                        Err(e) => {
                            tracing::error!(
                                channel = %channel_id,
                                error = %e,
                                "failed to perform broadcast monitoring"
                            );
                        }
                    }
                }
            }
        }
    })
}
