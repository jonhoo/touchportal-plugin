use crate::Channel;
use crate::youtube_api::client::YouTubeClient;
use crate::youtube_api::videos::LiveBroadcastContent;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, watch};

use crate::activity::AdaptivePollingState;

/// Stream selection data for coordinating between action handlers and background tasks
#[derive(Debug, Clone, PartialEq)]
pub enum StreamSelection {
    /// No stream is selected
    None,
    /// Only a channel is selected (no specific broadcast)
    ChannelOnly { channel_id: String },
    /// Channel and broadcast are selected
    ChannelAndBroadcast {
        channel_id: String,
        broadcast_id: String,
        live_chat_id: String,
        /// If true, return to WaitForActiveBroadcast mode when this broadcast completes
        return_to_latest_on_completion: bool,
    },
    /// Waiting for an active broadcast to appear on the channel
    WaitForActiveBroadcast { channel_id: String },
}

/// Poll video statistics and update TouchPortal states.
///
/// Also updates adaptive polling state based on metrics changes.
///
/// Returns true if broadcast appears to have completed.
pub async fn poll_and_update_metrics(
    outgoing: &mut crate::plugin::TouchPortalHandle,
    client: &YouTubeClient,
    broadcast_id: &str,
    stream_rx: &watch::Receiver<StreamSelection>,
    adaptive_state: Arc<Mutex<AdaptivePollingState>>,
) -> bool {
    match client.get_video_statistics(broadcast_id).await {
        Ok(stats) => {
            // Check if the selected broadcast has changed during the API call
            {
                let current_selection = stream_rx.borrow();
                let current_broadcast_id = match &*current_selection {
                    StreamSelection::ChannelAndBroadcast { broadcast_id, .. } => Some(broadcast_id),
                    _ => None,
                };
                if current_broadcast_id.map(|s| s.as_str()) != Some(broadcast_id) {
                    tracing::debug!(
                        polled_broadcast = %broadcast_id,
                        current_broadcast = ?current_broadcast_id,
                        "broadcast changed during metrics poll - discarding results"
                    );
                    return false;
                }
            }

            // Extract metrics for adaptive polling tracking
            let current_viewers = stats
                .live_streaming_details
                .as_ref()
                .and_then(|d| d.concurrent_viewers);
            let current_likes = stats
                .statistics
                .like_count
                .as_ref()
                .and_then(|s| s.parse::<u64>().ok());
            let current_views = stats
                .statistics
                .view_count
                .as_ref()
                .and_then(|s| s.parse::<u64>().ok());

            // If we spot that the broadcast has completed, re-start the search for "latest"
            let broadcast_completed = match &stats.snippet.live_broadcast_content {
                LiveBroadcastContent::Live => {
                    tracing::trace!(broadcast = %broadcast_id, status = "live", "broadcast is currently live");
                    false
                }
                LiveBroadcastContent::Upcoming => {
                    tracing::trace!(broadcast = %broadcast_id, status = "upcoming", "broadcast is upcoming");
                    false
                }
                LiveBroadcastContent::None => {
                    tracing::trace!(broadcast = %broadcast_id, status = "none", "broadcast completed");
                    true
                }
            };

            // Update adaptive polling state with new metrics
            {
                let mut state = adaptive_state.lock().await;
                state.update_from_metrics(current_viewers, current_likes, current_views);
            }

            // Update basic video statistics
            if let Some(view_count) = &stats.statistics.view_count {
                outgoing.update_ytl_views_count(view_count).await;
            }
            if let Some(like_count) = &stats.statistics.like_count {
                outgoing.update_ytl_likes_count(like_count).await;
            }
            if let Some(dislike_count) = &stats.statistics.dislike_count {
                outgoing.update_ytl_dislikes_count(dislike_count).await;
            }

            // Update live streaming metrics (only available during live broadcasts)
            if let Some(live_details) = &stats.live_streaming_details
                && let Some(concurrent_viewers) = live_details.concurrent_viewers
            {
                outgoing
                    .update_ytl_live_viewers_count(&concurrent_viewers.to_string())
                    .await;
            } else {
                // Not currently live - clear live viewer count
                outgoing.update_ytl_live_viewers_count("-").await;
            }

            tracing::trace!(
                broadcast_id = %broadcast_id,
                views = ?stats.statistics.view_count,
                likes = ?stats.statistics.like_count,
                live_viewers = ?stats.live_streaming_details.as_ref().and_then(|d| d.concurrent_viewers),
                broadcast_status = ?stats.snippet.live_broadcast_content,
                broadcast_completed = broadcast_completed,
                "updated metrics"
            );

            broadcast_completed
        }
        Err(e) => {
            // TODO(jon): Improve error handling robustness for production use
            // Current implementation immediately shows "X" on any error, but this could be improved:
            //
            // RATE LIMITING DETECTION:
            // - Check for HTTP 403 with quotaExceeded error to distinguish from other failures
            // - Show "QUOTA" instead of "X" when quota is exhausted
            // - Implement exponential backoff: start with 2x interval, max 10x interval
            // - Reset backoff on successful API call
            //
            // TRANSIENT ERROR HANDLING:
            // - Retry network errors (HTTP 500, timeout, connection reset) with backoff
            // - Don't clear states immediately on first failure - wait for 2-3 consecutive failures
            // - Show different indicators: "?" for network errors, "X" for persistent failures
            //
            // TOKEN REFRESH INTEGRATION:
            // - Detect HTTP 401/403 authentication errors
            // - Trigger token refresh and retry the API call
            // - Only clear states if token refresh also fails

            tracing::warn!(
                broadcast_id = %broadcast_id,
                error = %e,
                "failed to poll video statistics"
            );

            // Clear metrics states on repeated failures to show current status
            outgoing.update_ytl_views_count("X").await;
            outgoing.update_ytl_likes_count("X").await;
            outgoing.update_ytl_dislikes_count("X").await;
            outgoing.update_ytl_live_viewers_count("X").await;

            false // Error - assume not completed
        }
    }
}

/// Spawn the background metrics polling task
pub async fn spawn_metrics_task(
    mut outgoing: crate::plugin::TouchPortalHandle,
    channels: Arc<Mutex<HashMap<String, Channel>>>,
    mut stream_rx: watch::Receiver<StreamSelection>,
    stream_selection_tx: watch::Sender<StreamSelection>,
    adaptive_state: Arc<Mutex<AdaptivePollingState>>,
    base_interval: u64,
    polling_interval_rx: watch::Receiver<u64>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut current_interval = base_interval;
        // TODO(claude): instead of using a tokio interval here, use a tokio::sleep, and each time it expires or the polling interval changes, ask the adaptive state for what the next sleep interval should be. then add that to the time of the last poll.
        let mut interval = tokio::time::interval(Duration::from_secs(current_interval));
        let mut polling_interval_rx = polling_interval_rx;

        loop {
            // TODO(claude): compute the sleep time here at the top of the loop rather than updating it proactively

            tokio::select! {
                _ = interval.tick() => {
                    // Time to poll metrics - check if we should recalculate interval
                    let should_recalculate = {
                        let state = adaptive_state.lock().await;
                        state.should_recalculate_interval()
                    };

                    if should_recalculate {
                        let new_interval = {
                            let mut state = adaptive_state.lock().await;
                            state.calculate_optimal_interval()
                        };

                        if new_interval != current_interval {
                            let (chat_level, metrics_level) = {
                                let state = adaptive_state.lock().await;
                                (state.chat_tracker.calculate_activity_level(),
                                 state.metrics_tracker.calculate_volatility())
                            };

                            tracing::info!(
                                old_interval = current_interval,
                                new_interval = new_interval,
                                chat_activity = ?chat_level,
                                metrics_volatility = ?metrics_level,
                                "adaptive polling interval updated"
                            );
                            current_interval = new_interval;
                            interval = tokio::time::interval(Duration::from_secs(current_interval));

                            // Update status display
                            {
                                let state = adaptive_state.lock().await;
                                outgoing
                                    .update_ytl_adaptive_polling_status(&state.get_status_description())
                                    .await;
                            }
                        }
                    }
                }
                Ok(()) = polling_interval_rx.changed() => {
                    // Manual interval change (e.g., settings update)
                    let new_base_interval = *polling_interval_rx.borrow();
                    {
                        let mut state = adaptive_state.lock().await;
                        state.base_interval = new_base_interval;
                        // Recalculate with new base
                        let new_interval = state.calculate_optimal_interval();
                        if new_interval != current_interval {
                            tracing::debug!(
                                old_interval = current_interval,
                                new_interval = new_interval,
                                "updating polling interval from settings"
                            );
                            current_interval = new_interval;
                            interval = tokio::time::interval(Duration::from_secs(current_interval));
                        }
                        continue;
                    }
                }
            }

            // Get current stream selection (non-blocking)
            let (channel_id, broadcast_id, return_to_latest_on_completion) = loop {
                let selection = stream_rx.borrow().clone();
                match selection {
                    StreamSelection::ChannelAndBroadcast {
                        channel_id,
                        broadcast_id,
                        return_to_latest_on_completion,
                        live_chat_id: _,
                    } => {
                        break (channel_id, broadcast_id, return_to_latest_on_completion);
                    }
                    _ => {
                        // No broadcast selected - clear metrics states
                        outgoing.update_ytl_views_count("-").await;
                        outgoing.update_ytl_likes_count("-").await;
                        outgoing.update_ytl_dislikes_count("-").await;
                        outgoing.update_ytl_live_viewers_count("-").await;
                        // Then, wait for a stream to be selected
                        if stream_rx.changed().await.is_err() {
                            tracing::warn!("stream selection watch ended");
                            return;
                        }
                    }
                }
            };

            // Get channel from shared state
            let channel_opt = {
                let channels_guard = channels.lock().await;
                channels_guard.get(&channel_id).cloned()
            };

            let Some(channel) = channel_opt else {
                tracing::warn!(
                    channel = %channel_id,
                    "channel without authenticated client selected"
                );
                continue;
            };

            tracing::debug!(
                channel = %channel_id,
                broadcast = %broadcast_id,
                "polling metrics"
            );
            let broadcast_completed = poll_and_update_metrics(
                &mut outgoing,
                &channel.yt,
                &broadcast_id,
                &stream_rx,
                adaptive_state.clone(),
            )
            .await;

            // Check if broadcast has completed and we should return to waiting mode
            if broadcast_completed {
                let next_selection = if return_to_latest_on_completion {
                    // Broadcast completed - return to WaitForActiveBroadcast mode
                    tracing::info!(
                        channel = %channel_id,
                        broadcast = %broadcast_id,
                        "broadcast completed - returning to wait for active broadcast mode"
                    );

                    outgoing
                        .update_ytl_current_stream_title("Waiting for active broadcast...")
                        .await;

                    StreamSelection::WaitForActiveBroadcast {
                        channel_id: channel_id.clone(),
                    }
                } else {
                    // Broadcast completed, so user must select a new one
                    tracing::info!(
                        channel = %channel_id,
                        broadcast = %broadcast_id,
                        "broadcast completed - returning to broadcast selection mode"
                    );

                    // Clear the selected stream dropdown selection.
                    outgoing.set_selected_broadcast_id(String::new()).await;
                    outgoing
                        .update_ytl_current_stream_title("No stream selected...")
                        .await;

                    StreamSelection::ChannelOnly {
                        channel_id: channel_id.clone(),
                    }
                };

                // Send selection change to background tasks
                if let Err(e) = stream_selection_tx.send(next_selection) {
                    tracing::warn!(
                        error = %e,
                        "failed to send completion transition to background tasks"
                    );
                }
            }
        }
    })
}
