use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::{watch, Mutex};
use tokio_stream::StreamExt;
use crate::youtube_api::chat::{
    LiveChatMessage, LiveChatMessageDetails, LiveChatStream,
};
use crate::youtube_api::client::YouTubeClient;
use crate::Channel;

use crate::activity::AdaptivePollingState;
use crate::background::metrics::StreamSelection;

/// Process a chat message and trigger appropriate TouchPortal events
/// Also updates adaptive polling state based on chat activity
pub async fn process_chat_message(
    outgoing: &mut crate::plugin::TouchPortalHandle,
    message: LiveChatMessage,
    adaptive_state: Arc<Mutex<AdaptivePollingState>>,
) {
    let author_name = message
        .author_details
        .as_ref()
        .map(|a| a.display_name.clone())
        .unwrap_or_else(|| "Anonymous".to_string());
    let author_id = message
        .author_details
        .as_ref()
        .map(|a| a.channel_id.clone())
        .unwrap_or_else(|| "unknown".to_string());
    let timestamp = message.snippet.published_at.to_string();

    // Register chat activity for adaptive polling
    {
        let mut state = adaptive_state.lock().await;
        state.register_chat_message();
    }

    match message.snippet.details {
        LiveChatMessageDetails::TextMessage {
            text_message_details,
        } => {
            let message_text = &text_message_details.message_text;

            // Trigger chat message event with local states
            outgoing
                .force_trigger_ytl_new_chat_message(
                    message_text,
                    &author_name,
                    &author_id,
                    &timestamp,
                )
                .await;

            // Update latest message state
            outgoing.update_ytl_latest_chat_message(message_text).await;

            tracing::debug!(
                author = %author_name,
                message = %message_text,
                "processed chat message"
            );
        }
        LiveChatMessageDetails::SuperChat { super_chat_details } => {
            let message_text = super_chat_details
                .user_comment
                .as_deref()
                .unwrap_or("(no message)");
            let amount_micros: u64 = super_chat_details.amount_micros.parse().unwrap_or(0);
            let amount_display = format!(
                "{:.2} {}",
                amount_micros as f64 / 1_000_000.0,
                super_chat_details.currency
            );

            // Trigger super chat event with local states
            outgoing
                .trigger_ytl_new_super_chat(
                    message_text,
                    &author_name,
                    &amount_display,
                    &super_chat_details.currency,
                )
                .await;

            // Update latest super chat state
            outgoing
                .update_ytl_latest_super_chat(&format!(
                    "{}: {} ({})",
                    author_name, message_text, amount_display
                ))
                .await;

            tracing::info!(
                author = %author_name,
                amount = %amount_display,
                message = %message_text,
                "processed super chat"
            );
        }
        LiveChatMessageDetails::NewSponsor {
            new_sponsor_details,
        } => {
            let member_level_name = &new_sponsor_details.member_level_name;

            // Trigger new sponsor event with local states
            outgoing
                .trigger_ytl_new_sponsor(&author_name, member_level_name, "1")
                .await;

            // Update latest sponsor state
            outgoing
                .update_ytl_latest_sponsor(&format!(
                    "{}: 1 month - {}",
                    author_name, member_level_name
                ))
                .await;

            tracing::info!(
                author = %author_name,
                level = %member_level_name,
                "processed new sponsor"
            );
        }
        LiveChatMessageDetails::MemberMilestone {
            member_milestone_chat_details,
        } => {
            let member_level_name = &member_milestone_chat_details.member_level_name;

            // Treat milestone as sponsor event with month information
            outgoing
                .trigger_ytl_new_sponsor(
                    &author_name,
                    member_level_name,
                    &member_milestone_chat_details.member_month.to_string(),
                )
                .await;

            // Update latest sponsor state
            outgoing
                .update_ytl_latest_sponsor(&format!(
                    "{}: {} months - {}",
                    author_name, member_milestone_chat_details.member_month, member_level_name
                ))
                .await;

            tracing::info!(
                author = %author_name,
                level = %member_level_name,
                months = member_milestone_chat_details.member_month,
                "processed member milestone"
            );
        }
        _ => {
            // Log other message types but don't process them for now
            tracing::debug!(
                author = %author_name,
                message_type = ?message.snippet.details,
                "received unprocessed message type"
            );
        }
    }
}

/// Restart chat stream when stream selection changes - optimized version
pub async fn restart_chat_stream_optimized(
    chat_stream: &mut Option<Pin<Box<LiveChatStream>>>,
    channels: &HashMap<String, Channel>,
    channel_id: Option<String>,
    broadcast_id: Option<String>,
    live_chat_id: Option<String>,
) {
    // Clean up old stream
    *chat_stream = None;

    if let (Some(channel_id), Some(broadcast_id)) = (channel_id, broadcast_id) {
        if let Some(channel) = channels.get(&channel_id) {
            if let Some(chat_id) = live_chat_id {
                // We already have the live chat ID - start streaming immediately
                let new_stream = LiveChatStream::new(channel.yt.clone(), chat_id.clone());
                *chat_stream = Some(Box::pin(new_stream));

                tracing::info!(
                    channel = %channel_id,
                    broadcast = %broadcast_id,
                    chat_id = %chat_id,
                    "started chat monitoring with pre-fetched chat ID"
                );
            } else {
                // Fallback: we don't have the live chat ID, try to get it from broadcast data
                tracing::debug!(
                    channel = %channel_id,
                    broadcast = %broadcast_id,
                    "no pre-fetched chat ID, trying to get from broadcast data"
                );

                // This could happen for manually selected broadcasts
                match get_live_chat_id_fallback(&channel.yt, &broadcast_id).await {
                    Ok(Some(chat_id)) => {
                        let new_stream = LiveChatStream::new(channel.yt.clone(), chat_id.clone());
                        *chat_stream = Some(Box::pin(new_stream));

                        tracing::info!(
                            channel = %channel_id,
                            broadcast = %broadcast_id,
                            chat_id = %chat_id,
                            "started chat monitoring with fallback chat ID lookup"
                        );
                    }
                    Ok(None) => {
                        tracing::info!(
                            channel = %channel_id,
                            broadcast = %broadcast_id,
                            "no active chat for broadcast"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            channel = %channel_id,
                            broadcast = %broadcast_id,
                            error = %e,
                            "failed to get chat ID for broadcast"
                        );
                    }
                }
            }
        }
    } else {
        tracing::debug!("cleared chat stream (no stream selected)");
    }
}

/// Fallback function to get live chat ID when not pre-fetched
pub async fn get_live_chat_id_fallback(
    client: &YouTubeClient,
    broadcast_id: &str,
) -> eyre::Result<Option<String>> {
    // Use video statistics approach as fallback for manually selected broadcasts
    match client.get_video_statistics(broadcast_id).await {
        Ok(stats) => {
            if let Some(live_details) = &stats.live_streaming_details {
                return Ok(live_details.active_live_chat_id.clone());
            }
        }
        Err(e) => {
            tracing::warn!(
                broadcast_id = %broadcast_id,
                error = %e,
                "fallback: failed to get live chat ID from video statistics"
            );
        }
    }

    Ok(None)
}

/// Spawn the background chat monitoring task
pub async fn spawn_chat_task(
    mut outgoing: crate::plugin::TouchPortalHandle,
    channels: HashMap<String, Channel>,
    mut stream_rx: watch::Receiver<StreamSelection>,
    adaptive_state: Arc<Mutex<AdaptivePollingState>>,
) {
    tokio::spawn(async move {
        let mut chat_stream: Option<Pin<Box<LiveChatStream>>> = None;
        let mut current_broadcast: Option<String> = None;

        // Initialize chat stream if we have a current broadcast
        let selection = stream_rx.borrow().clone();
        if selection.broadcast_id != current_broadcast {
            restart_chat_stream_optimized(
                &mut chat_stream,
                &channels,
                selection.channel_id,
                selection.broadcast_id.clone(),
                selection.live_chat_id,
            )
            .await;
            current_broadcast = selection.broadcast_id;
        }

        loop {
            tokio::select! {
                // Process chat messages immediately - never blocked by API calls
                Some(chat_msg) = async {
                    match &mut chat_stream {
                        Some(stream) => stream.next().await,
                        None => std::future::pending().await, // Wait indefinitely if no stream
                    }
                } => {
                    if let Ok(msg) = chat_msg {
                        process_chat_message(&mut outgoing, msg, adaptive_state.clone()).await;
                    }
                }

                // React immediately to stream selection changes
                Ok(()) = stream_rx.changed() => {
                    let selection = stream_rx.borrow().clone();

                    if selection.broadcast_id != current_broadcast {
                        tracing::debug!(
                            old_broadcast = ?current_broadcast,
                            new_broadcast = ?selection.broadcast_id,
                            "stream selection changed - updating chat stream"
                        );

                        restart_chat_stream_optimized(
                            &mut chat_stream,
                            &channels,
                            selection.channel_id,
                            selection.broadcast_id.clone(),
                            selection.live_chat_id
                        ).await;
                        current_broadcast = selection.broadcast_id;
                    }
                }
            }
        }
    });
}