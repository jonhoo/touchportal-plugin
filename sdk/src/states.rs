use derive_builder::Builder;
use indexmap::IndexSet;
use serde::{Deserialize, Serialize};

/// In Touch Portal the user can use States which can be used by IF statement and with Events for
/// example but can also be used in button texts or most actions.
///
/// With your plugin you can add states to Touch Portal that represent states from the software you
/// are integrating as a plug-in. You can define a state as part of a category. Events can link to
/// the id of the states to be able to act on changes of those states for example.
///
/// Please note: when a user makes a reference to any of the states from a plug-in in their actions
/// they are stored in that text locally. When you change the state id for example in your plugin,
/// all existing references to the old state are not updated and will result in null errors and no
/// conversion will be done. Only change ID's when you are absolutely sure it will not break
/// anything for your users.
#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    /// This is the id of the state.
    ///
    /// It is used to identify the states within Touch Portal. This id needs to be unique across
    /// plugins. This means that if you give it the id "1" there is a big chance that it will be a
    /// duplicate. Touch Portal may reject it or when the other state is updated, yours will be as
    /// well with wrong data. Best practice is to create a unique prefix for all your states like
    /// in our case; `tp_sid_fruit`.
    #[builder(setter(into))]
    pub(crate) id: String,

    /// This text describes the state and is used in the IF statement to let the user see what
    /// state it is changing.
    ///
    /// We recommend to make this text work in the flow of the inline nature of the IF statement
    /// within Touch Portal. This is also the title that is used in list where you can use this
    /// state value for logic execution.
    #[builder(setter(into))]
    #[serde(rename = "desc")]
    pub(crate) description: String,

    /// This is the value the state will have if it is not set already but is looked up.
    #[builder(setter(into))]
    #[serde(rename = "default")]
    initial: String,

    #[serde(flatten)]
    pub(crate) kind: StateType,

    /// The name of the parent group of this state.
    ///
    /// The parent group of this state will be used to group the state in the menus used throughout
    /// Touch Portal. Every state belonging to the same parent group name will be in the same
    /// selection menu.
    #[builder(setter(into, strip_option), default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_group: Option<String>,
}

impl State {
    pub fn builder() -> StateBuilder {
        StateBuilder::default()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "lowercase")]
#[serde(tag = "type")]
pub enum StateType {
    /// A state where you specify a limited amount of state values the state can be.
    Choice(ChoiceState),

    /// A state that contains a free text field.
    ///
    /// This type can be used for smart conversion as well.
    ///
    /// `#FF115599` (`#AARRGGBB`) can be interpreted by the plug-in visuals action as a color. The
    /// format needs to be this or it will not be seen as a color and will not be converted.
    ///
    /// A base64 representation of an image will also be allowed for specific actions such as the
    /// plug-in visuals action. This will read the base64 string representation and convert it to
    /// an image and show it on the button. We suggest to keep these as small as possible. Images
    /// used like this on a button are not stored and only exist temporary. This allows for a
    /// performant updating process. Allow for multiple updates per second depending on the
    /// computer used, the device used and the quality of the network.
    ///
    /// The base64 string should only hold the base64 data. The meta data should be stripped. The
    /// format has to be a PNG. It has to be a squared image.
    Text(TextState),
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChoiceState {
    /// Specify the collection of values that can be used to choose from.
    ///
    /// These can also be dynamically changed if you use the dynamic actions.
    #[builder(setter(each(name = "choice", into)))]
    #[serde(rename = "valueChoices")]
    pub(crate) choices: IndexSet<String>,
}

impl ChoiceState {
    pub fn builder() -> ChoiceStateBuilder {
        ChoiceStateBuilder::default()
    }
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextState {}

impl TextState {
    pub fn builder() -> TextStateBuilder {
        TextStateBuilder::default()
    }
}

#[test]
fn serialize_example_state() {
    assert_eq!(
        serde_json::to_value(
            State::builder()
                .id("tp_sid_fruit")
                .description("Fruit Kind description")
                .initial("Apple")
                .parent_group("Fruits")
                .kind(StateType::Choice(
                    ChoiceState::builder()
                        .choice("Apple")
                        .choice("Pears")
                        .choice("Grapes")
                        .choice("Bananas")
                        .build()
                        .unwrap()
                ))
                .build()
                .unwrap()
        )
        .unwrap(),
        serde_json::json! {{
          "id":"tp_sid_fruit",
          "type":"choice",
          "desc":"Fruit Kind description",
          "default":"Apple",
          "parentGroup":"Fruits",
          "valueChoices": [
            "Apple",
            "Pears",
            "Grapes",
            "Bananas"
          ]
        }}
    );
}
