//! Notification helpers for consistent user messaging across the plugin.

use crate::plugin::TouchPortalHandle;
use touchportal_sdk::protocol::{CreateNotificationCommand, NotificationOption};

/// Notify user that no channel has been selected yet.
pub async fn no_channel_selected(tp: &mut TouchPortalHandle) -> eyre::Result<()> {
    tp.notify(
        CreateNotificationCommand::builder()
            .notification_id("ytl_no_channel_selected")
            .title("No channel selected")
            .message(
                "Please use the 'Select stream' action to choose a channel and broadcast first.",
            )
            .option(
                NotificationOption::builder()
                    .id("ok")
                    .title("OK")
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap(),
    )
    .await;
    Ok(())
}

/// Notify user that no broadcast has been selected yet.
pub async fn no_broadcast_selected(tp: &mut TouchPortalHandle) -> eyre::Result<()> {
    tp.notify(
        CreateNotificationCommand::builder()
            .notification_id("ytl_no_broadcast_selected")
            .title("No broadcast selected")
            .message("Please use the 'Select stream' action to choose a broadcast from the channel first.")
            .option(
                NotificationOption::builder()
                    .id("ok")
                    .title("OK")
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap(),
    )
    .await;
    Ok(())
}

/// Notify user that the selected channel is no longer available.
pub async fn channel_not_available(tp: &mut TouchPortalHandle) -> eyre::Result<()> {
    tp.notify(
        CreateNotificationCommand::builder()
            .notification_id("ytl_channel_not_available")
            .title("Channel not available")
            .message("The selected channel is no longer available. Please authenticate the channel again or select a different one.")
            .option(
                NotificationOption::builder()
                    .id("ok")
                    .title("OK")
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap(),
    )
    .await;
    Ok(())
}

/// Remind user to run the "Select Stream" action to save their selection.
pub async fn remind_to_save_selection(tp: &mut TouchPortalHandle) -> eyre::Result<()> {
    tp.notify(
        CreateNotificationCommand::builder()
            .notification_id("ytl_save_selection_reminder")
            .title("Selection ready")
            .message("Please run the 'Select stream' action to save your channel and broadcast selection.")
            .option(
                NotificationOption::builder()
                    .id("ok")
                    .title("OK")
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap(),
    )
    .await;
    Ok(())
}

/// Notify user they need to add a YouTube account first.
pub async fn need_to_add_youtube_account(tp: &mut TouchPortalHandle) -> eyre::Result<()> {
    tp.notify(
        CreateNotificationCommand::builder()
            .notification_id("ytl_no_channels_available")
            .title("No YouTube channels")
            .message(
                "You need to add a YouTube channel first. \
                Use the 'Add YouTube channel' action to authenticate and add your account.",
            )
            .option(
                NotificationOption::builder()
                    .id("ok")
                    .title("OK")
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap(),
    )
    .await;
    Ok(())
}

/// Notify user they need to select a channel first to see broadcasts.
pub async fn need_to_select_channel_first(tp: &mut TouchPortalHandle) -> eyre::Result<()> {
    tp.notify(
        CreateNotificationCommand::builder()
            .notification_id("ytl_select_channel_first")
            .title("Channel selection required")
            .message(
                "Please select a channel first to see available broadcasts. \
                The broadcast list will update automatically once a channel is selected.",
            )
            .option(
                NotificationOption::builder()
                    .id("ok")
                    .title("OK")
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap(),
    )
    .await;
    Ok(())
}

/// Notify user that their selected broadcast is no longer available.
pub async fn selected_broadcast_not_available(tp: &mut TouchPortalHandle) -> eyre::Result<()> {
    tp.notify(
        CreateNotificationCommand::builder()
            .notification_id("ytl_broadcast_not_available")
            .title("Broadcast not available")
            .message("The selected broadcast is no longer available - it may have finished, been deleted, or had chat disabled. Please select a different broadcast or choose 'Latest' to wait for the next live stream.")
            .option(
                NotificationOption::builder()
                    .id("ok")
                    .title("OK")
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap(),
    )
    .await;
    Ok(())
}
