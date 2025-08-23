//! YouTube Live Chat API types and streaming functionality.

use crate::youtube_api::client::YouTubeClient;
use bytes::Bytes;
use eyre::Context;
use http::Method;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fmt;
use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};
use tokio_stream::{Stream, StreamExt};

type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, eyre::Error>> + Send>>;

/// A streaming implementation for YouTube Live Chat Messages.
///
/// This stream connects to the YouTube Live Chat Messages `streamList` API and provides
/// a continuous feed of new chat messages. The `streamList` endpoint keeps the HTTP
/// connection open and streams new events in real-time rather than requiring polling.
///
/// The initial response contains historical messages, which are discarded to focus
/// only on new events that arrive after the connection is established.
///
/// TODO: Implement stream resumption using the nextPageToken when the connection drops
/// to avoid missing messages during reconnection.
pub struct LiveChatStream {
    /// The underlying byte stream from the HTTP response
    bytes_stream: Option<ByteStream>,
    /// Buffer for accumulating bytes until we have complete JSON lines
    buffer: Vec<u8>,
    /// Current batch of messages from the most recent API response
    current_messages: VecDeque<LiveChatMessage>,
    /// Whether we've processed the initial historical batch
    skipped_initial_batch: bool,
}

impl LiveChatStream {
    /// Creates a new live chat message stream for the given chat ID.
    pub fn new(client: YouTubeClient, live_chat_id: String) -> Self {
        let stream = Self::create_stream(client, live_chat_id);

        Self {
            bytes_stream: Some(Box::pin(stream)),
            buffer: Vec::new(),
            current_messages: VecDeque::new(),
            skipped_initial_batch: false,
        }
    }

    /// Creates the streaming connection to the YouTube Live Chat streamList API.
    fn create_stream(
        client: YouTubeClient,
        live_chat_id: String,
    ) -> impl Stream<Item = Result<Bytes, eyre::Error>> + 'static {
        async_stream::stream! {
            let access_token = match client.fresh_access_token().await {
                Ok(token) => token,
                Err(e) => {
                    yield Err(e).context("get fresh access token for live chat streaming");
                    return;
                }
            };

            let url = "https://www.googleapis.com/youtube/v3/liveChat/messages/streamList";

            let query_params = [
                ("part", "id,snippet,authorDetails"),
                ("liveChatId", live_chat_id.as_str()),
            ];

            let request = client
                .http_client()
                .request(Method::GET, url)
                .header("Authorization", format!("Bearer {}", access_token))
                .query(&query_params);

            tracing::debug!(live_chat_id, "starting live chat message stream");

            let response = match request.send().await {
                Ok(resp) => resp,
                Err(e) => {
                    yield Err(e).context("send live chat streamList request");
                    return;
                }
            };

            let status_code = response.status();
            if !status_code.is_success() {
                let error_text = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "unknown error".to_string());
                let error = eyre::eyre!(
                    "YouTube Live Chat streamList request failed with status {}: {}",
                    status_code,
                    error_text
                );
                yield Err(error);
                return;
            }

            // Stream the response body bytes
            let mut bytes_stream = response.bytes_stream();
            while let Some(chunk) = bytes_stream.next().await {
                match chunk {
                    Ok(bytes) => yield Ok(bytes),
                    Err(e) => {
                        yield Err(e).context("read chunk from live chat stream");
                        return;
                    }
                }
            }

            tracing::debug!("live chat stream connection closed");
        }
    }

    /// Processes a chunk of bytes, extracting complete JSON messages and updating the message queue.
    fn process_chunk(&mut self, chunk: Bytes) -> eyre::Result<()> {
        self.buffer.extend_from_slice(&chunk);

        // Process complete JSON objects in the buffer (separated by newlines)
        while let Some(newline_pos) = self.buffer.iter().position(|&b| b == b'\n') {
            let json_line = self.buffer.drain(..=newline_pos).collect::<Vec<u8>>();

            // Convert to string, removing the newline
            let json_str = String::from_utf8_lossy(&json_line[..newline_pos]);
            let json_str = json_str.trim();

            if json_str.is_empty() {
                continue;
            }

            let response = serde_json::from_str::<LiveChatMessageListResponse>(json_str)
                .with_context(|| {
                    format!("failed to parse streaming JSON response: {}", json_str)
                })?;

            if !self.skipped_initial_batch {
                // Skip the entire initial historical batch
                tracing::debug!(
                    historical_count = response.items.len(),
                    "skipping initial historical batch, waiting for new events"
                );
                self.skipped_initial_batch = true;
            } else {
                // These are new messages - add them to our queue
                tracing::trace!(
                    new_message_count = response.items.len(),
                    "received new live chat message batch"
                );
                for message in response.items {
                    tracing::trace!(
                        mid = message.id,
                        author = message.author_details.as_ref().map(|a| &a.display_name),
                        content = message
                            .snippet
                            .display_message
                            .as_deref()
                            .unwrap_or("[no content]"),
                        "new chat message"
                    );
                    self.current_messages.push_back(message);
                }
            }
        }

        Ok(())
    }
}

impl Stream for LiveChatStream {
    type Item = eyre::Result<LiveChatMessage>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        loop {
            // If we have messages in the current batch, return the next one
            if let Some(message) = self.current_messages.pop_front() {
                return Poll::Ready(Some(Ok(message)));
            }

            // Poll the byte stream for more data
            if let Some(bytes_stream) = self.bytes_stream.as_mut() {
                match bytes_stream.as_mut().poll_next(cx) {
                    Poll::Ready(Some(Ok(chunk))) => {
                        // Process the chunk and continue the loop
                        if let Err(e) = self.process_chunk(chunk) {
                            self.bytes_stream = None;
                            return Poll::Ready(Some(Err(e).context("process_chunk")));
                        }
                        // Continue the loop to check for messages
                        continue;
                    }
                    Poll::Ready(Some(Err(e))) => {
                        // Error reading from stream
                        self.bytes_stream = None;
                        return Poll::Ready(Some(Err(e)));
                    }
                    Poll::Ready(None) => {
                        // Stream ended
                        self.bytes_stream = None;
                        return Poll::Ready(None);
                    }
                    Poll::Pending => {
                        // Still waiting for more data
                        return Poll::Pending;
                    }
                }
            } else {
                // No stream - we're done
                return Poll::Ready(None);
            }
        }
    }
}

/// Response structure for the `liveChatMessages.streamList` API call.
///
/// Contains a list of [`LiveChatMessage`] resources for continuous message streaming.
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveChatMessages/streamList>
#[derive(Debug, Serialize, Deserialize)]
struct LiveChatMessageListResponse {
    /// A list of chat messages from the live stream.
    items: VecDeque<LiveChatMessage>,
    /// Token that can be used to retrieve the next set of messages.
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
    /// The currently active poll in the chat, if any.
    #[serde(rename = "activePollItem", skip_serializing_if = "Option::is_none")]
    active_poll_item: Option<serde_json::Value>,
}

/// A `liveChatMessage` resource represents a chat message in a YouTube live stream.
///
/// Chat messages include regular text messages, Super Chats, membership gifts,
/// and other interactive elements that viewers can send during live streams.
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveChatMessages#resource>
#[derive(Debug, Serialize, Deserialize)]
pub struct LiveChatMessage {
    /// The ID that YouTube assigns to uniquely identify the message.
    pub id: String,
    /// Contains basic details about the chat message.
    pub snippet: LiveChatMessageSnippet,
    /// Contains details about the message author.
    #[serde(rename = "authorDetails", skip_serializing_if = "Option::is_none")]
    pub author_details: Option<LiveChatMessageAuthor>,
}

/// The snippet object contains basic details about the chat message.
///
/// This includes common fields present in all message types, plus type-specific details
/// that are automatically deserialized based on the message type.
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveChatMessages#snippet>
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveChatMessageSnippet {
    /// The ID of the live chat that the message belongs to.
    pub live_chat_id: String,
    /// The ID of the user that authored this message.
    pub author_channel_id: String,
    /// The date and time when the message was orignally published.
    ///
    /// The value is specified in ISO 8601 format.
    pub published_at: Timestamp,
    /// Contains a string that can be displayed to the user.
    ///
    /// If this field is not present, the message is being deleted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_message: Option<String>,
    /// Type-specific message details, automatically deserialized based on the "type" field.
    #[serde(flatten)]
    pub details: LiveChatMessageDetails,
}

/// Type-specific details for live chat messages using tagged enum representation.
///
/// Each variant corresponds to a specific message type and contains the exact detail fields
/// that are present for that message type in the YouTube API response.
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveChatMessages#snippet.type>
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum LiveChatMessageDetails {
    /// A regular chat message posted by a viewer.
    ///
    /// These are the standard text messages that make up the majority of live chat activity.
    /// The message content is available in both the `display_message` field (formatted for display)
    /// and the `text_message_details.message_text` field (raw text content).
    #[serde(rename = "textMessageEvent")]
    TextMessage {
        text_message_details: TextMessageDetails,
    },
    /// A Super Chat message where a viewer paid to highlight their message.
    ///
    /// Super Chats are paid messages that stand out in the chat with special formatting
    /// and colors based on the amount paid. They may stay pinned at the top of the chat
    /// for a duration determined by the payment amount. The message includes both the
    /// payment details and any text comment from the viewer.
    #[serde(rename = "superChatEvent")]
    SuperChat {
        super_chat_details: SuperChatDetails,
    },
    /// A Super Sticker message where a viewer paid to send an animated sticker.
    ///
    /// Super Stickers are paid animated stickers that viewers can send instead of text.
    /// Like Super Chats, they are highlighted and may be pinned based on the payment amount.
    /// The sticker includes metadata about the specific sticker used and payment details.
    #[serde(rename = "superStickerEvent")]
    SuperSticker {
        super_sticker_details: SuperStickerDetails,
    },
    /// A message indicating that a viewer became a new channel member (sponsor).
    ///
    /// These messages appear when someone joins a channel's membership program for the first time
    /// or upgrades to a higher membership tier. The details include the membership level name
    /// and whether this represents an upgrade from a previous membership level.
    #[serde(rename = "newSponsorEvent")]
    NewSponsor {
        new_sponsor_details: NewSponsorDetails,
    },
    /// A message celebrating a member's milestone (e.g., 6 months of membership).
    ///
    /// These automated messages are generated when existing channel members reach milestone
    /// durations (like 1 month, 6 months, 1 year, etc.). Members can optionally include
    /// a custom message along with the milestone celebration. The message includes both
    /// the milestone duration and membership level.
    #[serde(rename = "memberMilestoneChatEvent")]
    MemberMilestone {
        member_milestone_chat_details: MemberMilestoneChatDetails,
    },
    /// A message indicating that a viewer purchased membership gifts for others.
    ///
    /// These messages appear when someone purchases channel memberships as gifts for other
    /// viewers. The message includes the number of memberships gifted and the membership
    /// level that was gifted. Recipients will receive separate `giftMembershipReceivedEvent`
    /// messages.
    #[serde(rename = "membershipGiftingEvent")]
    MembershipGifting {
        membership_gifting_details: MembershipGiftingDetails,
    },
    /// A message indicating that a viewer received a gifted membership.
    ///
    /// These messages are generated for each recipient of a membership gift. They include
    /// the membership level received and identify both the gifter and the associated
    /// gifting message that initiated the gift.
    #[serde(rename = "giftMembershipReceivedEvent")]
    GiftMembershipReceived {
        gift_membership_received_details: GiftMembershipReceivedDetails,
    },
    /// A system message indicating that a previous message was deleted by a moderator.
    ///
    /// These tombstone messages replace deleted chat messages to maintain chat context.
    /// The original message content is removed, but the deletion event is preserved
    /// with a reference to the ID of the deleted message. The `display_message` field
    /// is typically empty or contains a generic deletion notice.
    #[serde(rename = "messageDeletedEvent")]
    MessageDeleted {
        message_deleted_details: MessageDeletedDetails,
    },
    /// A system message indicating that a user was banned from the chat.
    ///
    /// These messages are generated when moderators ban users from participating in chat.
    /// They include details about the banned user, the type of ban (permanent or temporary),
    /// and the duration if it's a temporary ban. The ban may be channel-wide or chat-specific.
    #[serde(rename = "userBannedEvent")]
    UserBanned {
        user_banned_details: UserBannedDetails,
    },
    /// A system message indicating that a message was retracted by its author.
    ///
    /// These messages appear when users delete their own messages (as opposed to moderator
    /// deletions). Unlike `messageDeletedEvent`, these represent voluntary retractions
    /// by the original message author. No additional details are provided beyond the
    /// basic message fields.
    #[serde(rename = "messageRetractedEvent")]
    MessageRetracted,
    /// A system message indicating that the live chat has ended.
    ///
    /// This message is sent when the broadcaster ends the live stream and closes the chat.
    /// It serves as a final marker in the chat history and indicates that no new messages
    /// can be posted. No additional details are provided beyond the basic message fields.
    #[serde(rename = "chatEndedEvent")]
    ChatEnded,
    /// A system message indicating that sponsor-only mode was activated.
    ///
    /// When this mode is active, only channel members (sponsors) can post messages in chat.
    /// Regular viewers can still see the chat but cannot participate until the mode is
    /// disabled. This is a moderation tool used to reduce chat volume or maintain
    /// member-exclusive discussions.
    #[serde(rename = "sponsorOnlyModeStartedEvent")]
    SponsorOnlyModeStarted,
    /// A system message indicating that sponsor-only mode was deactivated.
    ///
    /// This message appears when the broadcaster or moderators disable sponsor-only mode,
    /// allowing all viewers to post messages in chat again. The chat returns to normal
    /// participation mode where any viewer can send messages.
    #[serde(rename = "sponsorOnlyModeEndedEvent")]
    SponsorOnlyModeEnded,
}

impl fmt::Display for LiveChatMessageDetails {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TextMessage { .. } => write!(f, "text"),
            Self::SuperChat { .. } => write!(f, "superChat"),
            Self::SuperSticker { .. } => write!(f, "superSticker"),
            Self::NewSponsor { .. } => write!(f, "newSponsor"),
            Self::MemberMilestone { .. } => write!(f, "memberMilestone"),
            Self::MembershipGifting { .. } => write!(f, "membershipGift"),
            Self::GiftMembershipReceived { .. } => write!(f, "giftMembershipReceived"),
            Self::MessageDeleted { .. } => write!(f, "messageDeleted"),
            Self::UserBanned { .. } => write!(f, "userBanned"),
            Self::MessageRetracted => write!(f, "messageRetracted"),
            Self::ChatEnded => write!(f, "chatEnded"),
            Self::SponsorOnlyModeStarted => write!(f, "sponsorOnlyModeStarted"),
            Self::SponsorOnlyModeEnded => write!(f, "sponsorOnlyModeEnded"),
        }
    }
}

/// Details about the author of a live chat message.
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveChatMessages#authorDetails>
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveChatMessageAuthor {
    /// The unique YouTube channel ID of the message author.
    ///
    /// This is the persistent identifier for the user's YouTube channel and can be
    /// used to track messages from the same user across different chat sessions.
    pub channel_id: String,
    /// The display name of the channel as it appears in chat.
    ///
    /// This is the user's current channel name at the time the message was sent.
    /// Users can change their display names, so this represents the name that was
    /// visible to other chat participants when the message was posted.
    pub display_name: String,
    /// URL to the channel's profile image/avatar.
    ///
    /// This is the URL to the user's channel avatar image as it appeared when the
    /// message was sent. The image is typically displayed next to messages in chat
    /// interfaces to help users identify message authors.
    pub profile_image_url: String,
    /// Whether the author has a verified channel badge.
    ///
    /// YouTube verified channels display a checkmark badge indicating they are
    /// authentic channels of public figures, celebrities, or well-known brands.
    /// This affects how the user's name and badge are displayed in chat.
    pub is_verified: bool,
    /// Whether the author is the owner/broadcaster of the live stream.
    ///
    /// `true` if this message was sent by the channel that is hosting the live stream.
    /// Chat owners typically have special privileges and distinctive visual styling
    /// in chat interfaces to distinguish them from regular viewers.
    pub is_chat_owner: bool,
    /// Whether the author is a channel member (sponsor).
    ///
    /// `true` if the user has purchased a channel membership. Members typically
    /// receive special badges, emoji privileges, and other perks. Their messages
    /// may be visually distinguished in chat with member badges or styling.
    pub is_chat_sponsor: bool,
    /// Whether the author is a chat moderator.
    ///
    /// `true` if the user has been granted moderator privileges for this channel's
    /// chat. Moderators can delete messages, ban users, and perform other moderation
    /// actions. They typically have special badges and visual styling in chat.
    pub is_chat_moderator: bool,
}

/// Details about a Super Chat purchase.
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveChatMessages#snippet.superChatDetails>
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuperChatDetails {
    /// A localized string displaying the purchase amount and currency for user interfaces.
    ///
    /// This is a formatted string like "$5.00" or "¥500" that's ready for display to users.
    /// The formatting follows the locale conventions for the currency used.
    pub amount_display_string: String,
    /// The purchase amount in micros (millionths of the currency unit).
    ///
    /// For example, $1.75 would be represented as "1750000" (1.75 * 1,000,000).
    /// This allows precise representation of monetary amounts without floating point issues.
    /// Always parse as a string since the value may exceed standard integer limits.
    pub amount_micros: String,
    /// The currency code in ISO 4217 format (e.g., "USD", "EUR", "JPY").
    ///
    /// Determines both the currency symbol in `amount_display_string` and the
    /// value interpretation of `amount_micros`. Different currencies may have
    /// different micro conversion rates.
    pub currency: String,
    /// The Super Chat tier level, determining visual styling and pin duration.
    ///
    /// Higher payment amounts correspond to higher tier numbers, which typically
    /// results in more prominent visual styling (brighter colors, longer pin duration
    /// at the top of chat). Tier boundaries vary by currency and region.
    pub tier: u32,
    /// Optional text message included with the Super Chat purchase.
    ///
    /// Users can include a custom message along with their Super Chat payment.
    /// If present, this message appears alongside the payment information in chat.
    /// May be absent if the user chose to send only a payment without text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_comment: Option<String>,
}

/// Details about a Super Sticker purchase.
///
/// See: <https://developers.google.com/youtube/v3/live/docs/liveChatMessages#snippet.superStickerDetails>
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuperStickerDetails {
    /// A localized string displaying the purchase amount and currency for user interfaces.
    ///
    /// This is a formatted string like "$2.00" or "¥200" that's ready for display to users.
    /// The formatting follows the locale conventions for the currency used.
    pub amount_display_string: String,
    /// The purchase amount in micros (millionths of the currency unit).
    ///
    /// For example, $2.00 would be represented as "2000000" (2.00 * 1,000,000).
    /// This allows precise representation of monetary amounts without floating point issues.
    /// Always parse as a string since the value may exceed standard integer limits.
    pub amount_micros: String,
    /// The currency code in ISO 4217 format (e.g., "USD", "EUR", "JPY").
    ///
    /// Determines both the currency symbol in `amount_display_string` and the
    /// value interpretation of `amount_micros`. Different currencies may have
    /// different micro conversion rates and available sticker price points.
    pub currency: String,
    /// The Super Sticker tier level, determining visual styling and pin duration.
    ///
    /// Higher payment amounts correspond to higher tier numbers, which typically
    /// results in more prominent visual styling and longer pin duration at the top
    /// of chat. Tier boundaries vary by currency and region.
    pub tier: u32,
    /// Metadata describing the specific animated sticker that was purchased.
    ///
    /// Contains identification and localization information for the sticker asset.
    /// Each sticker has unique visual content and may be localized for different languages.
    pub super_sticker_metadata: SuperStickerMetadata,
}

/// Metadata about a Super Sticker.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuperStickerMetadata {
    /// Unique identifier for this specific sticker within YouTube's sticker catalog.
    ///
    /// This ID can be used to identify the exact sticker asset for rendering or
    /// analytics purposes. Different stickers have different visual designs and animations.
    pub sticker_id: String,
    /// Human-readable alternative text describing the sticker for accessibility.
    ///
    /// This text describes the sticker's visual content and is used for screen readers
    /// and other accessibility tools. Also serves as a fallback display name when
    /// the sticker image cannot be rendered.
    pub alt_text: String,
    /// The language code for this sticker's localization.
    ///
    /// Some stickers may have different visual designs or text content based on
    /// the viewer's language preferences. This field indicates which localized
    /// version of the sticker was used.
    pub language: String,
}

/// Details about a text message.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextMessageDetails {
    /// The raw text content of the message as entered by the user.
    ///
    /// This contains the original message text without any formatting or processing.
    /// Note that the formatted version for display is available in the parent
    /// `LiveChatMessageSnippet.display_message` field, which may include additional
    /// formatting, emoji rendering, or link processing.
    pub message_text: String,
}

/// Details about a member milestone chat message.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberMilestoneChatDetails {
    /// Optional custom message included by the member with their milestone celebration.
    ///
    /// When members reach milestone durations (like 6 months, 1 year of membership),
    /// they can optionally include a personal message along with the automated milestone
    /// announcement. This field contains that custom message if provided.
    pub user_comment: Option<String>,
    /// The number of months the member has been subscribed to the channel.
    ///
    /// This represents the milestone duration being celebrated. Common milestone
    /// values include 1, 2, 3, 6, 12, 24, etc., depending on the channel's
    /// membership milestone configuration.
    pub member_month: u32,
    /// The name of the membership level the user subscribed to.
    ///
    /// Channels can offer multiple tiers of membership with different names and benefits.
    /// This field identifies which specific membership tier the milestone applies to
    /// (e.g., "Member", "VIP Member", "Supporter").
    pub member_level_name: String,
}

/// Details about a new sponsor event.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewSponsorDetails {
    /// The name of the membership level the user joined or upgraded to.
    ///
    /// Channels can offer multiple tiers of membership with different names and benefits.
    /// This field identifies which specific membership tier the user selected
    /// (e.g., "Member", "VIP Member", "Supporter").
    pub member_level_name: String,
    /// Whether this represents an upgrade from a previous membership level.
    ///
    /// `true` if the user already had a channel membership and upgraded to a higher tier.
    /// `false` if this is their first time becoming a channel member. Upgrades typically
    /// result in different visual styling or messaging compared to first-time memberships.
    pub is_upgrade: bool,
}

/// Details about a user ban event.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserBannedDetails {
    /// Information about the user who was banned from the chat.
    ///
    /// Contains the banned user's channel details including their display name,
    /// profile information, and channel identifiers. This information is preserved
    /// even after the ban to maintain chat context and moderation records.
    pub banned_user_details: BannedUserDetails,
    /// The type of ban that was applied to the user.
    ///
    /// Common values include "permanent" for permanent bans and "temporary" for
    /// time-limited bans. The specific ban type affects the user's ability to
    /// participate in future chat sessions.
    pub ban_type: String,
    /// Duration of the ban in seconds, if it's a temporary ban.
    ///
    /// Only present for temporary bans. When the duration expires, the user's
    /// ability to participate in chat is automatically restored. Permanent bans
    /// will not have this field set.
    pub ban_duration_seconds: Option<u64>,
}

/// Details about the banned user.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BannedUserDetails {
    /// The YouTube channel ID of the banned user.
    ///
    /// This is the unique identifier for the user's YouTube channel and can be
    /// used to identify the banned user across different chat sessions or streams.
    pub channel_id: String,
    /// The full YouTube channel URL for the banned user.
    ///
    /// This is the complete URL (e.g., "https://www.youtube.com/channel/UC...")
    /// that would link to the user's channel page, allowing moderators to view
    /// the user's profile and content history if needed.
    pub channel_url: String,
    /// The display name of the banned user at the time of the ban.
    ///
    /// This is the user's channel name as it appeared in chat when they were banned.
    /// Note that users can change their display names, so this represents a snapshot
    /// at the time of the moderation action.
    pub display_name: String,
    /// URL to the banned user's profile image.
    ///
    /// This is the URL to the user's channel avatar image as it appeared at the time
    /// of the ban. Like the display name, this represents a snapshot and may not
    /// reflect the user's current profile image.
    pub profile_image_url: String,
}

/// Details about a membership gifting event.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MembershipGiftingDetails {
    /// The number of channel memberships that were purchased as gifts.
    ///
    /// This represents the total count of memberships gifted in this single transaction.
    /// Each recipient will receive a separate `giftMembershipReceivedEvent` message,
    /// so this number indicates how many such recipient messages to expect.
    pub gift_memberships_count: u32,
    /// The name of the membership level that was gifted.
    ///
    /// All memberships in a single gifting transaction are for the same membership tier.
    /// This field identifies which specific membership level was purchased as gifts
    /// (e.g., "Member", "VIP Member", "Supporter").
    pub gift_memberships_level_name: String,
}

/// Details about receiving a membership gift.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GiftMembershipReceivedDetails {
    /// The name of the membership level that was received as a gift.
    ///
    /// This identifies which specific membership tier the recipient received
    /// (e.g., "Member", "VIP Member", "Supporter"). This should match the
    /// `gift_memberships_level_name` in the associated gifting message.
    pub member_level_name: String,
    /// The channel ID of the user who purchased and gifted the membership.
    ///
    /// This identifies the generous viewer who paid for the membership gift.
    /// The gifter's information may also be available in the associated
    /// `membershipGiftingEvent` message.
    pub gifter_channel_id: String,
    /// The message ID of the associated membership gifting event.
    ///
    /// This links back to the original `membershipGiftingEvent` message that
    /// announced the bulk gift purchase. Multiple recipients can share the
    /// same associated gifting message ID when memberships were bought in bulk.
    pub associated_membership_gifting_message_id: String,
}

/// Details about a deleted message.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageDeletedDetails {
    /// The unique ID of the message that was deleted by moderators.
    ///
    /// This references the original message that was removed from chat.
    /// The deleted message's content is no longer available, but this ID
    /// can be used to correlate the deletion event with moderation logs
    /// or other chat analysis systems.
    pub deleted_message_id: String,
}
