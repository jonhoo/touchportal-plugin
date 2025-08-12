//! YouTube Data API v3 client library.
//!
//! This module provides a comprehensive client for interacting with the YouTube Data API v3,
//! including support for live streaming operations, chat messaging, video statistics,
//! channel management, and more.
//!
//! # Core Concepts: Broadcasts vs Streams
//!
//! The YouTube Live API has two main resource types that work together but serve different purposes:
//!
//! ## [`broadcasts::LiveBroadcast`] - Viewer-Facing Events
//! - **What viewers see**: Title, description, thumbnail, scheduled time
//! - **Public metadata**: Privacy settings, recording options, monetization
//! - **Event lifecycle**: Created → Testing → Live → Complete
//! - **Use for**: UI listings, scheduling, user-facing operations
//! - **Relationship**: Each broadcast = exactly one YouTube video
//!
//! ## [`streams::LiveStream`] - Technical Infrastructure
//! - **Technical config**: Encoder settings, resolution, bitrate, CDN
//! - **Ingestion details**: Stream URLs, authentication tokens
//! - **Health monitoring**: Connection status, stream quality metrics
//! - **Use for**: Encoder setup, technical diagnostics, infrastructure management
//! - **Relationship**: One stream can power multiple broadcasts over time
//!
//! ## Typical Workflow
//! 1. Create a [`streams::LiveStream`] with encoder settings (done once, reusable)
//! 2. Create a [`broadcasts::LiveBroadcast`] for each live event
//! 3. Bind the broadcast to the stream before going live
//! 4. Use broadcast methods for user operations (start, end, schedule)
//! 5. Use stream methods for technical monitoring and configuration
//!
//! For most user-facing applications, you'll primarily work with broadcasts via
//! [`YouTubeClient::list_my_live_broadcasts`] and related methods.
//!
//! # Example Usage
//!
//! ```rust,no_run
//! use crate::youtube_client::{YouTubeClient, TimeBoundAccessToken};
//! use crate::oauth::OAuthManager;
//! use tokio_stream::StreamExt;
//!
//! # async fn example() -> eyre::Result<()> {
//! // Set up client with OAuth token
//! let oauth_manager = OAuthManager::new(/* ... */);
//! let token = TimeBoundAccessToken::new(/* oauth token */);
//! let client = YouTubeClient::new(token, oauth_manager);
//!
//! // List all broadcasts for the authenticated user
//! let mut broadcasts = client.list_my_live_broadcasts();
//! while let Some(broadcast) = broadcasts.next().await {
//!     let broadcast = broadcast?;
//!     println!("Broadcast: {} ({})", broadcast.snippet.title, broadcast.status.life_cycle_status);
//! }
//! # Ok(())
//! # }
//! ```

pub mod broadcasts;
pub mod channels;
pub mod chat;
pub mod client;
pub mod streams;
pub mod types;
pub mod videos;

// Re-export main types for convenience
pub use client::{TimeBoundAccessToken, YouTubeClient};
pub use types::{PageInfo, PagedStream};

// Re-export commonly used types from each module
pub use broadcasts::{
    BroadcastLifeCycleStatus, BroadcastPrivacyStatus, BroadcastStatus, CueType, CuepointRequest,
    LiveBroadcast, LiveBroadcastSnippet, LiveBroadcastStatus,
};

pub use streams::{LiveStream, LiveStreamSnippet, LiveStreamStatus, StreamStatus};

pub use chat::{
    GiftMembershipReceivedDetails, LiveChatMessage, LiveChatMessageAuthor, LiveChatMessageDetails,
    LiveChatMessageSnippet, LiveChatStream, MemberMilestoneChatDetails, MembershipGiftingDetails,
    MessageDeletedDetails, NewSponsorDetails, SuperChatDetails, SuperStickerDetails,
    TextMessageDetails, UserBannedDetails,
};

pub use videos::{Video, VideoStatistics};

pub use channels::{Channel, ChannelSnippet};
