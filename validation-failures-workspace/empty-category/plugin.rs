use touchportal_sdk::{reexport::HexColor, *};

pub fn plugin() -> PluginDescription {
    // This plugin tests validation failure for empty categories
    PluginDescription::builder()
        .api(ApiVersion::V4_3)
        .version(1)
        .name("Empty Category Test")
        .id("com.test.empty-category")
        .configuration(
            PluginConfiguration::builder()
                .color_dark(HexColor::from_u24(0xFF0000))
                .color_light(HexColor::from_u24(0x00FF00))
                .parent_category(PluginCategory::Misc)
                .build()
                .unwrap(),
        )
        .plugin_start_cmd(format!(
            "%TP_PLUGIN_FOLDER%EmptyCategory/{}{}",
            std::env::var("CARGO_PKG_NAME").unwrap(),
            std::env::consts::EXE_SUFFIX
        ))
        .category(
            Category::builder()
                .id("empty_cat")
                .name("Empty Category")
                // No actions, events, states, or connectors - should cause validation failure
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
