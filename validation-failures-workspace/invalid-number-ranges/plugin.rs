use touchportal_sdk::{reexport::HexColor, *};

pub fn plugin() -> PluginDescription {
    // This plugin tests validation failure for states with initial values outside min/max bounds
    PluginDescription::builder()
        .api(ApiVersion::V4_3)
        .version(1)
        .name("Invalid Number Ranges Test")
        .id("com.test.invalid-number-ranges")
        .configuration(
            PluginConfiguration::builder()
                .color_dark(HexColor::from_u24(0xFF0000))
                .color_light(HexColor::from_u24(0x00FF00))
                .parent_category(PluginCategory::Misc)
                .build()
                .unwrap(),
        )
        .plugin_start_cmd(format!(
            "%TP_PLUGIN_FOLDER%InvalidNumberRanges/{}{}",
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
                            Data::builder()
                                .id("out_of_range_number")
                                .format(DataFormat::Number(
                                    NumberData::builder()
                                        .initial(150.0) // Initial value outside min/max range
                                        .min_value(0.0)
                                        .max_value(100.0)
                                        .allow_decimals(true)
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
                                                .line_format("Value: {$out_of_range_number$}")
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
