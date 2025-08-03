use derive_builder::Builder;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Setting {
    /// This is the name of the settings in the settings overview.
    ///
    /// This is also the identifier.
    #[builder(setter(into))]
    pub(crate) name: String,

    /// This will be the default value for your setting.
    // TODO: validate this against SettingType
    #[builder(setter(into))]
    #[serde(rename = "default")]
    pub(crate) initial: String,

    /// This will specify what type of settings you can use. Currently you can only use "text" or "number".
    #[serde(flatten)]
    pub(crate) kind: SettingType,

    /// An optional tooltip object allowing you to explain more about the setting.
    ///
    /// As of API 10 (Touch Portal 4.3) all tooltips will be shown as a description text above the
    /// control in the plug-in settings. This is part of a redesign of the settings section.
    #[builder(setter(strip_option), default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    tooltip: Option<Tooltip>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "lowercase")]
#[serde(tag = "type")]
pub enum SettingType {
    /// A normal text field settings item, can be used with maxLength, readOnly and isPassword
    Text(TextSetting),

    /// A number text field settings item.
    Number(NumberSetting),

    /// A file selector.
    ///
    /// Only available in API version 10 and above.
    File(FileSetting),

    /// A folder selector.
    ///
    /// Only available in API version 10 and above.
    Folder(FolderSetting),

    /// A multiline text field.
    ///
    /// Only available in API version 10 and above.
    Multiline(MultilineSetting),

    /// A switch for boolean settings.
    ///
    /// Only available in API version 10 and above.
    Switch(SwitchSetting),

    /// A choice box for preset options, can be used with choices.
    ///
    /// Only available in API version 10 and above.
    Choice(ChoiceSetting),
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextSetting {
    /// This is the max amount of characters a text settings value can have.
    max_length: u32,

    /// If set, will hide the characters from the input field.
    ///
    /// It will show dots instead. Please do know that communication between Touch Portal and the
    /// plug-in is open text. This option is made so that random people will not be able to peek at
    /// the password field. It is not secure.
    #[builder(setter(strip_option), default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    is_password: Option<bool>,

    /// For some settings you do not want the user to edit them but you do want to share them.
    ///
    /// Using this switch will allow you to do so. Updating these setting values should be done
    /// with the dynamic functions.
    #[builder(setter(strip_option), default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    read_only: Option<bool>,
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NumberSetting {
    /// This is the max amount of characters a text settings value can have.
    max_length: u32,

    /// If set, will hide the characters from the input field.
    ///
    /// It will show dots instead. Please do know that communication between Touch Portal and the
    /// plug-in is open text. This option is made so that random people will not be able to peek at
    /// the password field. It is not secure.
    #[builder(setter(strip_option), default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    is_password: Option<bool>,

    /// For some settings you do not want the user to edit them but you do want to share them.
    ///
    /// Using this switch will allow you to do so. Updating these setting values should be done
    /// with the dynamic functions.
    #[builder(setter(strip_option), default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    read_only: Option<bool>,

    /// The minimum number value allowed for a number type setting.
    min_value: f64,

    /// The maximum number value allowed for a number type setting.
    max_value: f64,
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSetting {}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderSetting {}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MultilineSetting {
    /// This is the max amount of characters a text settings value can have.
    max_length: u32,

    /// For some settings you do not want the user to edit them but you do want to share them.
    ///
    /// Using this switch will allow you to do so. Updating these setting values should be done
    /// with the dynamic functions.
    #[builder(setter(strip_option), default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    read_only: Option<bool>,
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchSetting {}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChoiceSetting {
    /// These are all the options the user can select for the setting.
    #[builder(setter(each(name = "choice", into)))]
    pub(crate) choices: Vec<String>,
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tooltip {
    /// This is the title for the tooltip.
    ///
    /// If this is not added or is left empty, the title will not be shown in the tooltip.
    #[builder(setter(into, strip_option), default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,

    /// This is the body for the tooltip.
    #[builder(setter(into))]
    body: String,

    /// This is the url to the documentation if this is available.
    ///
    /// If this is empty, no link to documentation is added in the tooltip.
    #[builder(setter(into, strip_option), default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    doc_url: Option<String>,
}

#[test]
fn serialize_example_setting() {
    assert_eq!(
        serde_json::to_value(
            SettingBuilder::default()
                .name("Age")
                .initial("23")
                .kind(SettingType::Number(
                    NumberSettingBuilder::default()
                        .max_length(20)
                        .is_password(false)
                        .min_value(0.0)
                        .max_value(120.0)
                        .read_only(false)
                        .build()
                        .unwrap()
                ))
                .build()
                .unwrap()
        )
        .unwrap(),
        serde_json::json! {{
          "name":"Age",
          "default":"23",
          "type":"number",
          "maxLength":20,
          "isPassword":false,
          "minValue":0.,
          "maxValue":120.,
          "readOnly":false
        }}
    );
}

#[test]
fn serialize_example_setting_with_tooltip() {
    assert_eq!(
        serde_json::to_value(
            SettingBuilder::default()
                .name("Age")
                .initial("23")
                .kind(SettingType::Number(
                    NumberSettingBuilder::default()
                        .max_length(20)
                        .is_password(false)
                        .min_value(0.0)
                        .max_value(120.0)
                        .read_only(false)
                        .build()
                        .unwrap()
                ))
                .tooltip(
                    TooltipBuilder::default()
                        .title("Toolstip")
                        .body(
                            "Learn more about how tooltips work in the Touch Portal API documentation."
                        )
                        .doc_url(
                            "https://www.touch-portal.com/api/v2/index.php?section=description_file_settings"
                        )
                        .build()
                        .unwrap()
                )
                .build()
                .unwrap()
        )
        .unwrap(),
        serde_json::json! {{
          "name":"Age",
          "default":"23",
          "type":"number",
          "maxLength":20,
          "isPassword":false,
          "minValue":0.,
          "maxValue":120.,
          "readOnly":false,
          "tooltip":{
            "title":"Toolstip",
            "body":"Learn more about how tooltips work in the Touch Portal API documentation.",
            "docUrl":"https://www.touch-portal.com/api/v2/index.php?section=description_file_settings"
          }
        }}
    );
}
