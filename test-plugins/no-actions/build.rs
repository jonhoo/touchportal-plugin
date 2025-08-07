use touchportal_sdk::{reexport::HexColor, *};

pub fn plugin() -> PluginDescription {
    PluginDescription::builder()
        .api(ApiVersion::V4_3)
        .version(1)
        .name("No Actions Test Plugin")
        .id("com.thesquareplanet.touchportal.no-actions")
        .configuration(
            PluginConfiguration::builder()
                .color_dark(HexColor::from_u24(0x0000FF))
                .color_light(HexColor::from_u24(0x00AAFF))
                .parent_category(PluginCategory::Misc)
                .build()
                .unwrap(),
        )
        .plugin_start_cmd(format!(
            "%TP_PLUGIN_FOLDER%NoActions/{}{}",
            std::env::var("CARGO_PKG_NAME").unwrap(),
            std::env::consts::EXE_SUFFIX
        ))
        .category(
            Category::builder()
                .id("no_actions_cat")
                .name("No Actions Category")
                .event(
                    Event::builder()
                        .id("status_change")
                        .name("When status changes")
                        .format("When status becomes $val")
                        .value(EventValueType::Choice(
                            EventChoiceValue::builder()
                                .choice("Active")
                                .choice("Inactive")
                                .choice("Error")
                                .build()
                                .unwrap(),
                        ))
                        .value_state_id("status_state")
                        .build()
                        .unwrap(),
                )
                .event(
                    Event::builder()
                        .id("counter_event")
                        .name("Counter threshold")
                        .format("When counter $compare $val")
                        .value(EventValueType::Text(
                            EventTextConfiguration::builder()
                                .compare_with(CompareMethod::ExtendedString)
                                .build()
                                .unwrap(),
                        ))
                        .value_state_id("counter_state")
                        .build()
                        .unwrap(),
                )
                .state(
                    State::builder()
                        .id("status_state")
                        .description("Current plugin status")
                        .initial("Inactive")
                        .parent_group("Status")
                        .kind(StateType::Choice(
                            ChoiceState::builder()
                                .choice("Active")
                                .choice("Inactive")
                                .choice("Error")
                                .build()
                                .unwrap(),
                        ))
                        .build()
                        .unwrap(),
                )
                .state(
                    State::builder()
                        .id("counter_state")
                        .description("Event counter")
                        .initial("0")
                        .parent_group("Counters")
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
