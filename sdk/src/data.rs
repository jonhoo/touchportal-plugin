use derive_builder::Builder;
use hex_color::HexColor;
use indexmap::IndexSet;
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
    pub(crate) id: String,

    #[serde(flatten)]
    pub(crate) format: DataFormat,
}

impl Data {
    pub fn builder() -> DataBuilder {
        DataBuilder::default()
    }
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
    // TODO: valueStore
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextData {
    /// This is the default value the data object has.
    #[builder(setter(into), default)]
    #[serde(rename = "default")]
    #[doc(alias = "default")]
    pub(crate) initial: String,
}

impl TextData {
    pub fn builder() -> TextDataBuilder {
        TextDataBuilder::default()
    }
}

fn bool_is_true(b: &bool) -> bool {
    *b
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[builder(build_fn(validate = "Self::validate"))]
#[serde(rename_all = "camelCase")]
pub struct NumberData {
    /// This is the default value the data object has.
    #[serde(rename = "default")]
    #[doc(alias = "default")]
    pub(crate) initial: f64,

    /// This field tells the system whether this data field should allow decimals in the number.
    ///
    /// The default is `true`.
    #[builder(default = true)]
    #[serde(skip_serializing_if = "bool_is_true")]
    pub(crate) allow_decimals: bool,

    /// This is the lowest number that will be accepted.
    ///
    /// The user will get a message to correct the data if it is lower and the new value will be
    /// rejected.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(into, strip_option), default)]
    pub(crate) min_value: Option<f64>,

    /// This is the highest number that will be accepted.
    ///
    /// The user will get a message to correct the data if it is higher and the new value will be
    /// rejected.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(into, strip_option), default)]
    pub(crate) max_value: Option<f64>,
}

impl NumberData {
    pub fn builder() -> NumberDataBuilder {
        NumberDataBuilder::default()
    }
}

impl NumberDataBuilder {
    fn validate(&self) -> Result<(), String> {
        let initial = self.initial.expect("initial is required");
        let min = self.min_value.flatten();
        let max = self.max_value.flatten();

        if let Some(min_val) = min
            && initial < min_val
        {
            if let Some(max_val) = max {
                return Err(format!(
                    "initial value {} is outside the allowed range [{}, {}]",
                    initial, min_val, max_val
                ));
            } else {
                return Err(format!(
                    "initial value {} is below the minimum allowed value {}",
                    initial, min_val
                ));
            }
        }

        if let Some(max_val) = max
            && initial > max_val
        {
            if let Some(min_val) = min {
                return Err(format!(
                    "initial value {} is outside the allowed range [{}, {}]",
                    initial, min_val, max_val
                ));
            } else {
                return Err(format!(
                    "initial value {} is above the maximum allowed value {}",
                    initial, max_val
                ));
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchData {
    /// This is the default value the data object has.
    #[serde(rename = "default")]
    #[doc(alias = "default")]
    pub(crate) initial: bool,
}

impl SwitchData {
    pub fn builder() -> SwitchDataBuilder {
        SwitchDataBuilder::default()
    }
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChoiceData {
    /// This is the default value the data object has.
    #[builder(setter(into))]
    #[serde(rename = "default")]
    #[doc(alias = "default")]
    pub(crate) initial: String,

    #[builder(setter(each(name = "choice", into)))]
    pub(crate) value_choices: Vec<String>,
}

impl ChoiceData {
    pub fn builder() -> ChoiceDataBuilder {
        ChoiceDataBuilder::default()
    }
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileData {
    /// This is the default value the data object has.
    #[builder(setter(into))]
    #[serde(rename = "default")]
    #[doc(alias = "default")]
    pub(crate) initial: String,

    /// This is a collection of extensions allowed to open.
    ///
    /// eg: `"extensions": ["*.jpg","*.png"]`
    #[builder(setter(each(name = "extension")), default)]
    #[serde(skip_serializing_if = "IndexSet::is_empty")]
    pub(crate) extensions: IndexSet<String>,
}

impl FileData {
    pub fn builder() -> FileDataBuilder {
        FileDataBuilder::default()
    }
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderData {
    /// This is the default value the data object has.
    #[builder(setter(into))]
    #[serde(rename = "default")]
    #[doc(alias = "default")]
    pub(crate) initial: String,
}

impl FolderData {
    pub fn builder() -> FolderDataBuilder {
        FolderDataBuilder::default()
    }
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColorData {
    /// This is the default value the data object has.
    #[builder(setter(into))]
    #[serde(rename = "default")]
    #[doc(alias = "default")]
    pub(crate) initial: HexColor,
}

impl ColorData {
    pub fn builder() -> ColorDataBuilder {
        ColorDataBuilder::default()
    }
}

#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundData {
    /// This is the default value the data object has.
    #[serde(rename = "default")]
    #[doc(alias = "default")]
    pub(crate) initial: i64,

    /// This is the lowest number that will be accepted.
    ///
    /// The user will get a message to correct the data if it is lower and the new value will be
    /// rejected.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(into, strip_option), default)]
    pub(crate) min_value: Option<i64>,

    /// This is the highest number that will be accepted.
    ///
    /// The user will get a message to correct the data if it is higher and the new value will be
    /// rejected.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(into, strip_option), default)]
    pub(crate) max_value: Option<i64>,
}

impl BoundData {
    pub fn builder() -> BoundDataBuilder {
        BoundDataBuilder::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_json_snapshot;

    #[test]
    fn serialize_example_action_data_text() {
        let data = Data::builder()
            .id("actiondata001")
            .format(DataFormat::Text(
                TextData::builder().initial("any text").build().unwrap(),
            ))
            .build()
            .unwrap();

        assert_json_snapshot!(data);
    }

    #[test]
    fn serialize_example_action_data_number() {
        let data = Data::builder()
            .id("first")
            .format(DataFormat::Number(
                NumberData::builder()
                    .initial(200.)
                    .min_value(100.)
                    .max_value(350.)
                    .build()
                    .unwrap(),
            ))
            .build()
            .unwrap();

        assert_json_snapshot!(data);
    }

    #[test]
    fn serialize_example_action_data_choice() {
        let data = Data::builder()
            .id("second")
            .format(DataFormat::Choice(
                ChoiceData::builder()
                    .initial("200")
                    .choice("200")
                    .choice("400")
                    .choice("600")
                    .choice("800")
                    .build()
                    .unwrap(),
            ))
            .build()
            .unwrap();

        assert_json_snapshot!(data);
    }

    #[test]
    fn serialize_example_action_data_switch() {
        let data = Data::builder()
            .id("actiondata003")
            .format(DataFormat::Switch(
                SwitchData::builder().initial(true).build().unwrap(),
            ))
            .build()
            .unwrap();

        assert_json_snapshot!(data);
    }

    #[test]
    fn test_data_builder_validation() {
        // Test that data builders work correctly with different formats
        let _text_data = Data::builder()
            .id("test_text")
            .format(DataFormat::Text(
                TextData::builder().initial("default text").build().unwrap(),
            ))
            .build()
            .unwrap();

        let _number_data = Data::builder()
            .id("test_number")
            .format(DataFormat::Number(
                NumberData::builder()
                    .initial(50.0)
                    .min_value(0.0)
                    .max_value(100.0)
                    .build()
                    .unwrap(),
            ))
            .build()
            .unwrap();

        let _switch_data = Data::builder()
            .id("test_switch")
            .format(DataFormat::Switch(
                SwitchData::builder().initial(false).build().unwrap(),
            ))
            .build()
            .unwrap();
    }

    #[test]
    fn test_file_data_builder() {
        let file_data = FileData::builder()
            .initial("/path/to/file.txt")
            .extension("*.txt".to_string())
            .extension("*.log".to_string())
            .build()
            .unwrap();

        assert_eq!(file_data.initial, "/path/to/file.txt");
        assert!(file_data.extensions.contains("*.txt"));
        assert!(file_data.extensions.contains("*.log"));
        assert_eq!(file_data.extensions.len(), 2);
    }

    #[test]
    fn test_folder_data_builder() {
        let folder_data = FolderData::builder()
            .initial("/path/to/folder")
            .build()
            .unwrap();

        assert_eq!(folder_data.initial, "/path/to/folder");
    }

    #[test]
    fn test_color_data_builder() {
        use hex_color::HexColor;

        let color_data = ColorData::builder()
            .initial(HexColor::from_u24(0xFF0000)) // Red
            .build()
            .unwrap();

        assert_eq!(color_data.initial, HexColor::from_u24(0xFF0000));
    }

    #[test]
    fn test_bound_data_builder() {
        let bound_data = BoundData::builder()
            .initial(50)
            .min_value(0)
            .max_value(100)
            .build()
            .unwrap();

        assert_eq!(bound_data.initial, 50);
        assert_eq!(bound_data.min_value, Some(0));
        assert_eq!(bound_data.max_value, Some(100));

        // Test without optional bounds
        let bound_data_no_limits = BoundData::builder().initial(42).build().unwrap();

        assert_eq!(bound_data_no_limits.initial, 42);
        assert_eq!(bound_data_no_limits.min_value, None);
        assert_eq!(bound_data_no_limits.max_value, None);
    }

    #[test]
    fn test_number_data_edge_cases() {
        // Test with negative numbers
        let negative_data = NumberData::builder()
            .initial(-10.5)
            .min_value(-100.0)
            .max_value(-1.0)
            .build()
            .unwrap();

        assert_eq!(negative_data.initial, -10.5);
        assert_eq!(negative_data.min_value, Some(-100.0));
        assert_eq!(negative_data.max_value, Some(-1.0));

        // Test with zero
        let zero_data = NumberData::builder().initial(0.0).build().unwrap();

        assert_eq!(zero_data.initial, 0.0);
        assert_eq!(zero_data.min_value, None);
        assert_eq!(zero_data.max_value, None);
    }

    #[test]
    fn test_text_data_edge_cases() {
        // Test with empty string
        let empty_text = TextData::builder().initial("").build().unwrap();

        assert_eq!(empty_text.initial, "");

        // Test with regular text
        let regular_text = TextData::builder().initial("test").build().unwrap();

        assert_eq!(regular_text.initial, "test");
    }

    #[test]
    fn test_choice_data_edge_cases() {
        // Test with single choice
        let single_choice = ChoiceData::builder()
            .initial("only_option")
            .choice("only_option")
            .build()
            .unwrap();

        assert_eq!(single_choice.initial, "only_option");
        assert_eq!(single_choice.value_choices.len(), 1);
        assert!(
            single_choice
                .value_choices
                .contains(&"only_option".to_string())
        );

        // Test with empty choice strings (valid in TouchPortal)
        let empty_choice = ChoiceData::builder()
            .initial("")
            .choice("")
            .choice("non_empty")
            .build()
            .unwrap();

        assert_eq!(empty_choice.initial, "");
        assert!(empty_choice.value_choices.contains(&"".to_string()));
        assert!(
            empty_choice
                .value_choices
                .contains(&"non_empty".to_string())
        );
    }

    #[test]
    fn test_number_data_validation_invalid_range() {
        use insta::assert_snapshot;

        let result = NumberData::builder()
            .initial(150.0) // Outside allowed range
            .min_value(0.0)
            .max_value(100.0)
            .build();

        assert_snapshot!(result.unwrap_err());
    }
}
