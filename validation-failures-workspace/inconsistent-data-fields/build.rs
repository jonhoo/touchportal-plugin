// This plugin tests validation failure when the same data field ID is used with different numeric constraints across actions.

use touchportal_sdk::{reexport::HexColor, *};

pub fn plugin() -> PluginDescription {
    PluginDescription::builder()
        .api(ApiVersion::V4_3)
        .version(1)
        .name("Inconsistent Data Fields Test")
        .id("com.test.inconsistent-data")
        .configuration(
            PluginConfiguration::builder()
                .color_dark(HexColor::from_u24(0xFF0000))
                .color_light(HexColor::from_u24(0x00FF00))
                .parent_category(PluginCategory::Misc)
                .build()
                .unwrap(),
        )
        .plugin_start_cmd(format!(
            "%TP_PLUGIN_FOLDER%InconsistentData/{}{}",
            std::env::var("CARGO_PKG_NAME").unwrap(),
            std::env::consts::EXE_SUFFIX
        ))
        .category(
            Category::builder()
                .id("test_cat")
                .name("Test Category")
                .action(
                    Action::builder()
                        .id("action1")
                        .name("First Action")
                        .implementation(ActionImplementation::Dynamic)
                        .datum(
                            Data::builder()
                                .id("shared_number")
                                .format(DataFormat::Number(
                                    NumberData::builder()
                                        .initial(50.0)
                                        .min_value(0.0)
                                        .max_value(100.0)
                                        .allow_decimals(true)
                                        .build()
                                        .unwrap()
                                ))
                                .build()
                                .unwrap()
                        )
                        .lines(
                            Lines::builder()
                                .action(
                                    LingualLine::builder()
                                        .datum(
                                            Line::builder()
                                                .line_format("First: {$shared_number$}")
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
                .action(
                    Action::builder()
                        .id("action2")
                        .name("Second Action")
                        .implementation(ActionImplementation::Dynamic)
                        .datum(
                            Data::builder()
                                .id("shared_number")
                                // This should cause validation failure - different constraints
                                .format(DataFormat::Number(
                                    NumberData::builder()
                                        .initial(20.0)
                                        .min_value(10.0)  // Different min_value
                                        .max_value(200.0) // Different max_value
                                        .allow_decimals(false) // Different allow_decimals
                                        .build()
                                        .unwrap()
                                ))
                                .build()
                                .unwrap()
                        )
                        .lines(
                            Lines::builder()
                                .action(
                                    LingualLine::builder()
                                        .datum(
                                            Line::builder()
                                                .line_format("Second: {$shared_number$}")
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
                .build()
                .unwrap(),
        )
        .build()
        .unwrap()
}

fn main() {
    let plugin = plugin();

    std::fs::write(
        format!("{}/entry.rs", std::env::var("OUT_DIR").unwrap()),
        touchportal_sdk::codegen::build(&plugin),
    )
    .unwrap();

    std::fs::write(
        format!("{}/entry.tp", std::env::var("OUT_DIR").unwrap()),
        serde_json::to_vec(&plugin).unwrap(),
    )
    .unwrap();
}