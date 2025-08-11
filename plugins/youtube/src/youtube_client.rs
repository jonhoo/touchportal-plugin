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

use crate::oauth::OAuthManager;
use eyre::Context;
use http::Method;
use jiff::{SignedDuration, Timestamp};
use oauth2::TokenResponse;
use oauth2::basic::BasicTokenResponse;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll};
use std::time::{Duration, SystemTime};
use tokio::sync::Mutex;
use tokio_stream::Stream;
use tracing::instrument;

type OneFuturePage<'a, F, T> =
    Pin<Box<dyn Future<Output = eyre::Result<(F, (VecDeque<T>, Option<String>))>> + 'a>>;

impl<'a, T, F> PagedStream<'a, T, F> {
    /// Create a new PagedStream from the first page of results.
    pub fn new(fetcher: F) -> Self
    where
        F: AsyncFn(Option<String>) -> eyre::Result<(VecDeque<T>, Option<String>)> + 'a,
    {
        let first_page = async move {
            let results = fetcher(None).await?;
            Ok((fetcher, results))
        };
        Self {
            pending_request: Some(Box::pin(first_page)),
            current_items: VecDeque::new(),
            is_done: false,
        }
    }
}

/// A paginated stream that automatically fetches subsequent pages from a YouTube API list endpoint.
///
/// This stream yields items one by one, automatically fetching the next page when the current
/// page is exhausted. Only supports forward pagination (no previous page support).
pub struct PagedStream<'a, T, F> {
    /// Current batch of items from the most recent API response
    current_items: VecDeque<T>,
    /// Future representing the currently pending API request, if any
    pending_request: Option<OneFuturePage<'a, F, T>>,
    /// Whether we've reached the end of all available data
    is_done: bool,
}

impl<'a, T: Unpin, F> Unpin for PagedStream<'a, T, F> {}

impl<'a, T: Unpin, F> Stream for PagedStream<'a, T, F>
where
    F: AsyncFn(Option<String>) -> eyre::Result<(VecDeque<T>, Option<String>)> + 'a,
{
    type Item = eyre::Result<T>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        loop {
            // If we have items in the current batch, return the next one
            if let Some(item) = self.current_items.pop_front() {
                return Poll::Ready(Some(Ok(item)));
            }

            // If we're done (no more pages), return None
            if self.is_done {
                return Poll::Ready(None);
            }

            // If we have a pending request, poll it
            if let Some(pending) = self.pending_request.as_mut() {
                match pending.as_mut().poll(cx) {
                    Poll::Ready(Ok((fetcher, (items, next_token)))) => {
                        // We got the next page
                        self.current_items.extend(items);

                        if let Some(next_token) = next_token {
                            // Set up the future for the next page
                            // (but don't poll it yet)
                            self.pending_request = Some(Box::pin(async move {
                                let results = fetcher(Some(next_token)).await?;
                                Ok((fetcher, results))
                            }));
                        } else {
                            // If no next token, we're done
                            self.is_done = true;
                            self.pending_request = None;
                        }

                        // Continue the loop to try yielding an item
                        continue;
                    }
                    Poll::Ready(Err(e)) => {
                        // Error fetching next page
                        self.pending_request = None;
                        self.is_done = true;
                        return Poll::Ready(Some(Err(e)));
                    }
                    Poll::Pending => {
                        // Still waiting for the response
                        return Poll::Pending;
                    }
                }
            } else {
                // No pending request and no next page token means we're done
                self.is_done = true;
                return Poll::Ready(None);
            }
        }
    }
}

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
    /// OAuth manager for refreshing tokens
    oauth_manager: OAuthManager,
    /// HTTP client for API requests
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
    /// Contains information about the broadcast's status.
    pub status: LiveBroadcastStatus,
}

/// The snippet object contains basic details about the broadcast.
///
/// This is a subset of the full snippet data available from the YouTube API,
/// containing only the fields currently needed by this implementation.
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveBroadcasts#snippet>
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveBroadcastSnippet {
    /// The broadcast's title.
    ///
    /// Note that the broadcast represents exactly one YouTube video.
    pub title: String,
    /// The date and time that the broadcast was added to YouTube's live broadcast schedule.
    ///
    /// The value is specified in ISO 8601 format.
    #[serde(rename = "publishedAt")]
    pub published_at: Timestamp,
    /// The date and time that the broadcast is scheduled to start.
    ///
    /// The value is specified in ISO 8601 format.
    /// May be unset for broadcasts that are not yet scheduled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduled_start_time: Option<Timestamp>,
    /// The date and time that the broadcast is scheduled to end.
    ///
    /// The value is specified in ISO 8601 format.
    /// May be unset, which means the broadcast is scheduled to continue indefinitely.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduled_end_time: Option<Timestamp>,
    /// The date and time that the broadcast actually started.
    ///
    /// The value is specified in ISO 8601 format.
    /// Unset until the broadcast has actually started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_start_time: Option<Timestamp>,
    /// The date and time that the broadcast actually ended.
    ///
    /// The value is specified in ISO 8601 format.
    /// Unset until the broadcast has actually ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_end_time: Option<Timestamp>,
}

/// The status object contains information about the live broadcast's status and settings.
///
/// This includes the broadcast's lifecycle status (ready, testing, live, complete),
/// privacy settings, recording status, and monetization settings.
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveBroadcasts#status>
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveBroadcastStatus {
    /// The broadcast's lifecycle status.
    pub life_cycle_status: BroadcastLifeCycleStatus,
    /// The broadcast's privacy status.
    pub privacy_status: BroadcastPrivacyStatus,
    /// Whether the broadcast is made for kids.
    pub made_for_kids: bool,
}

/// The broadcast's current lifecycle status.
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveBroadcasts#status.lifeCycleStatus>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BroadcastLifeCycleStatus {
    /// The broadcast is ready to be activated but has not yet been activated.
    Ready,
    /// The broadcast is in testing mode and can be seen by viewers who have access to the URL.
    Testing,
    /// The broadcast is active and visible to anyone who has access to the URL.
    Live,
    /// The broadcast has finished and is no longer live.
    Complete,
    /// The broadcast was created but never activated.
    Created,
    /// The broadcast has been revoked and can no longer be activated.
    Revoked,
}

impl fmt::Display for BroadcastLifeCycleStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ready => write!(f, "ready"),
            Self::Testing => write!(f, "testing"),
            Self::Live => write!(f, "live"),
            Self::Complete => write!(f, "complete"),
            Self::Created => write!(f, "created"),
            Self::Revoked => write!(f, "revoked"),
        }
    }
}

/// The broadcast's privacy status.
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveBroadcasts#status.privacyStatus>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BroadcastPrivacyStatus {
    /// The broadcast is public and can be viewed by anyone.
    Public,
    /// The broadcast is unlisted and can only be viewed by people with the link.
    Unlisted,
    /// The broadcast is private and can only be viewed by the owner and authorized viewers.
    Private,
}

impl fmt::Display for BroadcastPrivacyStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Public => write!(f, "public"),
            Self::Unlisted => write!(f, "unlisted"),
            Self::Private => write!(f, "private"),
        }
    }
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BroadcastStatus {
    /// Start broadcast testing mode.
    Testing,
    /// Make broadcast visible to audience.
    Live,
    /// Mark broadcast as complete/over.
    Complete,
}

impl fmt::Display for BroadcastStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Testing => write!(f, "testing"),
            Self::Live => write!(f, "live"),
            Self::Complete => write!(f, "complete"),
        }
    }
}

/// The type of cuepoint that can be inserted into a live broadcast.
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveBroadcasts/cuepoint>
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CueType {
    /// Advertisement cuepoint that may trigger an ad break.
    #[serde(rename = "cueTypeAd")]
    CueTypeAd,
}

impl fmt::Display for CueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CueTypeAd => write!(f, "ad"),
        }
    }
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
    /// Duration of the cuepoint.
    ///
    /// Defaults to 30 seconds if not specified.
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_duration_as_seconds",
        deserialize_with = "deserialize_seconds_as_duration"
    )]
    pub duration: Option<SignedDuration>,
    /// Wall clock time for when to insert the cuepoint.
    ///
    /// If `None`, YouTube will use a default `insertionOffsetTimeMs` of `0`,
    /// meaning the cuepoint will be inserted immediately.
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "walltimeMs",
        with = "jiff::fmt::serde::timestamp::millisecond::optional"
    )]
    pub walltime: Option<Timestamp>,
}

fn serialize_duration_as_seconds<S>(
    duration: &Option<SignedDuration>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match duration {
        Some(d) => {
            let seconds = d.as_secs();
            serializer.serialize_u64(seconds as u64)
        }
        None => serializer.serialize_none(),
    }
}

fn deserialize_seconds_as_duration<'de, D>(
    deserializer: D,
) -> Result<Option<SignedDuration>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let seconds: Option<u64> = Option::deserialize(deserializer)?;
    Ok(seconds.map(|s| SignedDuration::from_secs(s as i64)))
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
    pub id: String,
    /// Contains basic details about the stream.
    ///
    /// Includes the stream's title and description.
    pub snippet: LiveStreamSnippet,
    /// Contains information about the stream's status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<LiveStreamStatus>,
}

/// The snippet object contains basic details about the stream.
///
/// This is a subset of the full snippet data available from the YouTube API,
/// containing only the fields currently needed by this implementation.
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveStreams#snippet>
#[derive(Debug, Serialize, Deserialize)]
pub struct LiveStreamSnippet {
    /// The stream's title.
    pub title: String,
    /// The stream's description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The date and time that the stream was created.
    ///
    /// The value is specified in ISO 8601 format.
    #[serde(rename = "publishedAt")]
    pub published_at: Timestamp,
}

/// The status of a live stream.
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveStreams#status>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

impl fmt::Display for StreamStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Created => write!(f, "created"),
            Self::Error => write!(f, "error"),
            Self::Inactive => write!(f, "inactive"),
            Self::Ready => write!(f, "ready"),
        }
    }
}

/// Contains information about the live stream's status.
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveStreams#status>
#[derive(Debug, Serialize, Deserialize)]
pub struct LiveStreamStatus {
    /// The stream's status.
    #[serde(rename = "streamStatus")]
    pub stream_status: StreamStatus,
}

/// Response structure for the `videos.list` API call.
///
/// Contains a list of [`Video`] resources that match the request criteria,
/// along with pagination information in [`PageInfo`].
///
/// See: <https://developers.google.com/youtube/v3/docs/videos/list>
#[derive(Debug, Serialize, Deserialize)]
struct VideoListResponse {
    /// Identifies the API resource's type.
    ///
    /// The value will be `youtube#videoListResponse`.
    kind: String,
    /// A list of videos that match the request criteria.
    items: VecDeque<Video>,
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
    /// Token that can be used as the value of the pageToken parameter to retrieve the next page in the result set.
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

/// A `video` resource represents a YouTube video.
///
/// Contains statistics about the video.
///
/// See: <https://developers.google.com/youtube/v3/docs/videos#resource>
#[derive(Debug, Serialize, Deserialize)]
pub struct Video {
    /// The ID that YouTube uses to uniquely identify the video.
    pub id: String,
    /// Contains statistics about the video.
    pub statistics: VideoStatistics,
}

/// Statistics about the video.
///
/// See: <https://developers.google.com/youtube/v3/docs/videos#statistics>
#[derive(Debug, Serialize, Deserialize)]
pub struct VideoStatistics {
    /// The number of times the video has been viewed.
    #[serde(rename = "viewCount")]
    pub view_count: Option<String>,
    /// The number of users who have indicated that they liked the video.
    #[serde(rename = "likeCount")]
    pub like_count: Option<String>,
    /// The number of users who have indicated that they disliked the video.
    /// Note: This is only visible to the video owner.
    #[serde(rename = "dislikeCount")]
    pub dislike_count: Option<String>,
    /// The number of users who currently have the video marked as a favorite video.
    /// Note: This property is deprecated and always returns 0.
    #[serde(rename = "favoriteCount")]
    pub favorite_count: Option<String>,
    /// The number of comments for the video.
    #[serde(rename = "commentCount")]
    pub comment_count: Option<String>,
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
    pub published_at: Timestamp,
}

impl YouTubeClient {
    /// Creates a new YouTube API client with the provided OAuth2 token and OAuth manager.
    ///
    /// The token expiry time is calculated from when the token was created plus the `expires_in`
    /// duration minus a 5-minute safety buffer to prevent edge-case failures.
    ///
    /// # Arguments
    ///
    /// * `token` - A valid [`BasicTokenResponse`] containing the OAuth2 access token
    /// * `oauth_manager` - Shared OAuth manager for token refresh operations
    pub fn new(token: TimeBoundAccessToken, oauth_manager: OAuthManager) -> Self {
        let client = reqwest::Client::new();

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
    async fn fresh_access_token(&self) -> eyre::Result<String> {
        let mut token = self.token.lock().await;
        let now = SystemTime::now();

        if now >= token.expires_at {
            tracing::info!("access token expired, attempting refresh");

            // Token needs refresh
            if token.refresh(&self.oauth_manager).await? {
                tracing::info!("access token successfully refreshed");
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
    async fn make_authenticated_request(
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
                tracing::info!("YouTube API token validation successful");
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
    /// A [`PagedStreamWithFetcher`] that yields all [`LiveBroadcast`] resources owned by the user.
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
    pub fn list_my_channels(&self) -> impl Stream<Item = eyre::Result<Channel>> + use<'_> {
        PagedStream::new(|page_token| async {
            let response = self.list_channels_internal(50, page_token).await?;
            Ok((response.items, response.next_page_token))
        })
    }

    /// Gets statistics for a single YouTube video by its ID.
    ///
    /// Uses the `videos.list` API to fetch statistics for the specified video.
    /// Returns view count, like count, comment count, and other engagement metrics.
    ///
    /// # Arguments
    ///
    /// * `video_id` - The YouTube video ID to get statistics for
    ///
    /// # Returns
    ///
    /// A [`Video`] resource containing the video's statistics, or an error if the video
    /// is not found or not accessible.
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
    pub async fn get_video_statistics(&self, video_id: &str) -> eyre::Result<Video> {
        let url = "https://www.googleapis.com/youtube/v3/videos";
        let query_params = [("part", "statistics"), ("id", video_id)];

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
