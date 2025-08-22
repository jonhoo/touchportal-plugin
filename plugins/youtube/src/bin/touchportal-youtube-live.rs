use eyre::Context;
use oauth2::{RefreshToken, TokenResponse};
use std::collections::{HashMap, HashSet};
use std::pin::Pin;
use std::time::Duration;
use tokio::sync::watch;
use tokio_stream::StreamExt;
use touchportal_sdk::protocol::{CreateNotificationCommand, InfoMessage};
use touchportal_youtube_live::youtube_api::broadcasts::{
    BroadcastLifeCycleStatus, BroadcastStatus, LiveBroadcastUpdateRequest,
    LiveBroadcastUpdateSnippet,
};
use touchportal_youtube_live::youtube_api::chat::{
    LiveChatMessage, LiveChatMessageDetails, LiveChatStream,
};
use touchportal_youtube_live::youtube_api::client::{TimeBoundAccessToken, YouTubeClient};
use touchportal_youtube_live::{Channel, oauth, setup_youtube_clients};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;

/// Stream selection data for coordinating between action handlers and background tasks
#[derive(Debug, Clone, PartialEq)]
struct StreamSelection {
    channel_id: Option<String>,
    broadcast_id: Option<String>,
    live_chat_id: Option<String>,
}

// You can look at the generated code for a plugin using this command:
//
// ```bash
// cat "$(dirname "$(cargo check --message-format=json | jq -r 'select(.reason == "build-script-executed") | select(.package_id | contains("#touchportal-")).out_dir')")/entry.rs"
// ```
include!(concat!(env!("OUT_DIR"), "/entry.rs"));

#[derive(Debug)]
struct Plugin {
    yt: HashMap<String, Channel>,
    tp: TouchPortalHandle,
    current_channel: Option<String>,
    current_broadcast: Option<String>,
    stream_selection_tx: watch::Sender<StreamSelection>,
    polling_interval_tx: watch::Sender<u64>,
}

impl PluginCallbacks for Plugin {
    #[tracing::instrument(skip(self), ret)]
    async fn on_ytl_add_youtube_channel(
        &mut self,
        _mode: protocol::ActionInteractionMode,
    ) -> eyre::Result<()> {
        let oauth_manager = oauth::OAuthManager::new();

        self.tp
            .notify(
                CreateNotificationCommand::builder()
                    .notification_id("ytl_add_account")
                    .title("Check your browser")
                    .message(
                        "You need to authenticate to YouTube \
                        to add another account.",
                    )
                    .build()
                    .unwrap(),
            )
            .await;

        let new_token = oauth_manager
            .authenticate()
            .await
            .context("authorize additional YouTube account")?;

        let client =
            YouTubeClient::new(TimeBoundAccessToken::new(new_token.clone()), oauth_manager);

        let is_valid = client
            .validate_token()
            .await
            .context("validate new YouTube token")?;

        if !is_valid {
            eyre::bail!("newly authenticated YouTube token failed validation");
        }

        let mut new_channel_count = 0;
        let channels_stream = client.list_my_channels();
        let mut channels_stream = std::pin::pin!(channels_stream);
        while let Some(channel) = channels_stream.next().await {
            let channel = channel.context("fetch channel for new account")?;
            let channel_id = channel.id.clone();
            let channel_name = channel.snippet.title.clone();

            // Overwrite any existing entry for this channel ID with the new token
            self.yt.insert(
                channel_id.clone(),
                Channel {
                    name: channel_name,
                    yt: client.clone(),
                },
            );

            new_channel_count += 1;
        }

        self.tp
            .update_choices_in_ytl_channel(
                self.yt.iter().map(|(id, c)| format!("{} - {id}", c.name)),
            )
            .await;

        // Collect unique tokens by refresh token uniqueness
        let mut seen_refresh_tokens = HashSet::new();
        let mut all_tokens = Vec::new();

        for channel in self.yt.values() {
            let token = channel.yt.token().await;

            if let Some(refresh_token) = token.refresh_token().map(RefreshToken::secret) {
                if seen_refresh_tokens.insert(refresh_token.clone()) {
                    all_tokens.push(token);
                }
            } else {
                // Token without refresh token - always include it
                all_tokens.push(token);
            }
        }

        self.tp
            .set_you_tube_api_access_tokens(
                serde_json::to_string(&all_tokens).expect("OAuth tokens always serialize"),
            )
            .await;

        tracing::info!(
            channel_count = new_channel_count,
            "successfully added new YouTube account"
        );

        Ok(())
    }

    #[tracing::instrument(skip(self), ret)]
    async fn on_ytl_select_stream(
        &mut self,
        _mode: protocol::ActionInteractionMode,
        ytl_channel: ChoicesForYtlChannel,
        ytl_broadcast: ChoicesForYtlBroadcast,
    ) -> eyre::Result<()> {
        // Extract channel ID from the selected channel
        let ChoicesForYtlChannel::Dynamic(channel_selection) = ytl_channel else {
            return Ok(());
        };
        let channel_id = channel_selection
            .rsplit_once(" - ")
            .map(|(_, id)| id)
            .ok_or_else(|| eyre::eyre!("Invalid channel selection format"))?;

        // Extract broadcast ID and live chat ID or handle "latest" selection
        let (broadcast_id, live_chat_id) = match ytl_broadcast {
            ChoicesForYtlBroadcast::Dynamic(broadcast_selection) => {
                if broadcast_selection == "Latest non-completed broadcast" {
                    // Find the latest non-completed broadcast for this channel
                    let channel = self
                        .yt
                        .get(channel_id)
                        .ok_or_else(|| eyre::eyre!("Selected channel not found"))?;

                    let broadcasts = channel.yt.list_my_live_broadcasts();
                    let mut broadcasts = std::pin::pin!(broadcasts);

                    let mut found_broadcast = None;
                    while let Some(broadcast) = broadcasts.next().await {
                        let broadcast =
                            broadcast.context("fetch broadcast for latest selection")?;
                        if broadcast.status.life_cycle_status != BroadcastLifeCycleStatus::Complete
                        {
                            let chat_id = broadcast.snippet.live_chat_id.clone();
                            found_broadcast = Some((broadcast.id, chat_id));
                            break;
                        }
                    }
                    found_broadcast
                        .ok_or_else(|| eyre::eyre!("No non-completed broadcast found"))?
                } else {
                    let id = broadcast_selection
                        .rsplit_once(" - ")
                        .map(|(_, id)| id.to_string())
                        .ok_or_else(|| eyre::eyre!("Invalid broadcast selection format"))?;
                    (id, None) // We don't have the live chat ID for manually selected broadcasts
                }
            }
            _ => return Ok(()),
        };

        // Store the selections
        self.current_channel = Some(channel_id.to_string());
        self.current_broadcast = Some(broadcast_id.clone());

        // Notify background tasks of stream selection change
        let selection = StreamSelection {
            channel_id: Some(channel_id.to_string()),
            broadcast_id: Some(broadcast_id.clone()),
            live_chat_id: live_chat_id.clone(),
        };
        if let Err(e) = self.stream_selection_tx.send(selection) {
            tracing::warn!(
                error = %e,
                "failed to notify background tasks of stream selection change"
            );
        }

        // Update settings for persistence
        self.tp
            .set_selected_channel_id(channel_id.to_string())
            .await;
        self.tp
            .set_selected_broadcast_id(broadcast_id.clone())
            .await;

        // Update states
        if let Some(channel) = self.yt.get(channel_id) {
            self.tp
                .update_ytl_selected_channel_name(&channel.name)
                .await;
        }

        tracing::info!(
            channel = %channel_id,
            broadcast = %broadcast_id,
            "stream selected"
        );

        Ok(())
    }

    #[tracing::instrument(skip(self), ret)]
    async fn on_ytl_start_broadcast(
        &mut self,
        _mode: protocol::ActionInteractionMode,
    ) -> eyre::Result<()> {
        let Some(channel_id) = &self.current_channel else {
            eyre::bail!("No channel selected - use 'Select Stream' action first");
        };
        let Some(broadcast_id) = &self.current_broadcast else {
            eyre::bail!("No broadcast selected - use 'Select Stream' action first");
        };

        let channel = self
            .yt
            .get(channel_id)
            .ok_or_else(|| eyre::eyre!("Selected channel not available"))?;

        tracing::info!(
            channel = %channel_id,
            broadcast = %broadcast_id,
            "starting live broadcast"
        );

        // Transition broadcast from testing -> live
        channel
            .yt
            .transition_live_broadcast(broadcast_id, BroadcastStatus::Live)
            .await
            .context("transition broadcast to live")?;

        tracing::info!("broadcast started successfully");
        Ok(())
    }

    #[tracing::instrument(skip(self), ret)]
    async fn on_ytl_stop_broadcast(
        &mut self,
        _mode: protocol::ActionInteractionMode,
    ) -> eyre::Result<()> {
        let Some(channel_id) = &self.current_channel else {
            eyre::bail!("No channel selected - use 'Select Stream' action first");
        };
        let Some(broadcast_id) = &self.current_broadcast else {
            eyre::bail!("No broadcast selected - use 'Select Stream' action first");
        };

        let channel = self
            .yt
            .get(channel_id)
            .ok_or_else(|| eyre::eyre!("Selected channel not available"))?;

        tracing::info!(
            channel = %channel_id,
            broadcast = %broadcast_id,
            "stopping live broadcast"
        );

        // Transition broadcast from live -> complete
        channel
            .yt
            .transition_live_broadcast(broadcast_id, BroadcastStatus::Complete)
            .await
            .context("transition broadcast to complete")?;

        tracing::info!("broadcast stopped successfully");
        Ok(())
    }

    #[tracing::instrument(skip(self), ret)]
    async fn on_ytl_update_title(
        &mut self,
        _mode: protocol::ActionInteractionMode,
        ytl_new_title: String,
    ) -> eyre::Result<()> {
        let Some(channel_id) = &self.current_channel else {
            eyre::bail!("No channel selected - use 'Select Stream' action first");
        };
        let Some(broadcast_id) = &self.current_broadcast else {
            eyre::bail!("No broadcast selected - use 'Select Stream' action first");
        };

        let channel = self
            .yt
            .get(channel_id)
            .ok_or_else(|| eyre::eyre!("Selected channel not available"))?;

        tracing::info!(
            channel = %channel_id,
            broadcast = %broadcast_id,
            new_title = %ytl_new_title,
            "updating broadcast title"
        );

        // Update broadcast title
        let update_request = LiveBroadcastUpdateRequest {
            id: broadcast_id.clone(),
            snippet: Some(LiveBroadcastUpdateSnippet {
                title: Some(ytl_new_title.clone()),
                description: None,
            }),
        };

        channel
            .yt
            .update_live_broadcast(&update_request)
            .await
            .context("update broadcast title")?;

        // Update the current stream title state
        self.tp
            .update_ytl_current_stream_title(&ytl_new_title)
            .await;

        tracing::info!("broadcast title updated successfully");
        Ok(())
    }

    #[tracing::instrument(skip(self), ret)]
    async fn on_ytl_update_description(
        &mut self,
        _mode: protocol::ActionInteractionMode,
        ytl_new_description: String,
    ) -> eyre::Result<()> {
        let Some(channel_id) = &self.current_channel else {
            eyre::bail!("No channel selected - use 'Select Stream' action first");
        };
        let Some(broadcast_id) = &self.current_broadcast else {
            eyre::bail!("No broadcast selected - use 'Select Stream' action first");
        };

        let channel = self
            .yt
            .get(channel_id)
            .ok_or_else(|| eyre::eyre!("Selected channel not available"))?;

        tracing::info!(
            channel = %channel_id,
            broadcast = %broadcast_id,
            new_description = %ytl_new_description,
            "updating broadcast description"
        );

        // Update broadcast description
        let update_request = LiveBroadcastUpdateRequest {
            id: broadcast_id.clone(),
            snippet: Some(LiveBroadcastUpdateSnippet {
                title: None,
                description: Some(ytl_new_description.clone()),
            }),
        };

        channel
            .yt
            .update_live_broadcast(&update_request)
            .await
            .context("update broadcast description")?;

        tracing::info!("broadcast description updated successfully");
        Ok(())
    }

    // TODO: Add action for updating video thumbnail
    // This would require:
    // - New action: ytl_update_thumbnail with file path parameter
    // - YouTube API: thumbnails.set endpoint
    // - File upload handling for image files
    // - Validation of image format and size requirements
    // See: https://developers.google.com/youtube/v3/docs/thumbnails/set

    // TODO: Add actions for poll creation and management
    // This would require:
    // - New action: ytl_create_poll with title, options, duration parameters
    // - New action: ytl_end_poll with poll ID parameter
    // - YouTube API: Currently no public API for YouTube polls (Community tab polls)
    // - Alternative: Could implement using Community posts API when available
    // - State tracking for active polls using activePollItem pattern
    // - Events for poll start, progress, and completion with local states

    async fn on_select_ytl_channel_in_ytl_select_stream(
        &mut self,
        instance: String,
        selected: ChoicesForYtlChannel,
    ) -> eyre::Result<()> {
        let ChoicesForYtlChannel::Dynamic(selected) = selected else {
            return Ok(());
        };
        let selected = selected
            .rsplit_once(" - ")
            .expect("all options are formatted this way")
            .1;

        let Some(channel) = self.yt.get(selected) else {
            eyre::bail!("user selected unknown channel '{selected}'");
        };

        let broadcasts = channel.yt.list_my_live_broadcasts();

        let mut broadcast_choices = vec!["Latest non-completed broadcast".to_string()];
        let mut stream = std::pin::pin!(broadcasts);
        while let Some(broadcast) = stream.next().await {
            let broadcast = broadcast.context("fetch broadcast")?;
            broadcast_choices.push(format!("{} - {}", broadcast.snippet.title, broadcast.id));
        }

        self.tp
            .update_choices_in_specific_ytl_broadcast(instance, broadcast_choices.into_iter())
            .await;

        Ok(())
    }

    async fn on_select_ytl_broadcast_in_ytl_select_stream(
        &mut self,
        _instance: String,
        _selected: ChoicesForYtlBroadcast,
    ) -> eyre::Result<()> {
        // Nothing special needed here - the actual selection happens in on_ytl_select_stream
        Ok(())
    }
}

// ==============================================================================
// Helper Functions for Background Tasks
// ==============================================================================

/// Poll video statistics and update TouchPortal states
async fn poll_and_update_metrics(
    outgoing: &mut TouchPortalHandle,
    client: &YouTubeClient,
    broadcast_id: &str,
    stream_rx: &watch::Receiver<StreamSelection>,
) {
    match client.get_video_statistics(broadcast_id).await {
        Ok(stats) => {
            // Check if the selected broadcast has changed during the API call
            let current_selection = stream_rx.borrow().clone();
            if current_selection.broadcast_id.as_ref() != Some(&broadcast_id.to_string()) {
                tracing::debug!(
                    polled_broadcast = %broadcast_id,
                    current_broadcast = ?current_selection.broadcast_id,
                    "broadcast changed during metrics poll - discarding results"
                );
                return;
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
            if let Some(live_details) = &stats.live_streaming_details {
                if let Some(concurrent_viewers) = live_details.concurrent_viewers {
                    outgoing
                        .update_ytl_live_viewers_count(&concurrent_viewers.to_string())
                        .await;
                } else {
                    // Not currently live - clear live viewer count
                    outgoing.update_ytl_live_viewers_count("-").await;
                }
            } else {
                // No live streaming details - clear live viewer count
                outgoing.update_ytl_live_viewers_count("-").await;
            }

            tracing::debug!(
                broadcast_id = %broadcast_id,
                views = ?stats.statistics.view_count,
                likes = ?stats.statistics.like_count,
                live_viewers = ?stats.live_streaming_details.as_ref().and_then(|d| d.concurrent_viewers),
                "updated metrics"
            );
        }
        Err(e) => {
            tracing::warn!(
                broadcast_id = %broadcast_id,
                error = %e,
                "failed to poll video statistics"
            );
        }
    }
}

// TODO: Add stream health monitoring metrics
// This would require:
// - New states: ytl_stream_health, ytl_stream_resolution, ytl_stream_framerate, ytl_stream_bitrate
// - YouTube API: liveStreams.list endpoint with status and contentDetails parts
// - Polling integration: Add health metrics to poll_and_update_metrics function
// - Error detection: Monitor for stream issues, quality drops, connection problems
// - Health status enum: "healthy", "warning", "error", "offline"
// - Integration with existing metrics polling loop
// See: https://developers.google.com/youtube/v3/live/docs/liveStreams/list

/// Process a chat message and trigger appropriate TouchPortal events
async fn process_chat_message(outgoing: &mut TouchPortalHandle, message: LiveChatMessage) {
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

// TODO: Add functionality for sending chat messages (two-way chat interaction)
// This would require:
// - New action: ytl_send_chat_message with message parameter
// - YouTube API: liveChatMessages.insert endpoint
// - Authentication scope: https://www.googleapis.com/auth/youtube.force-ssl
// - Validation of message content and rate limiting
// - Error handling for chat restrictions (slow mode, subscriber-only, etc.)
// - Integration with current live chat ID from selected broadcast
// See: https://developers.google.com/youtube/v3/live/docs/liveChatMessages/insert

/// Restart chat stream when stream selection changes - optimized version
async fn restart_chat_stream_optimized(
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
async fn get_live_chat_id_fallback(
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

impl Plugin {
    async fn new(
        settings: PluginSettings,
        mut outgoing: TouchPortalHandle,
        info: InfoMessage,
    ) -> eyre::Result<Self> {
        tracing::info!(version = info.tp_version_string, "paired with TouchPortal");
        tracing::debug!(settings = ?settings, "got settings");

        // Use shared token setup logic with notification callback
        let notify_callback = async |id: &str, title: &str, message: &str| {
            let _ = outgoing
                .notify(
                    CreateNotificationCommand::builder()
                        .notification_id(id)
                        .title(title)
                        .message(message)
                        .build()
                        .unwrap(),
                )
                .await;
        };

        let (client_by_channel, refreshed_tokens) =
            setup_youtube_clients(&settings.you_tube_api_access_tokens, notify_callback).await?;

        // Update stored tokens with the refreshed ones
        outgoing
            .set_you_tube_api_access_tokens(
                serde_json::to_string(&refreshed_tokens).expect("OAuth tokens always serialize"),
            )
            .await;

        // ==============================================================================
        // TouchPortal UI Initialization
        // ==============================================================================
        // Now that we know what channels are available, update the TouchPortal UI
        // with channel choices that users can select from in their actions.
        outgoing
            .update_choices_in_ytl_channel(
                client_by_channel
                    .iter()
                    .map(|(id, c)| format!("{} - {id}", c.name)),
            )
            .await;

        // Restore previous selections from settings
        let current_channel = if settings.selected_channel_id.is_empty() {
            None
        } else {
            Some(settings.selected_channel_id.clone())
        };

        let current_broadcast = if settings.selected_broadcast_id.is_empty() {
            None
        } else {
            Some(settings.selected_broadcast_id.clone())
        };

        // Update channel name state if we have a current channel
        if let Some(channel_id) = &current_channel {
            if let Some(channel) = client_by_channel.get(channel_id) {
                outgoing
                    .update_ytl_selected_channel_name(&channel.name)
                    .await;
            }
        }

        // ==============================================================================
        // Background Task Coordination
        // ==============================================================================
        // Create tokio::watch channels for coordinating between action handlers and background tasks
        let initial_stream = StreamSelection {
            channel_id: current_channel.clone(),
            broadcast_id: current_broadcast.clone(),
            live_chat_id: None,
        };
        let (stream_selection_tx, stream_selection_rx) = watch::channel(initial_stream);

        let initial_interval = settings.polling_interval_seconds.max(30.0) as u64;
        let (polling_interval_tx, polling_interval_rx) = watch::channel(initial_interval);

        // ==============================================================================
        // Background Metrics Polling Task
        // ==============================================================================
        // Spawn a dedicated task for metrics polling that won't block chat processing
        let mut metrics_outgoing = outgoing.clone();
        let metrics_channels = client_by_channel.clone();
        let metrics_stream_rx = stream_selection_rx.clone();
        let polling_interval = settings.polling_interval_seconds.max(30.0) as u64;

        tokio::spawn(async move {
            let mut current_interval = polling_interval;
            let mut interval = tokio::time::interval(Duration::from_secs(current_interval));
            let mut polling_interval_rx = polling_interval_rx;

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        // Time to poll metrics
                    }
                    Ok(()) = polling_interval_rx.changed() => {
                        let new_interval = *polling_interval_rx.borrow();
                        if new_interval != current_interval {
                            tracing::debug!(
                                old_interval = current_interval,
                                new_interval = new_interval,
                                "updating polling interval"
                            );
                            current_interval = new_interval;
                            interval = tokio::time::interval(Duration::from_secs(current_interval));
                            continue; // Skip this iteration to reset timing
                        }
                    }
                }

                // Get current stream selection (non-blocking)
                let selection = metrics_stream_rx.borrow().clone();

                if let (Some(channel_id), Some(broadcast_id)) =
                    (&selection.channel_id, &selection.broadcast_id)
                {
                    if let Some(channel) = metrics_channels.get(channel_id) {
                        tracing::debug!(
                            channel = %channel_id,
                            broadcast = %broadcast_id,
                            "polling metrics"
                        );

                        // Poll metrics without blocking chat processing
                        poll_and_update_metrics(
                            &mut metrics_outgoing,
                            &channel.yt,
                            broadcast_id,
                            &metrics_stream_rx,
                        )
                        .await;
                    }
                } else {
                    // No stream selected - clear metrics states
                    metrics_outgoing.update_ytl_views_count("-").await;
                    metrics_outgoing.update_ytl_likes_count("-").await;
                    metrics_outgoing.update_ytl_dislikes_count("-").await;
                    metrics_outgoing.update_ytl_live_viewers_count("-").await;
                }
            }
        });

        // ==============================================================================
        // Background Chat Monitoring Task
        // ==============================================================================
        // Spawn a dedicated task for chat monitoring that's always responsive
        let mut chat_outgoing = outgoing.clone();
        let chat_channels = client_by_channel.clone();
        let mut chat_stream_rx = stream_selection_rx.clone();

        tokio::spawn(async move {
            let mut chat_stream: Option<Pin<Box<LiveChatStream>>> = None;
            let mut current_broadcast: Option<String> = None;

            // Initialize chat stream if we have a current broadcast
            let selection = chat_stream_rx.borrow().clone();
            if selection.broadcast_id != current_broadcast {
                restart_chat_stream_optimized(
                    &mut chat_stream,
                    &chat_channels,
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
                            process_chat_message(&mut chat_outgoing, msg).await;
                        }
                    }

                    // React immediately to stream selection changes
                    Ok(()) = chat_stream_rx.changed() => {
                        let selection = chat_stream_rx.borrow().clone();

                        if selection.broadcast_id != current_broadcast {
                            tracing::debug!(
                                old_broadcast = ?current_broadcast,
                                new_broadcast = ?selection.broadcast_id,
                                "stream selection changed - updating chat stream"
                            );

                            restart_chat_stream_optimized(
                                &mut chat_stream,
                                &chat_channels,
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

        // TODO: Add background task for poll result tracking using activePollItem
        // This would require:
        // - New background task: Poll monitoring and result updates
        // - Dynamic state creation: Use activePollItem to create poll result states
        // - YouTube API integration: Poll creation/monitoring (when API becomes available)
        // - Real-time updates: Track vote counts and poll progress
        // - State cleanup: Remove poll states when polls end
        // - Integration pattern: Similar to existing chat/metrics background tasks
        // - Event triggering: Poll start, progress, and completion events
        // See TouchPortal SDK documentation for activePollItem usage patterns

        Ok(Self {
            yt: client_by_channel,
            tp: outgoing,
            current_channel,
            current_broadcast,
            stream_selection_tx,
            polling_interval_tx,
        })
    }
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::TRACE.into())
                .from_env_lossy(),
        )
        .without_time() // done by TouchPortal's logs
        .with_ansi(false) // not supported by TouchPortal's log output
        .init();

    Plugin::run_dynamic("127.0.0.1:12136").await?;

    Ok(())
}
