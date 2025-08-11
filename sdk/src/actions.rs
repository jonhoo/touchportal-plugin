use derive_builder::Builder;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Actions are one of the core components of Touch Portal. As a plug-in developer you can define
/// actions for your plug-in that the user can add to their flow of actions in their buttons and
/// events. An action is part of a [`Category`].
///
/// # Example actions
///
/// Below, you can see examples of both static and dynamic actions. Static Actions are actions that
/// can be run without communication. Touch Portal allows communication between the plugin and
/// Touch Portal but for some actions this is not required. For these situations you can use the
/// static actions. This allows for developers to create plugins easier without to much hassle.
/// With dynamic actions you are required to set up and run an application or service that
/// communicates with Touch Portal. With static actions you can use commandline execution that will
/// run directly from Touch Portal.
///
/// ## Static action
///
/// This shows a JSON of an action that does not require communication with a plugin application
/// (static). The action is set up to use powershell to create a beep sound when it is executed.
///
/// ```
/// use touchportal_sdk::{
///   Action,
///   ActionImplementation,
///   Line,
///   Lines,
///   LingualLine,
///   StaticAction,
/// };
///
/// Action::builder()
///     .id("tp_pl_action_001")
///     .name("Execute action")
///     .implementation(ActionImplementation::Static(
///         StaticAction::builder()
///             .execution_cmd("powershell [console]::beep(200,500)")
///             .build()?
///     ))
///     .lines(
///         Lines::builder()
///             .action(
///                 LingualLine::builder()
///                     .datum(
///                         Line::builder()
///                             .line_format("Play Beep Sound")
///                             .build()?
///                     )
///                     .build()?
///             )
///             .build()?
///     )
///     .build()?;
/// # Ok::<_, Box<dyn std::error::Error>>(())
/// ```
///
/// ## Dynamic action
///
/// This shows a JSON of an action that does require communication with a plugin application
/// (dynamic). When executed, it will send the information the user has entered in the text field
/// with id (tp_pl_002_text) to the plug-in. Touch Portal will parse the id from the format line
/// and will present the user with the given control to allow for user input.
///
/// ```
/// use touchportal_sdk::{
///   Action,
///   ActionImplementation,
///   Data,
///   DataFormat,
///   Line,
///   Lines,
///   LingualLine,
///   TextData,
/// };
///
/// Action::builder()
///     .id("tp_pl_action_002")
///     .name("Execute Dynamic Action")
///     .implementation(ActionImplementation::Dynamic)
///     .datum(
///         Data::builder()
///             .id("tp_pl_002_text")
///             .format(DataFormat::Text(
///                 TextData::builder().build()?
///             ))
///             .build()?
///     )
///     .lines(
///         Lines::builder()
///             .action(
///                 LingualLine::builder()
///                     .datum(
///                         Line::builder()
///                             .line_format("Do something with value {$tp_pl_002_text$}")
///                             .build()?
///                     )
///                     .build()?
///             )
///             .build()?
///     )
///     .build()?;
/// # Ok::<_, Box<dyn std::error::Error>>(())
/// ```
///
/// ## Multi-line action with multiple languages
///
/// ```
/// use touchportal_sdk::{
///   Action,
///   ActionImplementation,
///   Data,
///   DataFormat,
///   I18nNames,
///   Line,
///   Lines,
///   LingualLine,
///   TextData,
/// };
///
/// Action::builder()
///     .id("tp_pl_action_002")
///     .name("Do something")
///     .translated_names(
///         I18nNames::builder()
///             .dutch("Doe iets")
///             .build()?
///     )
///     .implementation(ActionImplementation::Dynamic)
///     .datum(
///         Data::builder()
///             .id("tp_pl_002_text")
///             .format(DataFormat::Text(
///                 TextData::builder().build()?
///             ))
///             .build()?
///     )
///     .lines(
///         Lines::builder()
///             .action(
///                 LingualLine::builder()
///                     .datum(
///                         Line::builder()
///                             .line_format("This actions shows multiple lines;")
///                             .build()?
///                     )
///                     .datum(
///                         Line::builder()
///                             .line_format("Do something with value {$tp_pl_002_text$}")
///                             .build()?
///                     )
///                     .build()?
///             )
///             .action(
///                 LingualLine::builder()
///                     .language("nl")
///                     .datum(
///                         Line::builder()
///                             .line_format("Deze actie bevat meerdere regels;")
///                             .build()?
///                     )
///                     .datum(
///                         Line::builder()
///                             .line_format("Doe iets met waarde {$tp_pl_002_text$}")
///                             .build()?
///                     )
///                     .build()?
///             )
///             .build()?
///     )
///     .build()?;
/// # Ok::<_, Box<dyn std::error::Error>>(())
/// ```
///
/// Please note: when a user adds an action belonging to a plugin, it will create a local copy of
/// the action and saves it along with the action. This means that if you change something in your
/// action the users need to remove their instance of that action and re-add it to be able to use
/// the new additions.
#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[builder(build_fn(validate = "Self::validate"))]
#[serde(rename_all = "camelCase")]
pub struct Action {
    /// This is the id of the action.
    ///
    /// It is used to identify the action within Touch Portal. This id needs to be unique across
    /// plugins. This means that if you give it the id "1" there is a big chance that it will be a
    /// duplicate. Touch Portal may reject it or when the other action is called, yours will be as
    /// well with wrong data. Best practice is to create a unique prefix for all your actions like
    /// in our case; `tp_pl_action_001`.
    #[builder(setter(into))]
    pub(crate) id: String,

    /// This is the name of the action.
    ///
    /// This will be used as the action name in the action category list.
    #[builder(setter(into))]
    pub(crate) name: String,

    #[serde(flatten)]
    #[builder(default)]
    translated_names: I18nNames,

    /// This is the attribute that specifies whether this is a static action "execute" or a dynamic
    /// action "communicate".
    #[serde(flatten)]
    pub(crate) implementation: ActionImplementation,

    /// This is a collection of action data (see definition further down this page) which can be
    /// specified by the user.
    ///
    /// These data id's can be used to fill up the `execution_cmd` text or the format (see example
    /// on the right side).
    #[builder(setter(each(name = "datum")), default)]
    pub(crate) data: Vec<super::data::Data>,

    /// This is the object for specifying the action and/or onhold lines.
    lines: Lines,

    /// This attribute allows you to connect this action to a specified subcategory id.
    ///
    /// This action will then be shown in Touch Portals Action selection list attached to that
    /// subcategory instead of the main parent category.
    #[builder(setter(into, strip_option), default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    sub_category_id: Option<String>,
}

impl Action {
    pub fn builder() -> ActionBuilder {
        ActionBuilder::default()
    }
}

impl ActionBuilder {
    fn validate(&self) -> Result<(), String> {
        // Check for empty required fields
        let name = self.name.as_ref().expect("name is required");
        if name.trim().is_empty() {
            return Err("action name cannot be empty".to_string());
        }

        let id = self.id.as_ref().expect("id is required");
        if id.trim().is_empty() {
            return Err("action id cannot be empty".to_string());
        }

        let mut data_ids = HashSet::new();
        for data in self.data.iter().flatten() {
            data_ids.insert(format!("{{${}$}}", data.id));

            if let crate::DataFormat::Choice(def) = &data.format
                && !def.value_choices.contains(&def.initial)
            {
                return Err(format!(
                    "initial value {} is not among valid choices {:?}",
                    def.initial, def.value_choices
                ));
            }
        }

        let lines = self.lines.as_ref().expect("lines is required");
        let mut languages = HashSet::new();
        for line in &lines.actions {
            if !languages.insert(&line.language) {
                return Err(format!("found two lines for language '{}'", line.language));
            }
        }

        for line in &lines.actions {
            for data_id in &data_ids {
                if !line
                    .data
                    .iter()
                    .any(|line| line.line_format.contains(&**data_id))
                {
                    return Err(format!(
                        "'{}' not found for language '{}'",
                        data_id, line.language
                    ));
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum ActionImplementation {
    #[serde(rename = "execute")]
    #[doc(alias = "execute")]
    Static(StaticAction),

    #[serde(rename = "communicate")]
    #[doc(alias = "communicate")]
    Dynamic,
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StaticAction {
    /// This is the attribute that specifies what kind of execution this action should use.
    ///
    /// This a Mac only functionality.
    #[cfg(target_os = "macos")]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(into, strip_option), default)]
    execution_type: Option<ExecutionType>,

    /// Specify the path of execution here.
    ///
    /// You should be aware that it will be passed to the OS process exection service. This means
    /// you need to be aware of spaces and use absolute paths to your executable.
    ///
    /// If you use `%TP_PLUGIN_FOLDER%` in the text here, it will be replaced with the path to the
    /// base plugin folder.
    #[serde(rename = "execution_cmd")]
    #[builder(setter(into))]
    execution_cmd: String,
}

impl StaticAction {
    pub fn builder() -> StaticActionBuilder {
        StaticActionBuilder::default()
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[non_exhaustive]
pub enum ExecutionType {
    AppleScript,
    Bash,
}

/// Language specific version of a name.
#[derive(Debug, Clone, Builder, Deserialize, Serialize, Default)]
pub struct I18nNames {
    #[serde(rename = "name_nl")]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(into, strip_option), default)]
    dutch: Option<String>,

    #[serde(rename = "name_de")]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(into, strip_option), default)]
    german: Option<String>,

    #[serde(rename = "name_es")]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(into, strip_option), default)]
    spanish: Option<String>,

    #[serde(rename = "name_fr")]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(into, strip_option), default)]
    french: Option<String>,

    #[serde(rename = "name_pt")]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(into, strip_option), default)]
    portugese: Option<String>,

    #[serde(rename = "name_tr")]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(into, strip_option), default)]
    turkish: Option<String>,
}

impl I18nNames {
    pub fn builder() -> I18nNamesBuilder {
        I18nNamesBuilder::default()
    }
}

/// The lines object consist of the parts; the action lines and the onhold lines.
///
/// You can specify either or both.
///
/// Those arrays then consist of lines information per supported language.
#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[builder(build_fn(validate = "Self::validate"))]
#[serde(rename_all = "camelCase")]
pub struct Lines {
    #[serde(rename = "action")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[builder(setter(each(name = "action")), default)]
    actions: Vec<LingualLine>,

    #[serde(rename = "onhold")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[builder(setter(each(name = "onhold")), default)]
    onholds: Vec<LingualLine>,
}

impl Lines {
    pub fn builder() -> LinesBuilder {
        LinesBuilder::default()
    }
}

impl LinesBuilder {
    fn validate(&self) -> Result<(), String> {
        if self.actions.as_ref().is_none_or(|a| a.is_empty())
            && self.onholds.as_ref().is_none_or(|o| o.is_empty())
        {
            return Err("At least one action or onhold must be set".to_string());
        }

        if self.actions.as_ref().is_some_and(|a| !a.is_empty())
            && !self
                .actions
                .iter()
                .flatten()
                .any(|line| line.language == "default")
        {
            return Err("The default language must be present among the action lines".to_string());
        }

        if self.onholds.as_ref().is_some_and(|a| !a.is_empty())
            && !self
                .onholds
                .iter()
                .flatten()
                .any(|line| line.language == "default")
        {
            return Err("The default language must be present among the onhold lines".to_string());
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[builder(build_fn(validate = "Self::validate"))]
#[serde(rename_all = "camelCase")]
pub struct LingualLine {
    /// This is the country code of the language this line information contains.
    ///
    /// Use the default for the English language. The default should always be present. If it is
    /// not, the lines will not be rendered in Touch Portal even if you have language specific
    /// lines.
    #[builder(setter(into), default = "String::from(\"default\")")]
    language: String,

    /// This is the array of line objects representing the lines of the action.
    ///
    /// This array should have at least 1 entry.
    ///
    /// We suggest to not use more than 3 lines to keep action lists clean and clear. Use a maximum
    /// of 8 lines in your actions as that will reduce the usability for the end user as the
    /// actions might get to big on smaller screens to properly view and scroll.
    #[builder(setter(each(name = "datum")))]
    data: Vec<Line>,

    #[builder(setter(into, strip_option), default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    suggestions: Option<Suggestions>,
}

impl LingualLine {
    pub fn builder() -> LingualLineBuilder {
        LingualLineBuilder::default()
    }
}

impl LingualLineBuilder {
    fn validate(&self) -> Result<(), String> {
        let data = self.data.as_ref().expect("data is required");
        if data.is_empty() {
            return Err("At least one line object must be set".to_string());
        }

        // Check TouchPortal recommended maximum of 8 lines per action
        if data.len() > 8 {
            return Err(format!(
                "action has {} lines, but TouchPortal recommends \
                a maximum of 8 lines for proper visibility \
                on smaller screens",
                data.len()
            ));
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Line {
    /// This will be the format of the rendered line in the action.
    ///
    /// Use the id's of the data objects to place them within the text, such as:
    ///
    /// ```ignore
    /// "When {$actiondata001$} has {$actiondata002$} and number {$actiondata003$} is {$actiondata004$}"
    /// ```
    ///
    /// This is a fictive form but it shows how to use this. The data object with the id
    /// `actiondata001` will be shown at the given location. To have an data object appear on the
    /// action line, use the format `{$id$}` where id is the id of the data object you want to show
    /// the control for.
    #[builder(setter(into))]
    line_format: String,
}

impl Line {
    pub fn builder() -> LineBuilder {
        LineBuilder::default()
    }
}

/// This is a suggestions object where you can specify certain rendering behaviours of the action
/// lines.
///
/// These are suggestions and my be overruled in certain situations in Touch Portal. One example is
/// rendering lines for different action rendering themes in Touch Portal.
#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestions {
    /// This option will set the width of the first part on a line if it is text.
    ///
    /// This can be used to make your action more clear for our users. This can be usefull when you
    /// list one item to set per line.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(strip_option), default)]
    first_line_item_label_width: Option<u32>,

    /// This option will add padding on the left for each line of a multiline format.
    ///
    /// If this is used together with `first_line_item_label_width`, the padding will be part of
    /// the that width and will not be added onto it.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(strip_option), default)]
    line_indentation: Option<u32>,
}

impl Suggestions {
    pub fn builder() -> SuggestionsBuilder {
        SuggestionsBuilder::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Data, DataFormat, TextData};
    use pretty_assertions::assert_eq;

    #[test]
    fn serialize_tutorial_sdk_plugin_static_action() {
        assert_eq!(
            serde_json::to_value(
                Action::builder()
                    .id("tp_pl_action_001")
                    .name("Execute action")
                    .implementation(ActionImplementation::Static(
                        StaticAction::builder()
                            .execution_cmd("powershell [console]::beep(200,500)")
                            .build()
                            .unwrap()
                    ))
                    .lines(
                        Lines::builder()
                            .action(
                                LingualLine::builder()
                                    .datum(
                                        Line::builder()
                                            .line_format("Play Beep Sound")
                                            .build()
                                            .unwrap()
                                    )
                                    .build()
                                    .unwrap()
                            )
                            .build()
                            .unwrap()
                    )
                    .build()
                    .unwrap()
            )
            .unwrap(),
            serde_json::json! {{
              "id":"tp_pl_action_001",
              "name":"Execute action",
              "type":"execute",
              "lines": {
                "action": [
                  {
                    "language": "default",
                    "data" : [
                      {
                        "lineFormat":"Play Beep Sound"
                      }
                    ]
                  }
                ]
              },
              "execution_cmd":"powershell [console]::beep(200,500)",
              "data":[ ]
            } }
        );
    }

    #[test]
    fn serialize_tutorial_sdk_plugin_dynamic_action() {
        assert_eq!(
            serde_json::to_value(
                Action::builder()
                    .id("tp_pl_action_002")
                    .name("Execute Dynamic Action")
                    .implementation(ActionImplementation::Dynamic)
                    .datum(
                        Data::builder()
                            .id("tp_pl_002_text")
                            .format(DataFormat::Text(TextData::builder().build().unwrap()))
                            .build()
                            .unwrap()
                    )
                    .lines(
                        Lines::builder()
                            .action(
                                LingualLine::builder()
                                    .datum(
                                        Line::builder()
                                            .line_format(
                                                "Do something with value {$tp_pl_002_text$}"
                                            )
                                            .build()
                                            .unwrap()
                                    )
                                    .build()
                                    .unwrap()
                            )
                            .build()
                            .unwrap()
                    )
                    .build()
                    .unwrap()
            )
            .unwrap(),
            serde_json::json! {{
              "id": "tp_pl_action_002",
              "name": "Execute Dynamic Action",
              "lines": {
                "action": [
                  {
                    "language": "default",
                    "data" : [
                      {
                        "lineFormat":"Do something with value {$tp_pl_002_text$}"
                      }
                    ]
                  }
                ]
              },
              "type": "communicate",
              "data": [
                {
                  "type": "text",
                  "default": "",
                  "id": "tp_pl_002_text"
                }
              ]
            } }
        );
    }

    #[test]
    fn serialize_tutorial_sdk_plugin_multi_lang_action() {
        assert_eq!(
            serde_json::to_value(
                Action::builder()
                    .id("tp_pl_action_002")
                    .name("Do something")
                    .translated_names(I18nNames::builder().dutch("Doe iets").build().unwrap())
                    .implementation(ActionImplementation::Dynamic)
                    .datum(
                        Data::builder()
                            .id("tp_pl_002_text")
                            .format(DataFormat::Text(TextData::builder().build().unwrap()))
                            .build()
                            .unwrap()
                    )
                    .lines(
                        Lines::builder()
                            .action(
                                LingualLine::builder()
                                    .datum(
                                        Line::builder()
                                            .line_format("This actions shows multiple lines;")
                                            .build()
                                            .unwrap()
                                    )
                                    .datum(
                                        Line::builder()
                                            .line_format(
                                                "Do something with value {$tp_pl_002_text$}"
                                            )
                                            .build()
                                            .unwrap()
                                    )
                                    .build()
                                    .unwrap()
                            )
                            .action(
                                LingualLine::builder()
                                    .language("nl")
                                    .datum(
                                        Line::builder()
                                            .line_format("Deze actie bevat meerdere regels;")
                                            .build()
                                            .unwrap()
                                    )
                                    .datum(
                                        Line::builder()
                                            .line_format("Doe iets met waarde {$tp_pl_002_text$}")
                                            .build()
                                            .unwrap()
                                    )
                                    .build()
                                    .unwrap()
                            )
                            .build()
                            .unwrap()
                    )
                    .build()
                    .unwrap()
            )
            .unwrap(),
            serde_json::json! {{
              "id": "tp_pl_action_002",
              "name": "Do something",
              "name_nl": "Doe iets",
              "lines": {
                "action": [
                  {
                    "language": "default",
                    "data" : [
                      {
                        "lineFormat":"This actions shows multiple lines;",
                      },
                      {
                        "lineFormat":"Do something with value {$tp_pl_002_text$}"
                      }
                    ]
                  },
                  {
                    "language": "nl",
                    "data" : [
                      {
                        "lineFormat":"Deze actie bevat meerdere regels;",
                      },
                      {
                        "lineFormat":"Doe iets met waarde {$tp_pl_002_text$}"
                      }
                    ]
                  }
                ]
              },
              "type": "communicate",
              "data": [
                {
                  "type": "text",
                  "default": "",
                  "id": "tp_pl_002_text"
                }
              ]
            }}
        );
    }
}
