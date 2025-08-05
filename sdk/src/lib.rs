use derive_builder::Builder;
use hex_color::HexColor;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

// root should be single folder without spaces in the name
// entry.tp -- "description file"
// https://www.touch-portal.com/api/index.php?section=description_file
//
// cargo build --release

pub mod codegen;
pub use codegen::build;

pub mod reexport {
    pub use hex_color::HexColor;
}

pub fn entry_tp(plugin: &PluginDescription) -> String {
    serde_json::to_string(plugin).expect("every PluginDescription serializes")
}

pub mod protocol;

/// Mapping from TouchPortal version to API version.
#[derive(Debug, Clone, Copy, Deserialize_repr, Serialize_repr)]
#[non_exhaustive]
#[repr(u16)]
pub enum ApiVersion {
    V2_1 = 1,
    V2_2 = 2,
    V2_3 = 3,
    V3_0 = 4,
    V3_0_6 = 5,
    V3_0_11 = 6,
    V4_0 = 7,
    V4_1 = 8,
    V4_2 = 9,
    V4_3 = 10,
    V4_5 = 12,
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[builder(build_fn(validate = "Self::validate"))]
#[serde(rename_all = "camelCase")]
pub struct PluginDescription {
    /// The API version of Touch Portal this plugin is build for.
    #[serde(alias = "sdk")]
    api: ApiVersion,

    /// A number representing your own versioning.
    ///
    /// Currently this variable is not used by Touch Portal but may be used in the future. This
    /// should be an integer value. So only whole numbers, no decimals.
    ///
    /// This version value will be
    /// send back after the pairing has been done.
    version: u16,

    /// This is the name of the Plugin.
    ///
    /// This will show in Touch Portal in the settings section "Plug-ins".
    ///
    /// (From Touch Portal version 2.2)
    #[builder(setter(into))]
    name: String,

    /// This is the unique ID of the Plugin.
    ///
    /// Use an id that will only be used by you. So use a prefix for example.
    #[builder(setter(into))]
    id: String,

    /// This object is used to specify some configuration options of the plug-in.
    configuration: PluginConfiguration,

    /// Specify the path of execution here.
    ///
    /// You should be aware that it will be passed to the OS process exection service. This means
    /// you need to be aware of spaces and use absolute paths to your executable.
    ///
    /// If you use `%TP_PLUGIN_FOLDER%` in the text here, it will be replaced with the path to the
    /// base plugins folder. So append your plugins folder name to it as well to access your plugin
    /// base folder.
    ///
    /// This execution will be done when the plugin is loaded in the system and only if it is a
    /// valid plugin. Use this to start your own service that communicates with the Touch Portal
    /// plugin system.
    // needs rename to override rename_all
    #[serde(rename = "plugin_start_cmd")]
    #[builder(setter(into))]
    plugin_start_cmd: String,

    /// This is the same plugin_start_cmd but will only run on a Windows desktop.
    ///
    /// If this is specified Touch Portal will not run the default `plugin_start_cmd` when this plug-in is used on Windows but only this entry.
    ///
    /// Only available on API version 4 and above.
    #[serde(rename = "plugin_start_cmd_windows")]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(into, strip_option), default)]
    plugin_start_cmd_windows: Option<String>,

    /// This is the same plugin_start_cmd but will only run on a MacOS desktop.
    ///
    /// If this is specified Touch Portal will not run the default `plugin_start_cmd` when this plug-in is used on MacOS but only this entry.
    ///
    /// Only available on API version 4 and above.
    #[serde(rename = "plugin_start_cmd_mac")]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(into, strip_option), default)]
    plugin_start_cmd_mac: Option<String>,

    /// This is the same plugin_start_cmd but will only run on a Linux desktop.
    ///
    /// If this is specified Touch Portal will not run the default `plugin_start_cmd` when this plug-in is used on Linux but only this entry.
    ///
    /// Only available on API version 4 and above.
    #[serde(rename = "plugin_start_cmd_linux")]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(into, strip_option), default)]
    plugin_start_cmd_linux: Option<String>,

    /// This is the collection that holds all the action categories.
    ///
    /// Categories are used in the action list in Touch Portal.
    ///
    /// Each Category must contain at least an item such as an action, an event or an connector. More on this in the category section.
    #[builder(setter(each(name = "category")), default)]
    categories: Vec<Category>,

    /// This is the collection that holds all the settings for this plug-in
    ///
    /// Only available in API version 3 and above.
    #[builder(setter(each(name = "setting")), default)]
    settings: Vec<Setting>,

    /// This description text can be used to add information on the top of the plug-in settings
    /// page.
    ///
    /// You can use this to have a setup guide or important text to show when setting up your
    /// plug-in.
    ///
    /// In general this is normal text but it can be formatted a bit with the following
    /// functionality:
    ///
    /// - `\n` will create a new line.
    /// - `#` will create a sub header but only if it is the first character on a new line.
    /// - `1.` can be used to create a list item. It only works if the first characters of the line
    ///   are a numbers followed by a period.
    ///
    /// Only available in API version 10 and above.
    #[serde(skip_serializing_if = "String::is_empty")]
    #[builder(setter(into), default)]
    settings_description: String,
}

impl PluginDescription {
    pub fn builder() -> PluginDescriptionBuilder {
        PluginDescriptionBuilder::default()
    }
}

impl PluginDescriptionBuilder {
    fn validate(&self) -> Result<(), String> {
        let states = self.categories.iter().flatten().flat_map(|c| &c.states);
        let states_by_id: HashMap<_, _> = states.map(|s| (&s.id, s)).collect();

        let events = self.categories.iter().flatten().flat_map(|c| &c.events);
        for event in events {
            if event.value_state_id.is_empty() {
                continue;
            }

            let Some(state) = states_by_id.get(&event.value_state_id) else {
                return Err(format!(
                    "event {} references unknown state {}",
                    event.id, event.value_state_id
                ));
            };

            match (&state.kind, &event.value) {
                (StateType::Choice(state_choices), EventValueType::Choice(event_choices)) => {
                    if state_choices.choices != event_choices.choices {
                        return Err(format!(
                            "event {} references state {}, \
                            but they have diverging choice-sets",
                            event.id, event.value_state_id
                        ));
                    }
                }
                (StateType::Choice(_), EventValueType::Text(_)) => {
                    return Err(format!(
                        "event {} is of free-text type, \
                        but references state {} which is of choice type",
                        event.id, event.value_state_id
                    ))
                }
                (StateType::Text(_), EventValueType::Choice(_)) => {
                    return Err(format!(
                        "event {} is of choice type, \
                        but references state {} which is of free-text type",
                        event.id, event.value_state_id
                    ))
                }
                (StateType::Text(_), EventValueType::Text(_)) => {}
            }
        }

        // data ids don't need to be unique, but they should not differ in their definition!
        let mut data_by_id: HashMap<_, Vec<_>> = Default::default();
        for action in self.categories.iter().flatten().flat_map(|c| &c.actions) {
            for Data { id, format } in &action.data {
                data_by_id.entry(id).or_default().push(format);
            }
        }
        for (id, formats) in data_by_id {
            if formats.len() == 1 {
                continue;
            }
            for &format in &formats[1..] {
                match (formats[0], format) {
                    (DataFormat::Text(TextData { initial: _ }), DataFormat::Text(_)) => {}
                    (
                        DataFormat::Number(NumberData {
                            allow_decimals,
                            min_value,
                            max_value,
                            initial: _,
                        }),
                        DataFormat::Number(n2),
                    ) => {
                        if *allow_decimals != n2.allow_decimals
                            || *min_value != n2.min_value
                            || *max_value != n2.max_value
                        {
                            return Err(format!(
                                "data field {id} appears multiple times with different numeric definitions"
                            ));
                        }
                    }
                    (
                        DataFormat::Choice(ChoiceData {
                            initial: _,
                            value_choices,
                        }),
                        DataFormat::Choice(c2),
                    ) => {
                        if *value_choices != c2.value_choices {
                            return Err(format!(
                                "data field {id} appears multiple times with different choice definitions"
                            ));
                        }
                    }
                    (
                        DataFormat::File(FileData {
                            extensions,
                            initial: _,
                        }),
                        DataFormat::File(f2),
                    ) => {
                        if *extensions != f2.extensions {
                            return Err(format!(
                                "data field {id} appears multiple times with different file definitions"
                            ));
                        }
                    }
                    (DataFormat::Switch(SwitchData { initial: _ }), DataFormat::Switch(_))
                    | (DataFormat::Folder(FolderData { initial: _ }), DataFormat::Folder(_))
                    | (DataFormat::Color(ColorData { initial: _ }), DataFormat::Color(_)) => {}
                    (
                        DataFormat::LowerBound(BoundData {
                            initial: _,
                            min_value,
                            max_value,
                        }),
                        DataFormat::LowerBound(b2),
                    )
                    | (
                        DataFormat::UpperBound(BoundData {
                            initial: _,
                            min_value,
                            max_value,
                        }),
                        DataFormat::UpperBound(b2),
                    ) => {
                        if *min_value != b2.min_value || *max_value != b2.max_value {
                            return Err(format!(
                                "data field {id} appears multiple times with different bound definitions"
                            ));
                        }
                    }
                    (DataFormat::Text(_), _)
                    | (DataFormat::Number(_), _)
                    | (DataFormat::Switch(_), _)
                    | (DataFormat::Choice(_), _)
                    | (DataFormat::File(_), _)
                    | (DataFormat::Folder(_), _)
                    | (DataFormat::Color(_), _)
                    | (DataFormat::LowerBound(_), _)
                    | (DataFormat::UpperBound(_), _) => {
                        return Err(format!(
                            "data field {id} appears multiple times with different definitions"
                        ));
                    }
                }
            }
        }

        Ok(())
    }
}

/// A category in your plugin will be an action category in Touch Portal.
///
/// Users can open that category and select actions, events and/or connectors from that to use in
/// their buttons or sliders. A plugin can include as many categories as you want, but best
/// practise is to use them as actual categories. Group actions for the same software integration
/// in one category. This will allow the users have the best experience. Also keep in mind that if
/// the users do not like the additions of your setup, they can just remove the plugins.
#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    /// This is the id of the category.
    #[builder(setter(into))]
    id: String,

    /// This is the name of the category.
    #[builder(setter(into))]
    name: String,

    /// This is the absolute path to an icon for the category.
    ///
    /// You should place this in your plugin folder and reference it. If you use
    /// `%TP_PLUGIN_FOLDER%` in the text here, it will be replaced with the path to the folder
    /// containing all plug-ins.
    ///
    /// Images must be 32bit PNG files of size 24x24 that are, and should be white icons with a
    /// transparent background.
    ///
    /// Although colored icons are possible, they will be removed in the near future.
    #[builder(setter(into, strip_option), default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    imagepath: Option<String>,

    /// This is the collection that holds all the actions.
    #[builder(setter(each(name = "action")), default)]
    actions: Vec<Action>,

    /// This is the collection that holds all the events.
    #[builder(setter(each(name = "event")), default)]
    events: Vec<Event>,

    /// This is the collection that holds all the connectors.
    #[builder(setter(each(name = "connector")), default)]
    connectors: Vec<Connector>,

    /// This is the collection that holds all the states.
    #[builder(setter(each(name = "state")), default)]
    states: Vec<State>,

    /// This is the collection of sub categories that you can define.
    ///
    /// You can assign actions, events and connectors to these categories. This will allow you to
    /// add subcategories for you plugin that will be shown in the action selection control.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[builder(setter(each(name = "sub_category")), default)]
    sub_categories: Vec<SubCategory>,
}

impl Category {
    pub fn builder() -> CategoryBuilder {
        CategoryBuilder::default()
    }
}

/// Plugin Categories can have sub categories which will be used to add structure to your list of
/// actions, events and connectors.
#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubCategory {
    /// This is the id of the sub category.
    ///
    /// It is used to identify the sub category within Touch Portal. This id needs to be unique
    /// across plugins. This means that if you give it the id "1" there is a big chance that it
    /// will be a duplicate. Touch Portal may reject it or when the other state is updated, yours
    /// will be as well with wrong data. Best practice is to create a unique prefix for all your
    /// sub categories like in our case; `tp_subcat_groceries.fruit`.
    #[builder(setter(into))]
    id: String,

    /// The name of the sub category.
    ///
    /// This name will be used to display the category in the actions lists. It can may be used in
    /// different systems as well.
    #[builder(setter(into))]
    name: String,

    /// This is the absolute path to an icon for the category.
    ///
    /// You should place this in your plugin folder and reference it. If you use
    /// `%TP_PLUGIN_FOLDER%` in the text here, it will be replaced with the path to the folder
    /// containing all plug-ins.
    ///
    /// Images must be 32bit PNG files of size 24x24 that are, and should be white icons with a
    /// transparent background.
    ///
    /// Although colored icons are possible, they will be removed in the near future.
    #[builder(setter(into, strip_option), default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    imagepath: Option<String>,
}

impl SubCategory {
    pub fn builder() -> SubCategoryBuilder {
        SubCategoryBuilder::default()
    }
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginConfiguration {
    /// When users use your actions and events they will be rendered in their own flows.
    ///
    /// This attribute tells Touch Portal which dark color to use in those actions and events. When
    /// this is not specified the default plug-in colors will be used in Touch Portal. Preferably
    /// use the color schemes of the software or product you are making a plug-in for to increase
    /// recognizability.
    ///
    /// Note: these color will be ignored in some of the themes within Touch Portal. There is no
    /// way to override this behaviour.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(into, strip_option), default)]
    color_dark: Option<HexColor>,

    /// When users use your actions and events they will be rendered in their own flows.
    ///
    /// This attribute tells Touch Portal which light color to use in those actions and events. When
    /// this is not specified the default plug-in colors will be used in Touch Portal. Preferably
    /// use the color schemes of the software or product you are making a plug-in for to increase
    /// recognizability.
    ///
    /// Note: these color will be ignored in some of the themes within Touch Portal. There is no
    /// way to override this behaviour.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(into, strip_option), default)]
    color_light: Option<HexColor>,

    /// The specific category within Touch Portal your plug-in falls into.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(into, strip_option), default)]
    parent_category: Option<PluginCategory>,
}

impl PluginConfiguration {
    pub fn builder() -> PluginConfigurationBuilder {
        PluginConfigurationBuilder::default()
    }
}

/// You can add your plug-in in specific categories within Touch Portal.
///
/// These main categories are used within the category and action control which the user uses to
/// add an action to a flow of actions.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum PluginCategory {
    /// For all audio, music and media related plug-ins.
    Audio,

    /// For all streaming related plug-ins.
    Streaming,

    /// For all Content Creation related plug-ins.
    Content,

    /// For all Home Automation related plug-ins.
    HomeAutomation,

    /// For all social media related plug-ins.
    Social,

    /// For all game related plug-ins.
    Games,

    /// This is the default category a plugin falls into even when this attribute of the
    /// configuration has not been specified.
    ///
    /// All plug-ins not fitting in one of the categories above should be placed in this category.
    #[default]
    Misc,

    /// For all conferencing calls related plug-ins.
    ///
    /// Only available in API version 10 and above.
    Conferencing,

    /// For all office type of application and services related plug-ins.
    ///
    /// Only available in API version 10 and above.
    Office,

    /// For all System related plug-ins.
    ///
    /// Only available in API version 10 and above.
    System,

    /// For all tools not fitting in other categories.
    ///
    /// Only available in API version 10 and above.
    Tools,

    /// For all communication and transport protocol related plug-ins.
    ///
    /// Only available in API version 10 and above.
    Transport,

    /// For all input device and controller related plug-ins.
    ///
    /// Only available in API version 10 and above.
    Input,
}

mod data;
pub use data::*;

mod actions;
pub use actions::*;

mod events;
pub use events::*;

mod connectors;
pub use connectors::*;

mod states;
pub use states::*;

mod settings;
pub use settings::*;
use std::collections::HashMap;

#[test]
fn serialize_tutorial_sdk_example() {
    assert_eq!(
        serde_json::to_value(
            PluginDescription::builder()
                .api(ApiVersion::V4_3)
                .version(1)
                .name("Tutorial SDK Plugin")
                .id("tp_tut_001")
                .configuration(
                    PluginConfiguration::builder()
                        .color_dark(HexColor::from_u24(0xFF0000))
                        .color_light(HexColor::from_u24(0x00FF00))
                        .parent_category(PluginCategory::Misc)
                        .build()
                        .unwrap()
                )
                .plugin_start_cmd("executable.exe -param")
                .build()
                .unwrap()
        )
        .unwrap(),
        serde_json::json! {{
          "api":10,
          "version":1,
          "name":"Tutorial SDK Plugin",
          "id":"tp_tut_001",
          "configuration" : {
            "colorDark" : "#FF0000",
            "colorLight" : "#00FF00",
            "parentCategory" : "misc"
          },
          "plugin_start_cmd":"executable.exe -param",
          "categories": [ ],
          "settings": [ ],
        }}
    );
}

#[test]
fn serialize_tutorial_sdk_category_example() {
    assert_eq!(
        serde_json::to_value(
            Category::builder()
                .id("tp_tut_001_cat_01")
                .name("Tools")
                .imagepath("%TP_PLUGIN_FOLDER%ExamplePlugin/images/tools.png")
                .build()
                .unwrap()
        )
        .unwrap(),
        serde_json::json! {{
          "id":"tp_tut_001_cat_01",
          "name":"Tools",
          "imagepath":"%TP_PLUGIN_FOLDER%ExamplePlugin/images/tools.png",
          "actions": [ ],
          "events": [ ],
          "connectors": [ ],
          "states": [ ]
        } }
    );
}

#[test]
fn serialize_tutorial_sdk_plugin_with_category_example() {
    assert_eq!(
        serde_json::to_value(
            PluginDescription::builder()
                .api(ApiVersion::V4_3)
                .version(1)
                .name("Tutorial SDK Plugin")
                .id("tp_tut_001")
                .configuration(
                    PluginConfiguration::builder()
                        .color_dark(HexColor::from_u24(0xFF0000))
                        .color_light(HexColor::from_u24(0x00FF00))
                        .parent_category(PluginCategory::Misc)
                        .build()
                        .unwrap()
                )
                .plugin_start_cmd("executable.exe -param")
                .category(
                    Category::builder()
                        .id("tp_tut_001_cat_01")
                        .name("Tools")
                        .imagepath("%TP_PLUGIN_FOLDER%Tutorial SDK Plugin/images/tools.png")
                        .build()
                        .unwrap()
                )
                .build()
                .unwrap()
        )
        .unwrap(),
        serde_json::json! {{
          "api":10,
          "version":1,
          "name":"Tutorial SDK Plugin",
          "id":"tp_tut_001",
          "configuration" : {
            "colorDark" : "#FF0000",
            "colorLight" : "#00FF00",
            "parentCategory" : "misc"
          },
          "plugin_start_cmd":"executable.exe -param",
          "categories": [
            {
              "id":"tp_tut_001_cat_01",
              "name":"Tools",
              "imagepath":"%TP_PLUGIN_FOLDER%Tutorial SDK Plugin/images/tools.png",
              "actions": [ ],
              "events": [ ],
              "connectors": [ ],
              "states": [ ],
            }
          ],
          "settings": [ ],
        } }
    );
}
