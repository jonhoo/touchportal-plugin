use touchportal_sdk::{reexport::HexColor, *};

pub fn plugin() -> PluginDescription {
    PluginDescription::builder()
        .api(ApiVersion::V4_3)
        .version(1)
        .name("YouTube Live")
        .id("com.thesquareplanet.touchportal.youtube")
        .configuration(
            PluginConfiguration::builder()
                .color_dark(HexColor::from_u24(0x282828))
                .color_light(HexColor::from_u24(0xff0000))
                .parent_category(PluginCategory::Streaming)
                .build()
                .unwrap(),
        )
        .setting(
            Setting::builder()
                .name("YouTube API Access Token")
                .initial("")
                .kind(SettingType::Text(
                    TextSetting::builder()
                        .read_only(true)
                        .is_password(true)
                        .build()
                        .unwrap(),
                ))
                .build()
                .unwrap(),
        )
        .plugin_start_cmd(format!(
            "%TP_PLUGIN_FOLDER%YouTubeLive/{}{}",
            std::env::var("CARGO_PKG_NAME").unwrap(),
            std::env::consts::EXE_SUFFIX
        ))
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
