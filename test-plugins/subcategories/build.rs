use touchportal_sdk::{reexport::HexColor, *};

pub fn plugin() -> PluginDescription {
    PluginDescription::builder()
        .api(ApiVersion::V4_3)
        .version(1)
        .name("Subcategories Test Plugin")
        .id("com.thesquareplanet.touchportal.subcategories")
        .configuration(
            PluginConfiguration::builder()
                .color_dark(HexColor::from_u24(0x800080))
                .color_light(HexColor::from_u24(0xFF00FF))
                .parent_category(PluginCategory::Misc)
                .build()
                .unwrap(),
        )
        .plugin_start_cmd(format!(
            "%TP_PLUGIN_FOLDER%Subcategories/{}{}",
            std::env::var("CARGO_PKG_NAME").unwrap(),
            std::env::consts::EXE_SUFFIX
        ))
        .category(
            Category::builder()
                .id("main_cat")
                .name("Main Category")
                .sub_category(
                    SubCategory::builder()
                        .id("media_subcat")
                        .name("Media Controls")
                        .build()
                        .unwrap(),
                )
                .sub_category(
                    SubCategory::builder()
                        .id("settings_subcat")
                        .name("Settings")
                        .build()
                        .unwrap(),
                )
                .sub_category(
                    SubCategory::builder()
                        .id("advanced_subcat")
                        .name("Advanced")
                        .build()
                        .unwrap(),
                )
                .action(
                    Action::builder()
                        .id("media_action")
                        .name("Media Action")
                        .sub_category_id("media_subcat")
                        .implementation(ActionImplementation::Dynamic)
                        .lines(
                            Lines::builder()
                                .action(
                                    LingualLine::builder()
                                        .datum(
                                            Line::builder()
                                                .line_format("Execute media action")
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
                .action(
                    Action::builder()
                        .id("settings_action")
                        .name("Settings Action")
                        .sub_category_id("settings_subcat")
                        .implementation(ActionImplementation::Dynamic)
                        .lines(
                            Lines::builder()
                                .action(
                                    LingualLine::builder()
                                        .datum(
                                            Line::builder()
                                                .line_format("Execute settings action")
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
                        .id("advanced_event")
                        .name("Advanced Event")
                        .sub_category_id("advanced_subcat")
                        .format("When advanced state is $val")
                        .value(EventValueType::Choice(
                            EventChoiceValue::builder()
                                .choice("Enabled")
                                .choice("Disabled")
                                .build()
                                .unwrap(),
                        ))
                        .value_state_id("advanced_state")
                        .build()
                        .unwrap(),
                )
                .state(
                    State::builder()
                        .id("advanced_state")
                        .description("Advanced feature state")
                        .initial("Disabled")
                        .parent_group("Advanced")
                        .kind(StateType::Choice(
                            ChoiceState::builder()
                                .choice("Enabled")
                                .choice("Disabled")
                                .build()
                                .unwrap(),
                        ))
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
