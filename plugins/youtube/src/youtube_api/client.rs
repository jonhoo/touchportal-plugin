//! Core YouTube API client functionality and authentication management.

use crate::oauth::OAuthManager;
use crate::youtube_api::chat::LiveChatStream;
use crate::youtube_api::{
    broadcasts::LiveBroadcastListResponse,
    broadcasts::{BroadcastStatus, CuepointRequest, LiveBroadcast, LiveBroadcastUpdateRequest},
    channels::Channel,
    channels::ChannelListResponse,
    chat::LiveChatMessage,
    streams::LiveStream,
    streams::LiveStreamListResponse,
    types::PagedStream,
    videos::{Video, VideoListResponse},
};
use eyre::Context;
use http::Method;
use oauth2::TokenResponse;
use oauth2::basic::BasicTokenResponse;
use serde::Serialize;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::Mutex;
use tokio_stream::Stream;
use tracing::instrument;

#[derive(Debug, Clone)]
pub struct TimeBoundAccessToken {
    /// The current OAuth2 token, protected by a mutex for thread-safe refresh operations
    token: BasicTokenResponse,
    /// When the current access token expires (with safety buffer)
    expires_at: SystemTime,
}

impl TimeBoundAccessToken {
    /// Creates a new YouTube token that is already expired, forcing immediate refresh.
    ///
    /// This is useful when loading tokens from storage where you want to ensure
    /// they are validated before use.
    pub fn expired(token: BasicTokenResponse) -> Self {
        Self {
            expires_at: SystemTime::UNIX_EPOCH,
            token,
        }
    }

    /// Creates a new YouTube token with calculated expiry time.
    ///
    /// The expiry time is calculated from the token's `expires_in` field minus
    /// a 5-minute safety buffer to prevent edge-case failures.
    pub fn new(token: BasicTokenResponse) -> Self {
        Self {
            expires_at: Self::calculate_token_expiry(&token),
            token,
        }
    }

    pub fn raw_token(&self) -> &BasicTokenResponse {
        &self.token
    }

    /// Refreshes this token using the provided OAuth manager, preserving the refresh token.
    ///
    /// This method handles the entire refresh flow internally, ensuring the refresh token
    /// is never lost during the process.
    ///
    /// # Returns
    ///
    /// * `Ok(true)` - Token was successfully refreshed
    /// * `Ok(false)` - Refresh failed (invalid grant, no refresh token, etc.)
    /// * `Err(_)` - Network or other error occurred
    ///
    pub async fn refresh(
        &mut self,
        oauth_manager: &crate::oauth::OAuthManager,
    ) -> eyre::Result<bool> {
        tracing::trace!("refreshing token");
        match oauth_manager
            .refresh_token(self.token.clone())
            .await
            .context("refresh OAuth token")?
        {
            Some(new_token) => {
                let old_token = std::mem::replace(&mut self.token, new_token);

                // If the new token doesn't have a refresh token, preserve the original one
                if self.token.refresh_token().is_none() {
                    tracing::trace!("new token lacks refresh token, preserving original");
                    self.token
                        .set_refresh_token(old_token.refresh_token().cloned());
                } else {
                    tracing::debug!("new token includes refresh token");
                }

                // Update the token expiry time
                self.expires_at = Self::calculate_token_expiry(&self.token);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Calculates when a token should be considered expired based on its expires_in field.
    ///
    /// Uses the current time + expires_in duration - 5 minute safety buffer.
    /// If no expires_in is provided, assumes a conservative 55-minute lifetime.
    fn calculate_token_expiry(token: &BasicTokenResponse) -> SystemTime {
        let now = SystemTime::now();
        if let Some(expires_in) = token.expires_in() {
            now + expires_in - Duration::from_secs(300) // 5 minute buffer
        } else {
            // If no expires_in field, assume 1 hour minus buffer (conservative default)
            now + Duration::from_secs(3300) // 55 minutes
        }
    }
}

/// Client for interacting with the YouTube Data API v3.
///
/// This client wraps an OAuth2 token and provides methods to call various YouTube API endpoints.
/// All API calls require a valid OAuth2 access token with appropriate scopes.
///
/// The client automatically refreshes expired access tokens before API calls using the stored
/// refresh token and OAuth manager. Token expiry is tracked based on the `expires_in` field
/// from the OAuth response, with a safety buffer to prevent edge-case failures.
#[derive(Debug, Clone)]
pub struct YouTubeClient {
    /// The current OAuth2 token.
    token: Arc<Mutex<TimeBoundAccessToken>>,
    /// OAuth manager for refreshing tokens (shared across clients)
    oauth_manager: Arc<OAuthManager>,
    /// HTTP client for API requests
    client: reqwest::Client,
}

impl YouTubeClient {
    /// Creates a new YouTube API client with the provided OAuth2 token, OAuth manager, and HTTP client.
    ///
    /// The token expiry time is calculated from when the token was created plus the `expires_in`
    /// duration minus a 5-minute safety buffer to prevent edge-case failures.
    ///
    /// # Arguments
    ///
    /// * `token` - A valid [`BasicTokenResponse`] containing the OAuth2 access token
    /// * `oauth_manager` - Shared OAuth manager for token refresh operations
    /// * `client` - Shared HTTP client for making API requests
    pub fn new(
        token: TimeBoundAccessToken,
        oauth_manager: Arc<OAuthManager>,
        client: reqwest::Client,
    ) -> Self {
        Self {
            token: Arc::new(Mutex::new(token)),
            oauth_manager,
            client,
        }
    }

    /// Returns a clone of the underlying OAuth2 token.
    ///
    /// This is useful when you need to extract the token for storage or
    /// passing to another component. Since the token is protected by a mutex,
    /// this method is async.
    pub async fn token(&self) -> BasicTokenResponse {
        self.token.lock().await.token.clone()
    }

    /// Returns a reference to the underlying HTTP client.
    ///
    /// This is useful for specialized requests that need direct access to the HTTP client,
    /// such as the live chat streaming functionality.
    pub(crate) fn http_client(&self) -> &reqwest::Client {
        &self.client
    }

    /// Gets a guaranteed-fresh access token, refreshing if necessary.
    ///
    /// This method is called automatically before each API request to ensure the token
    /// is valid. It checks if the token expires within the safety buffer and refreshes
    /// it if needed.
    ///
    /// # Returns
    ///
    /// * `Ok(token)` - A guaranteed-fresh access token
    /// * `Err(_)` - Token refresh failed or network error occurred
    ///
    #[instrument(skip(self), ret)]
    pub(crate) async fn fresh_access_token(&self) -> eyre::Result<String> {
        let mut token = self.token.lock().await;
        let now = SystemTime::now();

        if now >= token.expires_at {
            tracing::debug!("access token expired, attempting refresh");

            // Token needs refresh
            if token.refresh(&self.oauth_manager).await? {
                tracing::debug!("access token successfully refreshed");
            } else {
                tracing::error!("access token refresh failed, client is unusable");
                return Err(eyre::eyre!("Unable to refresh expired access token"));
            }
        }

        // Return the guaranteed-fresh access token
        Ok(token.token.access_token().secret().to_string())
    }

    /// Makes an authenticated HTTP request to the YouTube API with common error handling.
    ///
    /// This method consolidates the shared logic across all YouTube API requests:
    /// - Token freshness validation and refresh
    /// - Authorization header setup
    /// - Request building based on HTTP method
    /// - Query parameters (for both GET and POST requests)
    /// - JSON body (for POST requests that need a body)
    /// - Status code validation and error handling
    ///
    /// # Arguments
    ///
    /// * `method` - The HTTP method to use (GET, POST, etc.)
    /// * `url` - The API endpoint URL
    /// * `query_params` - Optional query parameters
    /// * `json_body` - Optional JSON body for POST requests
    ///
    /// # Returns
    ///
    /// The raw [`reqwest::Response`] for method-specific JSON parsing.
    #[instrument(skip(self, json_body), ret, level = tracing::Level::TRACE)]
    pub(crate) async fn make_authenticated_request(
        &self,
        method: Method,
        url: &str,
        query_params: Option<&[(&str, &str)]>,
        json_body: Option<&impl Serialize>,
    ) -> eyre::Result<reqwest::Response> {
        let access_token = self.fresh_access_token().await?;

        let mut request = self
            .client
            .request(method.clone(), url)
            .header("Authorization", format!("Bearer {}", access_token));

        // Add query parameters if provided
        if let Some(params) = query_params {
            request = request.query(params);
        }

        // Add JSON body and content-type if provided
        if let Some(body) = json_body {
            request = request
                .header("Content-Type", "application/json")
                .json(body);
        }

        let response = request
            .send()
            .await
            .with_context(|| format!("send {} request to YouTube API: {}", method, url))?;

        let status_code = response.status();
        if !status_code.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            return Err(eyre::eyre!(
                "YouTube API {} request failed with status {}: {}",
                method,
                status_code,
                error_text
            ));
        }

        Ok(response)
    }

    /// Validates the OAuth2 token by making a test API call to the YouTube Data API.
    ///
    /// This method first ensures the token is fresh (auto-refresh if needed), then makes
    /// a minimal call to [`Self::list_live_broadcasts_internal`] with `max_results=1`
    /// to test if the token is still valid and has the required scopes.
    ///
    /// # Returns
    ///
    /// * `Ok(true)` - Token is valid and can be used for API calls
    /// * `Ok(false)` - Token is invalid or refresh failed
    /// * `Err(_)` - Network or other error occurred during validation
    #[instrument(skip(self), ret)]
    pub async fn validate_token(&self) -> eyre::Result<bool> {
        match self.list_live_broadcasts_internal(1, None).await {
            Ok(_) => {
                tracing::debug!("YouTube API token validation successful");
                Ok(true)
            }
            Err(e) => {
                tracing::warn!("YouTube API token validation failed: {}", e);
                Ok(false)
            }
        }
    }

    /// Returns a paginated stream of all YouTube broadcasts for the authenticated user.
    ///
    /// **Broadcasts vs Streams**: A broadcast represents the viewer-facing live streaming event
    /// with metadata like title, description, scheduling, and viewer settings. This is what
    /// users see and interact with on YouTube. Use broadcasts for user-facing operations like
    /// listing, scheduling, and managing live events.
    ///
    /// Uses the `liveBroadcasts.list` API with `mine=true` to fetch all broadcast resources
    /// that belong to the authenticated user, regardless of their status (active, upcoming,
    /// completed, etc.). The stream automatically handles pagination and fetches subsequent
    /// pages as needed.
    ///
    /// **Status Filtering**: To filter broadcasts by status, collect the results and filter
    /// client-side using the `broadcast.status.life_cycle_status` field. The YouTube API
    /// does not support combining `mine=true` with `broadcastStatus` filtering.
    ///
    /// # Returns
    ///
    /// A [`PagedStream`] that yields all [`LiveBroadcast`] resources owned by the user.
    ///
    /// # Required Scopes
    ///
    /// * `https://www.googleapis.com/auth/youtube`
    ///
    /// # API Reference
    ///
    /// <https://developers.google.com/youtube/v3/live/docs/liveBroadcasts/list>
    #[instrument(skip(self))]
    pub fn list_my_live_broadcasts(
        &self,
    ) -> impl Stream<Item = eyre::Result<LiveBroadcast>> + use<'_> {
        PagedStream::new(|page_token| async {
            let response = self.list_live_broadcasts_internal(50, page_token).await?;
            Ok((response.items, response.next_page_token))
        })
    }

    /// Changes the status of a YouTube live broadcast and initiates processes associated with the new status.
    ///
    /// Uses the `liveBroadcasts.transition` API to transition a broadcast between different states
    /// like testing, live, or complete.
    ///
    /// # Arguments
    ///
    /// * `broadcast_id` - The unique ID of the broadcast to transition
    /// * `status` - The new [`BroadcastStatus`] to transition to
    ///
    /// # Returns
    ///
    /// The updated [`LiveBroadcast`] resource after the transition, or an error if the transition fails.
    ///
    /// # Required Scopes
    ///
    /// * `https://www.googleapis.com/auth/youtube`
    /// * `https://www.googleapis.com/auth/youtube.force-ssl`
    ///
    /// # API Reference
    ///
    /// <https://developers.google.com/youtube/v3/live/docs/liveBroadcasts/transition>
    #[instrument(skip(self), ret)]
    pub async fn transition_live_broadcast(
        &self,
        broadcast_id: &str,
        status: BroadcastStatus,
    ) -> eyre::Result<LiveBroadcast> {
        let url = "https://www.googleapis.com/youtube/v3/liveBroadcasts/transition";
        let status_string = serde_json::to_string(&status)
            .context("serialize broadcast status")?
            .trim_matches('"')
            .to_string(); // Remove JSON quotes for query param

        let query_params = [
            ("part", "id,snippet,status"),
            ("id", broadcast_id),
            ("broadcastStatus", &status_string),
        ];

        let response = self
            .make_authenticated_request(Method::POST, url, Some(&query_params), None::<&()>)
            .await?;

        let broadcast: LiveBroadcast = response
            .json()
            .await
            .context("parse YouTube API transition response as JSON")?;

        tracing::debug!(
            broadcast_id = broadcast.id,
            "successfully transitioned broadcast"
        );

        Ok(broadcast)
    }

    /// Updates a YouTube live broadcast with new title and/or description.
    ///
    /// Uses the `liveBroadcasts.update` API to modify broadcast title and description.
    /// Only the fields specified in the update request will be modified; unspecified
    /// fields will retain their current values.
    ///
    /// # Arguments
    ///
    /// * `update_request` - The [`LiveBroadcastUpdateRequest`] containing the fields to update
    ///
    /// # Returns
    ///
    /// The updated [`LiveBroadcast`] resource after the changes, or an error if the update fails.
    ///
    /// # Required Scopes
    ///
    /// * `https://www.googleapis.com/auth/youtube`
    /// * `https://www.googleapis.com/auth/youtube.force-ssl`
    ///
    /// # API Reference
    ///
    /// <https://developers.google.com/youtube/v3/live/docs/liveBroadcasts/update>
    #[instrument(skip(self), ret)]
    pub async fn update_live_broadcast(
        &self,
        update_request: &LiveBroadcastUpdateRequest,
    ) -> eyre::Result<LiveBroadcast> {
        let url = "https://www.googleapis.com/youtube/v3/liveBroadcasts";
        let query_params = [("part", "id,snippet,status")];

        let response = self
            .make_authenticated_request(Method::PUT, url, Some(&query_params), Some(update_request))
            .await?;

        let broadcast: LiveBroadcast = response
            .json()
            .await
            .context("parse YouTube API update response as JSON")?;

        tracing::debug!(
            broadcast_id = broadcast.id,
            "successfully updated broadcast"
        );

        Ok(broadcast)
    }

    /// Inserts a cuepoint into a live broadcast.
    ///
    /// Uses the `liveBroadcasts.cuepoint` API to insert cuepoints that might trigger
    /// ad breaks or other events during a live stream.
    ///
    /// # Arguments
    ///
    /// * `broadcast_id` - The ID of the actively streaming broadcast
    /// * `cuepoint` - The [`CuepointRequest`] containing cuepoint details
    ///
    /// # Returns
    ///
    /// `Ok(())` if the cuepoint was successfully inserted, or an error if the insertion fails.
    ///
    /// # Required Scopes
    ///
    /// * `https://www.googleapis.com/auth/youtube`
    /// * `https://www.googleapis.com/auth/youtube.force-ssl`
    /// * `https://www.googleapis.com/auth/youtubepartner`
    ///
    /// # API Reference
    ///
    /// <https://developers.google.com/youtube/v3/live/docs/liveBroadcasts/cuepoint>
    #[instrument(skip(self), ret)]
    pub async fn insert_cuepoint(
        &self,
        broadcast_id: &str,
        cuepoint: &CuepointRequest,
    ) -> eyre::Result<()> {
        let url = "https://www.googleapis.com/youtube/v3/liveBroadcasts/cuepoint";
        let query_params = [("id", broadcast_id)];

        let _response = self
            .make_authenticated_request(Method::POST, url, Some(&query_params), Some(cuepoint))
            .await?;

        tracing::debug!(
            broadcast_id,
            cue_type = ?cuepoint.cue_type,
            "successfully inserted cuepoint"
        );

        Ok(())
    }

    /// Get a specific live broadcast by ID.
    ///
    /// Uses the `liveBroadcasts.list` API with a specific broadcast ID to fetch
    /// detailed information about a single broadcast, including statistics.
    /// This is more efficient than listing all broadcasts when you only need
    /// information about a specific one.
    ///
    /// # Arguments
    ///
    /// * `broadcast_id` - The ID of the broadcast to retrieve
    ///
    /// # Returns
    ///
    /// The [`LiveBroadcast`] resource if found, or an error if the broadcast
    /// doesn't exist or cannot be accessed.
    ///
    /// # API Cost
    ///
    /// This operation costs 1 quota unit per call.
    ///
    /// See: <https://developers.google.com/youtube/v3/live/docs/liveBroadcasts/list>
    pub async fn get_live_broadcast(&self, broadcast_id: &str) -> eyre::Result<LiveBroadcast> {
        let response = self.get_live_broadcast_internal(broadcast_id).await?;

        // Extract the first (and should be only) broadcast from the response
        if let Some(broadcast) = response.items.into_iter().next() {
            Ok(broadcast)
        } else {
            Err(eyre::eyre!("broadcast not found: {}", broadcast_id))
        }
    }

    /// Returns a paginated stream of live streams for the authenticated user.
    ///
    /// **Broadcasts vs Streams**: A stream represents the technical video pipeline that sends
    /// content to YouTube servers. It contains encoder settings, ingestion URLs, CDN configuration,
    /// and technical metadata. Streams are the "behind-the-scenes" infrastructure that powers
    /// broadcasts. Use streams for technical operations like configuring encoders, monitoring
    /// stream health, or managing ingestion settings.
    ///
    /// **Note**: For user-facing operations like listing live events or showing titles/descriptions,
    /// use [`Self::list_my_live_broadcasts`] instead. Streams can be reused across multiple broadcasts.
    ///
    /// Uses the `liveStreams.list` API to fetch stream resources
    /// that belong to the authenticated user. The stream automatically handles
    /// pagination and fetches subsequent pages as needed.
    ///
    /// # Returns
    ///
    /// A [`PagedStream`] that yields [`LiveStream`] resources.
    ///
    /// # Required Scopes
    ///
    /// * `https://www.googleapis.com/auth/youtube.readonly`
    /// * `https://www.googleapis.com/auth/youtube`
    /// * `https://www.googleapis.com/auth/youtube.force-ssl`
    ///
    /// # API Reference
    ///
    /// <https://developers.google.com/youtube/v3/live/docs/liveStreams/list>
    #[instrument(skip(self))]
    pub fn list_my_live_streams(&self) -> impl Stream<Item = eyre::Result<LiveStream>> + use<'_> {
        PagedStream::new(|page_token| async {
            let response = self.list_live_streams_internal(50, page_token).await?;
            Ok((response.items, response.next_page_token))
        })
    }

    /// Returns a paginated stream of YouTube channels owned by the authenticated user.
    ///
    /// Uses the `channels.list` API with `mine=true` to fetch channel resources
    /// that belong to the authenticated user. This typically returns one channel
    /// for personal accounts, but may return multiple channels for content creators
    /// or organizations with multiple channels. The stream automatically handles
    /// pagination and fetches subsequent pages as needed.
    ///
    /// # Returns
    ///
    /// A [`PagedStream`] that yields [`Channel`] resources owned by the authenticated user.
    ///
    /// # Required Scopes
    ///
    /// * `https://www.googleapis.com/auth/youtube.readonly`
    /// * `https://www.googleapis.com/auth/youtube`
    /// * `https://www.googleapis.com/auth/youtube.force-ssl`
    ///
    /// # API Reference
    ///
    /// <https://developers.google.com/youtube/v3/docs/channels/list>
    #[instrument(skip(self))]
    pub fn list_my_channels(&self) -> impl Stream<Item = eyre::Result<Channel>> + use<'_> {
        PagedStream::new(|page_token| async {
            let response = self.list_channels_internal(50, page_token).await?;
            Ok((response.items, response.next_page_token))
        })
    }

    /// Gets statistics and live streaming details for a single YouTube video by its ID.
    ///
    /// Uses the `videos.list` API to fetch statistics and live streaming information for the specified video.
    /// Returns view count, like count, comment count, concurrent viewers (for live videos), and other engagement metrics.
    ///
    /// # Arguments
    ///
    /// * `video_id` - The YouTube video ID to get statistics for
    ///
    /// # Returns
    ///
    /// A [`Video`] resource containing the video's statistics and live streaming details (if applicable),
    /// or an error if the video is not found or not accessible.
    ///
    /// # Required Scopes
    ///
    /// * `https://www.googleapis.com/auth/youtube.readonly`
    /// * `https://www.googleapis.com/auth/youtube`
    /// * `https://www.googleapis.com/auth/youtube.force-ssl`
    ///
    /// # API Reference
    ///
    /// <https://developers.google.com/youtube/v3/docs/videos/list>
    #[instrument(skip(self), ret)]
    pub async fn get_video_metadata(&self, video_id: &str) -> eyre::Result<Video> {
        let url = "https://www.googleapis.com/youtube/v3/videos";
        let query_params = [
            ("part", "statistics,liveStreamingDetails,snippet"),
            ("id", video_id),
        ];

        let response = self
            .make_authenticated_request(Method::GET, url, Some(&query_params), None::<&()>)
            .await?;

        let videos: VideoListResponse = response
            .json()
            .await
            .context("parse YouTube videos API response as JSON")?;

        tracing::debug!(
            video_id,
            returned_items = videos.items.len(),
            "fetched video statistics"
        );

        videos
            .items
            .into_iter()
            .next()
            .ok_or_else(|| eyre::eyre!("video not found: {}", video_id))
    }

    /// Returns a continuous stream of live chat messages for the specified chat.
    ///
    /// This method uses the YouTube Live Chat Messages `streamList` API to provide
    /// real-time streaming of chat messages with low latency. The stream will first
    /// return recent chat history, then continuously yield new messages as they arrive.
    ///
    /// The stream handles server-streaming with automatic reconnection on failures,
    /// respecting the `pollingIntervalMillis` provided by the YouTube API to avoid
    /// overwhelming the servers.
    ///
    /// # Arguments
    ///
    /// * `live_chat_id` - The ID of the live chat to stream messages from
    ///
    /// # Returns
    ///
    /// A [`Stream`] that yields [`LiveChatMessage`] resources in real-time.
    ///
    /// # Required Scopes
    ///
    /// * `https://www.googleapis.com/auth/youtube.readonly`
    /// * `https://www.googleapis.com/auth/youtube`
    /// * `https://www.googleapis.com/auth/youtube.force-ssl`
    ///
    /// # API Reference
    ///
    /// <https://developers.google.com/youtube/v3/live/docs/liveChatMessages/streamList>
    #[instrument(skip(self))]
    pub fn stream_live_chat_messages(
        &self,
        live_chat_id: &str,
    ) -> impl Stream<Item = eyre::Result<LiveChatMessage>> + use<'_> {
        LiveChatStream::new(self.clone(), live_chat_id.to_string())
    }

    /// Internal method to call the `liveBroadcasts.list` API with configurable parameters.
    ///
    /// This method handles the actual HTTP request to the YouTube API, including
    /// authentication headers and query parameters. Uses `mine=true` to return
    /// all broadcasts owned by the authenticated user.
    ///
    /// # Arguments
    ///
    /// * `max_results` - Maximum number of broadcasts to return (1-50)
    /// * `page_token` - Optional page token for pagination
    ///
    /// # Returns
    ///
    /// A [`LiveBroadcastListResponse`] containing the API response data.
    ///
    /// # API Reference
    ///
    /// <https://developers.google.com/youtube/v3/live/docs/liveBroadcasts/list>
    async fn get_live_broadcast_internal(
        &self,
        broadcast_id: &str,
    ) -> eyre::Result<LiveBroadcastListResponse> {
        let url = "https://www.googleapis.com/youtube/v3/liveBroadcasts";

        let query_params = vec![
            ("part", "id,snippet,status,statistics"),
            ("id", broadcast_id),
        ];

        let response = self
            .make_authenticated_request(Method::GET, url, Some(&query_params), None::<&()>)
            .await?;

        let live_broadcasts: LiveBroadcastListResponse = response
            .json()
            .await
            .context("parse YouTube API response as JSON")?;

        tracing::debug!(
            broadcast_id = broadcast_id,
            total_results = live_broadcasts.page_info.total_results,
            "fetched broadcast by ID"
        );

        Ok(live_broadcasts)
    }

    /// Internal method to call the `liveBroadcasts.list` API with configurable parameters.
    ///
    /// This method lists broadcasts owned by the authenticated user using the `mine=true` parameter.
    /// Used internally by [`Self::list_my_live_broadcasts`] to handle pagination.
    ///
    /// # Arguments
    ///
    /// * `max_results` - Maximum number of broadcasts to return per page (1-50)
    /// * `page_token` - Token for retrieving a specific page of results
    ///
    /// # Returns
    ///
    /// A [`LiveBroadcastListResponse`] containing the API response data.
    ///
    /// # API Reference
    ///
    /// <https://developers.google.com/youtube/v3/live/docs/liveBroadcasts/list>
    async fn list_live_broadcasts_internal(
        &self,
        max_results: u32,
        page_token: Option<String>,
    ) -> eyre::Result<LiveBroadcastListResponse> {
        let url = "https://www.googleapis.com/youtube/v3/liveBroadcasts";

        let max_results_string = max_results.to_string();
        let mut query_params = vec![
            ("part", "id,snippet,status"),
            ("mine", "true"),
            ("maxResults", max_results_string.as_str()),
        ];

        // Add pageToken if provided
        if let Some(ref token) = page_token {
            query_params.push(("pageToken", token.as_str()));
        }

        let response = self
            .make_authenticated_request(Method::GET, url, Some(&query_params), None::<&()>)
            .await?;

        let live_broadcasts: LiveBroadcastListResponse = response
            .json()
            .await
            .context("parse YouTube API response as JSON")?;

        tracing::debug!(
            total_results = live_broadcasts.page_info.total_results,
            returned_items = live_broadcasts.items.len(),
            "fetched live broadcasts"
        );

        Ok(live_broadcasts)
    }

    /// Internal method to call the `liveStreams.list` API with configurable parameters.
    ///
    /// This method handles the actual HTTP request to the YouTube API, including
    /// authentication headers and query parameters.
    ///
    /// # Arguments
    ///
    /// * `max_results` - Maximum number of streams to return (1-50)
    /// * `page_token` - Optional page token for pagination
    ///
    /// # Returns
    ///
    /// A [`LiveStreamListResponse`] containing the API response data.
    ///
    /// # API Reference
    ///
    /// <https://developers.google.com/youtube/v3/live/docs/liveStreams/list>
    async fn list_live_streams_internal(
        &self,
        max_results: u32,
        page_token: Option<String>,
    ) -> eyre::Result<LiveStreamListResponse> {
        let url = "https://www.googleapis.com/youtube/v3/liveStreams";
        let max_results_string = max_results.to_string();
        let mut query_params = vec![
            ("part", "id,snippet,status"),
            ("mine", "true"),
            ("maxResults", max_results_string.as_str()),
        ];

        // Add pageToken if provided
        if let Some(ref token) = page_token {
            query_params.push(("pageToken", token.as_str()));
        }

        let response = self
            .make_authenticated_request(Method::GET, url, Some(&query_params), None::<&()>)
            .await?;

        let live_streams: LiveStreamListResponse = response
            .json()
            .await
            .context("parse YouTube liveStreams API response as JSON")?;

        tracing::debug!(
            total_results = live_streams.page_info.total_results,
            returned_items = live_streams.items.len(),
            "fetched live streams"
        );

        Ok(live_streams)
    }

    /// Internal method to call the `channels.list` API with configurable parameters.
    ///
    /// This method handles the actual HTTP request to the YouTube API, including
    /// authentication headers and query parameters. It uses the `mine=true` parameter
    /// to retrieve only channels owned by the authenticated user.
    ///
    /// # Arguments
    ///
    /// * `max_results` - Maximum number of channels to return (1-50)
    /// * `page_token` - Optional page token for pagination
    ///
    /// # Returns
    ///
    /// A [`ChannelListResponse`] containing the API response data.
    ///
    /// # API Reference
    ///
    /// <https://developers.google.com/youtube/v3/docs/channels/list>
    async fn list_channels_internal(
        &self,
        max_results: u32,
        page_token: Option<String>,
    ) -> eyre::Result<ChannelListResponse> {
        let url = "https://www.googleapis.com/youtube/v3/channels";
        let max_results_string = max_results.to_string();
        let mut query_params = vec![
            ("part", "id,snippet"),
            ("mine", "true"),
            ("maxResults", max_results_string.as_str()),
        ];

        // Add pageToken if provided
        if let Some(ref token) = page_token {
            query_params.push(("pageToken", token.as_str()));
        }

        let response = self
            .make_authenticated_request(Method::GET, url, Some(&query_params), None::<&()>)
            .await?;

        let channels: ChannelListResponse = response
            .json()
            .await
            .context("parse YouTube channels API response as JSON")?;

        tracing::debug!(
            total_results = channels.page_info.total_results,
            returned_items = channels.items.len(),
            "fetched channels"
        );

        Ok(channels)
    }
}
