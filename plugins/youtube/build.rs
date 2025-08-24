use touchportal_sdk::{reexport::HexColor, *};

/// Minimum polling interval to avoid YouTube API quota exhaustion
const MIN_POLLING_INTERVAL_SECONDS: u64 = 30;

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
        // Logging verbosity control for debugging and monitoring
        .setting(
            Setting::builder()
                .name("Logging verbosity")
                .initial("info")
                .tooltip(
                    Tooltip::builder()
                        .title("Plugin Logging Level")
                        .body(
                            "Controls the verbosity of plugin logging output. \
                            Info shows basic operation messages, debug shows detailed \
                            operation information, and trace shows comprehensive \
                            debugging information including API calls."
                        )
                        .build()
                        .unwrap(),
                )
                .kind(SettingType::Choice(
                    ChoiceSetting::builder()
                        .choice("info")
                        .choice("debug")
                        .choice("trace")
                        .build()
                        .unwrap(),
                ))
                .build()
                .unwrap(),
        )
        // Token storage for OAuth credentials across plugin restarts
        .setting(
            Setting::builder()
                .name("YouTube API access tokens")
                .initial("")
                .tooltip(
                    Tooltip::builder()
                        .title("OAuth Authentication")
                        .body(
                            "Stores encrypted OAuth tokens for YouTube API access. \
                            These tokens are automatically managed through the authentication \
                            flow and should not be modified manually."
                        )
                        .doc_url("https://developers.google.com/youtube/v3/guides/auth/installed-apps")
                        .build()
                        .unwrap(),
                )
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
        // Smart polling adjustment toggle
        .setting(
            Setting::builder()
                .name("Smart polling adjustment")
                .initial("On")
                .tooltip(
                    Tooltip::builder()
                        .title("Adaptive API Usage")
                        .body(
                            "Automatically adjusts polling frequency based on stream activity \
                            to optimize API quota usage. Increases polling during active streams \
                            and reduces it when idle."
                        )
                        .build()
                        .unwrap(),
                )
                .kind(SettingType::Switch(
                    SwitchSetting::builder()
                        .build()
                        .unwrap(),
                ))
                .build()
                .unwrap(),
        )
        // Base polling interval - enhanced description for adaptive mode
        .setting(
            Setting::builder()
                .name("Base polling interval (seconds)")
                .initial("60")
                .tooltip(
                    Tooltip::builder()
                        .title("API Request Frequency")
                        .body(
                            "Sets the base interval (30-3600 seconds) between YouTube API \
                            requests. Lower values provide faster updates but consume more API \
                            quota. Recommended: 60-300 seconds for active monitoring."
                        )
                        .build()
                        .unwrap(),
                )
                .kind(SettingType::Number(
                    NumberSetting::builder()
                        .min_value(MIN_POLLING_INTERVAL_SECONDS as f64)
                        .max_value(3600.0)
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
                .tooltip(
                    Tooltip::builder()
                        .title("Current Channel Context")
                        .body(
                            "The ID of the currently selected YouTube channel. This value is \
                            automatically set when you authenticate and select a channel for \
                            monitoring through the Select stream action."
                        )
                        .build()
                        .unwrap(),
                )
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
                .tooltip(
                    Tooltip::builder()
                        .title("Active Stream Context")
                        .body(
                            "The ID of the currently monitored live broadcast. This value is \
                            automatically updated when a new live stream is selected through \
                            the Select stream action."
                        )
                        .build()
                        .unwrap(),
                )
                .kind(SettingType::Text(
                    TextSetting::builder()
                        .read_only(true)
                        .build()
                        .unwrap(),
                ))
                .build()
                .unwrap(),
        )
        // Custom OAuth credentials for heavy users with dedicated Google projects
        .setting(
            Setting::builder()
                .name("Custom OAuth client ID")
                .initial("")
                .tooltip(
                    Tooltip::builder()
                        .title("Custom OAuth Client ID")
                        .body(
                            "Optional: Use your own Google OAuth client ID for dedicated API \
                            quota. Leave empty to use shared defaults. Both client ID and \
                            secret must be provided together for custom credentials to be used."
                        )
                        .doc_url("https://github.com/jonhoo/touchportal-plugin/tree/main/plugins/youtube/QUOTA.md")
                        .build()
                        .unwrap(),
                )
                .kind(SettingType::Text(
                    TextSetting::builder()
                        .build()
                        .unwrap(),
                ))
                .build()
                .unwrap(),
        )
        .setting(
            Setting::builder()
                .name("Custom OAuth client secret")
                .initial("")
                .tooltip(
                    Tooltip::builder()
                        .title("Custom OAuth Client Secret")
                        .body(
                            "Optional: Use your own Google OAuth client secret for dedicated \
                            API quota. Leave empty to use shared defaults. Both client ID and \
                            secret must be provided together for custom credentials to be used."
                        )
                        .doc_url("https://github.com/jonhoo/touchportal-plugin/tree/main/plugins/youtube/QUOTA.md")
                        .build()
                        .unwrap(),
                )
                .kind(SettingType::Text(
                    TextSetting::builder()
                        .is_password(true)
                        .build()
                        .unwrap(),
                ))
                .build()
                .unwrap(),
        )
        // ==============================================================================
        // Main YouTube Live Category with Subcategories
        // ==============================================================================
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
                        .name("Add YouTube channel")
                        .implementation(ActionImplementation::Dynamic)
                        .sub_category_id("ytl_configuration")
                        .lines(
                            Lines::builder()
                                .action(
                                    LingualLine::builder()
                                        .datum(
                                            Line::builder()
                                                .line_format(
                                                    "Add another YouTube channel for \
                                                    multi-account management"
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
                .action(
                    Action::builder()
                        .id("ytl_select_stream")
                        .name("Select stream")
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
                                                    "Use broadcast {$ytl_broadcast$} from \
                                                    channel {$ytl_channel$}"
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
                                                .line_format(
                                                    "Update stream title to {$ytl_new_title$}"
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
                                                .line_format(
                                                    "Update stream description to \
                                                    {$ytl_new_description$}"
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
                // State-Based Chat Events - Analytics & Monitoring
                // ==============================================================================
                // These events trigger automatically when global states change
                .event(
                    Event::builder()
                        .id("ytl_last_chat_message_changed")
                        .name("On last chat message changed")
                        .format("When the last chat message changes")
                        .sub_category_id("ytl_analytics_monitoring")
                        .value(EventValueType::Text(
                            EventTextConfiguration::builder().build().unwrap(),
                        ))
                        .value_state_id("ytl_last_chat_message")
                        .build()
                        .unwrap(),
                )
                .event(
                    Event::builder()
                        .id("ytl_last_super_chat_changed")
                        .name("On last Super Chat changed")
                        .format("When the last super chat changes")
                        .sub_category_id("ytl_analytics_monitoring")
                        .value(EventValueType::Text(
                            EventTextConfiguration::builder().build().unwrap(),
                        ))
                        .value_state_id("ytl_last_super_chat")
                        .build()
                        .unwrap(),
                )
                .event(
                    Event::builder()
                        .id("ytl_last_member_changed")
                        .name("On last member changed")
                        .format("When the last member changes")
                        .sub_category_id("ytl_analytics_monitoring")
                        .value(EventValueType::Text(
                            EventTextConfiguration::builder().build().unwrap(),
                        ))
                        .value_state_id("ytl_last_member")
                        .build()
                        .unwrap(),
                )
                .event(
                    Event::builder()
                        .id("ytl_last_chat_author_changed")
                        .name("On last chat author changed")
                        .format("When the last chat author changes")
                        .sub_category_id("ytl_analytics_monitoring")
                        .value(EventValueType::Text(
                            EventTextConfiguration::builder().build().unwrap(),
                        ))
                        .value_state_id("ytl_last_chat_author")
                        .build()
                        .unwrap(),
                )
                .event(
                    Event::builder()
                        .id("ytl_last_super_chat_author_changed")
                        .name("On last Super Chat author changed")
                        .format("When the last super chat author changes")
                        .sub_category_id("ytl_analytics_monitoring")
                        .value(EventValueType::Text(
                            EventTextConfiguration::builder().build().unwrap(),
                        ))
                        .value_state_id("ytl_last_super_chat_author")
                        .build()
                        .unwrap(),
                )
                .event(
                    Event::builder()
                        .id("ytl_last_super_chat_amount_changed")
                        .name("On last Super Chat amount changed")
                        .format("When the last super chat amount changes")
                        .sub_category_id("ytl_analytics_monitoring")
                        .value(EventValueType::Text(
                            EventTextConfiguration::builder().build().unwrap(),
                        ))
                        .value_state_id("ytl_last_super_chat_amount")
                        .build()
                        .unwrap(),
                )
                .event(
                    Event::builder()
                        .id("ytl_last_member_tenure_changed")
                        .name("On last member tenure changed")
                        .format("When the last member tenure changes")
                        .sub_category_id("ytl_analytics_monitoring")
                        .value(EventValueType::Text(
                            EventTextConfiguration::builder().build().unwrap(),
                        ))
                        .value_state_id("ytl_last_member_tenure")
                        .build()
                        .unwrap(),
                )
                .event(
                    Event::builder()
                        .id("ytl_last_member_level_changed")
                        .name("On last member level changed")
                        .format("When the last member level changes")
                        .sub_category_id("ytl_analytics_monitoring")
                        .value(EventValueType::Text(
                            EventTextConfiguration::builder().build().unwrap(),
                        ))
                        .value_state_id("ytl_last_member_level")
                        .build()
                        .unwrap(),
                )
                // ==============================================================================
                // Action-Based Chat Events - Analytics & Monitoring
                // ==============================================================================
                // These events trigger explicitly with rich local state data
                .event(
                    Event::builder()
                        .id("ytl_new_chat_message")
                        .name("On new chat message")
                        .format("When you receive a new chat message")
                        .sub_category_id("ytl_analytics_monitoring")
                        .value(EventValueType::Text(
                            EventTextConfiguration::builder().build().unwrap(),
                        ))
                        .local_state(
                            LocalState::builder()
                                .id("ytl_chat_message")
                                .name("YouTube Live - chat message")
                                .build()
                                .unwrap(),
                        )
                        .local_state(
                            LocalState::builder()
                                .id("ytl_chat_author")
                                .name("YouTube Live - chat author")
                                .build()
                                .unwrap(),
                        )
                        .local_state(
                            LocalState::builder()
                                .id("ytl_chat_author_id")
                                .name("YouTube Live - chat author ID")
                                .build()
                                .unwrap(),
                        )
                        .local_state(
                            LocalState::builder()
                                .id("ytl_chat_timestamp")
                                .name("YouTube Live - chat timestamp")
                                .build()
                                .unwrap(),
                        )
                        .build()
                        .unwrap(),
                )
                .event(
                    Event::builder()
                        .id("ytl_new_super_chat")
                        .name("On new Super Chat")
                        .format("When you receive a Super Chat")
                        .sub_category_id("ytl_analytics_monitoring")
                        .value(EventValueType::Text(
                            EventTextConfiguration::builder().build().unwrap(),
                        ))
                        .local_state(
                            LocalState::builder()
                                .id("ytl_super_chat_message")
                                .name("YouTube Live - Super Chat message")
                                .build()
                                .unwrap(),
                        )
                        .local_state(
                            LocalState::builder()
                                .id("ytl_super_chat_author")
                                .name("YouTube Live - Super Chat author")
                                .build()
                                .unwrap(),
                        )
                        .local_state(
                            LocalState::builder()
                                .id("ytl_super_chat_amount")
                                .name("YouTube Live - Super Chat amount")
                                .build()
                                .unwrap(),
                        )
                        .local_state(
                            LocalState::builder()
                                .id("ytl_super_chat_amount_micros")
                                .name("YouTube Live - Super Chat amount (in micros)")
                                .build()
                                .unwrap(),
                        )
                        .local_state(
                            LocalState::builder()
                                .id("ytl_super_chat_currency")
                                .name("YouTube Live - Super Chat currency")
                                .build()
                                .unwrap(),
                        )
                        .build()
                        .unwrap(),
                )
                .event(
                    Event::builder()
                        .id("ytl_new_member")
                        .name("On new member")
                        .format("When your channel gets a new member")
                        .sub_category_id("ytl_analytics_monitoring")
                        .value(EventValueType::Text(
                            EventTextConfiguration::builder().build().unwrap(),
                        ))
                        .local_state(
                            LocalState::builder()
                                .id("ytl_member_name")
                                .name("YouTube Live - member name")
                                .build()
                                .unwrap(),
                        )
                        .local_state(
                            LocalState::builder()
                                .id("ytl_member_level")
                                .name("YouTube Live - membership level")
                                .build()
                                .unwrap(),
                        )
                        .build()
                        .unwrap(),
                )
                .event(
                    Event::builder()
                        .id("ytl_new_member_milestone")
                        .name("On new member milestone")
                        .format("When an existing member reaches a new milestone")
                        .sub_category_id("ytl_analytics_monitoring")
                        .value(EventValueType::Text(
                            EventTextConfiguration::builder().build().unwrap(),
                        ))
                        .local_state(
                            LocalState::builder()
                                .id("ytl_member_milestone_name")
                                .name("YouTube Live - member milestone name")
                                .build()
                                .unwrap(),
                        )
                        .local_state(
                            LocalState::builder()
                                .id("ytl_member_milestone_level")
                                .name("YouTube Live - member milestone level")
                                .build()
                                .unwrap(),
                        )
                        .local_state(
                            LocalState::builder()
                                .id("ytl_member_milestone_months")
                                .name("YouTube Live - member milestone months")
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
                //
                .state(
                    State::builder()
                        .id("ytl_chat_count")
                        .description("YouTube Live - chat messages")
                        .initial("-")
                        .parent_group("Stream Metrics")
                        .kind(StateType::Text(TextState::builder().build().unwrap()))
                        .build()
                        .unwrap(),
                )
                .state(
                    State::builder()
                        .id("ytl_likes_count")
                        .description("YouTube Live - likes")
                        .initial("-")
                        .parent_group("Stream Metrics")
                        .kind(StateType::Text(TextState::builder().build().unwrap()))
                        .build()
                        .unwrap(),
                )
                .state(
                    State::builder()
                        .id("ytl_dislikes_count")
                        .description("YouTube Live - dislikes")
                        .initial("-")
                        .parent_group("Stream Metrics")
                        .kind(StateType::Text(TextState::builder().build().unwrap()))
                        .build()
                        .unwrap(),
                )
                .state(
                    State::builder()
                        .id("ytl_views_count")
                        .description("YouTube Live - views")
                        .initial("-")
                        .parent_group("Stream Metrics")
                        .kind(StateType::Text(TextState::builder().build().unwrap()))
                        .build()
                        .unwrap(),
                )
                .state(
                    State::builder()
                        .id("ytl_live_viewers_count")
                        .description("YouTube Live - live viewers")
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
                        .description("YouTube Live - stream title")
                        .initial("No stream selected...")
                        .parent_group("Stream Info")
                        .kind(StateType::Text(TextState::builder().build().unwrap()))
                        .build()
                        .unwrap(),
                )
                .state(
                    State::builder()
                        .id("ytl_selected_channel_name")
                        .description("YouTube Live - channel name")
                        .initial("-")
                        .parent_group("Stream Info")
                        .kind(StateType::Text(TextState::builder().build().unwrap()))
                        .build()
                        .unwrap(),
                )
                // System Status States - plugin operation information
                .state(
                    State::builder()
                        .id("ytl_adaptive_polling_status")
                        .description("YouTube Live - adaptive polling status")
                        .initial("Disabled")
                        .parent_group("System Status")
                        .kind(StateType::Text(TextState::builder().build().unwrap()))
                        .build()
                        .unwrap(),
                )
                // Event Value States - these states hold the triggering values for events
                .state(
                    State::builder()
                        .id("ytl_last_chat_message")
                        .description("YouTube Live - last chat message")
                        .initial("-")
                        .parent_group("Chat Events")
                        .kind(StateType::Text(TextState::builder().build().unwrap()))
                        .build()
                        .unwrap(),
                )
                .state(
                    State::builder()
                        .id("ytl_last_super_chat")
                        .description("YouTube Live - last Super Chat")
                        .initial("-")
                        .parent_group("Chat Events")
                        .kind(StateType::Text(TextState::builder().build().unwrap()))
                        .build()
                        .unwrap(),
                )
                .state(
                    State::builder()
                        .id("ytl_last_member")
                        .description("YouTube Live - last member")
                        .initial("-")
                        .parent_group("Chat Events")
                        .kind(StateType::Text(TextState::builder().build().unwrap()))
                        .build()
                        .unwrap(),
                )
                .state(
                    State::builder()
                        .id("ytl_last_chat_author")
                        .description("YouTube Live - last chat author")
                        .initial("-")
                        .parent_group("Chat Events")
                        .kind(StateType::Text(TextState::builder().build().unwrap()))
                        .build()
                        .unwrap(),
                )
                .state(
                    State::builder()
                        .id("ytl_last_super_chat_author")
                        .description("YouTube Live - last Super Chat author")
                        .initial("-")
                        .parent_group("Chat Events")
                        .kind(StateType::Text(TextState::builder().build().unwrap()))
                        .build()
                        .unwrap(),
                )
                .state(
                    State::builder()
                        .id("ytl_last_super_chat_amount")
                        .description("YouTube Live - last Super Chat amount")
                        .initial("-")
                        .parent_group("Chat Events")
                        .kind(StateType::Text(TextState::builder().build().unwrap()))
                        .build()
                        .unwrap(),
                )
                .state(
                    State::builder()
                        .id("ytl_last_member_tenure")
                        .description("YouTube Live - last member tenure")
                        .initial("-")
                        .parent_group("Chat Events")
                        .kind(StateType::Text(TextState::builder().build().unwrap()))
                        .build()
                        .unwrap(),
                )
                .state(
                    State::builder()
                        .id("ytl_last_member_level")
                        .description("YouTube Live - last member level")
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

    // Generate constants for use in src/
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let constants_content = format!(
        "/// Minimum polling interval to avoid YouTube API quota exhaustion\n\
         pub const MIN_POLLING_INTERVAL_SECONDS: u64 = {};\n",
        MIN_POLLING_INTERVAL_SECONDS
    );
    std::fs::write(out_dir.join("constants.rs"), constants_content).unwrap();
}
