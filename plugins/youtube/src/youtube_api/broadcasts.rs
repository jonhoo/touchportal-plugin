//! YouTube Live Broadcasts API types and functionality.
//!
//! # Core Concepts: Broadcasts vs Streams
//!
//! ## [`LiveBroadcast`] - Viewer-Facing Events
//! - **What viewers see**: Title, description, thumbnail, scheduled time
//! - **Public metadata**: Privacy settings, recording options, monetization
//! - **Event lifecycle**: Created → Testing → Live → Complete
//! - **Use for**: UI listings, scheduling, user-facing operations
//! - **Relationship**: Each broadcast = exactly one YouTube video

use crate::youtube_api::types::PageInfo;
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fmt;

/// Response structure for the `liveBroadcasts.list` API call.
///
/// Contains a list of [`LiveBroadcast`] resources that match the request criteria,
/// along with pagination information in [`PageInfo`].
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveBroadcasts/list>
#[derive(Debug, Serialize, Deserialize)]
pub struct LiveBroadcastListResponse {
    /// Identifies the API resource's type.
    ///
    /// The value will be `youtube#liveBroadcastListResponse`.
    pub kind: String,
    /// A list of broadcasts that match the request criteria.
    pub items: VecDeque<LiveBroadcast>,
    #[serde(rename = "pageInfo")]
    pub page_info: PageInfo,
    /// Token that can be used as the value of the pageToken parameter to retrieve the next page in the result set.
    #[serde(rename = "nextPageToken")]
    pub next_page_token: Option<String>,
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
