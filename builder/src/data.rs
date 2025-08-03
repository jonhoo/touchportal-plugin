use derive_builder::Builder;
use hex_color::HexColor;
use serde::{Deserialize, Serialize};

/// As a plug-in developer you can augment your actions with additional data that the user has to
/// fill in.
///
/// It uses the same structures as the native actions from Touch Portal itself.
///
/// You can use this for both static actions as dynamic actions. The user will have to specify
/// values for the given data field within Touch Portal.
#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
pub struct Data {
    /// This is the id of the data field.
    ///
    /// Touch Portal will use this for communicating the values or to place the values in the
    /// result.
    #[builder(setter(into))]
    id: String,

    #[serde(flatten)]
    format: DataFormat,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum DataFormat {
    /// A data type that accepts a string
    Text(TextData),
    /// A data type that accepts a number
    Number(NumberData),
    /// A data type that accepts a true or false value
    Switch(SwitchData),
    /// A data type that accepts a string where a collection of strings can be chosen from
    Choice(ChoiceData),
    /// A data type that represents a file which the user can pick with a file chooser
    File(FileData),
    /// A data type that represents a folder which the user can pick with a folder chooser
    Folder(FolderData),
    /// A data type that represents a color which the user can pick with a color chooser.
    ///
    /// This value must be in a the format `#RRGGBBAA`.
    Color(ColorData),
    /// A data type that represents a field for the user to specify the lower bound of the slider
    /// range.
    ///
    /// The amount of decimals will also specify the precision. For example, if the user sets the
    /// lower bound to 1, all values will be whole numbers. If the value is set to 1.0 it will
    /// return connector values times 10, if the value is set to 1.00 it will return connector
    /// values times 100. The plug-in is responsible of dividing the value to the proper range
    /// before it is used. Connectors are only capable of sending integer data.
    ///
    /// If `UpperBound` is also set, both fields will be checked for precision. The higher
    /// precision will be used. A range between 1 and 5.0 means it will use the 5.0 for the
    /// precision.
    ///
    /// Only available for connectors.
    ///
    /// Only available in API version 10 and above.
    LowerBound(BoundData),
    /// A data type that represents a field for the user to specify the upper bound of the slider
    /// range.
    ///
    /// The amount of decimals will also specify the precision. For example, if the user sets the
    /// upper bound to 1, all values of the connector will be send as normal but will be translated
    /// to the range specified. If the value is set to 1.0 it will return connector values times
    /// 10, if the value is set to 1.00 it will return connector values times 100. The plug-in is
    /// responsible of dividing the value to the proper range before it is used. Connectors are
    /// only capable of sending integer data.
    ///
    /// If `LowerBound` is also set, both fields will be checked for precision. The higher
    /// precision will be used. A range between 1 and 5.0 means it will use the 5.0 for the
    /// precision.
    ///
    /// Only available for connectors.
    ///
    /// Only available in API version 10 and above.
    UpperBound(BoundData),
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextData {
    /// This is the default value the data object has.
    #[builder(setter(into), default)]
    #[serde(rename = "default")]
    #[doc(alias = "default")]
    initial: String,
}

fn bool_is_true(b: &bool) -> bool {
    *b
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NumberData {
    /// This is the default value the data object has.
    #[serde(rename = "default")]
    #[doc(alias = "default")]
    initial: f64,

    /// This field tells the system whether this data field should allow decimals in the number.
    ///
    /// The default is `true`.
    #[builder(default = true)]
    #[serde(skip_serializing_if = "bool_is_true")]
    allow_decimals: bool,

    /// This is the lowest number that will be accepted.
    ///
    /// The user will get a message to correct the data if it is lower and the new value will be
    /// rejected.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(into, strip_option), default)]
    min_value: Option<f64>,

    /// This is the highest number that will be accepted.
    ///
    /// The user will get a message to correct the data if it is higher and the new value will be
    /// rejected.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(into, strip_option), default)]
    max_value: Option<f64>,
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchData {
    /// This is the default value the data object has.
    #[serde(rename = "default")]
    #[doc(alias = "default")]
    initial: bool,
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChoiceData {
    /// This is the default value the data object has.
    #[builder(setter(into))]
    #[serde(rename = "default")]
    #[doc(alias = "default")]
    initial: String,

    #[builder(setter(each(name = "choice", into)))]
    value_choices: Vec<String>,
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileData {
    /// This is the default value the data object has.
    #[builder(setter(into))]
    #[serde(rename = "default")]
    #[doc(alias = "default")]
    initial: String,

    /// This is a collection of extensions allowed to open.
    ///
    /// eg: `"extensions": ["*.jpg","*.png"]`
    #[builder(setter(each(name = "extension")), default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    extensions: Vec<String>,
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderData {
    /// This is the default value the data object has.
    #[builder(setter(into))]
    #[serde(rename = "default")]
    #[doc(alias = "default")]
    initial: String,
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColorData {
    /// This is the default value the data object has.
    #[builder(setter(into))]
    #[serde(rename = "default")]
    #[doc(alias = "default")]
    initial: HexColor,
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundData {
    /// This is the default value the data object has.
    #[serde(rename = "default")]
    #[doc(alias = "default")]
    initial: i64,

    /// This is the lowest number that will be accepted.
    ///
    /// The user will get a message to correct the data if it is lower and the new value will be
    /// rejected.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(into, strip_option), default)]
    min_value: Option<i64>,

    /// This is the highest number that will be accepted.
    ///
    /// The user will get a message to correct the data if it is higher and the new value will be
    /// rejected.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(into, strip_option), default)]
    max_value: Option<i64>,
}

#[test]
fn serialize_example_action_data_text() {
    assert_eq!(
        serde_json::to_value(
            DataBuilder::default()
                .id("actiondata001")
                .format(DataFormat::Text(
                    TextDataBuilder::default()
                        .initial("any text")
                        .build()
                        .unwrap()
                ))
                .build()
                .unwrap()
        )
        .unwrap(),
        serde_json::json! {{
          "id":"actiondata001",
          "type":"text",
          "default":"any text"
        }}
    );
}

#[test]
fn serialize_example_action_data_number() {
    assert_eq!(
        serde_json::to_value(
            DataBuilder::default()
                .id("first")
                .format(DataFormat::Number(
                    NumberDataBuilder::default()
                        .initial(200.)
                        .min_value(100.)
                        .max_value(350.)
                        .build()
                        .unwrap()
                ))
                .build()
                .unwrap()
        )
        .unwrap(),
        serde_json::json! { {
          "id":"first",
          "type":"number",
          "default":200.0,
          "minValue":100.0,
          "maxValue":350.0,
        }}
    );
}

#[test]
fn serialize_example_action_data_choice() {
    assert_eq!(
        serde_json::to_value(
            DataBuilder::default()
                .id("second")
                .format(DataFormat::Choice(
                    ChoiceDataBuilder::default()
                        .initial("200")
                        .choice("200")
                        .choice("400")
                        .choice("600")
                        .choice("800")
                        .build()
                        .unwrap()
                ))
                .build()
                .unwrap()
        )
        .unwrap(),
        serde_json::json! {{
          "id":"second",
          "type":"choice",
          "default":"200",
          "valueChoices": [
              "200",
              "400",
              "600",
              "800"
          ]
        }}
    );
}

#[test]
fn serialize_example_action_data_switch() {
    assert_eq!(
        serde_json::to_value(
            DataBuilder::default()
                .id("actiondata003")
                .format(DataFormat::Switch(
                    SwitchDataBuilder::default().initial(true).build().unwrap()
                ))
                .build()
                .unwrap()
        )
        .unwrap(),
        serde_json::json! {{
          "id":"actiondata003",
          "type":"switch",
          "default":true
        }}
    );
}
