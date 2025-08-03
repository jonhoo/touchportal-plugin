use derive_builder::Builder;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum TouchPortalCommand {
    Pair(PairCommand),
    CreateState(CreateStateCommand),
    CreateNotification(CreateNotificationCommand),
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

/// As a plug-in developer you can alert your users within Touch Portal for certain events.
///
/// This system should only be used for important messages that the user has to act on. Examples
/// are new updates for the plugin or changing settings like credentials. Maybe your user has set
/// up the plug-in incorrectly which is also a good reason to send a notification to alert them to
/// the issue and propose a solution.
///
/// <div class="warning">
///
/// **Rules of notifications**
///
/// You are only allowed to send user critical notifications to help them on their way.
/// Advertisements, donation request and all other non-essential messages are not allowed and may
/// result in your plug-in be blacklisted from the notification center.
///
/// </div>
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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum TouchPortalOutput {
    Info(InfoMessage),
    Action(ActionMessage),
    Up(HoldMessage),
    Down(HoldMessage),
    ConnectorChange(ConnectorChangeMessage),
    ShortConnectorIdNotification(ShortConnectorIdMessage),
    ListChange(ListChangeMessage),
    ClosePlugin(ClosePluginMessage),
    Broadcast(BroadcastEvent),
    NotificationOptionClicked(NotificationClickedMessage),
}

/// This message contains data about the Touch Portal application being used, it will contain plugin settings if set and it will send the current page information for all connected devices.
///
/// This last part will only contain information when the plugin is restarted during Touch Portal use. This page information will be empty when the Plugin pairs at Touch Portal startup.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct InfoMessage {
    pub sdk_version: super::ApiVersion,
    pub tp_version_string: String,
    /// Version of Touch Portal in code format (e.g., 4.4.2.0.0 is 404002)
    pub tp_version_code: u64,
    /// Currently installed version of this plugin.
    ///
    /// None if this is first install.
    #[serde(default)]
    pub plugin_version: Option<u16>,
    /// Currently set settings for this plugin.
    ///
    /// None if this is first install.
    #[serde(default)]
    pub settings: Vec<HashMap<String, serde_json::Value>>,
    /// Relative path of the page including extension
    ///
    /// Only available on API version 9 and above.
    #[serde(default)]
    pub current_page_path_main_device: Option<String>,
    /// Only available on API version 9 and above.
    #[serde(default)]
    pub current_page_path_secondary_devices: Vec<DevicePage>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct DevicePage {
    pub tp_device_id: String,
    pub current_page_path: String,
    pub device_name: String,
}

/// Touch Portal will send messages when an action is being triggered (when the button containing
/// one of your plug-in actions is pressed or when an event is triggered that contains your
/// action.)
///
/// Your plug-in software needs to handle these messages and act on it where applicable.
///
/// The following JSON Structure will be send to the plug-in. The message will hold a reference to
/// your plugin id and the action. The action data as setup by the user will also be send in the
/// data object.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ActionMessage {
    plugin_id: String,
    #[doc(hidden = "handled transparently by codegen")]
    pub action_id: String,
    #[doc(hidden = "handled transparently by codegen")]
    pub data: Vec<HashMap<String, String>>,
}

/// Touch Portal will send messages to your plugin when the action is used in a hold button event.
///
/// When the user presses the Touch Portal button down, Touch Portal will send the "down" event.
/// When the user releases the button, Touch Portal will send the "up" event. Only actions that
/// have hold settings can be used in Touch Portal in the Hold tab.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct HoldMessage {
    plugin_id: String,
    action_id: String,
    data: Vec<HashMap<String, String>>,
}

/// Touch Portal will send messages to your plugin when the connector is used in a connector event.
///
/// Currently this is when a user has connected the connector to a slider and uses the slider
/// control to change the value. This will trigger the `connectorChange` type of message. The value
/// is an integer number ranging from 0 to 100.
///
/// Touch Portal will send the connector data and value in the following way:
///
/// - On Finger Down, is always send
/// - On Finger Move, send each 100ms interval if value changed.
/// - On Finger Up, is always send
///
/// Touch Portal will send a value when the user presses his finger on the associated slider. While
/// the finger is still pressing on the slider control it will send every 100ms the value. If the
/// value is not updated because the finger does not move it will not resend the same value. The
/// 100ms is also an indication and can be slower on different set ups and network quality. The
/// minimum however is 100ms.
///
/// The slider will always send at least two messages. When the user presses the slider like a
/// button, the same value will be send twice due to the UP and DOWN event.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ConnectorChangeMessage {
    plugin_id: String,
    connector_id: String,
    /// Value between 0 and 100 for sliders.
    value: Option<u32>,
    /// Double value for dials.
    value_decimal: Option<f64>,
    data: Vec<HashMap<String, String>>,
}

/// Whenever a user creates a connector for the first time a `shortId` is generated for that
/// connector that represents the long `connectorId`.
///
/// This short id is useful for when you create long connector ids and the id will be longer than
/// the max of 200 characters.
///
/// You can use this `shortId` instead of the long `connectorId` to update the connector value in
/// Touch Portal.
///
/// This message can be send by Touch Portal on several occassions and can be sent multiple times
/// per `connectorId` and `shortId` combination.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ShortConnectorIdMessage {
    plugin_id: String,
    short_id: String,
    connector_id: String,
}

/// Touch Portal will send messages when a list of choices value is changed.
///
/// Your software needs to handle these messages and act on it if you want to use this
/// functionality. This is especially useful when your action (or event/connector) has multiple
/// drop down list boxes where selecting an item in the first needs to repopulate the second.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ListChangeMessage {
    plugin_id: String,
    action_id: String,
    list_id: String,
    instance_id: String,

    /// Holds all user input in the action the list belongs to.
    ///
    /// Be aware that this is during editting so it could be no values are input yet or all are.
    /// This array does not necessarily be in the same order as the data is in the action itself.
    ///
    /// Only available on API version 7 and above.
    #[serde(default)]
    values: Vec<IdValuePair>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct IdValuePair {
    id: String,
    value: String,
}

/// Touch Portal will send a message when it is closing the plugin for some reason.
///
/// Touch Portal will also try to close the process. This will happen approximately after 500 ms.
/// This will only happen if the process is being started through the entry.tp start command
/// attribute. This means that if this close call is received, be sure to properly shut down the
/// plugin application/service otherwise it may be hard killed by Touch Portal.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ClosePluginMessage {
    plugin_id: String,
}

/// Touch Portal will send messages to the plug-in at certain events.
///
/// Currently the only message that is broadcast is the page change event. You can use this
/// broadcast for example to resend states whenever a page is loaded. This will allow the user to
/// get the latest states just as a page is loaded. Here is an example of the message received:
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "event")]
#[non_exhaustive]
pub enum BroadcastEvent {
    PageChange(BroadcastPageChangeEvent),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct BroadcastPageChangeEvent {
    /// The name of the page navigated to.
    page_name: String,
    /// The name of the page navigated from.
    ///
    /// Only available on API version 8 and above.
    previous_page_name: Option<String>,
    /// The device ip of the device navigating pages.
    ///
    /// Only available on API version 8 and above.
    device_ip: Option<String>,
    /// The device name of the device navigating pages.
    ///
    /// Only available on API version 8 and above.
    device_name: Option<String>,
    /// The device id (set for multiple devices upgrade) of the device navigating pages.
    ///
    /// Only available on API version 9 and above.
    device_id: Option<String>,
}

/// Touch Portal will send a message when a user clicks on a notification action.
///
/// When they do the notification is also marked as read/handled.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct NotificationClickedMessage {
    notification_id: String,
    option_id: String,
}
