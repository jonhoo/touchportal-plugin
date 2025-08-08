use eyre::Context;
use oauth2::TokenResponse;
use oauth2::basic::BasicTokenResponse;
use serde::{Deserialize, Serialize};
use tracing::instrument;

/// Client for interacting with the YouTube Data API v3.
///
/// This client wraps an OAuth2 token and provides methods to call various YouTube API endpoints.
/// All API calls require a valid OAuth2 access token with appropriate scopes.
#[derive(Debug, Clone)]
pub struct YouTubeClient {
    token: BasicTokenResponse,
    client: reqwest::Client,
}

/// Response structure for the `liveBroadcasts.list` API call.
///
/// Contains a list of [`LiveBroadcast`] resources that match the request criteria,
/// along with pagination information in [`PageInfo`].
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveBroadcasts/list>
#[derive(Debug, Serialize, Deserialize)]
struct LiveBroadcastListResponse {
    /// Identifies the API resource's type.
    ///
    /// The value will be `youtube#liveBroadcastListResponse`.
    kind: String,
    /// A list of broadcasts that match the request criteria.
    items: Vec<LiveBroadcast>,
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
}

/// A `liveBroadcast` resource represents an event that will be streamed, via live video, on YouTube.
///
/// Each broadcast corresponds to exactly one YouTube video and contains an `id` and
/// basic details in the [`LiveBroadcastSnippet`].
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveBroadcasts#resource>
#[derive(Debug, Serialize, Deserialize)]
pub struct LiveBroadcast {
    /// The ID that YouTube assigns to uniquely identify the broadcast.
    id: String,
    /// Contains basic details about the broadcast.
    ///
    /// Includes the broadcast's title, description, and thumbnail images.
    snippet: LiveBroadcastSnippet,
}

/// The snippet object contains basic details about the broadcast.
///
/// This is a subset of the full snippet data available from the YouTube API,
/// containing only the fields currently needed by this implementation.
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveBroadcasts#snippet>
#[derive(Debug, Serialize, Deserialize)]
struct LiveBroadcastSnippet {
    /// The broadcast's title.
    ///
    /// Note that the broadcast represents exactly one YouTube video.
    title: String,
    /// The date and time that the broadcast is scheduled to start.
    ///
    /// The value is specified in ISO 8601 format.
    #[serde(rename = "scheduledStartTime")]
    scheduled_start_time: Option<String>,
}

/// Paging details for lists of resources.
///
/// Includes the total number of items available and the number of resources
/// returned in a single page response.
///
/// See: <https://developers.google.com/youtube/v3/docs/pageInfo>
#[derive(Debug, Serialize, Deserialize)]
struct PageInfo {
    /// The total number of results in the result set.
    #[serde(rename = "totalResults")]
    total_results: u32,
    /// The number of results included in the API response.
    #[serde(rename = "resultsPerPage")]
    results_per_page: u32,
}

/// Status values for live broadcast transitions.
///
/// Used with the `liveBroadcasts.transition` API to change broadcast state.
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveBroadcasts/transition>
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BroadcastStatus {
    /// Start broadcast testing mode.
    Testing,
    /// Make broadcast visible to audience.
    Live,
    /// Mark broadcast as complete/over.
    Complete,
}

/// Filter values for listing live broadcasts by status.
///
/// Used with the `liveBroadcasts.list` API to filter broadcasts.
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveBroadcasts/list>
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BroadcastStatusFilter {
    /// Current live broadcasts.
    Active,
    /// All broadcasts.
    All,
    /// Ended broadcasts.
    Completed,
    /// Broadcasts not yet started.
    Upcoming,
}

/// The type of cuepoint that can be inserted into a live broadcast.
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveBroadcasts/cuepoint>
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CueType {
    /// Advertisement cuepoint that may trigger an ad break.
    #[serde(rename = "cueTypeAd")]
    CueTypeAd,
}

/// Request body for inserting a cuepoint into a live broadcast.
///
/// Used with the `liveBroadcasts.cuepoint` API to trigger ad breaks or other cuepoints.
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveBroadcasts/cuepoint>
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CuepointRequest {
    /// The type of cuepoint to insert.
    pub cue_type: CueType,
    /// Duration of the cuepoint in seconds.
    ///
    /// Defaults to 30 seconds if not specified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<u32>,
    /// Time offset for cuepoint insertion in milliseconds.
    ///
    /// Cannot be used together with [`Self::walltime_ms`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insertion_offset_time_ms: Option<u64>,
    /// Specific wall clock time for insertion in milliseconds.
    ///
    /// Cannot be used together with [`Self::insertion_offset_time_ms`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub walltime_ms: Option<u64>,
}

impl CuepointRequest {
    /// Creates a new ad cuepoint request with default 30-second duration.
    ///
    /// This is a convenience method for the most common cuepoint type.
    ///
    /// # Returns
    ///
    /// A [`CuepointRequest`] configured for ad insertion with default settings.
    pub fn ad_cuepoint() -> Self {
        Self {
            cue_type: CueType::CueTypeAd,
            duration_secs: Some(30),
            insertion_offset_time_ms: None,
            walltime_ms: None,
        }
    }

    /// Creates a new ad cuepoint request with custom duration.
    ///
    /// # Arguments
    ///
    /// * `duration_secs` - Duration of the ad break in seconds
    ///
    /// # Returns
    ///
    /// A [`CuepointRequest`] configured for ad insertion with the specified duration.
    pub fn ad_cuepoint_with_duration(duration_secs: u32) -> Self {
        Self {
            cue_type: CueType::CueTypeAd,
            duration_secs: Some(duration_secs),
            insertion_offset_time_ms: None,
            walltime_ms: None,
        }
    }

    /// Sets the insertion offset time for this cuepoint.
    ///
    /// # Arguments
    ///
    /// * `offset_ms` - Time offset for cuepoint insertion in milliseconds
    ///
    /// # Returns
    ///
    /// Self with the insertion offset time set.
    pub fn with_insertion_offset(mut self, offset_ms: u64) -> Self {
        self.insertion_offset_time_ms = Some(offset_ms);
        self.walltime_ms = None; // Clear walltime if set
        self
    }

    /// Sets the wall clock time for this cuepoint.
    ///
    /// # Arguments
    ///
    /// * `walltime_ms` - Specific wall clock time for insertion in milliseconds
    ///
    /// # Returns
    ///
    /// Self with the wall clock time set.
    pub fn with_walltime(mut self, walltime_ms: u64) -> Self {
        self.walltime_ms = Some(walltime_ms);
        self.insertion_offset_time_ms = None; // Clear offset if set
        self
    }
}

/// Response structure for the `liveStreams.list` API call.
///
/// Contains a list of [`LiveStream`] resources that match the request criteria,
/// along with pagination information in [`PageInfo`].
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveStreams/list>
#[derive(Debug, Serialize, Deserialize)]
struct LiveStreamListResponse {
    /// Identifies the API resource's type.
    ///
    /// The value will be `youtube#liveStreamListResponse`.
    kind: String,
    /// A list of live streams that match the request criteria.
    items: Vec<LiveStream>,
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
}

/// A `liveStream` resource represents the encoder settings, ingestion type, and video stream.
///
/// Contains configuration details for the live video stream including CDN settings
/// and stream status information.
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveStreams#resource>
#[derive(Debug, Serialize, Deserialize)]
pub struct LiveStream {
    /// The ID that YouTube assigns to uniquely identify the stream.
    id: String,
    /// Contains basic details about the stream.
    ///
    /// Includes the stream's title and description.
    snippet: LiveStreamSnippet,
    /// Contains information about the stream's status.
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<LiveStreamStatus>,
}

/// The snippet object contains basic details about the stream.
///
/// This is a subset of the full snippet data available from the YouTube API,
/// containing only the fields currently needed by this implementation.
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveStreams#snippet>
#[derive(Debug, Serialize, Deserialize)]
struct LiveStreamSnippet {
    /// The stream's title.
    title: String,
    /// The stream's description.
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

/// The status of a live stream.
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveStreams#status>
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StreamStatus {
    /// The stream is receiving data.
    Active,
    /// The stream exists but lacks valid CDN settings.
    Created,
    /// An error condition exists on the stream.
    Error,
    /// The stream is not receiving data.
    Inactive,
    /// The stream has valid CDN settings.
    Ready,
}

/// Contains information about the live stream's status.
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveStreams#status>
#[derive(Debug, Serialize, Deserialize)]
struct LiveStreamStatus {
    /// The stream's status.
    #[serde(rename = "streamStatus")]
    stream_status: StreamStatus,
}

impl YouTubeClient {
    /// Creates a new YouTube API client with the provided OAuth2 token.
    ///
    /// # Arguments
    ///
    /// * `token` - A valid [`BasicTokenResponse`] containing the OAuth2 access token
    pub fn new(token: BasicTokenResponse) -> Self {
        let client = reqwest::Client::new();
        Self { token, client }
    }

    /// Consumes the client and returns the underlying OAuth2 token.
    ///
    /// This is useful when you need to extract the token for storage or
    /// passing to another component.
    pub fn into_token(self) -> BasicTokenResponse {
        self.token
    }

    /// Validates the OAuth2 token by making a test API call to the YouTube Data API.
    ///
    /// Makes a minimal call to [`Self::list_live_broadcasts_internal`] with `max_results=1`
    /// to test if the token is still valid and has the required scopes.
    ///
    /// # Returns
    ///
    /// * `Ok(true)` - Token is valid and can be used for API calls
    /// * `Ok(false)` - Token is invalid or expired
    /// * `Err(_)` - Network or other error occurred during validation
    #[instrument(skip(self), ret)]
    pub async fn validate_token(&self) -> eyre::Result<bool> {
        let result = dbg!(self.list_live_broadcasts_internal(1, None).await);
        match result {
            Ok(_) => {
                tracing::info!("YouTube API token validation successful");
                Ok(true)
            }
            Err(e) => {
                tracing::warn!("YouTube API token validation failed: {}", e);
                Ok(false)
            }
        }
    }

    /// Returns a list of YouTube broadcasts for the authenticated user.
    ///
    /// Uses the `liveBroadcasts.list` API to fetch up to 50 broadcast resources
    /// that belong to the authenticated user.
    ///
    /// # Returns
    ///
    /// A vector of [`LiveBroadcast`] resources, or an error if the API call fails.
    ///
    /// # Required Scopes
    ///
    /// * `https://www.googleapis.com/auth/youtube`
    ///
    /// # API Reference
    ///
    /// <https://developers.google.com/youtube/v3/live/docs/liveBroadcasts/list>
    #[instrument(skip(self), ret)]
    pub async fn list_live_broadcasts(&self) -> eyre::Result<Vec<LiveBroadcast>> {
        let response = self.list_live_broadcasts_internal(50, None).await?;
        Ok(response.items)
    }

    /// Returns a list of YouTube broadcasts filtered by status for the authenticated user.
    ///
    /// Uses the `liveBroadcasts.list` API to fetch broadcast resources with a specific status
    /// that belong to the authenticated user.
    ///
    /// # Arguments
    ///
    /// * `status_filter` - The [`BroadcastStatusFilter`] to apply
    ///
    /// # Returns
    ///
    /// A vector of [`LiveBroadcast`] resources matching the filter, or an error if the API call fails.
    ///
    /// # Required Scopes
    ///
    /// * `https://www.googleapis.com/auth/youtube`
    ///
    /// # API Reference
    ///
    /// <https://developers.google.com/youtube/v3/live/docs/liveBroadcasts/list>
    #[instrument(skip(self), ret)]
    pub async fn list_live_broadcasts_by_status(
        &self,
        status_filter: BroadcastStatusFilter,
    ) -> eyre::Result<Vec<LiveBroadcast>> {
        let response = self
            .list_live_broadcasts_internal(50, Some(status_filter))
            .await?;
        Ok(response.items)
    }

    /// Returns a list of active (currently live) YouTube broadcasts for the authenticated user.
    ///
    /// This is a convenience method that filters for broadcasts with status `active`.
    /// These are broadcasts that are currently streaming and visible to viewers.
    ///
    /// # Returns
    ///
    /// A vector of active [`LiveBroadcast`] resources, or an error if the API call fails.
    ///
    /// # Required Scopes
    ///
    /// * `https://www.googleapis.com/auth/youtube`
    ///
    /// # API Reference
    ///
    /// <https://developers.google.com/youtube/v3/live/docs/liveBroadcasts/list>
    #[instrument(skip(self), ret)]
    pub async fn list_active_live_broadcasts(&self) -> eyre::Result<Vec<LiveBroadcast>> {
        self.list_live_broadcasts_by_status(BroadcastStatusFilter::Active)
            .await
    }

    /// Returns a list of upcoming YouTube broadcasts for the authenticated user.
    ///
    /// This is a convenience method that filters for broadcasts with status `upcoming`.
    /// These are broadcasts that are scheduled but not yet started.
    ///
    /// # Returns
    ///
    /// A vector of upcoming [`LiveBroadcast`] resources, or an error if the API call fails.
    ///
    /// # Required Scopes
    ///
    /// * `https://www.googleapis.com/auth/youtube`
    ///
    /// # API Reference
    ///
    /// <https://developers.google.com/youtube/v3/live/docs/liveBroadcasts/list>
    #[instrument(skip(self), ret)]
    pub async fn list_upcoming_live_broadcasts(&self) -> eyre::Result<Vec<LiveBroadcast>> {
        self.list_live_broadcasts_by_status(BroadcastStatusFilter::Upcoming)
            .await
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
        let access_token = self.token.access_token().secret();

        let url = "https://www.googleapis.com/youtube/v3/liveBroadcasts/transition";
        let response = self
            .client
            .post(url)
            .header("Authorization", format!("Bearer {}", access_token))
            .query(&[
                ("part", "id,snippet,status"),
                ("id", broadcast_id),
                (
                    "broadcastStatus",
                    serde_json::to_string(&status)
                        .context("serialize broadcast status")?
                        .trim_matches('"'),
                ), // Remove JSON quotes for query param
            ])
            .send()
            .await
            .context("send transition request to YouTube API")?;

        let status_code = response.status();
        if !status_code.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            return Err(eyre::eyre!(
                "YouTube API transition request failed with status {}: {}",
                status_code,
                error_text
            ));
        }

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
        let access_token = self.token.access_token().secret();

        let url = "https://www.googleapis.com/youtube/v3/liveBroadcasts/cuepoint";
        let response = self
            .client
            .post(url)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Content-Type", "application/json")
            .query(&[("id", broadcast_id)])
            .json(cuepoint)
            .send()
            .await
            .context("send cuepoint request to YouTube API")?;

        let status_code = response.status();
        if !status_code.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            return Err(eyre::eyre!(
                "YouTube API cuepoint request failed with status {}: {}",
                status_code,
                error_text
            ));
        }

        tracing::debug!(
            broadcast_id,
            cue_type = ?cuepoint.cue_type,
            "successfully inserted cuepoint"
        );

        Ok(())
    }

    /// Returns a list of live streams for the authenticated user.
    ///
    /// Uses the `liveStreams.list` API to fetch up to 50 stream resources
    /// that belong to the authenticated user.
    ///
    /// # Returns
    ///
    /// A vector of [`LiveStream`] resources, or an error if the API call fails.
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
    #[instrument(skip(self), ret)]
    pub async fn list_live_streams(&self) -> eyre::Result<Vec<LiveStream>> {
        let response = self.list_live_streams_internal(50).await?;
        Ok(response.items)
    }

    /// Internal method to call the `liveBroadcasts.list` API with configurable parameters.
    ///
    /// This method handles the actual HTTP request to the YouTube API, including
    /// authentication headers and query parameters.
    ///
    /// # Arguments
    ///
    /// * `max_results` - Maximum number of broadcasts to return (1-50)
    /// * `status_filter` - Optional [`BroadcastStatusFilter`] to filter results
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
        status_filter: Option<BroadcastStatusFilter>,
    ) -> eyre::Result<LiveBroadcastListResponse> {
        let access_token = self.token.access_token().secret();

        let url = "https://www.googleapis.com/youtube/v3/liveBroadcasts";

        let max_results_string = max_results.to_string();
        let mut query_params = vec![
            ("part", "id,snippet"),
            ("mine", "true"),
            ("maxResults", max_results_string.as_str()),
        ];

        // Add broadcastStatus filter if provided
        let status_string;
        if let Some(status) = status_filter {
            status_string = serde_json::to_string(&status)
                .context("serialize broadcast status filter")?
                .trim_matches('"')
                .to_string(); // Remove JSON quotes
            query_params.push(("broadcastStatus", status_string.as_str()));
        }

        let response = self
            .client
            .get(url)
            .header("Authorization", format!("Bearer {}", access_token))
            .query(&query_params)
            .send()
            .await
            .context("send request to YouTube API")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            return Err(eyre::eyre!(
                "YouTube API request failed with status {}: {}",
                status,
                error_text
            ));
        }

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

    /// Internal method to call the `liveStreams.list` API with configurable `max_results`.
    ///
    /// This method handles the actual HTTP request to the YouTube API, including
    /// authentication headers and query parameters.
    ///
    /// # Arguments
    ///
    /// * `max_results` - Maximum number of streams to return (1-50)
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
    ) -> eyre::Result<LiveStreamListResponse> {
        let access_token = self.token.access_token().secret();

        let url = "https://www.googleapis.com/youtube/v3/liveStreams";
        let response = self
            .client
            .get(url)
            .header("Authorization", format!("Bearer {}", access_token))
            .query(&[
                ("part", "id,snippet,status"),
                ("mine", "true"),
                ("maxResults", &max_results.to_string()),
            ])
            .send()
            .await
            .context("send request to YouTube liveStreams API")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            return Err(eyre::eyre!(
                "YouTube liveStreams API request failed with status {}: {}",
                status,
                error_text
            ));
        }

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
}
