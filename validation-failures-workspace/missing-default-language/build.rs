// This plugin tests validation failure when action lines don't include a "default" language.

use touchportal_sdk::{reexport::HexColor, *};

pub fn plugin() -> PluginDescription {
    PluginDescription::builder()
        .api(ApiVersion::V4_3)
        .version(1)
        .name("Missing Default Language Test")
        .id("com.test.missing-default-language")
        .configuration(
            PluginConfiguration::builder()
                .color_dark(HexColor::from_u24(0xFF0000))
                .color_light(HexColor::from_u24(0x00FF00))
                .parent_category(PluginCategory::Misc)
                .build()
                .unwrap(),
        )
        .plugin_start_cmd(format!(
            "%TP_PLUGIN_FOLDER%MissingDefaultLanguage/{}{}",
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
                        .lines(
                            Lines::builder()
                                .action(
                                    LingualLine::builder()
                                        // Missing "default" language - only has "en"
                                        .language("en")
                                        .datum(
                                            Line::builder()
                                                .line_format("English action")
                                                .build()
                                                .unwrap()
                                        )
                                        .build()
                                        .unwrap()
                                )
                                .action(
                                    LingualLine::builder()
                                        .language("fr")
                                        .datum(
                                            Line::builder()
                                                .line_format("French action")
                                                .build()
                                                .unwrap()
                                        )
                                        .build()
                                        .unwrap()
                                )
                                // Should cause validation failure - no "default" language
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