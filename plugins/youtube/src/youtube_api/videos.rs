//! YouTube Videos API types and functionality.

use crate::youtube_api::types::PageInfo;
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
