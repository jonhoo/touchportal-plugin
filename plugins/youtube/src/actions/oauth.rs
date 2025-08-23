use eyre::Context;
use oauth2::{RefreshToken, TokenResponse};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_stream::StreamExt;
use touchportal_sdk::protocol::CreateNotificationCommand;

use crate::youtube_api::client::{TimeBoundAccessToken, YouTubeClient};
use crate::{Channel, oauth};

/// Handle the complex OAuth flow for adding a new YouTube channel
pub async fn handle_add_youtube_channel(
    tp: &mut crate::plugin::TouchPortalHandle,
    yt: &Arc<Mutex<HashMap<String, Channel>>>,
    custom_client_id: Option<String>,
    custom_client_secret: Option<String>,
) -> eyre::Result<()> {
    let oauth_manager = Arc::new(oauth::OAuthManager::with_custom_credentials(
        custom_client_id,
        custom_client_secret,
    ));

    tp.notify(
        CreateNotificationCommand::builder()
            .notification_id("ytl_add_account")
            .title("Check your browser")
            .message(
                "You need to authenticate to YouTube \
                in your browser to add another account.",
            )
            .build()
            .unwrap(),
    )
    .await;

    let new_token = oauth_manager
        .authenticate()
        .await
        .context("authorize additional YouTube account")?;

    let client = YouTubeClient::new(
        TimeBoundAccessToken::new(new_token.clone()),
        Arc::clone(&oauth_manager),
    );

    let is_valid = client
        .validate_token()
        .await
        .context("validate new YouTube token")?;

    if !is_valid {
        eyre::bail!("newly authenticated YouTube token failed validation");
    }

    let client_arc = Arc::new(client);
    let mut new_channel_count = 0;
    let channels_stream = client_arc.list_my_channels();
    let mut channels_stream = std::pin::pin!(channels_stream);
    while let Some(channel) = channels_stream.next().await {
        let channel = channel.context("fetch channel for new account")?;
        let channel_id = channel.id.clone();
        let channel_name = channel.snippet.title.clone();

        // Overwrite any existing entry for this channel ID with the new token
        {
            let mut yt_guard = yt.lock().await;
            yt_guard.insert(
                channel_id.clone(),
                Channel {
                    name: channel_name.clone(),
                    yt: Arc::clone(&client_arc),
                },
            );
        }

        // Send notification that the channel was successfully added
        tp.notify(
            CreateNotificationCommand::builder()
                .notification_id("ytl_channel_added")
                .title("YouTube channel added")
                .message(format!("{channel_name} added"))
                .build()
                .unwrap(),
        )
        .await;

        new_channel_count += 1;
    }

    // Update UI choices
    {
        let yt_guard = yt.lock().await;
        tp.update_choices_in_ytl_channel(
            yt_guard.iter().map(|(id, c)| format!("{} - {id}", c.name)),
        )
        .await;
    }

    // Collect unique tokens by refresh token uniqueness
    let mut seen_refresh_tokens = HashSet::new();
    let mut all_tokens = Vec::new();

    {
        let yt_guard = yt.lock().await;
        for channel in yt_guard.values() {
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
