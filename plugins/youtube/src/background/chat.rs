use crate::Channel;
use crate::youtube_api::chat::{LiveChatMessage, LiveChatMessageDetails, LiveChatStream};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::{Mutex, watch};
use tokio_stream::StreamExt;

use crate::activity::AdaptivePollingState;
use crate::background::video_metrics::StreamSelection;

/// Process a chat message and trigger appropriate TouchPortal events.
///
/// Also updates adaptive polling state based on chat activity.
pub async fn process_chat_message(
    outgoing: &mut crate::plugin::TouchPortalHandle,
    message: LiveChatMessage,
) {
    let author_name = message
        .author_details
        .as_ref()
        .map(|a| a.display_name.as_str())
        .unwrap_or("Anonymous");
    let author_id = message
        .author_details
        .as_ref()
        .map(|a| a.channel_id.as_str())
        .unwrap_or("unknown");
    let timestamp = message.snippet.published_at.to_string();

    match message.snippet.details {
        LiveChatMessageDetails::TextMessage {
            text_message_details,
        } => {
            let message_text = &text_message_details.message_text;

            // TODO(jon): Add message.id to chat event local states for moderation support
            // Currently missing message ID which is required for ytl_delete_chat_message action
            // The message.id field contains the unique identifier needed for liveChatMessages.delete
            // Add ytl_chat_message_id as a new local state to ytl_new_chat_message event in build.rs

            // Trigger chat message event with local states
            outgoing
                .trigger_ytl_new_chat_message(message_text, &author_name, &author_id, &timestamp)
                .await;

            // Update global states (triggers ytl_last_*_changed events)
            outgoing.update_ytl_last_chat_message(message_text).await;
            outgoing.update_ytl_last_chat_author(author_name).await;

            tracing::trace!(
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
            let amount = &super_chat_details.amount_display_string;

            // Trigger super chat event with local states
            outgoing
                .trigger_ytl_new_super_chat(
                    message_text,
                    author_name,
                    amount,
                    super_chat_details.amount_micros,
                    super_chat_details.currency,
                )
                .await;

            // Update global states (triggers ytl_last_*_changed events)
            outgoing.update_ytl_last_super_chat(message_text).await;
            outgoing
                .update_ytl_last_super_chat_author(author_name)
                .await;
            outgoing.update_ytl_last_super_chat_amount(amount).await;

            tracing::trace!(
                author = %author_name,
                amount = %amount,
                message = %message_text,
                "processed super chat"
            );
        }
        LiveChatMessageDetails::NewSponsor {
            new_sponsor_details,
        } => {
            let member_level_name = &new_sponsor_details.member_level_name;

            // Trigger new member event with local states
            outgoing
                .trigger_ytl_new_member(&author_name, member_level_name)
                .await;

            // Update global states (triggers ytl_last_*_changed events)
            outgoing.update_ytl_last_member(author_name).await;
            outgoing
                .update_ytl_last_member_level(member_level_name)
                .await;
            outgoing.update_ytl_last_member_tenure("0").await;

            tracing::trace!(
                author = %author_name,
                level = %member_level_name,
                "processed new member"
            );
        }
        LiveChatMessageDetails::MemberMilestone {
            member_milestone_chat_details,
        } => {
            let member_level_name = &member_milestone_chat_details.member_level_name;

            // Trigger milestone-specific event
            outgoing
                .trigger_ytl_new_member_milestone(
                    &author_name,
                    member_level_name,
                    &member_milestone_chat_details.member_month.to_string(),
                )
                .await;

            // Update global states (triggers ytl_last_*_changed events)
            outgoing.update_ytl_last_member(author_name).await;
            outgoing
                .update_ytl_last_member_level(member_level_name)
                .await;
            outgoing
                .update_ytl_last_member_tenure(
                    &member_milestone_chat_details.member_month.to_string(),
                )
                .await;

            tracing::trace!(
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
                "ignored chat message of odd type"
            );
        }
    }
}

/// Restart chat stream when stream selection changes
pub async fn restart_chat_stream(
    chat_stream: &mut Option<Pin<Box<LiveChatStream>>>,
    channels: &Arc<Mutex<HashMap<String, Channel>>>,
    selection: &StreamSelection,
) {
    // Clean up old stream
    *chat_stream = None;

    match selection {
        StreamSelection::ChannelAndBroadcast {
            channel_id,
            broadcast_id,
            live_chat_id,
            return_to_latest_on_completion: _,
        } => {
            // Get channel from shared state
            let channel_opt = {
                let channels_guard = channels.lock().await;
                channels_guard.get(channel_id).cloned()
            };

            let Some(channel) = channel_opt else {
                tracing::warn!(
                    channel = %channel_id,
                    broadcast = %broadcast_id,
                    "asked to monitor broadcast, \
                    but its channel does not have an authenticated client"
                );
                return;
            };

            let new_stream = LiveChatStream::new((*channel.yt).clone(), live_chat_id.clone());
            *chat_stream = Some(Box::pin(new_stream));

            tracing::info!(
                channel = %channel_id,
                broadcast = %broadcast_id,
                chat_id = %live_chat_id,
                "started monitoring chat"
            );
        }
        StreamSelection::None
        | StreamSelection::ChannelOnly { .. }
        | StreamSelection::WaitForActiveBroadcast { .. } => {
            tracing::debug!("cleared chat stream (no broadcast selected)");
        }
    }
}

/// Spawn the background chat monitoring task
pub async fn spawn_chat_task(
    mut outgoing: crate::plugin::TouchPortalHandle,
    channels: Arc<Mutex<HashMap<String, Channel>>>,
    mut stream_rx: watch::Receiver<StreamSelection>,
    adaptive_state: Arc<Mutex<AdaptivePollingState>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut chat_stream: Option<Pin<Box<LiveChatStream>>> = None;
        let mut current_broadcast: Option<String> = None;

        // Initialize chat stream if we have a current broadcast
        let selection = stream_rx.borrow_and_update().clone();
        let current_broadcast_id = match &selection {
            StreamSelection::ChannelAndBroadcast { broadcast_id, .. } => Some(broadcast_id.clone()),
            StreamSelection::None
            | StreamSelection::ChannelOnly { .. }
            | StreamSelection::WaitForActiveBroadcast { .. } => None,
        };
        if current_broadcast_id != current_broadcast {
            restart_chat_stream(&mut chat_stream, &channels, &selection).await;
            current_broadcast = current_broadcast_id;
        }

        let mut retry = tokio::time::interval(tokio::time::Duration::from_secs(1));
        loop {
            tokio::select! {
                Some(chat_msg) = async {
                    match &mut chat_stream {
                        Some(stream) => stream.next().await,
                        None => std::future::pending().await,
                    }
                } => {
                    match chat_msg {
                        Ok(msg) => {
                            // Register chat activity for adaptive polling
                            {
                                let mut state = adaptive_state.lock().await;
                                state.register_chat_message();
                            }

                            process_chat_message(&mut outgoing, msg).await;
                        }
                        Err(e) => {
                            tracing::error!(
                                broadcast = %current_broadcast.as_ref().expect("we only have a chat stream if we have a broadcast"),
                                error = %e,
                                "failed to parse incoming chat message"
                            );
                        }
                    }
                }

                // Retry creating the chat stream if we failed initially
                _ = retry.tick(), if chat_stream.is_none() && current_broadcast.is_some() => {
                    restart_chat_stream(
                        &mut chat_stream,
                        &channels,
                        &selection,
                    ).await;
                }

                Ok(()) = stream_rx.changed() => {
                    let selection = stream_rx.borrow_and_update().clone();
                    let new_broadcast_id = match &selection {
                        StreamSelection::ChannelAndBroadcast { broadcast_id, .. } => Some(broadcast_id.clone()),
                        StreamSelection::None | StreamSelection::ChannelOnly { .. } | StreamSelection::WaitForActiveBroadcast { .. } => None,
                    };

                    if new_broadcast_id != current_broadcast {
                        tracing::debug!(
                            old_broadcast = ?current_broadcast,
                            new_broadcast = ?new_broadcast_id,
                            "stream selection changed - updating chat stream"
                        );

                        restart_chat_stream(
                            &mut chat_stream,
                            &channels,
                            &selection,
                        ).await;
                        current_broadcast = new_broadcast_id;
                    }
                }
            }
        }
    })
}
