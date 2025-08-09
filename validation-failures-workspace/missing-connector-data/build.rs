use touchportal_sdk::{reexport::HexColor, *};

pub fn plugin() -> PluginDescription {
    // This plugin tests validation failure for connectors without required data fields
    PluginDescription::builder()
        .api(ApiVersion::V4_3)
        .version(1)
        .name("Missing Connector Data Test")
        .id("com.test.missing-connector-data")
        .configuration(
            PluginConfiguration::builder()
                .color_dark(HexColor::from_u24(0xFF0000))
                .color_light(HexColor::from_u24(0x00FF00))
                .parent_category(PluginCategory::Misc)
                .build()
                .unwrap(),
        )
        .plugin_start_cmd(format!(
            "%TP_PLUGIN_FOLDER%MissingConnectorData/{}{}",
            std::env::var("CARGO_PKG_NAME").unwrap(),
            std::env::consts::EXE_SUFFIX
        ))
        .category(
            Category::builder()
                .id("test_cat")
                .name("Test Category")
                .connector(
                    Connector::builder()
                        .id("incomplete_connector")
                        .name("Connector Missing Data")
                        .format("Control {$missing_data$} value: {$another_missing$}")
                        // Missing the actual data fields that are referenced in the format string
                        // This should cause a validation error
                        .build()
                        .unwrap(),
                )
                .build()
                .unwrap(),
        )
        .build()
        .unwrap()
}

fn main() {
    let plugin = plugin();

    touchportal_sdk::codegen::export(&plugin);
}