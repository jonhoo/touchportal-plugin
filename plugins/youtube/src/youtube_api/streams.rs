//! YouTube Live Streams API types and functionality.
//!
//! # Core Concepts: Broadcasts vs Streams
//!
//! ## [`LiveStream`] - Technical Infrastructure
//! - **Technical config**: Encoder settings, resolution, bitrate, CDN
//! - **Ingestion details**: Stream URLs, authentication tokens
//! - **Health monitoring**: Connection status, stream quality metrics
//! - **Use for**: Encoder setup, technical diagnostics, infrastructure management
//! - **Relationship**: One stream can power multiple broadcasts over time

use crate::youtube_api::types::PageInfo;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fmt;

/// Response structure for the `liveStreams.list` API call.
///
/// Contains a list of [`LiveStream`] resources that match the request criteria,
/// along with pagination information in [`PageInfo`].
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveStreams/list>
#[derive(Debug, Serialize, Deserialize)]
pub struct LiveStreamListResponse {
    /// Identifies the API resource's type.
    ///
    /// The value will be `youtube#liveStreamListResponse`.
    pub kind: String,
    /// A list of live streams that match the request criteria.
    pub items: VecDeque<LiveStream>,
    #[serde(rename = "pageInfo")]
    pub page_info: PageInfo,
    /// Token that can be used as the value of the pageToken parameter to retrieve the next page in the result set.
    #[serde(rename = "nextPageToken")]
    pub next_page_token: Option<String>,
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
