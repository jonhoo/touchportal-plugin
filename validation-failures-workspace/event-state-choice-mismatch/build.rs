// This plugin tests validation failure when an event references a state but they have different choice sets.

use touchportal_sdk::{reexport::HexColor, *};

pub fn plugin() -> PluginDescription {
    PluginDescription::builder()
        .api(ApiVersion::V4_3)
        .version(1)
        .name("Validation Failures Test Plugin")
        .id("com.thesquareplanet.touchportal.validation-failures")
        .configuration(
            PluginConfiguration::builder()
                .color_dark(HexColor::from_u24(0xFF0000))
                .color_light(HexColor::from_u24(0x00FF00))
                .parent_category(PluginCategory::Misc)
                .build()
                .unwrap(),
        )
        .plugin_start_cmd(format!(
            "%TP_PLUGIN_FOLDER%ValidationFailures/{}{}",
            std::env::var("CARGO_PKG_NAME").unwrap(),
            std::env::consts::EXE_SUFFIX
        ))
        .category(
            Category::builder()
                .id("validation_cat")
                .name("Validation Category")
                .event(
                    Event::builder()
                        .id("mismatched_event")
                        .name("Mismatched Event")
                        .format("When value is $val")
                        .value(EventValueType::Choice(
                            EventChoiceValue::builder()
                                .choice("Option1")
                                .choice("Option2")
                                // This will cause validation failure - choices don't match state
                                .build()
                                .unwrap(),
                        ))
                        .value_state_id("mismatched_state")
                        .build()
                        .unwrap(),
                )
                .state(
                    State::builder()
                        .id("mismatched_state")
                        .description("State with different choices than event")
                        .initial("Different1")
                        .kind(StateType::Choice(
                            ChoiceState::builder()
                                .choice("Different1")
                                .choice("Different2")
                                .choice("Different3")
                                // These choices don't match the event above
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