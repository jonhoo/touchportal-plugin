use crate::youtube_client::TimeBoundAccessToken;
use eyre::Context;
use oauth2::{RefreshToken, TokenResponse};
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use tokio_stream::StreamExt;
use touchportal_sdk::protocol::{CreateNotificationCommand, InfoMessage};
use touchportal_youtube_live::{oauth, setup_youtube_clients, youtube_client, Channel};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;
use youtube_client::YouTubeClient;

// You can look at the generated code for a plugin using this command:
//
// ```bash
// cat "$(dirname "$(cargo check --message-format=json | jq -r 'select(.reason == "build-script-executed") | select(.package_id | contains("#touchportal-")).out_dir')")"
// ```
include!(concat!(env!("OUT_DIR"), "/entry.rs"));

#[derive(Debug)]
struct Plugin {
    yt: HashMap<String, Channel>,
    tp: TouchPortalHandle,
    current_channel: Option<String>,
}

impl PluginCallbacks for Plugin {
    #[tracing::instrument(skip(self), ret)]
    async fn on_ytl_authenticate_account(
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

    async fn on_ytl_live_broadcast_toggle(
        &mut self,
        _mode: protocol::ActionInteractionMode,
        _ytl_channel: ChoicesForYtlChannel,
        _ytl_broadcast: ChoicesForYtlBroadcast,
    ) -> eyre::Result<()> {
        todo!()
    }

    async fn on_select_ytl_channel_in_ytl_live_broadcast_toggle(
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
        self.current_channel = Some(selected.to_string());
        let Some(channel) = self.yt.get_mut(selected) else {
            eyre::bail!("user selected unknown channel '{selected}'");
        };

        let broadcasts = channel.yt.list_my_live_broadcasts();

        let mut broadcast_choices = Vec::new();
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

    async fn on_select_ytl_broadcast_in_ytl_live_broadcast_toggle(
        &mut self,
        _instance: String,
        _selected: ChoicesForYtlBroadcast,
    ) -> eyre::Result<()> {
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

        // ==============================================================================
        // Background State Monitoring Task
        // ==============================================================================
        // Spawn a background task to periodically refresh live stream status and metrics.
        // This keeps the plugin's state synchronized with YouTube's backend without
        // requiring user interaction.
        let handle = outgoing.clone();
        tokio::spawn(async move {
            let _ = handle;
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                // TODO: refresh latest live video + view count?
            }
        });

        Ok(Self {
            yt: client_by_channel,
            tp: outgoing,
            current_channel: None,
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
