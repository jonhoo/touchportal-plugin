// This plugin tests validation failure when a choice-type event references a text-type state (type mismatch).

use touchportal_sdk::{reexport::HexColor, *};

pub fn plugin() -> PluginDescription {
    PluginDescription::builder()
        .api(ApiVersion::V4_3)
        .version(1)
        .name("Choice Event Text State Test")
        .id("com.test.choice-event-text-state")
        .configuration(
            PluginConfiguration::builder()
                .color_dark(HexColor::from_u24(0xFF0000))
                .color_light(HexColor::from_u24(0x00FF00))
                .parent_category(PluginCategory::Misc)
                .build()
                .unwrap(),
        )
        .plugin_start_cmd(format!(
            "%TP_PLUGIN_FOLDER%ChoiceEventTextState/{}{}",
            std::env::var("CARGO_PKG_NAME").unwrap(),
            std::env::consts::EXE_SUFFIX
        ))
        .category(
            Category::builder()
                .id("test_cat")
                .name("Test Category")
                .event(
                    Event::builder()
                        .id("choice_event")
                        .name("Choice Event")
                        .format("When value is $val")
                        .value(EventValueType::Choice(
                            EventChoiceValue::builder()
                                .choice("Active")
                                .choice("Inactive")
                                .build()
                                .unwrap(),
                        ))
                        // This references a text state - should cause validation failure
                        .value_state_id("text_state")
                        .build()
                        .unwrap(),
                )
                .state(
                    State::builder()
                        .id("text_state")
                        .description("A text state")
                        .initial("Some text")
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
