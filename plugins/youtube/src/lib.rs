#![allow(dead_code)]

use crate::youtube_api::client::{TimeBoundAccessToken, YouTubeClient};
use eyre::Context;
use oauth2::basic::BasicTokenResponse;
use std::collections::HashMap;
use std::ops::AsyncFnMut;
use tokio_stream::StreamExt;
pub mod oauth;
pub mod youtube_api;

#[derive(Debug, Clone)]
pub struct Channel {
    pub name: String,
    pub yt: YouTubeClient,
}

/// Complete token setup for both plugin and CLI
/// Handles token acquisition, refresh, validation, and channel mapping
/// Returns (channel_mapping, refreshed_tokens)
pub async fn setup_youtube_clients<F>(
    stored_tokens: &str,
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
    let oauth_manager = oauth::OAuthManager::new();

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
        let client = YouTubeClient::new(final_token, oauth_manager);
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

    Ok((client_by_channel, refreshed_tokens))
}
