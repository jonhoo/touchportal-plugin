use touchportal_sdk::{reexport::HexColor, *};

pub fn plugin() -> PluginDescription {
    // This plugin tests validation failure for actions with no lines
    PluginDescription::builder()
        .api(ApiVersion::V4_3)
        .version(1)
        .name("Actions No Lines Test")
        .id("com.test.actions-no-lines")
        .configuration(
            PluginConfiguration::builder()
                .color_dark(HexColor::from_u24(0xFF0000))
                .color_light(HexColor::from_u24(0x00FF00))
                .parent_category(PluginCategory::Misc)
                .build()
                .unwrap(),
        )
        .plugin_start_cmd(format!(
            "%TP_PLUGIN_FOLDER%ActionsNoLines/{}{}",
            std::env::var("CARGO_PKG_NAME").unwrap(),
            std::env::consts::EXE_SUFFIX
        ))
        .category(
            Category::builder()
                .id("test_cat")
                .name("Test Category")
                .action(
                    Action::builder()
                        .id("no_lines_action")
                        .name("No Lines Action")
                        .implementation(ActionImplementation::Dynamic)
                        // Intentionally not adding any lines - this should cause validation failure
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
