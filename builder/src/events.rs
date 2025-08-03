use super::PluginCategory;
use derive_builder::Builder;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// In Touch Portal there are events which will be triggered when a certain state changes.
///
/// You can create events for the plugin as well. These events can be triggered when a linked state
/// is changed.
///
/// Please note: when a user adds an event belonging to a plugin, it will create a local copy of
/// the event and saves it along with the event. This means that if you change something in your
/// event the users need to remove their instance of that event and re-add it to be able to use the
/// new additions.
#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    /// This is the id of the event.
    ///
    /// When the event is triggered, Touch Portal will send this information to the plugin with this id.
    #[builder(setter(into))]
    pub(crate) id: String,

    /// This is the name in the action category list.
    #[builder(setter(into))]
    pub(crate) name: String,

    /// This is the text the action will show in the user generated action list.
    ///
    /// The `$val` location will be changed with a dropdown holding the choices that the user can
    /// make for the status.
    #[builder(setter(into))]
    format: String,

    /// Currently the only option here is "communicate" which indicates that the value will be
    /// communicated through the sockets.
    #[builder(setter(skip), default)]
    #[serde(rename = "type")]
    _type: EventType,

    #[serde(flatten)]
    pub(crate) value: EventValueType,

    /// Reference to a state.
    ///
    /// When this states changes, this event will be evaluated and possibly triggered if the
    /// condition is correct. Can be empty but is mandatory.
    #[builder(setter(into), default)]
    pub(crate) value_state_id: String,

    /// This attribute allows you to connect this event to a specified subcategory id.
    ///
    /// This event will then be shown in Touch Portals Action selection list attached to that
    /// subcategory instead of the main parent category.
    #[builder(setter(strip_option), default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    sub_category_id: Option<PluginCategory>,

    /// Array of all Local State objects related to this event.
    ///
    /// These can be selected by the user only when the event is used and added. If not added, the
    /// local states will not be shown in the state selector popups.
    ///
    /// Only available in API version 10 and above.
    #[serde(rename = "localstates")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[builder(setter(each(name = "local_state")), default)]
    pub(crate) local_states: Vec<LocalState>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[non_exhaustive]
#[serde(rename_all = "lowercase")]
enum EventType {
    #[default]
    Communicate,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "lowercase")]
#[serde(tag = "valueType")]
pub enum EventValueType {
    /// Indicates that the type of event will be an dropdown with predefined values.
    Choice(EventChoiceValue),

    /// This will check whether the state is the same as the user specified value in the text box.
    Text,
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventChoiceValue {
    /// These are all the options the user can select in the event.
    #[builder(setter(each(name = "choice", into)))]
    #[serde(rename = "valueChoices")]
    pub(crate) choices: BTreeSet<String>,
}

/// The local states object represents the representation and visualisation within Touch Portal.
///
/// The id is the reference when used as a tag in text. The actual setting of the local states
/// object when the event is triggered are described in the communication section.
#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalState {
    /// This id of the local state.
    #[builder(setter(into))]
    id: String,

    /// This name of the local state.
    #[builder(setter(into))]
    name: String,

    /// The parent category the local state belongs to.
    #[builder(setter(strip_option), default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_category: Option<PluginCategory>,
}

#[test]
fn serialize_example_event() {
    assert_eq!(
        serde_json::to_value(
            EventBuilder::default()
                .id("event002")
                .name("On breakfast eating")
                .format("When we eat $val as breakfast")
                .value(EventValueType::Choice(
                    EventChoiceValueBuilder::default()
                        .choice("Apple")
                        .choice("Pears")
                        .choice("Grapes")
                        .choice("Bananas")
                        .build()
                        .unwrap()
                ))
                .value_state_id("fruit")
                .build()
                .unwrap()
        )
        .unwrap(),
        serde_json::json! {{
          "id":"event002",
          "name":"On breakfast eating",
          "format":"When we eat $val as breakfast",
          "type":"communicate",
          "valueType":"choice",
          "valueChoices": [
            "Apple",
            "Pears",
            "Grapes",
            "Bananas",
          ],
          "valueStateId":"fruit"
        }}
    );
}
