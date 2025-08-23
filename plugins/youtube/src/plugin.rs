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
use crate::{Channel, setup_youtube_clients};

// You can look at the generated code for a plugin using this command:
//
// ```bash
// cat "$(dirname "$(cargo check --message-format=json | jq -r 'select(.reason == "build-script-executed") | select(.package_id | contains("#touchportal-")).out_dir')")/entry.rs"
// ```
include!(concat!(env!("OUT_DIR"), "/entry.rs"));

#[derive(Debug)]
pub struct Plugin {
    yt: Arc<Mutex<HashMap<String, Channel>>>,
    tp: TouchPortalHandle,
    current_channel: Option<String>,
    current_broadcast: Option<String>,
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
            // Read-only settings updated by the plugin - ignore these changes
            you_tube_api_access_tokens: _,
            selected_channel_id: _,
            selected_broadcast_id: _,
        }: PluginSettings,
    ) -> eyre::Result<()> {
        // Handle OAuth credential changes (highest priority - invalidates all tokens)
        self.handle_oauth_credential_change(&custom_o_auth_client_id, &custom_o_auth_client_secret)
            .await?;

        // Handle polling interval changes - notify background tasks and adaptive state
        let new_polling_interval =
            base_polling_interval_seconds.max(crate::MIN_POLLING_INTERVAL_SECONDS as f64) as u64;
        if let Err(e) = self.polling_interval_tx.send(new_polling_interval) {
            tracing::warn!(
                error = %e,
                new_interval = new_polling_interval,
                "failed to notify background tasks of polling interval change"
            );
        }

        // Update adaptive polling base interval
        {
            let mut adaptive_state = self.adaptive_state.lock().await;
            adaptive_state.base_interval = new_polling_interval;
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
            return Ok(());
        };

        let (channel_id, broadcast_id, selection) = match ytl_broadcast {
            ChoicesForYtlBroadcast::LatestNonCompletedBroadcast => {
                // Handle "Latest non-completed broadcast" selection
                stream_selection::handle_select_stream(
                    &mut self.tp,
                    &self.yt,
                    channel_selection,
                    stream_selection::BroadcastSelection::Latest,
                )
                .await?
            }
            ChoicesForYtlBroadcast::Dynamic(broadcast_selection) => {
                // Handle manually selected broadcast
                stream_selection::handle_select_stream(
                    &mut self.tp,
                    &self.yt,
                    channel_selection,
                    stream_selection::BroadcastSelection::Specific(broadcast_selection),
                )
                .await?
            }
            _ => {
                // Skip other choices (like "Select channel first")
                return Ok(());
            }
        };

        // Store the selections
        self.current_channel = Some(channel_id);
        self.current_broadcast = Some(broadcast_id);

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
        let Some(channel_id) = &self.current_channel else {
            // Provide user-friendly notification for missing selection
            self.tp
                .notify(
                    CreateNotificationCommand::builder()
                        .notification_id("ytl_no_channel_selected")
                        .title("No Channel Selected")
                        .message("Please use the 'Select Stream' action to choose a channel and broadcast first.")
                        .build()
                        .unwrap(),
                )
                .await;
            eyre::bail!("No channel selected - use 'Select Stream' action first");
        };
        let Some(broadcast_id) = &self.current_broadcast else {
            self.tp
                .notify(
                    CreateNotificationCommand::builder()
                        .notification_id("ytl_no_broadcast_selected")
                        .title("No Broadcast Selected")
                        .message(
                            "Please use the 'Select Stream' action to choose a broadcast first.",
                        )
                        .build()
                        .unwrap(),
                )
                .await;
            eyre::bail!("No broadcast selected - use 'Select Stream' action first");
        };

        let channel = {
            let yt_guard = self.yt.lock().await;
            yt_guard.get(channel_id).cloned()
        }
        .ok_or_else(|| eyre::eyre!("Selected channel not available"))?;

        tracing::info!(
            channel = %channel_id,
            broadcast = %broadcast_id,
            "starting live broadcast"
        );

        // Transition broadcast from testing -> live
        match channel
            .yt
            .transition_live_broadcast(broadcast_id, BroadcastStatus::Live)
            .await
        {
            Ok(_) => {
                self.tp
                    .notify(
                        CreateNotificationCommand::builder()
                            .notification_id("ytl_broadcast_started")
                            .title("Broadcast Started")
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
                            .title("Failed to Start Broadcast")
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
        let Some(channel_id) = &self.current_channel else {
            eyre::bail!("No channel selected - use 'Select Stream' action first");
        };
        let Some(broadcast_id) = &self.current_broadcast else {
            eyre::bail!("No broadcast selected - use 'Select Stream' action first");
        };

        let channel = {
            let yt_guard = self.yt.lock().await;
            yt_guard.get(channel_id).cloned()
        }
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
        // Input validation
        if ytl_new_title.trim().is_empty() {
            self.tp
                .notify(
                    CreateNotificationCommand::builder()
                        .notification_id("ytl_empty_title")
                        .title("Empty Title")
                        .message("Please provide a title for your stream.")
                        .build()
                        .unwrap(),
                )
                .await;
            eyre::bail!("Title cannot be empty");
        }

        if ytl_new_title.len() > 100 {
            self.tp
                .notify(
                    CreateNotificationCommand::builder()
                        .notification_id("ytl_title_too_long")
                        .title("Title Too Long")
                        .message("Stream title must be 100 characters or less.")
                        .build()
                        .unwrap(),
                )
                .await;
            eyre::bail!("Title too long (max 100 characters)");
        }
        let Some(channel_id) = &self.current_channel else {
            eyre::bail!("No channel selected - use 'Select Stream' action first");
        };
        let Some(broadcast_id) = &self.current_broadcast else {
            eyre::bail!("No broadcast selected - use 'Select Stream' action first");
        };

        let channel = {
            let yt_guard = self.yt.lock().await;
            yt_guard.get(channel_id).cloned()
        }
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

        // Notify user of successful update
        self.tp
            .notify(
                CreateNotificationCommand::builder()
                    .notification_id("ytl_title_updated")
                    .title("Title Updated")
                    .message(format!("Stream title updated to: {}", ytl_new_title))
                    .build()
                    .unwrap(),
            )
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

        let channel = {
            let yt_guard = self.yt.lock().await;
            yt_guard.get(channel_id).cloned()
        }
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

impl Plugin {
    pub async fn new(
        settings: PluginSettings,
        mut outgoing: TouchPortalHandle,
        info: InfoMessage,
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

        let (client_by_channel, refreshed_tokens) = setup_youtube_clients(
            &settings.you_tube_api_access_tokens,
            custom_client_id.clone(),
            custom_client_secret.clone(),
            notify_callback,
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

        let current_broadcast = if settings.selected_broadcast_id.is_empty() {
            None
        } else {
            Some(settings.selected_broadcast_id.clone())
        };

        // Update channel name state if we have a current channel
        if let Some(channel_id) = &current_channel
            && let Some(channel) = client_by_channel.get(channel_id)
        {
            outgoing
                .update_ytl_selected_channel_name(format!("{} - {}", channel.name, channel_id))
                .await;
        }

        // ==============================================================================
        // Background Task Coordination
        // ==============================================================================
        // Create tokio::watch channels for coordinating between action handlers and background tasks
        let initial_stream = match (&current_channel, &current_broadcast) {
            (Some(channel_id), Some(_broadcast_id)) => {
                // We have both channel and broadcast, but need to get live_chat_id
                // For now, we'll use ChannelOnly since we don't have live_chat_id at startup
                // TODO(claude): query for the live chat id here and use the ChannelAndBroadcast variant.
                StreamSelection::ChannelOnly {
                    channel_id: channel_id.clone(),
                }
            }
            (Some(channel_id), None) => StreamSelection::ChannelOnly {
                channel_id: channel_id.clone(),
            },
            _ => StreamSelection::None,
        };
        let (stream_selection_tx, stream_selection_rx) = watch::channel(initial_stream);

        // Initialize adaptive polling state
        let adaptive_enabled = settings.smart_polling_adjustment;
        let base_interval = settings
            .base_polling_interval_seconds
            .max(crate::MIN_POLLING_INTERVAL_SECONDS as f64) as u64;
        let adaptive_state = Arc::new(Mutex::new(AdaptivePollingState::new(
            base_interval,
            adaptive_enabled,
        )));

        // Initial status update
        {
            let state = adaptive_state.lock().await;
            outgoing
                .update_ytl_adaptive_polling_status(&state.get_status_description())
                .await;
        }
        let (polling_interval_tx, polling_interval_rx) = watch::channel(base_interval);

        // Create shared channel state for background tasks
        let shared_channels = Arc::new(Mutex::new(client_by_channel));

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
            outgoing.clone(),
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

        Ok(Self {
            yt: shared_channels,
            tp: outgoing,
            current_channel,
            current_broadcast,
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
        })
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
        self.tp.set_you_tube_api_access_tokens(String::new()).await;

        // Clear all YouTube clients and channels
        {
            let mut yt_guard = self.yt.lock().await;
            yt_guard.clear();
        }

        // Reset current selections
        self.current_channel = None;
        self.current_broadcast = None;

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

        // Restart background tasks with empty channel map (they will be idle until re-auth)
        let empty_channels = Arc::new(Mutex::new(HashMap::new()));
        let base_interval = crate::MIN_POLLING_INTERVAL_SECONDS; // Use default until settings are applied again

        // Spawn new metrics task
        let metrics_task_handle = metrics::spawn_metrics_task(
            self.tp.clone(),
            empty_channels.clone(),
            self.stream_selection_rx.clone(),
            self.stream_selection_tx.clone(),
            self.adaptive_state.clone(),
            base_interval,
            self.polling_interval_rx.clone(),
        )
        .await;

        // Spawn new chat task
        let chat_task_handle = chat::spawn_chat_task(
            self.tp.clone(),
            empty_channels.clone(),
            self.stream_selection_rx.clone(),
            self.adaptive_state.clone(),
        )
        .await;

        if let Some(handle) = self.latest_monitor_task_handle.take() {
            handle.abort();
            let _ = handle.await; // Wait for clean shutdown
            tracing::debug!("canceled latest monitor background task");
        }

        // Spawn new latest monitor task
        let latest_monitor_task_handle = latest_monitor::spawn_latest_monitor_task(
            self.tp.clone(),
            empty_channels.clone(),
            self.stream_selection_rx.clone(),
            self.stream_selection_tx.clone(),
        )
        .await;

        // Store new task handles
        self.metrics_task_handle = Some(metrics_task_handle);
        self.chat_task_handle = Some(chat_task_handle);
        self.latest_monitor_task_handle = Some(latest_monitor_task_handle);

        tracing::info!(
            "restarted background tasks with empty channel map after OAuth credential change"
        );

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

        // Clear stream selection since we have no valid channels
        if let Err(e) = self
            .stream_selection_tx
            .send(crate::background::metrics::StreamSelection::None)
        {
            tracing::warn!(
                error = %e,
                "failed to send empty stream selection to new background tasks"
            );
        }

        tracing::info!(
            "OAuth credentials changed - background tasks restarted, user must re-authenticate"
        );

        Ok(())
    }
}
