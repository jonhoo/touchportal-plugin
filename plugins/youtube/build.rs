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
        // ==============================================================================
        // Settings Configuration
        // ==============================================================================
        // Token storage for OAuth credentials across plugin restarts
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
        // Polling interval with minimum to respect API quotas
        .setting(
            Setting::builder()
                .name("Polling interval (seconds)")
                .initial("60")
                .kind(SettingType::Number(
                    NumberSetting::builder()
                        .min_value(30.0) // Minimum to avoid API quota exhaustion
                        .max_value(3600.0) // Maximum 1 hour
                        .build()
                        .unwrap(),
                ))
                .build()
                .unwrap(),
        )
        // Current selected channel (persisted across restarts)
        .setting(
            Setting::builder()
                .name("Selected channel ID")
                .initial("")
                .kind(SettingType::Text(
                    TextSetting::builder()
                        .read_only(true)
                        .build()
                        .unwrap(),
                ))
                .build()
                .unwrap(),
        )
        // Current selected broadcast (persisted across restarts)
        .setting(
            Setting::builder()
                .name("Selected broadcast ID")
                .initial("")
                .kind(SettingType::Text(
                    TextSetting::builder()
                        .read_only(true)
                        .build()
                        .unwrap(),
                ))
                .build()
                .unwrap(),
        )
        // ==============================================================================
        // Main YouTube Live Category with Subcategories
        // ==============================================================================
        // Following TwitchTheSecond plugin pattern for better organization
        .category(
            Category::builder()
                .id("ytl_youtube_live")
                .name("YouTube Live")
                // ==============================================================================
                // Configuration Subcategory
                // ==============================================================================
                // Account management and stream selection - setup functions
                .sub_category(
                    SubCategory::builder()
                        .id("ytl_configuration")
                        .name("Configuration")
                        .build()
                        .unwrap()
                )
                // ==============================================================================
                // Broadcaster Controls Subcategory  
                // ==============================================================================
                // Stream management and content creation tools
                .sub_category(
                    SubCategory::builder()
                        .id("ytl_broadcaster_controls")
                        .name("Broadcaster Controls")
                        .build()
                        .unwrap()
                )
                // ==============================================================================
                // Analytics & Monitoring Subcategory
                // ==============================================================================
                // Real-time metrics, chat events, and monitoring states
                .sub_category(
                    SubCategory::builder()
                        .id("ytl_analytics_monitoring")
                        .name("Analytics & Monitoring")
                        .build()
                        .unwrap()
                )
                // ==============================================================================
                // Configuration Actions
                // ==============================================================================
                .action(
                    Action::builder()
                        .id("ytl_add_youtube_channel")
                        .name("Add YouTube Channel")
                        .implementation(ActionImplementation::Dynamic)
                        .sub_category_id("ytl_configuration")
                        .lines(
                            Lines::builder()
                                .action(
                                    LingualLine::builder()
                                        .datum(
                                            Line::builder()
                                                .line_format("Add another YouTube channel for multi-account management")
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
                        .id("ytl_select_stream")
                        .name("Select Stream")
                        .implementation(ActionImplementation::Dynamic)
                        .sub_category_id("ytl_configuration")
                        .datum(
                            Data::builder()
                                .id("ytl_channel")
                                .format(DataFormat::Choice(
                                    ChoiceData::builder()
                                        .initial("No channels available")
                                        .choice("No channels available")
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
                                        .choice("Latest non-completed broadcast")
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
                                                    "Select broadcast {$ytl_broadcast$} from channel {$ytl_channel$}",
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
                // ==============================================================================
                // Broadcaster Control Actions
                // ==============================================================================
                .action(
                    Action::builder()
                        .id("ytl_start_broadcast")
                        .name("Start Live Broadcast")
                        .implementation(ActionImplementation::Dynamic)
                        .sub_category_id("ytl_broadcaster_controls")
                        .lines(
                            Lines::builder()
                                .action(
                                    LingualLine::builder()
                                        .datum(
                                            Line::builder()
                                                .line_format("Start the selected live broadcast")
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
                        .id("ytl_stop_broadcast")
                        .name("Stop Live Broadcast")
                        .implementation(ActionImplementation::Dynamic)
                        .sub_category_id("ytl_broadcaster_controls")
                        .lines(
                            Lines::builder()
                                .action(
                                    LingualLine::builder()
                                        .datum(
                                            Line::builder()
                                                .line_format("Stop the selected live broadcast")
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
                        .id("ytl_update_title")
                        .name("Update Stream Title")
                        .implementation(ActionImplementation::Dynamic)
                        .sub_category_id("ytl_broadcaster_controls")
                        .datum(
                            Data::builder()
                                .id("ytl_new_title")
                                .format(DataFormat::Text(
                                    TextData::builder()
                                        .initial("")
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
                                                .line_format("Update stream title to {$ytl_new_title$}")
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
                        .id("ytl_update_description")
                        .name("Update Stream Description")
                        .implementation(ActionImplementation::Dynamic)
                        .sub_category_id("ytl_broadcaster_controls")
                        .datum(
                            Data::builder()
                                .id("ytl_new_description")
                                .format(DataFormat::Text(
                                    TextData::builder()
                                        .initial("")
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
                                                .line_format("Update stream description to {$ytl_new_description$}")
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
                // ==============================================================================
                // Chat Events - Analytics & Monitoring
                // ==============================================================================
                .event(
                    Event::builder()
                        .id("ytl_new_chat_message")
                        .name("On New Chat Message")
                        .format("When you receive a chat message that $compare to $val")
                        .sub_category_id("ytl_analytics_monitoring")
                        .value(EventValueType::Text(
                            EventTextConfiguration::builder().build().unwrap(),
                        ))
                        .value_state_id("ytl_latest_chat_message")
                        .local_state(
                            LocalState::builder()
                                .id("ytl_chat_message")
                                .name("YouTube Live - Chat Message")
                                .build()
                                .unwrap(),
                        )
                        .local_state(
                            LocalState::builder()
                                .id("ytl_chat_author")
                                .name("YouTube Live - Chat Author")
                                .build()
                                .unwrap(),
                        )
                        .local_state(
                            LocalState::builder()
                                .id("ytl_chat_author_id")
                                .name("YouTube Live - Chat Author ID")
                                .build()
                                .unwrap(),
                        )
                        .local_state(
                            LocalState::builder()
                                .id("ytl_chat_timestamp")
                                .name("YouTube Live - Chat Timestamp")
                                .build()
                                .unwrap(),
                        )
                        .build()
                        .unwrap(),
                )
                .event(
                    Event::builder()
                        .id("ytl_new_super_chat")
                        .name("On New Super Chat")
                        .format("When you receive a Super Chat")
                        .sub_category_id("ytl_analytics_monitoring")
                        .value(EventValueType::Text(
                            EventTextConfiguration::builder().build().unwrap(),
                        ))
                        .value_state_id("ytl_latest_super_chat")
                        .local_state(
                            LocalState::builder()
                                .id("ytl_super_chat_message")
                                .name("YouTube Live - Super Chat Message")
                                .build()
                                .unwrap(),
                        )
                        .local_state(
                            LocalState::builder()
                                .id("ytl_super_chat_author")
                                .name("YouTube Live - Super Chat Author")
                                .build()
                                .unwrap(),
                        )
                        .local_state(
                            LocalState::builder()
                                .id("ytl_super_chat_amount")
                                .name("YouTube Live - Super Chat Amount")
                                .build()
                                .unwrap(),
                        )
                        .local_state(
                            LocalState::builder()
                                .id("ytl_super_chat_currency")
                                .name("YouTube Live - Super Chat Currency")
                                .build()
                                .unwrap(),
                        )
                        .build()
                        .unwrap(),
                )
                .event(
                    Event::builder()
                        .id("ytl_new_sponsor")
                        .name("On New Sponsor")
                        .format("When you receive a new sponsor/member")
                        .sub_category_id("ytl_analytics_monitoring")
                        .value(EventValueType::Text(
                            EventTextConfiguration::builder().build().unwrap(),
                        ))
                        .value_state_id("ytl_latest_sponsor")
                        .local_state(
                            LocalState::builder()
                                .id("ytl_sponsor_name")
                                .name("YouTube Live - Sponsor Name")
                                .build()
                                .unwrap(),
                        )
                        .local_state(
                            LocalState::builder()
                                .id("ytl_sponsor_level")
                                .name("YouTube Live - Sponsorship Level")
                                .build()
                                .unwrap(),
                        )
                        .local_state(
                            LocalState::builder()
                                .id("ytl_sponsor_months")
                                .name("YouTube Live - Months Sponsored")
                                .build()
                                .unwrap(),
                        )
                        .build()
                        .unwrap(),
                )
                // ==============================================================================
                // States - Analytics & Monitoring
                // ==============================================================================
                // Stream Statistics States - polled periodically based on polling interval setting
                .state(
                    State::builder()
                        .id("ytl_likes_count")
                        .description("YouTube Live - Likes Count")
                        .initial("-")
                        .parent_group("Stream Metrics")
                        .kind(StateType::Text(TextState::builder().build().unwrap()))
                        .build()
                        .unwrap(),
                )
                .state(
                    State::builder()
                        .id("ytl_dislikes_count")
                        .description("YouTube Live - Dislikes Count")
                        .initial("-")
                        .parent_group("Stream Metrics")
                        .kind(StateType::Text(TextState::builder().build().unwrap()))
                        .build()
                        .unwrap(),
                )
                .state(
                    State::builder()
                        .id("ytl_views_count")
                        .description("YouTube Live - Views Count")
                        .initial("-")
                        .parent_group("Stream Metrics")
                        .kind(StateType::Text(TextState::builder().build().unwrap()))
                        .build()
                        .unwrap(),
                )
                .state(
                    State::builder()
                        .id("ytl_live_viewers_count")
                        .description("YouTube Live - Live Viewers Count")
                        .initial("-")
                        .parent_group("Stream Metrics")
                        .kind(StateType::Text(TextState::builder().build().unwrap()))
                        .build()
                        .unwrap(),
                )
                // Stream Info States - current stream and channel information
                .state(
                    State::builder()
                        .id("ytl_current_stream_title")
                        .description("YouTube Live - Current Stream Title")
                        .initial("-")
                        .parent_group("Stream Info")
                        .kind(StateType::Text(TextState::builder().build().unwrap()))
                        .build()
                        .unwrap(),
                )
                .state(
                    State::builder()
                        .id("ytl_selected_channel_name")
                        .description("YouTube Live - Selected Channel Name")
                        .initial("-")
                        .parent_group("Stream Info")
                        .kind(StateType::Text(TextState::builder().build().unwrap()))
                        .build()
                        .unwrap(),
                )
                // Event Value States - these states hold the triggering values for events
                .state(
                    State::builder()
                        .id("ytl_latest_chat_message")
                        .description("YouTube Live - Latest Chat Message")
                        .initial("-")
                        .parent_group("Chat Events")
                        .kind(StateType::Text(TextState::builder().build().unwrap()))
                        .build()
                        .unwrap(),
                )
                .state(
                    State::builder()
                        .id("ytl_latest_super_chat")
                        .description("YouTube Live - Latest Super Chat")
                        .initial("-")
                        .parent_group("Chat Events")
                        .kind(StateType::Text(TextState::builder().build().unwrap()))
                        .build()
                        .unwrap(),
                )
                .state(
                    State::builder()
                        .id("ytl_latest_sponsor")
                        .description("YouTube Live - Latest Sponsor")
                        .initial("-")
                        .parent_group("Chat Events")
                        .kind(StateType::Text(TextState::builder().build().unwrap()))
                        .build()
                        .unwrap(),
                )
                .build()
                .unwrap(),
        )
        .plugin_start_cmd(format!(
            "%TP_PLUGIN_FOLDER%YouTubeLive/touchportal-youtube-live{}",
            std::env::consts::EXE_SUFFIX
        ))
        .build()
        .unwrap()
}

fn main() {
    let plugin = plugin();
    touchportal_sdk::codegen::export(&plugin);
}
