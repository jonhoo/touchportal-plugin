use touchportal_sdk::{reexport::HexColor, *};

pub fn plugin() -> PluginDescription {
    // This plugin tests validation failure for actions exceeding maximum lines
    PluginDescription::builder()
        .api(ApiVersion::V4_3)
        .version(1)
        .name("Actions Max Lines Test")
        .id("com.test.actions-max-lines")
        .configuration(
            PluginConfiguration::builder()
                .color_dark(HexColor::from_u24(0xFF0000))
                .color_light(HexColor::from_u24(0x00FF00))
                .parent_category(PluginCategory::Misc)
                .build()
                .unwrap(),
        )
        .plugin_start_cmd(format!(
            "%TP_PLUGIN_FOLDER%ActionsMaxLines/{}{}",
            std::env::var("CARGO_PKG_NAME").unwrap(),
            std::env::consts::EXE_SUFFIX
        ))
        .category(
            Category::builder()
                .id("test_cat")
                .name("Test Category")
                .action(
                    Action::builder()
                        .id("excessive_lines_action")
                        .name("Action with Too Many Lines")
                        .implementation(ActionImplementation::Dynamic)
                        .lines(
                            Lines::builder()
                                .action(
                                    LingualLine::builder()
                                        .datum(
                                            Line::builder()
                                                .line_format("Line 1: Action with excessive lines")
                                                .build()
                                                .unwrap(),
                                        )
                                        .datum(
                                            Line::builder()
                                                .line_format("Line 2: This action has too many")
                                                .build()
                                                .unwrap(),
                                        )
                                        .datum(
                                            Line::builder()
                                                .line_format("Line 3: lines for good usability")
                                                .build()
                                                .unwrap(),
                                        )
                                        .datum(
                                            Line::builder()
                                                .line_format("Line 4: TouchPortal recommends")
                                                .build()
                                                .unwrap(),
                                        )
                                        .datum(
                                            Line::builder()
                                                .line_format("Line 5: maximum of 8 lines")
                                                .build()
                                                .unwrap(),
                                        )
                                        .datum(
                                            Line::builder()
                                                .line_format("Line 6: for proper visibility")
                                                .build()
                                                .unwrap(),
                                        )
                                        .datum(
                                            Line::builder()
                                                .line_format("Line 7: on smaller screens")
                                                .build()
                                                .unwrap(),
                                        )
                                        .datum(
                                            Line::builder()
                                                .line_format("Line 8: This is the maximum")
                                                .build()
                                                .unwrap(),
                                        )
                                        .datum(
                                            Line::builder()
                                                .line_format("Line 9: This line exceeds the limit!")
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