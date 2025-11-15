use crate::youtube_api::broadcasts::{
    BroadcastStatus, LiveBroadcastUpdateRequest, LiveBroadcastUpdateSnippet,
};
use eyre::Context;
use oauth2::TokenResponse as _;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, watch};
use tokio_stream::StreamExt;
use touchportal_sdk::protocol::{CreateNotificationCommand, InfoMessage, NotificationOption};

use crate::actions::stream_selection;
use crate::activity::AdaptivePollingState;
use crate::background::video_metrics::StreamSelection;
use crate::background::{broadcast_metrics, broadcast_monitor, chat, video_metrics};
use crate::oauth::OAuthManager;
use crate::youtube_api::client::{TimeBoundAccessToken, YouTubeClient};
use crate::{Channel, notifications};

// You can look at the generated code for a plugin using this command:
//
// ```bash
// cat "$(dirname "$(cargo check --message-format=json | jq -r 'select(.reason == "build-script-executed") | select(.package_id | contains("#touchportal-")).out_dir')")/out/entry.rs"
// ```
include!(concat!(env!("OUT_DIR"), "/entry.rs"));

/// Source of an OAuth flow, used to determine notification behavior.
#[derive(Debug, Clone, Copy)]
pub enum OAuthSource {
    /// OAuth flow initiated during plugin startup
    Startup,
    /// OAuth flow initiated by user action (add YouTube channel)
    UserInitiated,
}

/// Self-triggered events for background OAuth flows.
#[derive(Debug)]
pub enum OAuthEvent {
    /// OAuth flow completed successfully with tokens
    TokensAcquired {
        tokens: Vec<oauth2::basic::BasicTokenResponse>,
        source: OAuthSource,
    },
}

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
    video_metrics_task_handle: Option<tokio::task::JoinHandle<()>>,
    broadcast_metrics_task_handle: Option<tokio::task::JoinHandle<()>>,
    chat_task_handle: Option<tokio::task::JoinHandle<()>>,
    broadcast_monitor_task_handle: Option<tokio::task::JoinHandle<()>>,
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
    // Self-trigger channel for background OAuth flows
    self_trigger: tokio::sync::mpsc::Sender<OAuthEvent>,
    // Current settings for restoration logic
    current_settings: PluginSettings,
}

impl PluginCallbacks for Plugin {
    type SelfTriggered = OAuthEvent;

    #[tracing::instrument(skip(self), ret)]
    async fn on_settings_changed(&mut self, new_settings: PluginSettings) -> eyre::Result<()> {
        // Store the new settings for restoration logic
        self.current_settings = new_settings.clone();

        let PluginSettings {
            smart_polling_adjustment,
            base_polling_interval_seconds,
            custom_o_auth_client_id,
            custom_o_auth_client_secret,
            logging_verbosity,
            // Read-only settings updated by the plugin - ignore these changes
            you_tube_api_access_tokens: _,
            selected_channel_id: _,
            selected_broadcast_id: _,
        } = new_settings;
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
        // Create OAuth manager and spawn non-blocking OAuth flow
        let oauth_manager = Arc::new(OAuthManager::with_custom_credentials(
            self.current_custom_client_id.clone(),
            self.current_custom_client_secret.clone(),
        ));

        self.spawn_oauth_flow(oauth_manager, OAuthSource::UserInitiated)
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

        let broadcast_choices = self.fetch_broadcast_choices(&channel).await;

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

    #[tracing::instrument(skip(self), ret)]
    async fn on_self_triggered(&mut self, event: OAuthEvent) -> eyre::Result<()> {
        match event {
            OAuthEvent::TokensAcquired { tokens, source } => {
                tracing::info!(
                    token_count = tokens.len(),
                    source = ?source,
                    "processing OAuth tokens from self-triggered event"
                );

                // ==============================================================================
                // Unified Token Adoption Logic
                // ==============================================================================
                // This is the single place where we handle newly acquired OAuth tokens,
                // whether from startup or user-initiated flows.

                // Create OAuth manager with current custom credentials
                let oauth_manager = Arc::new(OAuthManager::with_custom_credentials(
                    self.current_custom_client_id.clone(),
                    self.current_custom_client_secret.clone(),
                ));

                // Create YouTube clients from fresh tokens
                let mut yt_clients = Vec::new();
                for token in &tokens {
                    let time_bound_token = TimeBoundAccessToken::new(token.clone());
                    let client = YouTubeClient::new(
                        time_bound_token,
                        Arc::clone(&oauth_manager),
                        self.http_client.clone(),
                    );
                    yt_clients.push(client);
                }

                // Validate all tokens
                for client in &yt_clients {
                    let is_valid = client
                        .validate_token()
                        .await
                        .context("validate fresh YouTube token")?;

                    if !is_valid {
                        eyre::bail!("freshly acquired YouTube token failed validation");
                    }
                }

                // Enumerate channels for all clients
                let mut new_channels = HashMap::new();
                for client in yt_clients {
                    let client_arc = Arc::new(client);
                    let channels_stream = client_arc.list_my_channels();
                    let mut channels_stream = std::pin::pin!(channels_stream);
                    while let Some(channel) = channels_stream.next().await {
                        let channel = channel.context("fetch channel")?;
                        new_channels.insert(
                            channel.id.clone(),
                            Channel {
                                name: channel.snippet.title,
                                yt: Arc::clone(&client_arc),
                            },
                        );
                    }
                }

                tracing::info!(
                    channel_count = new_channels.len(),
                    "enumerated channels from new tokens"
                );

                // Update the plugin's channel map
                let mut yt = self.yt.lock().await;
                yt.extend(new_channels.clone());

                // Collect all unique tokens from all channels (deduplicated by refresh token)
                use oauth2::RefreshToken;
                use std::collections::HashSet;

                let mut seen_refresh_tokens = HashSet::new();
                let mut all_tokens = Vec::new();

                for channel in yt.values() {
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

                drop(yt); // Release lock before async operations

                // Store all tokens
                self.tp
                    .set_you_tube_api_access_tokens(
                        serde_json::to_string(&all_tokens).expect("OAuth tokens always serialize"),
                    )
                    .await;

                // Update UI with all available channels
                let yt = self.yt.lock().await;
                self.tp
                    .update_choices_in_ytl_channel(
                        yt.iter().map(|(id, c)| format!("{} - {id}", c.name)),
                    )
                    .await;
                drop(yt);

                // Restore previous selections from settings (idempotent operation)
                self.restore_selections_from_settings().await;

                // Send notification if user-initiated
                if matches!(source, OAuthSource::UserInitiated) {
                    self.tp
                        .notify(
                            CreateNotificationCommand::builder()
                                .notification_id("ytl_channel_added")
                                .title("YouTube Channel Added")
                                .message("Your YouTube channel has been successfully added!")
                                .option(
                                    NotificationOption::builder()
                                        .id("ok")
                                        .title("OK")
                                        .build()
                                        .expect("notification option build should succeed"),
                                )
                                .build()
                                .expect("notification command build should succeed"),
                        )
                        .await;
                }

                tracing::info!("successfully adopted new OAuth tokens");

                Ok(())
            }
        }
    }
}

impl Plugin {
    /// Fetch and filter broadcasts for a channel, returning only those with active live chat.
    ///
    /// This builds a list of broadcast choices suitable for populating dropdowns,
    /// including the "Latest" option first, followed by specific broadcasts that
    /// have active live chat.
    async fn fetch_broadcast_choices(&self, channel: &Channel) -> Vec<String> {
        let broadcasts = channel.yt.list_my_live_broadcasts();

        let mut broadcast_choices =
            vec![ChoicesForYtlBroadcast::LatestNonCompletedBroadcast.to_string()];
        let mut stream = std::pin::pin!(broadcasts);

        while let Some(broadcast_result) = stream.next().await {
            let broadcast = match broadcast_result {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!(error = %e, "failed to fetch broadcast, skipping");
                    continue;
                }
            };

            // Only include broadcasts that have a live chat ID
            if broadcast.snippet.live_chat_id.is_some() {
                broadcast_choices
                    .push(format!("{} - {}", broadcast.snippet.title, broadcast.id));
            } else {
                tracing::debug!(
                    broadcast = %broadcast.id,
                    title = %broadcast.snippet.title,
                    "skipping broadcast without live chat"
                );
            }
        }

        broadcast_choices
    }

    pub async fn new(
        settings: PluginSettings,
        outgoing: TouchPortalHandle,
        info: InfoMessage,
        log_level_reload_handle: tracing_subscriber::reload::Handle<
            tracing_subscriber::EnvFilter,
            tracing_subscriber::Registry,
        >,
        self_trigger: tokio::sync::mpsc::Sender<OAuthEvent>,
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

        // Create shared HTTP client for all YouTube API operations
        let shared_http_client = reqwest::Client::new();

        // ==============================================================================
        // Parse Stored Tokens (If Any)
        // ==============================================================================
        // Check if we have stored tokens that we can immediately adopt.
        // Token adoption will happen via on_self_triggered after plugin construction.
        let stored_tokens = &settings.you_tube_api_access_tokens;
        let parsed_tokens = if !stored_tokens.is_empty() && stored_tokens != "[]" {
            match serde_json::from_str::<Vec<oauth2::basic::BasicTokenResponse>>(stored_tokens) {
                Ok(tokens) => {
                    tracing::info!(token_count = tokens.len(), "found stored tokens");
                    Some(tokens)
                }
                Err(e) => {
                    tracing::warn!(error = %e, "failed to parse stored tokens");
                    None
                }
            }
        } else {
            tracing::info!("no stored tokens found");
            None
        };

        // Start with empty channels - they'll be populated via on_self_triggered
        let shared_channels = Arc::new(Mutex::new(HashMap::new()));

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

        let (polling_interval_tx, polling_interval_rx) = watch::channel(base_interval);

        // Create tokio::watch channels for coordinating between action handlers and background tasks
        // Start with no stream selected - will be updated when tokens are adopted
        let initial_stream = StreamSelection::None;
        let (stream_selection_tx, stream_selection_rx) = watch::channel(initial_stream);

        // ==============================================================================
        // Background Video Metrics Polling Task
        // ==============================================================================
        // Spawn the video metrics polling task (views, likes, concurrent viewers)
        let video_metrics_task_handle = video_metrics::spawn_metrics_task(
            outgoing.clone(),
            Arc::clone(&shared_channels),
            stream_selection_rx.clone(),
            Arc::clone(&adaptive_state),
            polling_interval_rx.clone(),
        )
        .await;

        // ==============================================================================
        // Background Broadcast Metrics Polling Task
        // ==============================================================================
        // Spawn the broadcast metrics polling task (chat count)
        let broadcast_metrics_task_handle = broadcast_metrics::spawn_broadcast_metrics_task(
            outgoing.clone(),
            Arc::clone(&shared_channels),
            stream_selection_rx.clone(),
            Arc::clone(&adaptive_state),
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
        // Spawn the broadcast monitoring task
        let broadcast_monitor_task_handle = broadcast_monitor::spawn_broadcast_monitor_task(
            Arc::clone(&shared_channels),
            stream_selection_rx.clone(),
            stream_selection_tx.clone(),
            outgoing.clone(),
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
            current_custom_client_id: custom_client_id.clone(),
            current_custom_client_secret: custom_client_secret.clone(),
            video_metrics_task_handle: Some(video_metrics_task_handle),
            broadcast_metrics_task_handle: Some(broadcast_metrics_task_handle),
            chat_task_handle: Some(chat_task_handle),
            broadcast_monitor_task_handle: Some(broadcast_monitor_task_handle),
            adaptive_state,
            stream_selection_rx,
            polling_interval_rx,
            log_level_reload_handle,
            http_client: shared_http_client,
            self_trigger,
            current_settings: settings.clone(),
        };

        // Apply the initial logging level from plugin settings
        if let Err(e) = plugin.update_logging_level(settings.logging_verbosity) {
            tracing::warn!(error = %e, "failed to apply initial logging level");
        }

        // ==============================================================================
        // Initial Authentication: Adopt Stored Tokens or Trigger OAuth
        // ==============================================================================
        // Try to adopt stored tokens if available; fall back to OAuth flow on any failure.
        let needs_oauth = if let Some(tokens) = parsed_tokens {
            tracing::info!("adopting stored tokens via on_self_triggered");
            match plugin
                .on_self_triggered(OAuthEvent::TokensAcquired {
                    tokens,
                    source: OAuthSource::Startup,
                })
                .await
            {
                Ok(()) => false,
                Err(e) => {
                    tracing::error!(error = ?e, "failed to adopt stored tokens");
                    true
                }
            }
        } else {
            tracing::info!("no stored tokens found");
            true
        };

        if needs_oauth {
            let oauth_manager = Arc::new(OAuthManager::with_custom_credentials(
                custom_client_id,
                custom_client_secret,
            ));
            if let Err(e) = plugin
                .spawn_oauth_flow(oauth_manager, OAuthSource::Startup)
                .await
            {
                tracing::error!(error = ?e, "failed to spawn OAuth flow at startup");
            }
        }

        Ok(plugin)
    }

    /// Restore previous channel and broadcast selections from settings.
    ///
    /// This method is idempotent and can be safely called multiple times. It reads
    /// the selected channel and broadcast IDs from settings and restores the UI state
    /// and stream selection accordingly.
    ///
    /// The restoration process:
    /// 1. Reads `selected_channel_id` from settings
    /// 2. If a channel is selected and exists:
    ///    - Updates the channel name UI state
    ///    - Fetches and updates the broadcast choice list
    ///    - Restores the broadcast selection if one was saved
    ///
    /// All operations handle errors gracefully with logging, never failing the overall
    /// token adoption flow.
    async fn restore_selections_from_settings(&mut self) {
        let selected_channel_id = &self.current_settings.selected_channel_id;

        if selected_channel_id.is_empty() {
            tracing::debug!("no channel selection to restore");
            return;
        }

        let yt = self.yt.lock().await;
        let channel = match yt.get(selected_channel_id) {
            Some(channel) => channel,
            None => {
                tracing::warn!(
                    channel = %selected_channel_id,
                    "previously selected channel no longer available"
                );
                drop(yt);
                return;
            }
        };

        // Update channel name state
        self.tp
            .update_ytl_selected_channel_name(format!("{} - {}", channel.name, selected_channel_id))
            .await;

        // Fetch broadcasts for the current channel and update the choices
        let broadcast_choices = self.fetch_broadcast_choices(channel).await;

        self.tp
            .update_choices_in_ytl_broadcast(broadcast_choices.into_iter())
            .await;

        tracing::debug!(
            channel = %selected_channel_id,
            "restored broadcast choices for selected channel"
        );

        // Restore broadcast selection if one was saved
        let selected_broadcast_id = &self.current_settings.selected_broadcast_id;
        let broadcast_selection =
            stream_selection::BroadcastSelection::from_saved_id(selected_broadcast_id);

        match broadcast_selection {
            Some(stream_selection::BroadcastSelection::Latest) => {
                // User selected "latest" mode
                let _ = self
                    .stream_selection_tx
                    .send(StreamSelection::WaitForActiveBroadcast {
                        channel_id: selected_channel_id.clone(),
                    });
                tracing::info!(
                    channel = %selected_channel_id,
                    "restored selection with 'latest' broadcast mode"
                );
            }
            Some(stream_selection::BroadcastSelection::Specific(broadcast_id)) => {
                // User selected specific broadcast - validate it
                match channel.yt.get_live_broadcast(&broadcast_id).await {
                    Ok(broadcast) => {
                        if let Some(live_chat_id) = broadcast.snippet.live_chat_id {
                            let _ = self.stream_selection_tx.send(
                                StreamSelection::ChannelAndBroadcast {
                                    channel_id: selected_channel_id.clone(),
                                    broadcast_id: broadcast_id.clone(),
                                    live_chat_id,
                                    return_to_latest_on_completion: false,
                                },
                            );
                            tracing::info!(
                                channel = %selected_channel_id,
                                broadcast = %broadcast_id,
                                "restored selection with specific broadcast"
                            );
                        } else {
                            tracing::warn!(
                                broadcast = %broadcast_id,
                                "saved broadcast has no live chat; it may have ended"
                            );
                            self.tp.set_selected_broadcast_id(String::new()).await;
                            let _ = self.stream_selection_tx.send(StreamSelection::ChannelOnly {
                                channel_id: selected_channel_id.clone(),
                            });
                            tracing::info!(
                                channel = %selected_channel_id,
                                "restored selection with channel-only mode (broadcast ended)"
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            broadcast = %broadcast_id,
                            "failed to validate saved broadcast; it may have been deleted"
                        );
                        self.tp.set_selected_broadcast_id(String::new()).await;
                        let _ = self.stream_selection_tx.send(StreamSelection::ChannelOnly {
                            channel_id: selected_channel_id.clone(),
                        });
                        tracing::info!(
                            channel = %selected_channel_id,
                            "restored selection with channel-only mode (broadcast validation failed)"
                        );
                    }
                }
            }
            None => {
                // No broadcast selected, just channel
                let _ = self.stream_selection_tx.send(StreamSelection::ChannelOnly {
                    channel_id: selected_channel_id.clone(),
                });
                tracing::info!(
                    channel = %selected_channel_id,
                    "restored selection with channel-only mode"
                );
            }
        }
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
        if let Some(handle) = self.video_metrics_task_handle.take() {
            handle.abort();
            let _ = handle.await; // Wait for clean shutdown
            tracing::debug!("canceled metrics background task");
        }
        if let Some(handle) = self.broadcast_metrics_task_handle.take() {
            handle.abort();
            let _ = handle.await; // Wait for clean shutdown
            tracing::debug!("canceled chat metrics background task");
        }
        if let Some(handle) = self.chat_task_handle.take() {
            handle.abort();
            let _ = handle.await; // Wait for clean shutdown
            tracing::debug!("canceled chat background task");
        }
        if let Some(handle) = self.broadcast_monitor_task_handle.take() {
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

        // Spawn new metrics task
        self.video_metrics_task_handle = Some(
            video_metrics::spawn_metrics_task(
                self.tp.clone(),
                Arc::clone(&empty_channels),
                self.stream_selection_rx.clone(),
                Arc::clone(&self.adaptive_state),
                self.polling_interval_rx.clone(),
            )
            .await,
        );

        // Spawn new broadcast metrics task
        self.broadcast_metrics_task_handle = Some(
            broadcast_metrics::spawn_broadcast_metrics_task(
                self.tp.clone(),
                Arc::clone(&empty_channels),
                self.stream_selection_rx.clone(),
                Arc::clone(&self.adaptive_state),
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

        // Spawn new broadcast monitor task
        self.broadcast_monitor_task_handle = Some(
            broadcast_monitor::spawn_broadcast_monitor_task(
                Arc::clone(&empty_channels),
                self.stream_selection_rx.clone(),
                self.stream_selection_tx.clone(),
                self.tp.clone(),
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

    /// Spawns a background OAuth flow that doesn't block the main task.
    ///
    /// This method initiates an OAuth flow in a background task:
    /// 1. Starts authentication and gets the authorization URL
    /// 2. Sends "Check your browser" notification to the user
    /// 3. Opens the user's browser to the authorization URL
    /// 4. Spawns a background task to wait for user authorization
    /// 5. When complete, sends a self-triggered event with the token
    ///
    /// # Arguments
    ///
    /// * `oauth_manager` - The OAuth manager to use for authentication
    /// * `source` - The source of this OAuth flow (Startup or UserInitiated)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Setting up the authentication fails
    /// - Sending the notification fails
    /// - Opening the browser fails
    ///
    /// Note: Errors in the background task (waiting for authorization, token exchange)
    /// are logged but don't propagate - the plugin continues running.
    async fn spawn_oauth_flow(
        &mut self,
        oauth_manager: Arc<OAuthManager>,
        source: OAuthSource,
    ) -> eyre::Result<()> {
        // Stage 1: Start OAuth authentication (non-blocking)
        let (auth_url, continuation) = oauth_manager
            .start_authentication()
            .await
            .context("start OAuth authentication")?;

        // Send "Check your browser" notification
        self.tp
            .notify(
                CreateNotificationCommand::builder()
                    .notification_id("ytl_oauth_check_browser")
                    .title("YouTube Authentication")
                    .message(
                        "Please check your browser to authorize access to your YouTube account.",
                    )
                    .option(
                        NotificationOption::builder()
                            .id("ok")
                            .title("OK")
                            .build()
                            .expect("notification option build should succeed"),
                    )
                    .build()
                    .expect("notification command build should succeed"),
            )
            .await;

        // Open the user's browser
        tracing::info!(url = %auth_url, "opening user's browser for OAuth flow");
        webbrowser::open(&auth_url).context("open user's browser")?;

        // Stage 2: Spawn background task to wait for authorization
        let self_trigger = self.self_trigger.clone();
        tokio::spawn(async move {
            match oauth_manager.complete_authentication(continuation).await {
                Ok(token) => {
                    tracing::info!("OAuth flow completed successfully");
                    if let Err(e) = self_trigger
                        .send(OAuthEvent::TokensAcquired {
                            tokens: vec![token],
                            source,
                        })
                        .await
                    {
                        tracing::error!(
                            error = %e,
                            "failed to send OAuth completion event to plugin"
                        );
                    }
                }
                Err(e) => {
                    tracing::error!(error = ?e, "OAuth flow failed in background task");
                }
            }
        });

        Ok(())
    }
}
