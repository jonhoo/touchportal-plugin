use derive_builder::Builder;
use serde::{Deserialize, Serialize};

/// Connectors can be connected to controls in Touch Portal such as Sliders and Dials.
///
/// A connector is set up the same way as most other elements such as the action. A user will be
/// able to add the connector to a control and from that moment the slider will send data from that
/// connector and the control value to the plug-in. Although the connector is universal, the
/// implementation for specific controls may differ.
///
/// ## Sliders
///
/// The Slider connector has some specific characteristics such as always having a value range of 0
/// - 100 in whole numbers.
///
/// ## Dials
///
/// Dial connectors can have variable value ranges depending on how the user sets up the dial
/// itself. The precision is also directly related to how the dial itself is set up. Both should be
/// taken into account when processing the data received. Be sure to safeguard your plug-in by
/// checking boundaries and precision. Users can set up the dial virtually to all values and all
/// precision.
#[derive(Debug, Clone, Builder, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Connector {
    /// This is the id of the connector.
    ///
    /// It is used to identify the connectors within Touch Portal. This id needs to be unique
    /// across plugins. This means that if you give it the id "1" there is a big chance that it
    /// will be a duplicate. Touch Portal may reject it or mix it up with a different plug-in. Best
    /// practise is to create a unique prefix for all your actions like in our case;
    /// `tp_yourplugin_connector_001`.
    #[builder(setter(into))]
    pub(crate) id: String,

    /// This is the name of the connector.
    ///
    /// This will be used as the connector name in the connector category list.
    #[builder(setter(into))]
    name: String,

    /// List of types that are supported.
    ///
    /// If this key is not included, the connector will be made available to sliders only.
    ///
    /// Only available in API version 12 and above. Since this parameter is not being interpreted
    /// in versions prior to the API v12, it means that connectors set up for dials might show up
    /// as normal slider connectors on old versions of Touch Portal. This means that these
    /// connectors might send the changes as a normal slider message as well. Be sure to either
    /// implement this situation as well or alert your users to upgrade Touch Portal to match the
    /// plugins intentional functionality.
    #[builder(setter(each(name = "supports")), default)]
    supported_types: Vec<ConnectorType>,

    /// This will be the format of the inline connector.
    ///
    /// Use the id's of the data objects to place them within the text, such as
    ///
    /// ```ignore
    /// "Control {$connectordata001$} which has {$connectordata002$} and number {$connectordata003$} is {$connectordata004$}",
    /// ```
    ///
    /// This is a fictive form but it shows how to use this. The data object with the id
    /// `connectordata001` will be shown at the given location. To have an data object appear on
    /// the connector line, use the format `{$id$}` where id is the id of the data object you want
    /// to show the control for.
    #[builder(setter(into))]
    pub(crate) format: String,

    /// This is a collection of data which can be specified by the user.
    ///
    /// These data id's can be used to fill up the format attribute.
    #[builder(setter(each(name = "datum")), default)]
    pub(crate) data: Vec<super::data::Data>,

    /// This attribute allows you to connect this connector to a specified subcategory id.
    ///
    /// This connector will then be shown in Touch Portals Action selection list attached to that
    /// subcategory instead of the main parent category.
    #[builder(setter(into, strip_option), default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    sub_category_id: Option<String>,
}

impl Connector {
    pub fn builder() -> ConnectorBuilder {
        ConnectorBuilder::default()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "lowercase")]
pub enum ConnectorType {
    /// Indicates that the type of event will be an dropdown with predefined values.
    Dial,

    /// This will check whether the state is the same as the user specified value in the text box.
    Slider,
}
