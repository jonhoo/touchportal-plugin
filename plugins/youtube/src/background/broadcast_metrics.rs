use crate::Channel;
use crate::youtube_api::client::YouTubeClient;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, watch};

use crate::activity::AdaptivePollingState;
use crate::background::video_metrics::StreamSelection;

/// Poll broadcast statistics (specifically chat count) and update TouchPortal states.
///
/// Uses direct broadcast lookup for efficiency instead of listing all broadcasts.
pub async fn poll_and_update_broadcast_metrics(
    outgoing: &mut crate::plugin::TouchPortalHandle,
    client: &YouTubeClient,
    broadcast_id: &str,
    stream_rx: &watch::Receiver<StreamSelection>,
) {
    // Check if we have a specific broadcast selected before making API call
    let current_broadcast_id = {
        let current_selection = stream_rx.borrow();
        match &*current_selection {
            StreamSelection::ChannelAndBroadcast { broadcast_id, .. } => Some(broadcast_id.clone()),
            _ => None,
        }
    };
    if current_broadcast_id.as_deref() != Some(broadcast_id) {
        tracing::debug!(
            polled_broadcast = %broadcast_id,
            current_broadcast = ?current_broadcast_id,
            "broadcast changed during broadcast metrics polling setup - skipping"
        );
        return;
    }

    // Get broadcast statistics using direct broadcast lookup
    match client.get_live_broadcast(broadcast_id).await {
        Ok(broadcast) => {
            let chat_count_opt = broadcast
                .statistics
                .and_then(|stats| stats.total_chat_count);
            // Check if the selected broadcast has changed during the API call
            let current_broadcast_id = {
                let current_selection = stream_rx.borrow();
                match &*current_selection {
                    StreamSelection::ChannelAndBroadcast { broadcast_id, .. } => {
                        Some(broadcast_id.clone())
                    }
                    _ => None,
                }
            };
            if current_broadcast_id.as_deref() != Some(broadcast_id) {
                tracing::debug!(
                    polled_broadcast = %broadcast_id,
                    current_broadcast = ?current_broadcast_id,
                    "broadcast changed during broadcast metrics API call - discarding results"
                );
                return;
            }

            // Update chat count state
            if let Some(ref chat_count) = chat_count_opt {
                outgoing.update_ytl_chat_count(chat_count).await;
            } else {
                // No chat count available (chat disabled, not live, etc.)
                outgoing.update_ytl_chat_count("-").await;
            }

            // Note: We don't update adaptive polling state here since
            // broadcast metrics (chat count) are different from video metrics.
            // The adaptive polling system is primarily driven by
            // chat activity (individual messages) and video metrics volatility.

            tracing::trace!(
                broadcast_id = %broadcast_id,
                chat_count = ?chat_count_opt,
                "updated broadcast metrics"
            );
        }
        Err(e) => {
            tracing::warn!(
                broadcast_id = %broadcast_id,
                error = %e,
                "failed to poll broadcast statistics"
            );

            // Clear broadcast metrics state on failure
            outgoing.update_ytl_chat_count("X").await;
        }
    }
}

/// Spawn the broadcast metrics polling background task.
///
/// This task polls broadcast statistics (specifically chat count) for the currently
/// selected broadcast and updates TouchPortal states. It uses direct broadcast lookup
/// for efficiency and the same adaptive polling logic as the video metrics task.
pub async fn spawn_broadcast_metrics_task(
    mut outgoing: crate::plugin::TouchPortalHandle,
    channels: Arc<Mutex<HashMap<String, Channel>>>,
    mut stream_rx: watch::Receiver<StreamSelection>,
    adaptive_state: Arc<Mutex<AdaptivePollingState>>,
    polling_interval_rx: watch::Receiver<u64>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut polling_interval_rx = polling_interval_rx;
        let mut last_poll_time = tokio::time::Instant::now();

        loop {
            // Compute the sleep time at the top of each loop iteration
            // so that we don't fail to take into account early continue or return.
            let sleep_duration = {
                let interval = {
                    let mut state = adaptive_state.lock().await;
                    state.calculate_optimal_interval(&mut outgoing).await
                };

                // Calculate time since last poll and determine sleep duration
                let elapsed_since_last_poll = last_poll_time.elapsed();
                let target_interval = Duration::from_secs(interval);

                if elapsed_since_last_poll >= target_interval {
                    Duration::from_millis(0) // Poll immediately
                } else {
                    target_interval - elapsed_since_last_poll
                }
            };

            tokio::select! {
                _ = tokio::time::sleep(sleep_duration) => {
                    // Time to poll chat metrics!
                }
                Ok(()) = polling_interval_rx.changed() => {
                    // Manual interval change (e.g., settings update)
                    let new_base_interval = *polling_interval_rx.borrow_and_update();
                    {
                        let mut state = adaptive_state.lock().await;
                        state.base_interval = new_base_interval;
                        tracing::debug!(
                            new_base_interval = new_base_interval,
                            "updating base broadcast metrics polling interval from settings"
                        );
                    }
                    continue; // Recalculate sleep time with new base interval
                }
                _ = stream_rx.changed() => {
                    // Stream selection changed -- immediately fetch new broadcast metrics if we can
                }
            }

            // Get current stream selection (non-blocking)
            let (channel_id, broadcast_id) = loop {
                let selection = stream_rx.borrow_and_update().clone();
                match selection {
                    StreamSelection::ChannelAndBroadcast {
                        channel_id,
                        broadcast_id,
                        return_to_latest_on_completion: _,
                        live_chat_id: _,
                    } => {
                        break (channel_id, broadcast_id);
                    }
                    _ => {
                        // No broadcast selected - clear broadcast metrics state
                        outgoing.update_ytl_chat_count("-").await;
                        // Then, wait for a stream to be selected
                        if stream_rx.changed().await.is_err() {
                            tracing::warn!(
                                "stream selection watch ended in broadcast metrics task"
                            );
                            return;
                        }
                    }
                }
            };

            // Get the authenticated client for the channel
            let channel = {
                let channels_guard = channels.lock().await;
                channels_guard.get(&channel_id).cloned()
            };

            let Some(channel) = channel else {
                tracing::warn!(
                    channel = %channel_id,
                    "broadcast metrics task cannot find authenticated client for channel"
                );
                outgoing.update_ytl_chat_count("?").await;
                continue;
            };

            // Record poll time before making API call
            last_poll_time = tokio::time::Instant::now();

            // Poll broadcast metrics for this broadcast
            poll_and_update_broadcast_metrics(
                &mut outgoing,
                &channel.yt,
                &broadcast_id,
                &stream_rx,
            )
            .await;
        }
    })
}
