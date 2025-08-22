use eyre::Context;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{watch, Mutex};
use touchportal_sdk::protocol::{CreateNotificationCommand, InfoMessage};
use tokio_stream::StreamExt;
use crate::youtube_api::broadcasts::{
    BroadcastStatus, LiveBroadcastUpdateRequest, LiveBroadcastUpdateSnippet,
};

use crate::activity::AdaptivePollingState;
use crate::background::metrics::StreamSelection;
use crate::background::{chat, metrics};
use crate::actions::{oauth, stream_selection};
use crate::{Channel, setup_youtube_clients};

// You can look at the generated code for a plugin using this command:
//
// ```bash
// cat "$(dirname "$(cargo check --message-format=json | jq -r 'select(.reason == "build-script-executed") | select(.package_id | contains("#touchportal-")).out_dir')")/entry.rs"
// ```
include!(concat!(env!("OUT_DIR"), "/entry.rs"));

#[derive(Debug)]
pub struct Plugin {
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
        oauth::handle_add_youtube_channel(&mut self.tp, &mut self.yt).await
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
        let ChoicesForYtlBroadcast::Dynamic(broadcast_selection) = ytl_broadcast else {
            return Ok(());
        };

        // Delegate to the stream selection module
        let (channel_id, broadcast_id, selection) = stream_selection::handle_select_stream(
            &mut self.tp,
            &self.yt,
            channel_selection,
            broadcast_selection,
        )
        .await?;

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

impl Plugin {
    pub async fn new(
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
        // Spawn the metrics polling task
        metrics::spawn_metrics_task(
            outgoing.clone(),
            client_by_channel.clone(),
            stream_selection_rx.clone(),
            adaptive_state.clone(),
            base_interval,
            polling_interval_rx,
        ).await;

        // ==============================================================================
        // Background Chat Monitoring Task
        // ==============================================================================
        // Spawn the chat monitoring task
        chat::spawn_chat_task(
            outgoing.clone(),
            client_by_channel.clone(),
            stream_selection_rx.clone(),
            adaptive_state.clone(),
        ).await;

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