//! YouTube Data API v3 client for live streaming operations.
//!
//! # Core Concepts: Broadcasts vs Streams
//!
//! The YouTube Live API has two main resource types that work together but serve different purposes:
//!
//! ## [`LiveBroadcast`] - Viewer-Facing Events
//! - **What viewers see**: Title, description, thumbnail, scheduled time
//! - **Public metadata**: Privacy settings, recording options, monetization
//! - **Event lifecycle**: Created → Testing → Live → Complete
//! - **Use for**: UI listings, scheduling, user-facing operations
//! - **Relationship**: Each broadcast = exactly one YouTube video
//!
//! ## [`LiveStream`] - Technical Infrastructure
//! - **Technical config**: Encoder settings, resolution, bitrate, CDN
//! - **Ingestion details**: Stream URLs, authentication tokens
//! - **Health monitoring**: Connection status, stream quality metrics
//! - **Use for**: Encoder setup, technical diagnostics, infrastructure management
//! - **Relationship**: One stream can power multiple broadcasts over time
//!
//! ## Typical Workflow
//! 1. Create a [`LiveStream`] with encoder settings (done once, reusable)
//! 2. Create a [`LiveBroadcast`] for each live event
//! 3. Bind the broadcast to the stream before going live
//! 4. Use broadcast methods for user operations (start, end, schedule)
//! 5. Use stream methods for technical monitoring and configuration
//!
//! For most user-facing applications, you'll primarily work with broadcasts via
//! [`YouTubeClient::list_my_live_broadcasts`] and related methods.

use eyre::Context;
use oauth2::basic::BasicTokenResponse;
use oauth2::TokenResponse;
use pin_project::pin_project;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};
use tokio_stream::Stream;
use tracing::instrument;

type OneFuturePage<T> =
    Pin<Box<dyn Future<Output = eyre::Result<(VecDeque<T>, Option<String>)>> + Send>>;

/// A paginated stream that automatically fetches subsequent pages from a YouTube API list endpoint.
///
/// This stream yields items one by one, automatically fetching the next page when the current
/// page is exhausted. Only supports forward pagination (no previous page support).
pub struct PagedStream<T> {
    /// Current batch of items from the most recent API response
    current_items: VecDeque<T>,
    /// Token for the next page, if available
    next_page_token: Option<String>,
    /// Future representing the currently pending API request, if any
    pending_request: Option<OneFuturePage<T>>,
    /// Whether we've reached the end of all available data
    is_done: bool,
}

impl<T> PagedStream<T> {
    /// Create a new PagedStream from the first page of results.
    pub fn new(items: VecDeque<T>, next_page_token: Option<String>) -> Self {
        Self {
            current_items: items,
            next_page_token,
            pending_request: None,
            is_done: false,
        }
    }

    /// Set the fetch function that will be called to get the next page.
    /// This function should return a future that resolves to (items, next_page_token).
    pub fn with_fetcher<F, Fut>(self, fetcher: F) -> PagedStreamWithFetcher<T, F>
    where
        F: Fn(Option<String>) -> Fut + Send,
        Fut: Future<Output = eyre::Result<(VecDeque<T>, Option<String>)>> + Send + 'static,
    {
        PagedStreamWithFetcher {
            current_items: self.current_items,
            next_page_token: self.next_page_token,
            pending_request: None,
            is_done: self.is_done,
            fetcher,
        }
    }
}

/// A paginated stream with an associated fetch function for subsequent pages.
#[pin_project]
pub struct PagedStreamWithFetcher<T, F> {
    current_items: VecDeque<T>,
    next_page_token: Option<String>,
    #[pin]
    pending_request: Option<OneFuturePage<T>>,
    is_done: bool,
    fetcher: F,
}

impl<T, F, Fut> Stream for PagedStreamWithFetcher<T, F>
where
    T: Send + 'static,
    F: Fn(Option<String>) -> Fut + Send,
    Fut: Future<Output = eyre::Result<(VecDeque<T>, Option<String>)>> + Send + 'static,
{
    type Item = eyre::Result<T>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        loop {
            // If we have items in the current batch, return the next one
            if let Some(item) = this.current_items.pop_front() {
                return Poll::Ready(Some(Ok(item)));
            }

            // If we're done (no more pages), return None
            if *this.is_done {
                return Poll::Ready(None);
            }

            // If we don't have a pending request and we have a next page token, start fetching
            if this.pending_request.is_none() && this.next_page_token.is_some() {
                let token = this.next_page_token.clone();
                let future = (this.fetcher)(token);
                this.pending_request.set(Some(Box::pin(future)));
            }

            // If we have a pending request, poll it
            if let Some(pending) = this.pending_request.as_mut().as_pin_mut() {
                match pending.poll(cx) {
                    Poll::Ready(Ok((items, next_token))) => {
                        // We got the next page
                        this.current_items.clear();
                        *this.current_items = items;
                        *this.next_page_token = next_token;

                        // Clear the pending request
                        this.pending_request.set(None);

                        // If no items and no next token, we're done
                        if this.current_items.is_empty() && this.next_page_token.is_none() {
                            *this.is_done = true;
                        }

                        // Continue the loop to try yielding an item
                        continue;
                    }
                    Poll::Ready(Err(e)) => {
                        // Error fetching next page
                        *this.is_done = true;
                        this.pending_request.set(None);
                        return Poll::Ready(Some(Err(e)));
                    }
                    Poll::Pending => {
                        // Still waiting for the response
                        return Poll::Pending;
                    }
                }
            } else {
                // No pending request and no next page token means we're done
                *this.is_done = true;
                return Poll::Ready(None);
            }
        }
    }
}

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
    items: VecDeque<LiveBroadcast>,
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
    /// Token that can be used as the value of the pageToken parameter to retrieve the next page in the result set.
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

/// A `liveBroadcast` resource represents a viewer-facing live streaming event on YouTube.
///
/// **Broadcasts vs Streams**: Broadcasts are what users see and interact with - they contain
/// the title, description, thumbnail, scheduled times, and viewer-facing settings. Each broadcast
/// corresponds to exactly one YouTube video that viewers can watch and comment on.
///
/// Broadcasts must be bound to a [`LiveStream`] to actually transmit video, but the broadcast
/// defines the public-facing aspects of the live event.
///
/// Each broadcast contains an `id` and basic details in the [`LiveBroadcastSnippet`].
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveBroadcasts#resource>
#[derive(Debug, Serialize, Deserialize)]
pub struct LiveBroadcast {
    /// The ID that YouTube assigns to uniquely identify the broadcast.
    pub id: String,
    /// Contains basic details about the broadcast.
    ///
    /// Includes the broadcast's title, description, and thumbnail images.
    pub snippet: LiveBroadcastSnippet,
}

/// The snippet object contains basic details about the broadcast.
///
/// This is a subset of the full snippet data available from the YouTube API,
/// containing only the fields currently needed by this implementation.
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveBroadcasts#snippet>
#[derive(Debug, Serialize, Deserialize)]
pub struct LiveBroadcastSnippet {
    /// The broadcast's title.
    ///
    /// Note that the broadcast represents exactly one YouTube video.
    pub title: String,
    /// The date and time that the broadcast is scheduled to start.
    ///
    /// The value is specified in ISO 8601 format.
    #[serde(rename = "scheduledStartTime")]
    pub scheduled_start_time: Option<String>,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    items: VecDeque<LiveStream>,
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
    /// Token that can be used as the value of the pageToken parameter to retrieve the next page in the result set.
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

/// A `liveStream` resource represents the technical video pipeline for transmitting content to YouTube.
///
/// **Broadcasts vs Streams**: Streams are the technical infrastructure that handles video encoding,
/// ingestion URLs, CDN configuration, and transmission protocols. They contain encoder settings,
/// resolution/bitrate parameters, and health monitoring data. Streams are "behind-the-scenes"
/// technical resources that power the viewer-facing broadcasts.
///
/// A single stream can be reused across multiple broadcasts, and streams can exist independently
/// of any specific broadcast event.
///
/// Contains configuration details for the live video stream including CDN settings
/// and stream status information.
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveStreams#resource>
#[derive(Debug, Serialize, Deserialize)]
pub struct LiveStream {
    /// The ID that YouTube assigns to uniquely identify the stream.
    pub(crate) id: String,
    /// Contains basic details about the stream.
    ///
    /// Includes the stream's title and description.
    pub(crate) snippet: LiveStreamSnippet,
    /// Contains information about the stream's status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) status: Option<LiveStreamStatus>,
}

/// The snippet object contains basic details about the stream.
///
/// This is a subset of the full snippet data available from the YouTube API,
/// containing only the fields currently needed by this implementation.
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveStreams#snippet>
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct LiveStreamSnippet {
    /// The stream's title.
    pub(crate) title: String,
    /// The stream's description.
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

/// The status of a live stream.
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveStreams#status>
#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub(crate) struct LiveStreamStatus {
    /// The stream's status.
    #[serde(rename = "streamStatus")]
    stream_status: StreamStatus,
}

/// Response structure for the `channels.list` API call.
///
/// Contains a list of [`Channel`] resources that match the request criteria,
/// along with pagination information in [`PageInfo`].
///
/// See: <https://developers.google.com/youtube/v3/docs/channels/list>
#[derive(Debug, Serialize, Deserialize)]
struct ChannelListResponse {
    /// Identifies the API resource's type.
    ///
    /// The value will be `youtube#channelListResponse`.
    kind: String,
    /// A list of channels that match the request criteria.
    items: VecDeque<Channel>,
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
    /// Token that can be used as the value of the pageToken parameter to retrieve the next page in the result set.
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

/// A `channel` resource contains information about a YouTube channel.
///
/// Each channel represents a user or organization account on YouTube and contains
/// basic details, branding settings, statistics, and other metadata.
///
/// See: <https://developers.google.com/youtube/v3/docs/channels#resource>
#[derive(Debug, Serialize, Deserialize)]
pub struct Channel {
    /// The ID that YouTube uses to uniquely identify the channel.
    pub id: String,
    /// Contains basic details about the channel.
    ///
    /// Includes the channel's title, description, and other metadata.
    pub snippet: ChannelSnippet,
}

/// The snippet object contains basic details about the channel.
///
/// This is a subset of the full snippet data available from the YouTube API,
/// containing only the fields currently needed by this implementation.
///
/// See: <https://developers.google.com/youtube/v3/docs/channels#snippet>
#[derive(Debug, Serialize, Deserialize)]
pub struct ChannelSnippet {
    /// The channel's title.
    pub title: String,
    /// The channel's description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The date and time that the channel was created.
    ///
    /// The value is specified in ISO 8601 format.
    #[serde(rename = "publishedAt")]
    pub published_at: String,
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

    /// Returns the underlying OAuth2 token.
    ///
    /// This is useful when you need to extract the token for storage or
    /// passing to another component.
    pub fn token(&self) -> &BasicTokenResponse {
        &self.token
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
        match self.list_live_broadcasts_internal(1, None, None).await {
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

    /// Returns a paginated stream of YouTube broadcasts for the authenticated user.
    ///
    /// **Broadcasts vs Streams**: A broadcast represents the viewer-facing live streaming event
    /// with metadata like title, description, scheduling, and viewer settings. This is what
    /// users see and interact with on YouTube. Use broadcasts for user-facing operations like
    /// listing, scheduling, and managing live events.
    ///
    /// Uses the `liveBroadcasts.list` API to fetch broadcast resources
    /// that belong to the authenticated user. The stream automatically handles
    /// pagination and fetches subsequent pages as needed.
    ///
    /// # Returns
    ///
    /// A [`PagedStreamWithFetcher`] that yields [`LiveBroadcast`] resources.
    ///
    /// # Required Scopes
    ///
    /// * `https://www.googleapis.com/auth/youtube`
    ///
    /// # API Reference
    ///
    /// <https://developers.google.com/youtube/v3/live/docs/liveBroadcasts/list>
    #[instrument(skip(self))]
    pub async fn list_my_live_broadcasts(
        &self,
    ) -> eyre::Result<impl Stream<Item = eyre::Result<LiveBroadcast>>> {
        let response = self.list_live_broadcasts_internal(50, None, None).await?;
        let client = self.clone();
        Ok(
            PagedStream::new(response.items, response.next_page_token).with_fetcher(
                move |page_token| {
                    let client = client.clone();
                    async move {
                        let response = client
                            .list_live_broadcasts_internal(50, None, page_token)
                            .await?;
                        Ok((response.items, response.next_page_token))
                    }
                },
            ),
        )
    }

    /// Returns a paginated stream of YouTube broadcasts filtered by status for the authenticated user.
    ///
    /// **Broadcasts vs Streams**: A broadcast represents the viewer-facing live streaming event.
    /// Use this method to find broadcasts in specific states like "active" (currently live),
    /// "upcoming" (scheduled), or "completed" (ended). This is ideal for UI applications
    /// that need to show users their current and upcoming live events.
    ///
    /// Uses the `liveBroadcasts.list` API to fetch broadcast resources with a specific status
    /// that belong to the authenticated user. The stream automatically handles
    /// pagination and fetches subsequent pages as needed.
    ///
    /// # Arguments
    ///
    /// * `status_filter` - The [`BroadcastStatusFilter`] to apply
    ///
    /// # Returns
    ///
    /// A [`PagedStreamWithFetcher`] that yields [`LiveBroadcast`] resources matching the filter.
    ///
    /// # Required Scopes
    ///
    /// * `https://www.googleapis.com/auth/youtube`
    ///
    /// # API Reference
    ///
    /// <https://developers.google.com/youtube/v3/live/docs/liveBroadcasts/list>
    #[instrument(skip(self))]
    pub async fn list_my_live_broadcasts_by_status(
        &self,
        status_filter: BroadcastStatusFilter,
    ) -> eyre::Result<impl Stream<Item = eyre::Result<LiveBroadcast>>> {
        let status_filter_clone = status_filter.clone();
        let response = self
            .list_live_broadcasts_internal(50, Some(status_filter), None)
            .await?;
        let client = self.clone();
        Ok(
            PagedStream::new(response.items, response.next_page_token).with_fetcher(
                move |page_token| {
                    let client = client.clone();
                    let status_filter = status_filter_clone.clone();
                    async move {
                        let response = client
                            .list_live_broadcasts_internal(50, Some(status_filter), page_token)
                            .await?;
                        Ok((response.items, response.next_page_token))
                    }
                },
            ),
        )
    }

    /// Returns a paginated stream of active (currently live) YouTube broadcasts for the authenticated user.
    ///
    /// This is a convenience method that filters for broadcasts with status `active`.
    /// These are broadcasts that are currently streaming and visible to viewers on YouTube.
    /// Use this to find broadcasts that are live right now and can be controlled (e.g., ended,
    /// have cuepoints inserted, etc.).
    ///
    /// # Returns
    ///
    /// A [`PagedStreamWithFetcher`] that yields active [`LiveBroadcast`] resources.
    ///
    /// # Required Scopes
    ///
    /// * `https://www.googleapis.com/auth/youtube`
    ///
    /// # API Reference
    ///
    /// <https://developers.google.com/youtube/v3/live/docs/liveBroadcasts/list>
    #[instrument(skip(self))]
    pub async fn list_my_active_live_broadcasts(
        &self,
    ) -> eyre::Result<impl Stream<Item = eyre::Result<LiveBroadcast>>> {
        self.list_my_live_broadcasts_by_status(BroadcastStatusFilter::Active)
            .await
    }

    /// Returns a paginated stream of upcoming YouTube broadcasts for the authenticated user.
    ///
    /// This is a convenience method that filters for broadcasts with status `upcoming`.
    /// These are broadcasts that are scheduled but not yet started. Use this to show users
    /// their upcoming live events that can be started or modified before going live.
    ///
    /// # Returns
    ///
    /// A [`PagedStreamWithFetcher`] that yields upcoming [`LiveBroadcast`] resources.
    ///
    /// # Required Scopes
    ///
    /// * `https://www.googleapis.com/auth/youtube`
    ///
    /// # API Reference
    ///
    /// <https://developers.google.com/youtube/v3/live/docs/liveBroadcasts/list>
    #[instrument(skip(self))]
    pub async fn list_my_upcoming_live_broadcasts(
        &self,
    ) -> eyre::Result<impl Stream<Item = eyre::Result<LiveBroadcast>>> {
        self.list_my_live_broadcasts_by_status(BroadcastStatusFilter::Upcoming)
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
    /// A [`PagedStreamWithFetcher`] that yields [`LiveStream`] resources.
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
    pub async fn list_my_live_streams(
        &self,
    ) -> eyre::Result<impl Stream<Item = eyre::Result<LiveStream>>> {
        let response = self.list_live_streams_internal(50, None).await?;
        let client = self.clone();
        Ok(
            PagedStream::new(response.items, response.next_page_token).with_fetcher(
                move |page_token| {
                    let client = client.clone();
                    async move {
                        let response = client.list_live_streams_internal(50, page_token).await?;
                        Ok((response.items, response.next_page_token))
                    }
                },
            ),
        )
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
    /// A [`PagedStreamWithFetcher`] that yields [`Channel`] resources owned by the authenticated user.
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
    pub async fn list_my_channels(
        &self,
    ) -> eyre::Result<impl Stream<Item = eyre::Result<Channel>>> {
        let response = self.list_channels_internal(50, None).await?;
        let client = self.clone();
        Ok(
            PagedStream::new(response.items, response.next_page_token).with_fetcher(
                move |page_token| {
                    let client = client.clone();
                    async move {
                        let response = client.list_channels_internal(50, page_token).await?;
                        Ok((response.items, response.next_page_token))
                    }
                },
            ),
        )
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
    /// * `page_token` - Optional page token for pagination
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
        page_token: Option<String>,
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

        // Add pageToken if provided
        if let Some(ref token) = page_token {
            query_params.push(("pageToken", token.as_str()));
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
        let access_token = self.token.access_token().secret();

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
            .client
            .get(url)
            .header("Authorization", format!("Bearer {}", access_token))
            .query(&query_params)
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
        let access_token = self.token.access_token().secret();

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
            .client
            .get(url)
            .header("Authorization", format!("Bearer {}", access_token))
            .query(&query_params)
            .send()
            .await
            .context("send request to YouTube channels API")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            return Err(eyre::eyre!(
                "YouTube channels API request failed with status {}: {}",
                status,
                error_text
            ));
        }

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
