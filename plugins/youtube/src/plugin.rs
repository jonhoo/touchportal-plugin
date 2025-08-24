use crate::youtube_api::broadcasts::{
    BroadcastStatus, LiveBroadcastUpdateRequest, LiveBroadcastUpdateSnippet,
};
use eyre::Context;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, watch};
use tokio_stream::StreamExt;
use touchportal_sdk::protocol::{CreateNotificationCommand, InfoMessage};

use crate::actions::{oauth, stream_selection};
use crate::activity::AdaptivePollingState;
use crate::background::metrics::StreamSelection;
use crate::background::{chat, latest_monitor, metrics};
use crate::{Channel, notifications, setup_youtube_clients};

// You can look at the generated code for a plugin using this command:
//
// ```bash
// cat "$(dirname "$(cargo check --message-format=json | jq -r 'select(.reason == "build-script-executed") | select(.package_id | contains("#touchportal-")).out_dir')")/out/entry.rs"
// ```
include!(concat!(env!("OUT_DIR"), "/entry.rs"));

#[derive(Debug)]
pub struct Plugin {
    yt: Arc<Mutex<HashMap<String, Channel>>>,
    tp: TouchPortalHandle,
    stream_selection_tx: watch::Sender<StreamSelection>,
    polling_interval_tx: watch::Sender<u64>,
    // Track OAuth credentials to detect changes that invalidate tokens
    current_custom_client_id: Option<String>,
    current_custom_client_secret: Option<String>,
    // Background task handles for cancellation and restart
    metrics_task_handle: Option<tokio::task::JoinHandle<()>>,
    chat_task_handle: Option<tokio::task::JoinHandle<()>>,
    latest_monitor_task_handle: Option<tokio::task::JoinHandle<()>>,
    // Stored parameters for background task restart
    adaptive_state: Arc<Mutex<AdaptivePollingState>>,
    stream_selection_rx: watch::Receiver<StreamSelection>,
    polling_interval_rx: watch::Receiver<u64>,
    // Runtime log level adjustment handle
    pub log_level_reload_handle: tracing_subscriber::reload::Handle<
        tracing_subscriber::EnvFilter,
        tracing_subscriber::Registry,
    >,
    // Shared HTTP client for all YouTube API operations
    http_client: reqwest::Client,
}

impl PluginCallbacks for Plugin {
    #[tracing::instrument(skip(self), ret)]
    async fn on_settings_changed(
        &mut self,
        PluginSettings {
            smart_polling_adjustment,
            base_polling_interval_seconds,
            custom_o_auth_client_id,
            custom_o_auth_client_secret,
            logging_verbosity,
            // Read-only settings updated by the plugin - ignore these changes
            you_tube_api_access_tokens: _,
            selected_channel_id: _,
            selected_broadcast_id: _,
        }: PluginSettings,
    ) -> eyre::Result<()> {
        self.handle_oauth_credential_change(&custom_o_auth_client_id, &custom_o_auth_client_secret)
            .await?;

        // Apply logging verbosity setting
        self.update_logging_level(logging_verbosity)?;

        let new_polling_interval =
            base_polling_interval_seconds.max(crate::MIN_POLLING_INTERVAL_SECONDS as f64) as u64;
        if let Err(e) = self.polling_interval_tx.send(new_polling_interval) {
            tracing::warn!(
                error = %e,
                new_interval = new_polling_interval,
                "failed to notify background tasks of polling interval change"
            );
        }

        tracing::debug!(
            polling_interval = new_polling_interval,
            smart_polling = smart_polling_adjustment,
            "processed user-modifiable settings changes"
        );

        Ok(())
    }

    #[tracing::instrument(skip(self), ret)]
    async fn on_ytl_add_youtube_channel(
        &mut self,
        _mode: protocol::ActionInteractionMode,
    ) -> eyre::Result<()> {
        oauth::handle_add_youtube_channel(
            &mut self.tp,
            &self.yt,
            self.current_custom_client_id.clone(),
            self.current_custom_client_secret.clone(),
            self.http_client.clone(),
        )
        .await
    }

    #[tracing::instrument(skip(self), ret)]
    async fn on_ytl_select_stream(
        &mut self,
        _mode: protocol::ActionInteractionMode,
        ytl_channel: ChoicesForYtlChannel,
        ytl_broadcast: ChoicesForYtlBroadcast,
    ) -> eyre::Result<()> {
        // Extract selections from the UI choices
        let ChoicesForYtlChannel::Dynamic(channel_selection) = ytl_channel else {
            // User selected "No channels available" - they need to add a YouTube account
            notifications::need_to_add_youtube_account(&mut self.tp).await?;
            return Ok(());
        };

        let chosen = match ytl_broadcast {
            ChoicesForYtlBroadcast::LatestNonCompletedBroadcast => {
                stream_selection::handle_select_stream(
                    &mut self.tp,
                    &self.yt,
                    channel_selection,
                    stream_selection::BroadcastSelection::Latest,
                )
                .await?
            }
            ChoicesForYtlBroadcast::Dynamic(broadcast_selection) => {
                // Manually selected specific broadcast
                stream_selection::handle_select_stream(
                    &mut self.tp,
                    &self.yt,
                    channel_selection,
                    stream_selection::BroadcastSelection::Specific(broadcast_selection),
                )
                .await?
            }
            ChoicesForYtlBroadcast::SelectChannelFirst => {
                // User selected "Select channel first" - they haven't selected a channel yet
                notifications::need_to_select_channel_first(&mut self.tp).await?;
                None
            }
        };

        let Some((channel_id, broadcast_id, selection)) = chosen else {
            // The handle functions above have sent some notifications instead.
            return Ok(());
        };

        tracing::info!(
            channel = %channel_id,
            broadcast = ?broadcast_id,
            "broadcast selection updated"
        );

        // Notify background tasks of stream selection change
        if let Err(e) = self.stream_selection_tx.send(selection) {
            tracing::warn!(
                error = %e,
                "failed to notify background tasks of stream selection change"
            );
        }

        Ok(())
    }

    #[tracing::instrument(skip(self), ret)]
    async fn on_ytl_start_broadcast(
        &mut self,
        _mode: protocol::ActionInteractionMode,
    ) -> eyre::Result<()> {
        let Some(channel_id) = self.current_channel() else {
            notifications::no_channel_selected(&mut self.tp).await?;
            return Ok(());
        };
        let Some(broadcast_id) = self.current_broadcast_id() else {
            notifications::no_broadcast_selected(&mut self.tp).await?;
            return Ok(());
        };

        let channel = {
            let yt_guard = self.yt.lock().await;
            yt_guard.get(&channel_id).cloned()
        };
        let Some(channel) = channel else {
            notifications::channel_not_available(&mut self.tp).await?;
            return Ok(());
        };

        tracing::info!(
            channel = %channel_id,
            broadcast = %broadcast_id,
            "starting live broadcast"
        );

        // Transition broadcast from testing -> live
        match channel
            .yt
            .transition_live_broadcast(&broadcast_id, BroadcastStatus::Live)
            .await
        {
            Ok(_) => {
                self.tp
                    .notify(
                        CreateNotificationCommand::builder()
                            .notification_id("ytl_broadcast_started")
                            .title("Broadcast started")
                            .message("Your live broadcast has been started successfully!")
                            .build()
                            .unwrap(),
                    )
                    .await;
                tracing::info!("broadcast started successfully");
                Ok(())
            }
            Err(e) => {
                self.tp
                    .notify(
                        CreateNotificationCommand::builder()
                            .notification_id("ytl_broadcast_start_failed")
                            .title("Failed to start broadcast")
                            .message(format!("Could not start broadcast: {}", e))
                            .build()
                            .unwrap(),
                    )
                    .await;
                Err(e).context("transition broadcast to live")
            }
        }
    }

    #[tracing::instrument(skip(self), ret)]
    async fn on_ytl_stop_broadcast(
        &mut self,
        _mode: protocol::ActionInteractionMode,
    ) -> eyre::Result<()> {
        let Some(channel_id) = self.current_channel() else {
            notifications::no_channel_selected(&mut self.tp).await?;
            return Ok(());
        };
        let Some(broadcast_id) = self.current_broadcast_id() else {
            notifications::no_broadcast_selected(&mut self.tp).await?;
            return Ok(());
        };

        let channel = {
            let yt_guard = self.yt.lock().await;
            yt_guard.get(&channel_id).cloned()
        };
        let Some(channel) = channel else {
            notifications::channel_not_available(&mut self.tp).await?;
            return Ok(());
        };

        tracing::info!(
            channel = %channel_id,
            broadcast = %broadcast_id,
            "stopping live broadcast"
        );

        // Transition broadcast from live -> complete
        channel
            .yt
            .transition_live_broadcast(&broadcast_id, BroadcastStatus::Complete)
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
        // Input validation
        if ytl_new_title.trim().is_empty() {
            self.tp
                .notify(
                    CreateNotificationCommand::builder()
                        .notification_id("ytl_empty_title")
                        .title("Empty title")
                        .message("Please provide a title for your stream.")
                        .build()
                        .unwrap(),
                )
                .await;
            return Ok(());
        }

        if ytl_new_title.len() > 100 {
            self.tp
                .notify(
                    CreateNotificationCommand::builder()
                        .notification_id("ytl_title_too_long")
                        .title("Title too long")
                        .message("Stream title must be 100 characters or less.")
                        .build()
                        .unwrap(),
                )
                .await;
            return Ok(());
        }
        let Some(channel_id) = self.current_channel() else {
            notifications::no_channel_selected(&mut self.tp).await?;
            return Ok(());
        };
        let Some(broadcast_id) = self.current_broadcast_id() else {
            notifications::no_broadcast_selected(&mut self.tp).await?;
            return Ok(());
        };

        let channel = {
            let yt_guard = self.yt.lock().await;
            yt_guard.get(&channel_id).cloned()
        };
        let Some(channel) = channel else {
            notifications::channel_not_available(&mut self.tp).await?;
            return Ok(());
        };

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
        let Some(channel_id) = self.current_channel() else {
            notifications::no_channel_selected(&mut self.tp).await?;
            return Ok(());
        };
        let Some(broadcast_id) = self.current_broadcast_id() else {
            notifications::no_broadcast_selected(&mut self.tp).await?;
            return Ok(());
        };

        let channel = {
            let yt_guard = self.yt.lock().await;
            yt_guard.get(&channel_id).cloned()
        };
        let Some(channel) = channel else {
            notifications::channel_not_available(&mut self.tp).await?;
            return Ok(());
        };

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

    // TODO(jon): CRITICAL - Add chat moderation actions (verified available, essential for production use)
    // Live streamers need these moderation tools. All verified working in YouTube Data API v3:
    //
    // HIGH PRIORITY MODERATION ACTIONS:
    // - ytl_send_chat_message: Use liveChatMessages.insert endpoint
    //   * POST /youtube/v3/liveChat/messages with snippet.textMessageDetails.messageText
    //   * Cost: 50 quota units per message (expensive - warn users about quota usage)
    //   * Parameters: message text (validate length, filter inappropriate content)
    //   * Requires current liveChatId from selected broadcast
    //
    // - ytl_ban_user: Use liveChatBans.insert endpoint
    //   * POST /youtube/v3/liveChat/bans with snippet.bannedUserDetails.channelId
    //   * Cost: 50 quota units per ban
    //   * Parameters: channel ID to ban, ban type ("permanent" or "temporary")
    //   * For temporary bans: include banDurationSeconds (10 seconds to 24 hours max)
    //
    // - ytl_delete_chat_message: Use liveChatMessages.delete endpoint
    //   * DELETE /youtube/v3/liveChat/messages/{messageId}
    //   * Requires message ID from chat events (add messageId to chat event local states)
    //   * Cost: moderate quota usage
    //
    // IMPLEMENTATION REQUIREMENTS:
    // - Add messageId field to chat message event local states for deletion support
    // - Add user channelId to chat events for easy banning from chat messages
    // - Validate ban durations (10s minimum, 24h maximum for temporary bans)
    // - Handle quota exhaustion gracefully with user-friendly error messages
    // - Consider rate limiting for chat message sending to prevent spam

    // TODO(jon): Add thumbnail management (verified available, moderate priority)
    // - ytl_update_thumbnail: Use thumbnails.set endpoint (POST /youtube/v3/thumbnails/set)
    //   * Supports JPEG, PNG, GIF, BMP formats, max 2MB file size
    //   * YouTube auto-resizes while maintaining aspect ratio
    //   * Parameters: file path to image, uses current broadcast ID as video ID
    //   * Implementation: multipart/form-data upload with proper error handling

    // TODO: Add channel information retrieval actions
    // - New actions to get channel data and store in TouchPortal's value system
    // - Example: "Get Channel Profile Image" action that stores image URL in value slot
    // - Example: "Get Channel Stats" action for subscriber count, view count, etc.
    // - This allows TouchPortal users to display channel info in their layouts
    // - Similar to how some other streaming plugins provide user info actions
    // - Would use YouTube Data API channels.list endpoint

    // TODO: Add stream highlight markers
    // - Create action to mark important moments during live streams
    // - Unlike Twitch stream markers, YouTube doesn't have direct equivalent
    // - Possible implementations: add timestamped entries to video description
    // - Or: create Community Tab posts with stream timestamps
    // - Or: use video comments with specific formatting for later retrieval
    // - Would help streamers mark highlights for later editing/clipping

    // TODO(jon): Add live chat poll functionality (VERIFIED AVAILABLE via liveChatMessages API)
    // YouTube supports polls through the Live Chat API. Research confirmed this works:
    //
    // POLL CREATION ACTION:
    // - ytl_create_poll: Use liveChatMessages.insert with type="pollEvent"
    //   * Endpoint: POST /youtube/v3/liveChat/messages
    //   * Parameters: question text, 2-4 poll options (minimum 2, maximum 4)
    //   * JSON structure: snippet.pollDetails.metadata with questionText and options array
    //   * Critical limitation: Only ONE active poll at a time per channel
    //   * Error handling: Creating second poll while one active returns "preconditionCheckFailed"
    //
    // POLL RESULTS TRACKING (we have channel owner permissions):
    // - Parse tally field from pollDetails.options in chat message responses
    // - Add dynamic states using activePollItem pattern for vote counts
    // - Monitor chat message stream for pollDetails to get real-time vote updates
    //
    // POLL MANAGEMENT STATES:
    // - ytl_active_poll_question: Current poll question text (empty if no active poll)
    // - ytl_active_poll_status: "active", "closed", or "-" for no poll
    // - Dynamic poll option states: ytl_poll_option_N_text and ytl_poll_option_N_votes
    //
    // IMPLEMENTATION APPROACH:
    // - Use activePollItem pattern for creating/destroying poll result states dynamically
    // - Clear all poll states when stream selection changes
    // - Handle the "one poll at a time" limitation in UI validation
    // - Poll voting happens through YouTube chat interface (users click to vote)
    // - No direct poll closing API - polls close via YouTube UI or automatically

    async fn on_select_ytl_channel_in_ytl_select_stream(
        &mut self,
        instance: String,
        selected: ChoicesForYtlChannel,
    ) -> eyre::Result<()> {
        let ChoicesForYtlChannel::Dynamic(selected) = selected else {
            // User selected "No channels available" in dropdown - they need to add a YouTube account
            notifications::need_to_add_youtube_account(&mut self.tp).await?;
            return Ok(());
        };
        let selected = selected
            .rsplit_once(" - ")
            .expect("all options are formatted this way")
            .1;

        let channel = {
            let yt_guard = self.yt.lock().await;
            yt_guard.get(selected).cloned()
        };
        let Some(channel) = channel else {
            notifications::channel_not_available(&mut self.tp).await?;
            return Ok(());
        };

        let broadcasts = channel.yt.list_my_live_broadcasts();

        let mut broadcast_choices =
            vec![ChoicesForYtlBroadcast::LatestNonCompletedBroadcast.to_string()];
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
        selected: ChoicesForYtlBroadcast,
    ) -> eyre::Result<()> {
        // Check if user selected "Select channel first" option
        if matches!(selected, ChoicesForYtlBroadcast::SelectChannelFirst) {
            notifications::need_to_select_channel_first(&mut self.tp).await?;
            return Ok(());
        }

        // For other selections, remind user to save their selection
        notifications::remind_to_save_selection(&mut self.tp).await?;
        Ok(())
    }
}

impl Plugin {
    pub async fn new(
        settings: PluginSettings,
        mut outgoing: TouchPortalHandle,
        info: InfoMessage,
        log_level_reload_handle: tracing_subscriber::reload::Handle<
            tracing_subscriber::EnvFilter,
            tracing_subscriber::Registry,
        >,
    ) -> eyre::Result<Self> {
        tracing::info!(version = info.tp_version_string, "paired with TouchPortal");
        tracing::debug!(settings = ?settings, "got settings");

        // ==============================================================================
        // Custom OAuth Credential Handling
        // ==============================================================================
        // Extract custom OAuth credentials, treating empty strings as None
        let custom_client_id = if settings.custom_o_auth_client_id.trim().is_empty() {
            None
        } else {
            Some(settings.custom_o_auth_client_id.clone())
        };
        let custom_client_secret = if settings.custom_o_auth_client_secret.trim().is_empty() {
            None
        } else {
            Some(settings.custom_o_auth_client_secret.clone())
        };

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

        // Create shared HTTP client for all YouTube API operations
        let shared_http_client = reqwest::Client::new();

        let (client_by_channel, refreshed_tokens) = setup_youtube_clients(
            &settings.you_tube_api_access_tokens,
            custom_client_id.clone(),
            custom_client_secret.clone(),
            notify_callback,
            shared_http_client.clone(),
        )
        .await?;

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

        let current_broadcast =
            stream_selection::BroadcastSelection::from_saved_id(&settings.selected_broadcast_id);

        // Update channel name state and initialize broadcast choices if we have a current channel
        if let Some(channel_id) = &current_channel
            && let Some(channel) = client_by_channel.get(channel_id)
        {
            outgoing
                .update_ytl_selected_channel_name(format!("{} - {}", channel.name, channel_id))
                .await;

            // Fetch broadcasts for the current channel and update the choices
            let broadcasts = channel.yt.list_my_live_broadcasts();
            let mut broadcast_choices =
                vec![ChoicesForYtlBroadcast::LatestNonCompletedBroadcast.to_string()];
            let mut stream = std::pin::pin!(broadcasts);
            while let Some(broadcast) = stream.next().await {
                match broadcast {
                    Ok(broadcast) => {
                        broadcast_choices
                            .push(format!("{} - {}", broadcast.snippet.title, broadcast.id));
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            channel = %channel_id,
                            "failed to fetch broadcast during initialization"
                        );
                        break;
                    }
                }
            }

            outgoing
                .update_choices_in_ytl_broadcast(broadcast_choices.into_iter())
                .await;

            tracing::debug!(
                channel = %channel_id,
                "initialized broadcast choices on startup"
            );
        }

        // ==============================================================================
        // Background Task Coordination
        // ==============================================================================

        // Initialize adaptive polling state
        let adaptive_enabled = settings.smart_polling_adjustment;
        let base_interval = settings
            .base_polling_interval_seconds
            .max(crate::MIN_POLLING_INTERVAL_SECONDS as f64) as u64;
        let adaptive_state = Arc::new(Mutex::new(AdaptivePollingState::new(
            base_interval,
            adaptive_enabled,
        )));

        // Create shared channel state for background tasks
        let shared_channels = Arc::new(Mutex::new(client_by_channel));
        let (polling_interval_tx, polling_interval_rx) = watch::channel(base_interval);

        // Create tokio::watch channels for coordinating between action handlers and background tasks
        let initial_stream = match (&current_channel, &current_broadcast) {
            (Some(channel_id), Some(stream_selection::BroadcastSelection::Latest)) => {
                // Special case: "latest" means WaitForActiveBroadcast mode
                StreamSelection::WaitForActiveBroadcast {
                    channel_id: channel_id.clone(),
                }
            }
            (
                Some(channel_id),
                Some(stream_selection::BroadcastSelection::Specific(broadcast_id)),
            ) => {
                // We have both channel and broadcast - get the live_chat_id
                let yt_guard = shared_channels.lock().await;
                let channel = yt_guard.get(channel_id);
                match channel {
                    Some(channel) => {
                        match channel.yt.get_video_metadata(broadcast_id).await {
                            Ok(video) => {
                                match video
                                    .live_streaming_details
                                    .and_then(|details| details.active_live_chat_id)
                                {
                                    Some(live_chat_id) => StreamSelection::ChannelAndBroadcast {
                                        channel_id: channel_id.clone(),
                                        broadcast_id: broadcast_id.clone(),
                                        live_chat_id,
                                        return_to_latest_on_completion: false,
                                    },
                                    None => {
                                        tracing::warn!(
                                            channel = %channel_id,
                                            broadcast = %broadcast_id,
                                            "at startup, selected broadcast has no active live chat; it may have ended"
                                        );
                                        // Clear the broadcast setting since it's not usable for chat
                                        outgoing.set_selected_broadcast_id(String::new()).await;
                                        StreamSelection::ChannelOnly {
                                            channel_id: channel_id.clone(),
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    channel = %channel_id,
                                    broadcast = %broadcast_id,
                                    error = %e,
                                    "failed to get video metadata for selected broadcast at startup; it may have been deleted"
                                );
                                // Clear the broadcast setting since it's not accessible
                                outgoing.set_selected_broadcast_id(String::new()).await;
                                StreamSelection::ChannelOnly {
                                    channel_id: channel_id.clone(),
                                }
                            }
                        }
                    }
                    None => {
                        tracing::warn!(
                            channel = %channel_id,
                            "selected channel not found at startup"
                        );
                        StreamSelection::None
                    }
                }
            }
            (Some(channel_id), None) => StreamSelection::ChannelOnly {
                channel_id: channel_id.clone(),
            },
            _ => StreamSelection::None,
        };
        let (stream_selection_tx, stream_selection_rx) = watch::channel(initial_stream);

        // ==============================================================================
        // Background Metrics Polling Task
        // ==============================================================================
        // Spawn the metrics polling task
        let metrics_task_handle = metrics::spawn_metrics_task(
            outgoing.clone(),
            Arc::clone(&shared_channels),
            stream_selection_rx.clone(),
            stream_selection_tx.clone(),
            Arc::clone(&adaptive_state),
            base_interval,
            polling_interval_rx.clone(),
        )
        .await;

        // ==============================================================================
        // Background Chat Monitoring Task
        // ==============================================================================
        // Spawn the chat monitoring task
        let chat_task_handle = chat::spawn_chat_task(
            outgoing.clone(),
            Arc::clone(&shared_channels),
            stream_selection_rx.clone(),
            Arc::clone(&adaptive_state),
        )
        .await;

        // ==============================================================================
        // Background Latest Broadcast Monitoring Task
        // ==============================================================================
        // Spawn the latest broadcast monitoring task
        let latest_monitor_task_handle = latest_monitor::spawn_latest_monitor_task(
            Arc::clone(&shared_channels),
            stream_selection_rx.clone(),
            stream_selection_tx.clone(),
        )
        .await;

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

        let mut plugin = Self {
            yt: shared_channels,
            tp: outgoing,
            stream_selection_tx,
            polling_interval_tx,
            current_custom_client_id: custom_client_id,
            current_custom_client_secret: custom_client_secret,
            metrics_task_handle: Some(metrics_task_handle),
            chat_task_handle: Some(chat_task_handle),
            latest_monitor_task_handle: Some(latest_monitor_task_handle),
            adaptive_state,
            stream_selection_rx,
            polling_interval_rx,
            log_level_reload_handle,
            http_client: shared_http_client,
        };

        // Apply the initial logging level from plugin settings
        if let Err(e) = plugin.update_logging_level(settings.logging_verbosity) {
            tracing::warn!(error = %e, "failed to apply initial logging level");
        }

        Ok(plugin)
    }

    /// Get the current broadcast ID from the stream selection watch receiver.
    ///
    /// This method synchronously reads the current stream selection and extracts
    /// the broadcast ID if one is selected. Returns None if no broadcast is
    /// selected or we're in WaitForActiveBroadcast mode.
    fn current_broadcast_id(&self) -> Option<String> {
        match &*self.stream_selection_rx.borrow() {
            StreamSelection::ChannelAndBroadcast { broadcast_id, .. } => Some(broadcast_id.clone()),
            _ => None,
        }
    }

    /// Get the current channel ID from the stream selection watch receiver.
    ///
    /// This method synchronously reads the current stream selection and extracts
    /// the channel ID if one is selected. Returns None if no channel is selected.
    fn current_channel(&self) -> Option<String> {
        match &*self.stream_selection_rx.borrow() {
            StreamSelection::ChannelOnly { channel_id } => Some(channel_id.clone()),
            StreamSelection::ChannelAndBroadcast { channel_id, .. } => Some(channel_id.clone()),
            StreamSelection::WaitForActiveBroadcast { channel_id } => Some(channel_id.clone()),
            StreamSelection::None => None,
        }
    }

    /// Update the logging level based on user settings
    fn update_logging_level(&mut self, level: LoggingVerbositySettingOptions) -> eyre::Result<()> {
        use tracing_subscriber::filter::LevelFilter;

        let level_filter = match level {
            LoggingVerbositySettingOptions::Info => LevelFilter::INFO,
            LoggingVerbositySettingOptions::Debug => LevelFilter::DEBUG,
            LoggingVerbositySettingOptions::Trace => LevelFilter::TRACE,
        };

        self.log_level_reload_handle
            .modify(|filter| {
                *filter = tracing_subscriber::EnvFilter::builder()
                    .with_default_directive(level_filter.into())
                    .from_env_lossy()
            })
            .context("update logging level")?;
        tracing::info!(level = %level, "logging level updated");

        Ok(())
    }

    /// Handle OAuth credential changes when TouchPortal settings are updated.
    ///
    /// This method will be called by the settings change callback to properly handle
    /// changes to custom OAuth client ID and secret settings.
    ///
    /// When OAuth credentials change:
    /// 1. All existing access/refresh tokens become invalid immediately  
    /// 2. All YouTube clients must be recreated with new OAuth manager
    /// 3. Users must re-authenticate all accounts
    /// 4. Plugin state must be reset (channels, current selections, etc.)
    ///
    /// # Arguments
    /// * `new_custom_client_id` - New custom OAuth client ID (empty string if not set)
    /// * `new_custom_client_secret` - New custom OAuth client secret (empty string if not set)
    async fn handle_oauth_credential_change(
        &mut self,
        new_custom_client_id: &str,
        new_custom_client_secret: &str,
    ) -> eyre::Result<()> {
        // Extract new custom OAuth credentials
        let new_custom_client_id = if new_custom_client_id.trim().is_empty() {
            None
        } else {
            Some(new_custom_client_id.to_string())
        };
        let new_custom_client_secret = if new_custom_client_secret.trim().is_empty() {
            None
        } else {
            Some(new_custom_client_secret.to_string())
        };

        // Early return if OAuth credentials haven't changed
        if new_custom_client_id == self.current_custom_client_id
            && new_custom_client_secret == self.current_custom_client_secret
        {
            return Ok(());
        }

        // OAuth credentials have changed - handle the implications
        tracing::warn!(
            old_client_id = ?self.current_custom_client_id,
            new_client_id = ?new_custom_client_id,
            "OAuth credentials changed - invalidating all tokens and requiring re-authentication"
        );

        // Clear all stored access tokens (they're now invalid)
        self.tp
            .set_you_tube_api_access_tokens(String::from("[]"))
            .await;

        // Update stored OAuth credentials for future comparisons
        self.current_custom_client_id = new_custom_client_id.clone();
        self.current_custom_client_secret = new_custom_client_secret.clone();

        // Cancel existing background tasks since they have invalid YouTube clients
        if let Some(handle) = self.metrics_task_handle.take() {
            handle.abort();
            let _ = handle.await; // Wait for clean shutdown
            tracing::debug!("canceled metrics background task");
        }
        if let Some(handle) = self.chat_task_handle.take() {
            handle.abort();
            let _ = handle.await; // Wait for clean shutdown
            tracing::debug!("canceled chat background task");
        }
        if let Some(handle) = self.latest_monitor_task_handle.take() {
            handle.abort();
            let _ = handle.await; // Wait for clean shutdown
            tracing::debug!("canceled latest monitor background task");
        }

        // Clear all YouTube clients and channels
        {
            let mut yt_guard = self.yt.lock().await;
            yt_guard.clear();
        }

        // Preserve stream selection across OAuth credential changes
        // (i.e., don't update stream_selection_tx).
        //
        // The user's stream selection remains conceptually valid - only the authentication tokens
        // have changed. Background tasks are restarted with an empty channel map and will remain
        // idle until re-authentication, at which point the preserved selection can be used again
        // without requiring user re-selection.

        // Restart background tasks with empty channel map (they will be idle until re-auth)
        let empty_channels = Arc::new(Mutex::new(HashMap::new()));
        let base_interval = crate::MIN_POLLING_INTERVAL_SECONDS; // Use default until settings are applied again

        // Spawn new metrics task
        self.metrics_task_handle = Some(
            metrics::spawn_metrics_task(
                self.tp.clone(),
                Arc::clone(&empty_channels),
                self.stream_selection_rx.clone(),
                self.stream_selection_tx.clone(),
                Arc::clone(&self.adaptive_state),
                base_interval,
                self.polling_interval_rx.clone(),
            )
            .await,
        );

        // Spawn new chat task
        self.chat_task_handle = Some(
            chat::spawn_chat_task(
                self.tp.clone(),
                Arc::clone(&empty_channels),
                self.stream_selection_rx.clone(),
                Arc::clone(&self.adaptive_state),
            )
            .await,
        );

        // Spawn new latest monitor task
        self.latest_monitor_task_handle = Some(
            latest_monitor::spawn_latest_monitor_task(
                Arc::clone(&empty_channels),
                self.stream_selection_rx.clone(),
                self.stream_selection_tx.clone(),
            )
            .await,
        );

        tracing::info!("restarted background tasks after OAuth credential change");

        // Notify user that re-authentication is required
        self.tp
            .notify(
                touchportal_sdk::protocol::CreateNotificationCommand::builder()
                    .notification_id("ytl_oauth_changed")
                    .title("OAuth Credentials Changed")
                    .message(
                        "Custom OAuth credentials have been updated. All stored tokens are now invalid. \
                        Please use 'Add YouTube Channel' to re-authenticate your accounts."
                    )
                    .build()
                    .unwrap(),
            )
            .await;

        // Update UI to reflect empty state
        self.tp
            .update_choices_in_ytl_channel(std::iter::once("No channels available"))
            .await;

        tracing::info!(
            "OAuth credentials changed - background tasks restarted, user must re-authenticate"
        );

        Ok(())
    }
}
