//! YouTube Channels API types and functionality.

use crate::youtube_api::types::PageInfo;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Response structure for the `channels.list` API call.
///
/// Contains a list of [`Channel`] resources that match the request criteria,
/// along with pagination information in [`PageInfo`].
///
/// See: <https://developers.google.com/youtube/v3/docs/channels/list>
#[derive(Debug, Serialize, Deserialize)]
pub struct ChannelListResponse {
    /// Identifies the API resource's type.
    ///
    /// The value will be `youtube#channelListResponse`.
    pub kind: String,
    /// A list of channels that match the request criteria.
    pub items: VecDeque<Channel>,
    #[serde(rename = "pageInfo")]
    pub page_info: PageInfo,
    /// Token that can be used as the value of the pageToken parameter to retrieve the next page in the result set.
    #[serde(rename = "nextPageToken")]
    pub next_page_token: Option<String>,
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
