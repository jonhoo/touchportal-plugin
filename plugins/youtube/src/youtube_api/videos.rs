//! YouTube Videos API types and functionality.

use crate::youtube_api::types::PageInfo;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Response structure for the `videos.list` API call.
///
/// Contains a list of [`Video`] resources that match the request criteria,
/// along with pagination information in [`PageInfo`].
///
/// See: <https://developers.google.com/youtube/v3/docs/videos/list>
#[derive(Debug, Serialize, Deserialize)]
pub struct VideoListResponse {
    /// Identifies the API resource's type.
    ///
    /// The value will be `youtube#videoListResponse`.
    pub kind: String,
    /// A list of videos that match the request criteria.
    pub items: VecDeque<Video>,
    #[serde(rename = "pageInfo")]
    pub page_info: PageInfo,
    /// Token that can be used as the value of the pageToken parameter to retrieve the next page in the result set.
    #[serde(rename = "nextPageToken")]
    pub next_page_token: Option<String>,
}

/// A `video` resource represents a YouTube video.
///
/// Contains statistics about the video, and optionally live streaming details
/// if the video is a live broadcast.
///
/// See: <https://developers.google.com/youtube/v3/docs/videos#resource>
#[derive(Debug, Serialize, Deserialize)]
pub struct Video {
    /// The ID that YouTube uses to uniquely identify the video.
    pub id: String,
    /// Contains statistics about the video.
    pub statistics: VideoStatistics,
    /// Contains live streaming details for live broadcasts.
    ///
    /// This field is only present for videos that are upcoming, live, or
    /// completed live broadcasts. Regular uploaded videos will not have
    /// this field populated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_streaming_details: Option<LiveStreamingDetails>,
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

/// Live streaming details for a video that is a live broadcast.
///
/// This object contains metadata about the live streaming aspects of a video,
/// including timing information and current live statistics. It is only present
/// for videos that are upcoming, live, or completed live broadcasts.
///
/// See: <https://developers.google.com/youtube/v3/docs/videos#liveStreamingDetails>
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveStreamingDetails {
    /// The datetime when the broadcast actually started.
    ///
    /// This field will not be available until the broadcast begins.
    /// The value is specified in ISO 8601 format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_start_time: Option<Timestamp>,
    /// The datetime when the broadcast actually ended.
    ///
    /// This field will not be available until the broadcast ends.
    /// The value is specified in ISO 8601 format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_end_time: Option<Timestamp>,
    /// The datetime when the broadcast is scheduled to begin.
    ///
    /// The value is specified in ISO 8601 format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduled_start_time: Option<Timestamp>,
    /// The datetime when the broadcast is scheduled to end.
    ///
    /// The value is specified in ISO 8601 format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduled_end_time: Option<Timestamp>,
    /// The number of viewers currently watching the broadcast.
    ///
    /// This field is only populated for live broadcasts and represents the
    /// real-time concurrent viewer count. For completed broadcasts, this
    /// field will be absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concurrent_viewers: Option<u64>,
    /// The ID of the currently active live chat attached to the video.
    ///
    /// This field is only present for live broadcasts with active chat.
    /// It provides another way to access the live chat ID beyond the
    /// broadcast resource's snippet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_live_chat_id: Option<String>,
}
