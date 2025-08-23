#![allow(dead_code)]

// Include build-generated constants
// Contains: MIN_POLLING_INTERVAL_SECONDS
include!(concat!(env!("OUT_DIR"), "/constants.rs"));

use crate::youtube_api::client::{TimeBoundAccessToken, YouTubeClient};
use eyre::Context;
use oauth2::basic::BasicTokenResponse;
use std::collections::HashMap;
use std::ops::AsyncFnMut;
use std::sync::Arc;
use tokio_stream::StreamExt;
pub mod actions;
pub mod activity;
pub mod background;
pub mod oauth;
pub mod plugin;
pub mod youtube_api;

#[derive(Debug, Clone)]
pub struct Channel {
    pub name: String,
    pub yt: Arc<YouTubeClient>,
}

/// Complete token setup for both plugin and CLI
/// Handles token acquisition, refresh, validation, and channel mapping
/// Returns (channel_mapping, refreshed_tokens)
///
/// TODO(jon): Add quota usage tracking and user education system
/// The YouTube Data API v3 has a daily quota limit of 10,000 units per project.
/// Current plugin usage patterns and costs (as of 2025):
/// - liveChatMessages.list: ~1 unit (but needs polling every 1-2 seconds during active chat)
/// - videos.list for metrics: ~1 unit (polled every 30-600 seconds based on adaptive algorithm)
/// - liveChatMessages.insert (send message): 50 units each (expensive!)
/// - liveChatBans.insert (ban user): 50 units each (expensive!)
/// - liveBroadcasts operations: 50 units each (title/description updates)
///
/// QUOTA EXHAUSTION SCENARIOS:
/// - 150-minute stream with 2-second chat polling = ~4,500 units just for chat monitoring
/// - Add metrics polling every 60 seconds = ~150 units
/// - A few chat messages or bans can easily push over daily limit
///
/// RECOMMENDED IMPROVEMENTS:
/// - Add quota usage estimation and display in TouchPortal states
/// - Show daily usage tracking: "Used 2,847 / 10,000 quota units today"
/// - Warn users when approaching limits: "90% quota used - consider reducing polling frequency"
/// - Implement quota-aware features: disable expensive actions when quota is low
/// - Add user education about quota sharing across all plugin users
/// - Consider implementing quota donation/sharing system for heavy users
pub async fn setup_youtube_clients<F>(
    stored_tokens: &str,
    custom_client_id: Option<String>,
    custom_client_secret: Option<String>,
    mut notify_callback: F,
) -> eyre::Result<(HashMap<String, Channel>, Vec<BasicTokenResponse>)>
where
    F: AsyncFnMut(&str, &str, &str),
{
    // ==============================================================================
    // OAuth Manager Setup
    // ==============================================================================
    // We centralize all OAuth operations through a single manager to maintain
    // consistency in client configuration and error handling across token flows.
    // Custom OAuth credentials are used if provided, otherwise fallback to defaults.
    // Use Arc to share the manager across multiple YouTube clients efficiently.
    let oauth_manager = Arc::new(oauth::OAuthManager::with_custom_credentials(
        custom_client_id,
        custom_client_secret,
    ));

    // ==============================================================================
    // Token Acquisition Strategy
    // ==============================================================================
    // For new users with no stored tokens, we initiate a fresh OAuth flow.
    // The user gets a browser notification to complete the authorization process.
    let (tokens, is_old) = if stored_tokens.is_empty() || stored_tokens == "[]" {
        notify_callback(
            "ytl_auth",
            "Check your browser",
            "You need to authenticate to YouTube to give access to your channel.",
        )
        .await;

        let token = oauth_manager
            .authenticate()
            .await
            .context("authorize user to YouTube")?;
        let tokens = vec![token];
        (tokens, false)
    } else {
        (
            serde_json::from_str(stored_tokens).context("parse YouTube access token")?,
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

            let mut token = TimeBoundAccessToken::expired(token);

            if token
                .refresh(&oauth_manager)
                .await
                .context("refresh token")?
            {
                tracing::debug!("successfully refreshed old token");
            } else {
                // Refresh failed - fall back to full re-authentication
                notify_callback(
                    "ytl_reauth",
                    "Check your browser",
                    "YouTube token refresh failed. You need to re-authenticate to YouTube.",
                )
                .await;

                tracing::warn!("token refresh failed, getting new token via full OAuth");
                let raw_token = oauth_manager
                    .authenticate()
                    .await
                    .context("authorize user to YouTube")?;

                token = TimeBoundAccessToken::new(raw_token);
            }

            token
        } else {
            // Token is fresh from this session, use as-is
            TimeBoundAccessToken::new(token)
        };

        // Create client with refreshed/fresh token and shared OAuth manager
        refreshed_tokens.push(final_token.raw_token().clone());
        let client = YouTubeClient::new(final_token, Arc::clone(&oauth_manager));
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

    // ==============================================================================
    // Multi-Channel Client Setup
    // ==============================================================================
    // Each valid token may correspond to multiple YouTube channels.
    // We build a mapping from channel ID to authenticated client for efficient
    // action routing. This allows users to manage multiple channels from a single
    // TouchPortal plugin instance.
    let mut client_by_channel = HashMap::new();
    for client in yt_clients {
        let client_arc = Arc::new(client);
        let channels_stream = client_arc.list_my_channels();
        let mut channels_stream = std::pin::pin!(channels_stream);
        while let Some(channel) = channels_stream.next().await {
            let channel = channel.context("fetch channel")?;
            client_by_channel.insert(
                channel.id.clone(),
                Channel {
                    name: channel.snippet.title,
                    yt: Arc::clone(&client_arc),
                },
            );
        }
    }

    Ok((client_by_channel, refreshed_tokens))
}
