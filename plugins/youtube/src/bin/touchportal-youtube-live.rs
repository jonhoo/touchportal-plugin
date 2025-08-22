use eyre::Context;
use oauth2::{RefreshToken, TokenResponse};
use std::collections::{HashMap, HashSet, VecDeque};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, watch};
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

/// Activity level for chat or metrics
#[derive(Debug, Clone, Copy, PartialEq)]
enum ActivityLevel {
    High,
    Medium,
    Low,
}

impl ActivityLevel {
    fn description(&self) -> &'static str {
        match self {
            ActivityLevel::High => "High Activity",
            ActivityLevel::Medium => "Normal",
            ActivityLevel::Low => "Low Activity",
        }
    }
}

/// Tracks chat message activity patterns to determine relative activity levels
#[derive(Debug, Clone)]
struct ChatActivityTracker {
    message_timestamps: VecDeque<Instant>,
    session_start: Option<Instant>,
    session_baseline: Option<f64>, // Messages/min when stream started
    rolling_average: f64,          // Recent average (last 15 minutes)
    recent_peak: f64,              // Highest rate in last hour
    last_activity_check: Instant,
    total_messages: u64,
}

impl ChatActivityTracker {
    fn new() -> Self {
        Self {
            message_timestamps: VecDeque::new(),
            session_start: None,
            session_baseline: None,
            rolling_average: 0.0,
            recent_peak: 0.0,
            last_activity_check: Instant::now(),
            total_messages: 0,
        }
    }

    fn register_message(&mut self) {
        let now = Instant::now();
        self.message_timestamps.push_back(now);
        self.total_messages += 1;

        // Log significant activity changes
        let current_rate = self.messages_per_minute(2); // Immediate activity
        let recent_rate = self.messages_per_minute(10); // Recent baseline
        if current_rate > recent_rate * 3.0 && current_rate > 1.0 {
            tracing::debug!(
                current_rate = %current_rate,
                recent_rate = %recent_rate,
                "chat activity spike detected"
            );
        }

        // Initialize session start on first message
        if self.session_start.is_none() {
            self.session_start = Some(now);
        }

        // Clean old messages (keep last hour)
        let cutoff = now - Duration::from_secs(3600);
        while let Some(&front) = self.message_timestamps.front() {
            if front < cutoff {
                self.message_timestamps.pop_front();
            } else {
                break;
            }
        }

        // Update metrics periodically
        if now.duration_since(self.last_activity_check) > Duration::from_secs(60) {
            self.update_metrics();
            self.last_activity_check = now;
        }
    }

    fn update_metrics(&mut self) {
        let now = Instant::now();

        // Update rolling average (last 15 minutes)
        self.rolling_average = self.messages_per_minute(15);

        // Update recent peak (last hour)
        self.recent_peak = self.recent_peak.max(self.rolling_average);

        // Set session baseline if we have enough data (10 minutes into session)
        if self.session_baseline.is_none() {
            if let Some(start) = self.session_start {
                if now.duration_since(start) > Duration::from_secs(600) {
                    // 10 minutes
                    self.session_baseline = Some(self.rolling_average);
                }
            }
        }
    }

    fn messages_per_minute(&self, minutes: u64) -> f64 {
        let cutoff = Instant::now() - Duration::from_secs(minutes * 60);
        let count = self
            .message_timestamps
            .iter()
            .filter(|&&timestamp| timestamp >= cutoff)
            .count();
        count as f64 / minutes as f64
    }

    fn calculate_activity_level(&self) -> ActivityLevel {
        let current_rate = self.messages_per_minute(5); // Last 5 minutes
        let baseline = self.rolling_average.max(0.1); // Avoid division by zero

        // Multi-window analysis
        let immediate_burst = self.messages_per_minute(2);
        let recent_trend = self.messages_per_minute(10);

        // Check for sudden excitement spike
        if immediate_burst > recent_trend * 3.0 && immediate_burst > 1.0 {
            return ActivityLevel::High;
        }

        // For very quiet streams (< 0.1 msg/min average), any activity is significant
        if baseline < 0.1 {
            return if current_rate > 0.5 {
                ActivityLevel::High
            } else if current_rate > 0.1 {
                ActivityLevel::Medium
            } else {
                ActivityLevel::Low
            };
        }

        // Relative activity level based on established patterns
        let ratio = current_rate / baseline;
        match ratio {
            r if r > 2.0 => ActivityLevel::High,   // 2x+ recent average
            r if r > 1.3 => ActivityLevel::Medium, // 30%+ above average
            r if r < 0.5 => ActivityLevel::Low,    // 50%+ below average
            _ => ActivityLevel::Medium,            // Near average
        }
    }

    fn was_inactive_recently(&self) -> bool {
        // Consider inactive if no messages in last 10 minutes but had activity before
        self.messages_per_minute(10) < 0.1 && self.total_messages > 5
    }
}

/// Tracks metrics changes to determine volatility levels
#[derive(Debug, Clone)]
struct MetricsVolatilityTracker {
    last_viewers: Option<u64>,
    last_likes: Option<u64>,
    last_views: Option<u64>,
    volatility_history: VecDeque<f64>, // Recent change percentages
    last_update: Instant,
}

impl MetricsVolatilityTracker {
    fn new() -> Self {
        Self {
            last_viewers: None,
            last_likes: None,
            last_views: None,
            volatility_history: VecDeque::new(),
            last_update: Instant::now(),
        }
    }

    fn update_from_metrics(
        &mut self,
        viewers: Option<u64>,
        likes: Option<u64>,
        views: Option<u64>,
    ) {
        let now = Instant::now();
        let mut total_change = 0.0;
        let mut change_count = 0;

        // Calculate percentage changes
        if let (Some(current), Some(last)) = (viewers, self.last_viewers) {
            if last > 0 {
                let change = (current as f64 - last as f64).abs() / last as f64;
                total_change += change;
                change_count += 1;
            }
        }

        if let (Some(current), Some(last)) = (likes, self.last_likes) {
            if last > 0 {
                let change = (current as f64 - last as f64).abs() / last as f64;
                total_change += change;
                change_count += 1;
            }
        }

        // Views typically grow monotonically, so we look at rate of growth
        if let (Some(current), Some(last)) = (views, self.last_views) {
            if last > 0 && current > last {
                let growth_rate = (current as f64 - last as f64) / last as f64;
                total_change += growth_rate;
                change_count += 1;
            }
        }

        // Store average change if we have any measurements
        if change_count > 0 {
            let avg_change = total_change / change_count as f64;
            self.volatility_history.push_back(avg_change);

            // Log significant volatility
            if avg_change > 0.2 {
                // >20% change
                tracing::debug!(
                    avg_change = %avg_change,
                    viewers = ?viewers,
                    likes = ?likes,
                    views = ?views,
                    "high metrics volatility detected"
                );
            }

            // Keep only last 10 measurements
            while self.volatility_history.len() > 10 {
                self.volatility_history.pop_front();
            }
        }

        // Update stored values
        self.last_viewers = viewers;
        self.last_likes = likes;
        self.last_views = views;
        self.last_update = now;
    }

    fn calculate_volatility(&self) -> ActivityLevel {
        if self.volatility_history.is_empty() {
            return ActivityLevel::Medium; // Default when no data
        }

        // Average recent volatility
        let avg_volatility: f64 =
            self.volatility_history.iter().sum::<f64>() / self.volatility_history.len() as f64;

        // Check for recent high volatility (last 3 measurements)
        let recent_volatility = if self.volatility_history.len() >= 3 {
            self.volatility_history.iter().rev().take(3).sum::<f64>() / 3.0
        } else {
            avg_volatility
        };

        match recent_volatility {
            v if v > 0.15 => ActivityLevel::High,   // >15% change
            v if v > 0.05 => ActivityLevel::Medium, // 5-15% change
            _ => ActivityLevel::Low,                // <5% change
        }
    }
}

/// Main adaptive polling state manager
#[derive(Debug, Clone)]
struct AdaptivePollingState {
    base_interval: u64,
    current_interval: u64,
    enabled: bool,
    chat_tracker: ChatActivityTracker,
    metrics_tracker: MetricsVolatilityTracker,
    last_interval_update: Instant,
}

impl AdaptivePollingState {
    fn new(base_interval: u64, enabled: bool) -> Self {
        Self {
            base_interval,
            current_interval: base_interval,
            enabled,
            chat_tracker: ChatActivityTracker::new(),
            metrics_tracker: MetricsVolatilityTracker::new(),
            last_interval_update: Instant::now(),
        }
    }

    fn register_chat_message(&mut self) {
        self.chat_tracker.register_message();
    }

    fn update_from_metrics(
        &mut self,
        viewers: Option<u64>,
        likes: Option<u64>,
        views: Option<u64>,
    ) {
        self.metrics_tracker
            .update_from_metrics(viewers, likes, views);
    }

    fn should_recalculate_interval(&self) -> bool {
        // Recalculate every 2 minutes or after significant changes
        Instant::now().duration_since(self.last_interval_update) > Duration::from_secs(120)
    }

    fn calculate_optimal_interval(&mut self) -> u64 {
        if !self.enabled {
            return self.base_interval;
        }

        let chat_level = self.chat_tracker.calculate_activity_level();
        let metrics_volatility = self.metrics_tracker.calculate_volatility();

        let multiplier = match (chat_level, metrics_volatility) {
            (ActivityLevel::High, ActivityLevel::High) => 1.0, // Maximum responsiveness
            (ActivityLevel::High, _) => 2.5,                   // Chat provides real-time data
            (_, ActivityLevel::High) => 1.0,                   // Track rapid metrics changes
            (ActivityLevel::Medium, ActivityLevel::Medium) => 1.8, // Balanced monitoring
            (ActivityLevel::Low, ActivityLevel::Low) => 4.0,   // Minimal activity
            _ => 2.0,                                          // Default moderate adjustment
        };

        let optimal = (self.base_interval as f64 * multiplier) as u64;
        let new_interval = optimal.clamp(self.base_interval, self.base_interval * 6);

        self.last_interval_update = Instant::now();
        self.current_interval = new_interval;

        new_interval
    }

    fn get_status_description(&self) -> String {
        if !self.enabled {
            return format!("{}s (Disabled)", self.base_interval);
        }

        let chat_level = self.chat_tracker.calculate_activity_level();
        let metrics_level = self.metrics_tracker.calculate_volatility();

        let reason = match (chat_level, metrics_level) {
            (ActivityLevel::High, ActivityLevel::High) => "Very Active",
            (ActivityLevel::High, _) => "Active Chat",
            (_, ActivityLevel::High) => "Changing Metrics",
            (ActivityLevel::Low, ActivityLevel::Low) => "Quiet",
            _ => "Normal",
        };

        format!("{}s ({})", self.current_interval, reason)
    }
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
                            .message(&format!("Could not start broadcast: {}", e))
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

        // Notify user of successful update
        self.tp
            .notify(
                CreateNotificationCommand::builder()
                    .notification_id("ytl_title_updated")
                    .title("Title Updated")
                    .message(&format!("Stream title updated to: {}", ytl_new_title))
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

    // TODO: Add thumbnail management action
    // - New action: ytl_update_thumbnail with file path parameter
    // - YouTube API: thumbnails.set endpoint
    // - File upload handling for image files
    // - Validation of image format and size requirements
    // See: https://developers.google.com/youtube/v3/docs/thumbnails/set

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

    // TODO: Add poll/community integration (Limited by API availability)
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
/// Also updates adaptive polling state based on metrics changes
async fn poll_and_update_metrics(
    outgoing: &mut TouchPortalHandle,
    client: &YouTubeClient,
    broadcast_id: &str,
    stream_rx: &watch::Receiver<StreamSelection>,
    adaptive_state: Arc<Mutex<AdaptivePollingState>>,
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

            // Clear metrics states on repeated failures to show current status
            outgoing.update_ytl_views_count("X").await;
            outgoing.update_ytl_likes_count("X").await;
            outgoing.update_ytl_dislikes_count("X").await;
            outgoing.update_ytl_live_viewers_count("X").await;
        }
    }
}

// TODO: Add stream health monitoring metrics
// - New states: ytl_stream_health, ytl_stream_resolution, ytl_stream_framerate, ytl_stream_bitrate
// - YouTube API: liveStreams.list endpoint with status and contentDetails parts
// - Polling integration: Add health metrics to poll_and_update_metrics function
// - Error detection: Monitor for stream issues, quality drops, connection problems
// - Health status enum: "healthy", "warning", "error", "offline"
// - Integration with existing metrics polling loop
// See: https://developers.google.com/youtube/v3/live/docs/liveStreams/list

// TODO: Add advanced analytics states
// - Enhanced Metrics Group: ytl_chat_message_rate, ytl_super_chat_total_session, ytl_membership_count
// - System Health Group: ytl_last_api_error, ytl_stream_uptime
// - Session Statistics: Track session-level metrics, reset on stream change

// TODO: Enhance chat events with richer data
// - Add more local states to chat events (message IDs, user badges, channel URLs, etc.)
// - Currently chat events only provide basic info (author, message, timestamp)
// - More states would allow TouchPortal users to create richer automations
// - Example: trigger different actions for moderators vs regular viewers
// - Add message ID tracking to detect when messages are deleted by moderators
// - Improve timestamp formatting (currently raw ISO string, could be more user-friendly)
// - Add chat deletion detection if YouTube API supports it

/// Process a chat message and trigger appropriate TouchPortal events
/// Also updates adaptive polling state based on chat activity
async fn process_chat_message(
    outgoing: &mut TouchPortalHandle,
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

// TODO: Add two-way chat integration
// - New action: ytl_send_chat_message with message parameter
// - Implement liveChatMessages.insert API endpoint
// - Add proper rate limiting and error handling for chat restrictions
// - Update chat monitoring to detect our own sent messages
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

        // Initialize adaptive polling state
        let adaptive_enabled = settings.smart_polling_adjustment;
        let base_interval = settings.base_polling_interval_seconds.max(30.0) as u64;
        let adaptive_state = Arc::new(Mutex::new(AdaptivePollingState::new(
            base_interval,
            adaptive_enabled,
        )));

        tracing::info!(
            adaptive_enabled = adaptive_enabled,
            base_interval = base_interval,
            "adaptive polling system initialized"
        );

        // Initial status update
        {
            let state = adaptive_state.lock().await;
            outgoing
                .update_ytl_adaptive_polling_status(&state.get_status_description())
                .await;
        }
        let (polling_interval_tx, polling_interval_rx) = watch::channel(base_interval);

        // ==============================================================================
        // Background Metrics Polling Task
        // ==============================================================================
        // Spawn a dedicated task for metrics polling that won't block chat processing
        let mut metrics_outgoing = outgoing.clone();
        let metrics_channels = client_by_channel.clone();
        let metrics_stream_rx = stream_selection_rx.clone();
        let metrics_adaptive_state = adaptive_state.clone();

        tokio::spawn(async move {
            let mut current_interval = base_interval;
            let mut interval = tokio::time::interval(Duration::from_secs(current_interval));
            let mut polling_interval_rx = polling_interval_rx;

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        // Time to poll metrics - check if we should recalculate interval
                        let should_recalculate = {
                            let state = metrics_adaptive_state.lock().await;
                            state.should_recalculate_interval()
                        };

                        if should_recalculate {
                            let new_interval = {
                                let mut state = metrics_adaptive_state.lock().await;
                                state.calculate_optimal_interval()
                            };

                            if new_interval != current_interval {
                                let (chat_level, metrics_level) = {
                                    let state = metrics_adaptive_state.lock().await;
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
                                    let state = metrics_adaptive_state.lock().await;
                                    metrics_outgoing
                                        .update_ytl_adaptive_polling_status(&state.get_status_description())
                                        .await;
                                }
                                continue; // Skip this iteration to reset timing
                            }
                        }
                    }
                    Ok(()) = polling_interval_rx.changed() => {
                        // Manual interval change (e.g., settings update)
                        let new_base_interval = *polling_interval_rx.borrow();
                        {
                            let mut state = metrics_adaptive_state.lock().await;
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
                                continue;
                            }
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
                            metrics_adaptive_state.clone(),
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
        let chat_adaptive_state = adaptive_state.clone();

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
                            process_chat_message(&mut chat_outgoing, msg, chat_adaptive_state.clone()).await;
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
