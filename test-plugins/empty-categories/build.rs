use touchportal_sdk::{reexport::HexColor, *};

pub fn plugin() -> PluginDescription {
    PluginDescription::builder()
        .api(ApiVersion::V4_3)
        .version(1)
        .name("Empty Categories Test Plugin")
        .id("com.thesquareplanet.touchportal.empty-categories")
        .configuration(
            PluginConfiguration::builder()
                .color_dark(HexColor::from_u24(0x000000))
                .color_light(HexColor::from_u24(0x222222))
                .parent_category(PluginCategory::Misc)
                .build()
                .unwrap(),
        )
        .plugin_start_cmd(format!(
            "%TP_PLUGIN_FOLDER%EmptyCategories/{}{}",
            std::env::var("CARGO_PKG_NAME").unwrap(),
            std::env::consts::EXE_SUFFIX
        ))
        .category(
            Category::builder()
                .id("empty_cat_1")
                .name("Empty Category 1")
                .build()
                .unwrap(),
        )
        .category(
            Category::builder()
                .id("empty_cat_2")
                .name("Empty Category 2")
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
