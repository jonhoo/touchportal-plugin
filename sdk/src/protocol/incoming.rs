use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum TouchPortalOutput {
    Info(InfoMessage),
    Action(ActionMessage),
    Down(ActionMessage),
    Up(ActionMessage),
    ConnectorChange(ConnectorChangeMessage),
    ShortConnectorIdNotification(ShortConnectorIdMessage),
    ListChange(ListChangeMessage),
    ClosePlugin(ClosePluginMessage),
    Broadcast(BroadcastEvent),
    NotificationOptionClicked(NotificationClickedMessage),
    Settings(SettingsMessage),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ActionInteractionMode {
    Execute,
    HoldDown,
    HoldUp,
}

/// This message contains data about the Touch Portal application being used, it will contain plugin settings if set and it will send the current page information for all connected devices.
///
/// This last part will only contain information when the plugin is restarted during Touch Portal use. This page information will be empty when the Plugin pairs at Touch Portal startup.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct InfoMessage {
    pub sdk_version: crate::ApiVersion,
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
/// This same structure is also used for the hold events. When the user presses the Touch Portal
/// button down, Touch Portal will send the "down" event. When the user releases the button, Touch
/// Portal will send the "up" event. Only actions that have hold settings can be used in Touch
/// Portal in the Hold tab.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ActionMessage {
    pub plugin_id: String,
    #[doc(hidden = "handled transparently by codegen")]
    pub action_id: String,
    #[doc(hidden = "handled transparently by codegen")]
    pub data: Vec<IdValuePair>,
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
    pub plugin_id: String,
    pub connector_id: String,
    /// Value between 0 and 100 for sliders.
    pub value: Option<u32>,
    /// Double value for dials.
    pub value_decimal: Option<f64>,
    pub data: Vec<HashMap<String, String>>,
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
    #[doc(hidden = "handled transparently by codegen")]
    pub plugin_id: String,
    #[doc(hidden = "handled transparently by codegen")]
    pub action_id: String,
    #[doc(hidden = "handled transparently by codegen")]
    pub list_id: String,
    #[doc(hidden = "handled transparently by codegen")]
    pub instance_id: String,

    #[doc(hidden = "handled transparently by codegen")]
    pub value: String,

    /// Holds all user input in the action the list belongs to.
    ///
    /// Be aware that this is during editting so it could be no values are input yet or all are.
    /// This array does not necessarily be in the same order as the data is in the action itself.
    ///
    /// Only available on API version 7 and above.
    #[serde(default)]
    pub values: Vec<IdValuePair>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct IdValuePair {
    #[doc(hidden = "handled transparently by codegen")]
    pub id: String,
    #[doc(hidden = "handled transparently by codegen")]
    pub value: String,
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
    pub plugin_id: String,
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
    pub page_name: String,
    /// The name of the page navigated from.
    ///
    /// Only available on API version 8 and above.
    pub previous_page_name: Option<String>,
    /// The device ip of the device navigating pages.
    ///
    /// Only available on API version 8 and above.
    pub device_ip: Option<String>,
    /// The device name of the device navigating pages.
    ///
    /// Only available on API version 8 and above.
    pub device_name: Option<String>,
    /// The device id (set for multiple devices upgrade) of the device navigating pages.
    ///
    /// Only available on API version 9 and above.
    pub device_id: Option<String>,
}

/// Touch Portal will send a message when a user clicks on a notification action.
///
/// When they do the notification is also marked as read/handled.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct NotificationClickedMessage {
    pub notification_id: String,
    pub option_id: String,
}

/// Touch Portal sends this message when the user modifies and saves plugin settings.
///
/// This message contains the entire current state of all settings, both changed and unchanged.
/// Plugins can use this message to synchronize their internal configuration with user-modified
/// settings.
///
/// The message format matches the settings structure from InfoMessage, but is sent specifically
/// when settings are updated rather than during initial plugin pairing.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct SettingsMessage {
    /// Currently set settings for this plugin.
    ///
    /// Each setting is represented as a key-value pair where the key is the setting name
    /// and the value is the current setting value.
    pub values: Vec<HashMap<String, serde_json::Value>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_message_deserialization() {
        let json = r#"
        {
            "type": "settings",
            "values": [
                {"setting1": "value1"},
                {"setting2": "value2"}
            ]
        }"#;

        let parsed: TouchPortalOutput = serde_json::from_str(json).unwrap();

        if let TouchPortalOutput::Settings(settings) = parsed {
            assert_eq!(settings.values.len(), 2);
            assert_eq!(settings.values[0]["setting1"], "value1");
            assert_eq!(settings.values[1]["setting2"], "value2");
        } else {
            panic!("Expected Settings message, got {:?}", parsed);
        }
    }
}
