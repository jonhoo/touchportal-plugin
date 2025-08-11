use touchportal_sdk::{reexport::HexColor, *};

pub fn plugin() -> PluginDescription {
    PluginDescription::builder()
        .api(ApiVersion::V4_3)
        .version(1)
        .name("Boundary Values Test Plugin")
        .id("com.thesquareplanet.touchportal.boundary-values")
        .configuration(
            PluginConfiguration::builder()
                .color_dark(HexColor::from_u24(0xFFFFFF))
                .color_light(HexColor::from_u24(0x000000))
                .parent_category(PluginCategory::Misc)
                .build()
                .unwrap(),
        )
        .setting(
            Setting::builder()
                .name("MaxLengthText")
                .initial("x".repeat(200))
                .kind(SettingType::Text(
                    TextSetting::builder()
                        .max_length(200)
                        .is_password(false)
                        .read_only(false)
                        .build()
                        .unwrap(),
                ))
                .build()
                .unwrap(),
        )
        .setting(
            Setting::builder()
                .name("BoundaryNumber")
                .initial("999.99")
                .kind(SettingType::Number(
                    NumberSetting::builder()
                        .max_length(10)
                        .is_password(false)
                        .min_value(-1000.0)
                        .max_value(1000.0)
                        .read_only(false)
                        .build()
                        .unwrap(),
                ))
                .build()
                .unwrap(),
        )
        .plugin_start_cmd(format!(
            "%TP_PLUGIN_FOLDER%BoundaryValues/{}{}",
            std::env::var("CARGO_PKG_NAME").unwrap(),
            std::env::consts::EXE_SUFFIX
        ))
        .category(
            Category::builder()
                .id("boundary_cat")
                .name("Boundary Values Category")
                .action(
                    Action::builder()
                        .id("boundary_action")
                        .name("Test Boundary Values")
                        .implementation(ActionImplementation::Dynamic)
                        .datum(
                            Data::builder()
                                .id("max_text")
                                .format(DataFormat::Text(TextData::builder().build().unwrap()))
                                .build()
                                .unwrap(),
                        )
                        .datum(
                            Data::builder()
                                .id("boundary_number")
                                .format(DataFormat::Number(
                                    NumberData::builder()
                                        .initial(-999.99)
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
                                                .line_format("Process max text: {$max_text$}")
                                                .build()
                                                .unwrap(),
                                        )
                                        .datum(
                                            Line::builder()
                                                .line_format("With number: {$boundary_number$}")
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
                .state(
                    State::builder()
                        .id("long_text_state")
                        .description("Very long text state for testing")
                        .initial("This is a very long initial text value that tests the state handling with longer strings and various characters: !@#$%^&*()_+-={}[]|:;\"'<>?,./ and numbers 0123456789")
                        .kind(StateType::Text(TextState::builder().build().unwrap()))
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
