use eyre::Context;
use oauth2::{RefreshToken, TokenResponse};
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use tokio_stream::StreamExt;
use touchportal_sdk::protocol::{CreateNotificationCommand, InfoMessage};
use touchportal_youtube_live::youtube_api::client::{TimeBoundAccessToken, YouTubeClient};
use touchportal_youtube_live::youtube_api::broadcasts::{BroadcastLifeCycleStatus, BroadcastStatus, LiveBroadcastUpdateRequest, LiveBroadcastUpdateSnippet};
use touchportal_youtube_live::{Channel, oauth, setup_youtube_clients};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;

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

        // Extract broadcast ID or handle "latest" selection
        let broadcast_id = match ytl_broadcast {
            ChoicesForYtlBroadcast::Dynamic(broadcast_selection) => {
                if broadcast_selection == "Latest non-completed broadcast" {
                    // Find the latest non-completed broadcast for this channel
                    let channel = self.yt.get(channel_id)
                        .ok_or_else(|| eyre::eyre!("Selected channel not found"))?;
                    
                    let broadcasts = channel.yt.list_my_live_broadcasts();
                    let mut broadcasts = std::pin::pin!(broadcasts);
                    let mut latest_broadcast = None;
                    
                    while let Some(broadcast) = broadcasts.next().await {
                        let broadcast = broadcast.context("fetch broadcast for latest selection")?;
                        if broadcast.status.life_cycle_status != BroadcastLifeCycleStatus::Complete {
                            latest_broadcast = Some(broadcast.id);
                            break;
                        }
                    }
                    
                    latest_broadcast.ok_or_else(|| eyre::eyre!("No non-completed broadcast found"))?
                } else {
                    broadcast_selection
                        .rsplit_once(" - ")
                        .map(|(_, id)| id.to_string())
                        .ok_or_else(|| eyre::eyre!("Invalid broadcast selection format"))?
                }
            }
            _ => return Ok(()),
        };

        // Store the selections
        self.current_channel = Some(channel_id.to_string());
        self.current_broadcast = Some(broadcast_id.clone());

        // Update settings for persistence
        self.tp.set_selected_channel_id(channel_id.to_string()).await;
        self.tp.set_selected_broadcast_id(broadcast_id.clone()).await;

        // Update states
        if let Some(channel) = self.yt.get(channel_id) {
            self.tp.update_ytl_selected_channel_name(&channel.name).await;
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

        let channel = self.yt.get(channel_id)
            .ok_or_else(|| eyre::eyre!("Selected channel not available"))?;

        tracing::info!(
            channel = %channel_id,
            broadcast = %broadcast_id,
            "starting live broadcast"
        );

        // Transition broadcast from testing -> live
        channel.yt
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

        let channel = self.yt.get(channel_id)
            .ok_or_else(|| eyre::eyre!("Selected channel not available"))?;

        tracing::info!(
            channel = %channel_id,
            broadcast = %broadcast_id,
            "stopping live broadcast"
        );

        // Transition broadcast from live -> complete
        channel.yt
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

        let channel = self.yt.get(channel_id)
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
        
        channel.yt
            .update_live_broadcast(&update_request)
            .await
            .context("update broadcast title")?;

        // Update the current stream title state
        self.tp.update_ytl_current_stream_title(&ytl_new_title).await;

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

        let channel = self.yt.get(channel_id)
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
        
        channel.yt
            .update_live_broadcast(&update_request)
            .await
            .context("update broadcast description")?;

        tracing::info!("broadcast description updated successfully");
        Ok(())
    }

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
                outgoing.update_ytl_selected_channel_name(&channel.name).await;
            }
        }

        // ==============================================================================
        // Background State Monitoring Task
        // ==============================================================================
        // Spawn a background task to periodically refresh live stream status and metrics.
        // This keeps the plugin's state synchronized with YouTube's backend without
        // requiring user interaction.
        let _handle = outgoing.clone();
        let polling_interval = settings.polling_interval_seconds.max(30.0) as u64;
        let channels = client_by_channel.clone();
        let current_channel_clone = current_channel.clone();
        let current_broadcast_clone = current_broadcast.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(polling_interval));
            loop {
                interval.tick().await;
                
                // TODO: Implement metrics polling and chat event monitoring
                // - Poll video statistics for current broadcast
                // - Update likes, dislikes, views, live viewers states  
                // - Monitor chat stream for new messages, super chats, sponsors
                // - Trigger TouchPortal events with local state updates

                if let (Some(channel_id), Some(broadcast_id)) = 
                    (&current_channel_clone, &current_broadcast_clone) {
                    if let Some(_channel) = channels.get(channel_id) {
                        tracing::debug!(
                            channel = %channel_id,
                            broadcast = %broadcast_id,
                            "polling metrics (TODO: implement)"
                        );
                    }
                }
            }
        });

        Ok(Self {
            yt: client_by_channel,
            tp: outgoing,
            current_channel,
            current_broadcast,
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