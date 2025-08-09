use touchportal_sdk::{reexport::HexColor, *};

pub fn plugin() -> PluginDescription {
    PluginDescription::builder()
        .api(ApiVersion::V4_3)
        .version(1)
        .name("No Events Test Plugin")
        .id("com.thesquareplanet.touchportal.no-events")
        .configuration(
            PluginConfiguration::builder()
                .color_dark(HexColor::from_u24(0xFF6600))
                .color_light(HexColor::from_u24(0xFFAA00))
                .parent_category(PluginCategory::Misc)
                .build()
                .unwrap(),
        )
        .plugin_start_cmd(format!(
            "%TP_PLUGIN_FOLDER%NoEvents/{}{}",
            std::env::var("CARGO_PKG_NAME").unwrap(),
            std::env::consts::EXE_SUFFIX
        ))
        .category(
            Category::builder()
                .id("no_events_cat")
                .name("No Events Category")
                .action(
                    Action::builder()
                        .id("simple_action")
                        .name("Simple Action")
                        .implementation(ActionImplementation::Dynamic)
                        .datum(
                            Data::builder()
                                .id("text_input")
                                .format(DataFormat::Text(TextData::builder().build().unwrap()))
                                .build()
                                .unwrap(),
                        )
                        .lines(
                            Lines::builder()
                                .action(
                                    LingualLine::builder()
                                        .datum(
                                            Line::builder()
                                                .line_format("Process text: {$text_input$}")
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
                        .id("counter_action")
                        .name("Increment Counter")
                        .implementation(ActionImplementation::Dynamic)
                        .lines(
                            Lines::builder()
                                .action(
                                    LingualLine::builder()
                                        .datum(
                                            Line::builder()
                                                .line_format("Increment counter")
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
                        .id("result_state")
                        .description("Last action result")
                        .initial("Ready")
                        .kind(StateType::Text(TextState::builder().build().unwrap()))
                        .build()
                        .unwrap(),
                )
                .state(
                    State::builder()
                        .id("counter_state")
                        .description("Action counter")
                        .initial("0")
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
