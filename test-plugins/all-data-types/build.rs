use touchportal_sdk::{reexport::HexColor, *};

pub fn plugin() -> PluginDescription {
    PluginDescription::builder()
        .api(ApiVersion::V4_3)
        .version(1)
        .name("All Data Types Test Plugin")
        .id("com.thesquareplanet.touchportal.all-data-types")
        .configuration(
            PluginConfiguration::builder()
                .color_dark(HexColor::from_u24(0x6600FF))
                .color_light(HexColor::from_u24(0x9900FF))
                .parent_category(PluginCategory::Misc)
                .build()
                .unwrap(),
        )
        .setting(
            Setting::builder()
                .name("TextSetting")
                .initial("default text")
                .kind(SettingType::Text(
                    TextSetting::builder()
                        .max_length(50)
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
                .name("PasswordSetting")
                .initial("secret")
                .kind(SettingType::Text(
                    TextSetting::builder()
                        .max_length(20)
                        .is_password(true)
                        .read_only(false)
                        .build()
                        .unwrap(),
                ))
                .build()
                .unwrap(),
        )
        .setting(
            Setting::builder()
                .name("ReadOnlySetting")
                .initial("readonly value")
                .kind(SettingType::Text(
                    TextSetting::builder()
                        .max_length(30)
                        .is_password(false)
                        .read_only(true)
                        .build()
                        .unwrap(),
                ))
                .build()
                .unwrap(),
        )
        .setting(
            Setting::builder()
                .name("NumberSetting")
                .initial("42")
                .kind(SettingType::Number(
                    NumberSetting::builder()
                        .max_length(10)
                        .is_password(false)
                        .min_value(0.0)
                        .max_value(100.0)
                        .read_only(false)
                        .build()
                        .unwrap(),
                ))
                .build()
                .unwrap(),
        )
        .setting(
            Setting::builder()
                .name("SwitchSetting")
                .initial("On")
                .kind(SettingType::Switch(
                    SwitchSetting::builder().build().unwrap(),
                ))
                .build()
                .unwrap(),
        )
        .setting(
            Setting::builder()
                .name("ChoiceSetting")
                .initial("Option2")
                .kind(SettingType::Choice(
                    ChoiceSetting::builder()
                        .choice("Option1")
                        .choice("Option2")
                        .choice("Option3")
                        .build()
                        .unwrap(),
                ))
                .build()
                .unwrap(),
        )
        .plugin_start_cmd(format!(
            "%TP_PLUGIN_FOLDER%AllDataTypes/{}{}",
            std::env::var("CARGO_PKG_NAME").unwrap(),
            std::env::consts::EXE_SUFFIX
        ))
        .category(
            Category::builder()
                .id("data_types_cat")
                .name("Data Types Category")
                .action(
                    Action::builder()
                        .id("comprehensive_action")
                        .name("Test All Data Types")
                        .implementation(ActionImplementation::Dynamic)
                        .datum(
                            Data::builder()
                                .id("text_field")
                                .format(DataFormat::Text(TextData::builder().build().unwrap()))
                                .build()
                                .unwrap(),
                        )
                        .datum(
                            Data::builder()
                                .id("number_field")
                                .format(DataFormat::Number(
                                    NumberData::builder().initial(10.5).build().unwrap(),
                                ))
                                .build()
                                .unwrap(),
                        )
                        .datum(
                            Data::builder()
                                .id("switch_field")
                                .format(DataFormat::Switch(
                                    SwitchData::builder().initial(false).build().unwrap(),
                                ))
                                .build()
                                .unwrap(),
                        )
                        .datum(
                            Data::builder()
                                .id("choice_field")
                                .format(DataFormat::Choice(
                                    ChoiceData::builder()
                                        .initial("Red")
                                        .choice("Red")
                                        .choice("Green")
                                        .choice("Blue")
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
                                                .line_format("Text: {$text_field$}")
                                                .build()
                                                .unwrap(),
                                        )
                                        .datum(
                                            Line::builder()
                                                .line_format("Number: {$number_field$}")
                                                .build()
                                                .unwrap(),
                                        )
                                        .datum(
                                            Line::builder()
                                                .line_format("Switch: {$switch_field$}")
                                                .build()
                                                .unwrap(),
                                        )
                                        .datum(
                                            Line::builder()
                                                .line_format("Choice: {$choice_field$}")
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
                .event(
                    Event::builder()
                        .id("text_event")
                        .name("Text State Change")
                        .format("When text becomes $val")
                        .value(EventValueType::Text(
                            EventTextConfiguration::builder()
                                .compare_with(CompareMethod::ExtendedString)
                                .build()
                                .unwrap(),
                        ))
                        .value_state_id("text_state")
                        .build()
                        .unwrap(),
                )
                .event(
                    Event::builder()
                        .id("choice_event")
                        .name("Color Selection")
                        .format("When color is $val")
                        .value(EventValueType::Choice(
                            EventChoiceValue::builder()
                                .choice("Red")
                                .choice("Green")
                                .choice("Blue")
                                .build()
                                .unwrap(),
                        ))
                        .value_state_id("color_state")
                        .build()
                        .unwrap(),
                )
                .state(
                    State::builder()
                        .id("text_state")
                        .description("Dynamic text state")
                        .initial("Initial text")
                        .parent_group("Text States")
                        .kind(StateType::Text(TextState::builder().build().unwrap()))
                        .build()
                        .unwrap(),
                )
                .state(
                    State::builder()
                        .id("color_state")
                        .description("Color choice state")
                        .initial("Red")
                        .parent_group("Choice States")
                        .kind(StateType::Choice(
                            ChoiceState::builder()
                                .choice("Red")
                                .choice("Green")
                                .choice("Blue")
                                .build()
                                .unwrap(),
                        ))
                        .build()
                        .unwrap(),
                )
                .state(
                    State::builder()
                        .id("counter_state")
                        .description("Numeric counter")
                        .initial("0")
                        .parent_group("Numeric States")
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
