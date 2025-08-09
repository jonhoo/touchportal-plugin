use touchportal_sdk::{reexport::HexColor, *};

pub fn plugin() -> PluginDescription {
    // This plugin tests validation failure for events referencing non-existent states
    PluginDescription::builder()
        .api(ApiVersion::V4_3)
        .version(1)
        .name("Events Nonexistent States Test")
        .id("com.test.events-nonexistent-states")
        .configuration(
            PluginConfiguration::builder()
                .color_dark(HexColor::from_u24(0xFF0000))
                .color_light(HexColor::from_u24(0x00FF00))
                .parent_category(PluginCategory::Misc)
                .build()
                .unwrap(),
        )
        .plugin_start_cmd(format!(
            "%TP_PLUGIN_FOLDER%EventsNonexistentStates/{}{}",
            std::env::var("CARGO_PKG_NAME").unwrap(),
            std::env::consts::EXE_SUFFIX
        ))
        .category(
            Category::builder()
                .id("test_cat")
                .name("Test Category")
                .state(
                    State::builder()
                        .id("existing_state")
                        .description("Existing State")
                        .initial("default")
                        .kind(StateType::Text(TextState::builder().build().unwrap()))
                        .build()
                        .unwrap(),
                )
                .event(
                    Event::builder()
                        .id("test_event")
                        .name("Test Event")
                        .format("Test event triggered")
                        .value(EventValueType::Text(EventTextConfiguration::builder().build().unwrap()))
                        .value_state_id("nonexistent_state") // References state that doesn't exist
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