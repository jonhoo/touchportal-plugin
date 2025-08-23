use eyre::Context;
use oauth2::{RefreshToken, TokenResponse};
use std::collections::{HashMap, HashSet};
use tokio_stream::StreamExt;
use touchportal_sdk::protocol::CreateNotificationCommand;

use crate::youtube_api::client::{TimeBoundAccessToken, YouTubeClient};
use crate::{Channel, oauth};

/// Handle the complex OAuth flow for adding a new YouTube channel
pub async fn handle_add_youtube_channel(
    tp: &mut crate::plugin::TouchPortalHandle,
    yt: &mut HashMap<String, Channel>,
) -> eyre::Result<()> {
    let oauth_manager = oauth::OAuthManager::new();

    tp.notify(
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

    let client = YouTubeClient::new(TimeBoundAccessToken::new(new_token.clone()), oauth_manager);

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
        yt.insert(
            channel_id.clone(),
            Channel {
                name: channel_name,
                yt: client.clone(),
            },
        );

        new_channel_count += 1;
    }

    tp.update_choices_in_ytl_channel(yt.iter().map(|(id, c)| format!("{} - {id}", c.name)))
        .await;

    // Collect unique tokens by refresh token uniqueness
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

    tp.set_you_tube_api_access_tokens(
        serde_json::to_string(&all_tokens).expect("OAuth tokens always serialize"),
    )
    .await;

    tracing::info!(
        channel_count = new_channel_count,
        "successfully added new YouTube account"
    );

    Ok(())
}
