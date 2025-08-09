use touchportal_sdk::{reexport::HexColor, *};

fn plugin() -> PluginDescription {
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
                .name("YouTube API access tokens")
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
        .category(
            Category::builder()
                .id("ytl_account_management")
                .name("Account Management")
                .action(
                    Action::builder()
                        .id("ytl_authenticate_account")
                        .name("Authenticate account")
                        .implementation(ActionImplementation::Dynamic)
                        .lines(
                            Lines::builder()
                                .action(
                                    LingualLine::builder()
                                        .datum(
                                            Line::builder()
                                                .line_format("Add another YouTube account")
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
        .category(
            Category::builder()
                .id("ytl_live_broadcasts")
                .name("Live Broadcasts")
                .action(
                    Action::builder()
                        .id("ytl_live_broadcast_toggle")
                        .name("Toggle broadcast liveness")
                        .implementation(ActionImplementation::Dynamic)
                        .datum(
                            Data::builder()
                                .id("ytl_channel")
                                .format(DataFormat::Choice(
                                    ChoiceData::builder()
                                        .initial("")
                                        .choice("")
                                        .build()
                                        .unwrap(),
                                ))
                                .build()
                                .unwrap(),
                        )
                        .datum(
                            Data::builder()
                                .id("ytl_broadcast")
                                .format(DataFormat::Choice(
                                    ChoiceData::builder()
                                        .initial("Select channel first")
                                        .choice("Select channel first")
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
                                                .line_format(
                                                    "Start or end live broadcast {$ytl_broadcast$} \
                                                    on channel {$ytl_channel$}",
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
