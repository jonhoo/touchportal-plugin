#![allow(dead_code)]

use crate::youtube_client::Token;
use eyre::Context;
use oauth2::{RefreshToken, TokenResponse};
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use tokio_stream::StreamExt;
use touchportal_sdk::protocol::{CreateNotificationCommand, InfoMessage};
use touchportal_sdk::ApiVersion;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;
use youtube_client::YouTubeClient;

mod oauth;
mod youtube_client;

// You can look at the generated code for a plugin using this command:
//
// ```bash
// cat "$(dirname "$(cargo check --message-format=json | jq -r 'select(.reason == "build-script-executed") | select(.package_id | contains("#touchportal-")).out_dir')")"
// ```
include!(concat!(env!("OUT_DIR"), "/entry.rs"));

const OAUTH_CLIENT_ID: &str =
    "392239669497-in1s6h0alvakffbb5bjbqjegn2m5aram.apps.googleusercontent.com";

// As per <https://developers.google.com/identity/protocols/oauth2#installed>, for an installed
// desktop application using PKCE, it's expected that the secret gets embedded, and it is _not_
// considered secret.
const OAUTH_SECRET: &str = "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";

const OAUTH_DONE: &str = include_str!("../oauth_success.html");

#[derive(Debug)]
struct Channel {
    name: String,
    yt: YouTubeClient,
}

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
        let oauth_manager = oauth::OAuthManager::new(OAUTH_CLIENT_ID, OAUTH_SECRET, OAUTH_DONE);

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

        let client = YouTubeClient::new(Token::from_fresh_token(new_token.clone()), oauth_manager);

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

        // ==============================================================================
        // OAuth Manager Setup
        // ==============================================================================
        // We centralize all OAuth operations through a single manager to maintain
        // consistency in client configuration and error handling across token flows.
        let oauth_manager = oauth::OAuthManager::new(OAUTH_CLIENT_ID, OAUTH_SECRET, OAUTH_DONE);

        // ==============================================================================
        // Token Acquisition Strategy
        // ==============================================================================
        // For new users with no stored tokens, we initiate a fresh OAuth flow.
        // The user gets a browser notification to complete the authorization process.
        let (tokens, is_old) = if settings.you_tube_api_access_tokens.is_empty()
            || settings.you_tube_api_access_tokens == "[]"
        {
            outgoing
                .notify(
                    CreateNotificationCommand::builder()
                        .notification_id("ytl_auth")
                        .title("Check your browser")
                        .message(
                            "You need to authenticate to YouTube \
                            to give access to your channel.",
                        )
                        .build()
                        .unwrap(),
                )
                .await;

            let token = oauth_manager
                .authenticate()
                .await
                .context("authorize user to YouTube")?;
            let tokens = vec![token];

            outgoing
                .set_you_tube_api_access_tokens(
                    serde_json::to_string(&tokens).expect("OAuth tokens always serialize"),
                )
                .await;

            (tokens, false)
        } else {
            (
                serde_json::from_str(&settings.you_tube_api_access_tokens)
                    .context("parse YouTube access token")?,
                true,
            )
        };

        // ==============================================================================
        // Token Refresh Strategy for Long-Running Plugin
        // ==============================================================================
        // For long-running plugins, we proactively refresh all old tokens to ensure
        // they have maximum lifetime. Fresh tokens are validated after refresh to
        // confirm they work correctly.
        let mut yt_clients = Vec::new();
        let mut refreshed_tokens = Vec::new();

        for token in tokens {
            let final_token = if is_old {
                // Always refresh old tokens proactively for long-running plugin
                tracing::info!("proactively refreshing old token for maximum lifetime");

                let mut token = Token::from_expired_token(token);

                if token
                    .refresh(&oauth_manager)
                    .await
                    .context("refresh token")?
                {
                    tracing::debug!("successfully refreshed old token");
                } else {
                    // Refresh failed - fall back to full re-authentication
                    outgoing
                        .notify(
                            CreateNotificationCommand::builder()
                                .notification_id("ytl_reauth")
                                .title("Check your browser")
                                .message(
                                    "YouTube token refresh failed. \
                                        You need to re-authenticate to YouTube.",
                                )
                                .build()
                                .unwrap(),
                        )
                        .await;

                    tracing::warn!("token refresh failed, getting new token via full OAuth");
                    let raw_token = oauth_manager
                        .authenticate()
                        .await
                        .context("authorize user to YouTube")?;

                    token = Token::from_fresh_token(raw_token);
                }

                token
            } else {
                // Token is fresh from this session, use as-is
                Token::from_fresh_token(token)
            };

            // Create client with refreshed/fresh token and shared OAuth manager
            refreshed_tokens.push(final_token.raw_token().clone());
            let client = YouTubeClient::new(final_token, oauth_manager.clone());
            yt_clients.push(client);
        }

        // ==============================================================================
        // Token Validation After Refresh
        // ==============================================================================
        // Now that all tokens are fresh, validate them to ensure they work correctly.
        // Any validation failures at this point indicate serious issues.
        for client in &yt_clients {
            let is_valid = client
                .validate_token()
                .await
                .context("validate refreshed YouTube token")?;

            if !is_valid {
                eyre::bail!("freshly refreshed YouTube token failed validation");
            }
        }

        // Update stored tokens with the refreshed ones
        outgoing
            .set_you_tube_api_access_tokens(
                serde_json::to_string(&refreshed_tokens).expect("OAuth tokens always serialize"),
            )
            .await;

        // ==============================================================================
        // Multi-Channel Client Setup
        // ==============================================================================
        // Each valid token may correspond to multiple YouTube channels.
        // We build a mapping from channel ID to authenticated client for efficient
        // action routing. This allows users to manage multiple channels from a single
        // TouchPortal plugin instance.
        let mut client_by_channel = HashMap::new();
        for client in yt_clients {
            let channels_stream = client.list_my_channels();
            let mut channels_stream = std::pin::pin!(channels_stream);
            while let Some(channel) = channels_stream.next().await {
                let channel = channel.context("fetch channel")?;
                client_by_channel.insert(
                    channel.id.clone(),
                    Channel {
                        name: channel.snippet.title,
                        yt: client.clone(),
                    },
                );
            }
        }

        // TODO: keep a state that reflects the current stream state for every known stream?

        // TODO: event when a stream becomes active or inactive

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
        .with_ansi(false)
        .init();

    // when run without arguments, we're running as a plugin
    if std::env::args().len() == 1 {
        Plugin::run_dynamic("127.0.0.1:12136").await?;
    } else {
        let mut tokens = String::new();
        if tokio::fs::try_exists("tokens.json").await.unwrap() {
            tokens = tokio::fs::read_to_string("tokens.json").await.unwrap();
        }
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let plugin = Plugin::new(
            PluginSettings {
                you_tube_api_access_tokens: tokens,
            },
            TouchPortalHandle(tx),
            serde_json::from_value(serde_json::json!({
                "sdkVersion": ApiVersion::V4_3,
                "tpVersionString": "4.4",
                "tpVersionCode": 4044,
                "pluginVersion": 1,
            }))
            .context("fake InfoMessage")?,
        )
        .await?;
        // Collect all tokens asynchronously
        let mut tokens = Vec::new();
        for channel in plugin.yt.values() {
            tokens.push(channel.yt.token().await);
        }
        let json = serde_json::to_string(&tokens).unwrap();
        tokio::fs::write("tokens.json", &json).await.unwrap();
    }

    Ok(())
}
