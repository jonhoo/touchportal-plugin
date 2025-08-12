use touchportal_sdk::{reexport::HexColor, *};

pub fn plugin() -> PluginDescription {
    // This plugin tests validation failure for features used with incompatible API versions
    PluginDescription::builder()
        .api(ApiVersion::V2_1) // Using very old API version
        .version(1)
        .name("Invalid API Version Test")
        .id("com.test.invalid-api-versions")
        .configuration(
            PluginConfiguration::builder()
                .color_dark(HexColor::from_u24(0xFF0000))
                .color_light(HexColor::from_u24(0x00FF00))
                .parent_category(PluginCategory::Misc)
                .build()
                .unwrap(),
        )
        .plugin_start_cmd(format!(
            "%TP_PLUGIN_FOLDER%InvalidApiVersions/{}{}",
            std::env::var("CARGO_PKG_NAME").unwrap(),
            std::env::consts::EXE_SUFFIX
        ))
        .category(
            Category::builder()
                .id("test_cat")
                .name("Test Category")
                .action(
                    Action::builder()
                        .id("test_action")
                        .name("Test Action")
                        .implementation(ActionImplementation::Dynamic)
                        .datum(
                            // Using newer data types with old API version
                            Data::builder()
                                .id("bound_data")
                                .format(DataFormat::LowerBound(
                                    // LowerBound requires API v10+
                                    BoundData::builder()
                                        .initial(50)
                                        .min_value(0)
                                        .max_value(100)
                                        .build()
                                        .unwrap(),
                                ))
                                .build()
                                .unwrap(),
                        )
                        .lines(
                            Lines::builder()
                                .action(
                                    LingualLine::builder()
                                        .datum(
                                            Line::builder()
                                                .line_format("Bound: {$bound_data$}")
                                                .build()
                                                .unwrap(),
                                        )
                                        .build()
                                        .unwrap(),
                                )
                                .build()
                                .unwrap(),
                        )
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
