use derive_builder::Builder;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum TouchPortalCommand {
    Pair(PairCommand),
    CreateState(CreateStateCommand),
    CreateNotification(CreateNotificationCommand),
    StateUpdate(UpdateStateCommand),
    SettingUpdate(UpdateSettingCommand),
    TriggerEvent(TriggerEventCommand),
    RemoveState(RemoveStateCommand),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairCommand {
    pub id: String,
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateStateCommand {
    /// The id of the newly created plug-in state.
    ///
    /// Please ensure unique names, otherwise you may corrupt other plug-ins.
    #[builder(setter(into))]
    id: String,

    /// The displayed name within Touch Portal which represents the state.
    #[builder(setter(into))]
    #[serde(rename = "desc")]
    description: String,

    /// The default value the state will have on creation.
    #[builder(setter(into))]
    #[serde(rename = "default")]
    initial: String,

    /// The name of the parent group of this state.
    ///
    /// The parent group of this state will be used to group the state in the menus used throughout
    /// Touch Portal. Every state belonging to the same parent group name will be in the same
    /// selection menu.
    ///
    /// Only available on API version 6 and above.
    #[builder(setter(into, strip_option), default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_group: Option<String>,

    /// This will force the update of the state if it is already created or existing and will
    /// trigger the state changed event even if the value is the same as the already existing one.
    #[builder(setter(strip_option), default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    force_update: Option<bool>,
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateNotificationCommand {
    /// This is the id of this notification.
    ///
    /// Every notification with a unique id will have its own entry in the notification center. The
    /// same id should be used for the same kind of message to the user. For example; if you want
    /// to show a notification to update to a specific version, use the same id each time you send
    /// this notification. This will just show the one notification to the user.
    #[builder(setter(into))]
    notification_id: String,

    /// This is the title of the notification.
    #[builder(setter(into))]
    title: String,

    /// This is the message that is shown in the notification to the user.
    #[builder(setter(into))]
    #[serde(rename = "msg")]
    message: String,

    /// This is the collection of options to go with your notification.
    ///
    /// When a user clicks on the action it will be send to the plugin. The plug-in then can react on the choice the user made. Usually this will contain only one option such as an "Update" or "More Info" option. At least one option is required.
    #[builder(setter(each(name = "option")), default)]
    options: Vec<NotificationOption>,
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct NotificationOption {
    /// This is the id of the notification option.
    ///
    /// This id will be send back to the plug-in if the user selects the option.
    #[builder(setter(into))]
    id: String,

    /// This is the title of the notification option.
    #[builder(setter(into))]
    title: String,
}

/// You can send state updates to Touch Portal.
///
/// More information about states and how to set them up in the description file can be found in
/// the states section. You can only change the states from your own plug-in. Changing states of
/// Touch Portal itself may result in undesired behaviour.
#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStateCommand {
    /// The state id to set/update
    #[builder(setter(into))]
    #[serde(rename = "id")]
    state_id: String,

    /// The value of the state.
    ///
    /// Ensure this is a text and nothing else. Touch Portal will handle this value as a piece of text (string).
    #[builder(setter(into))]
    value: String,
}

/// With this option you can update a setting from your plug-in.
///
/// This will overwrite the user setting.
#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSettingCommand {
    /// The name of the settings, should be case sensitive correct
    #[builder(setter(into))]
    name: String,

    /// The new value the setting should hold
    #[builder(setter(into))]
    value: String,
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerEventCommand {
    /// The event id to trigger.
    #[builder(setter(into))]
    event_id: String,

    /// This is a JSON Object that holds key value pairs of data that are used within Touch Portal
    /// as Local States.
    ///
    /// Only available on API version 10 and above.
    #[builder(setter(each(name = "state")), default)]
    states: HashMap<String, String>,
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveStateCommand {
    /// The id of the plug-in state to remove.
    #[builder(setter(into))]
    id: String,
}
