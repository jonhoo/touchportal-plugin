#![allow(dead_code)]

use std::sync::Arc;
use tokio::sync::RwLock;
use touchportal_sdk::protocol::{ActionInteractionMode, InfoMessage};
use tracing_subscriber::EnvFilter;

include!(concat!(env!("OUT_DIR"), "/entry.rs"));

#[derive(Debug)]
struct Plugin {
    handle: TouchPortalHandle,
    counter: Arc<RwLock<i32>>,
}

impl PluginCallbacks for Plugin {
    #[tracing::instrument(skip(self), ret)]
    async fn on_comprehensive_action(
        &mut self,
        mode: ActionInteractionMode,
        text_field: String,
        number_field: f64,
        switch_field: bool,
        choice_field: ChoicesForChoiceField,
    ) -> eyre::Result<()> {
        tracing::info!(
            "Processing comprehensive action with mode: {:?} - text: '{}', number: {}, switch: {}, choice: {:?}",
            mode, text_field, number_field, switch_field, choice_field
        );

        let mut counter_val = self.counter.write().await;
        *counter_val += 1;
        let count = *counter_val;
        drop(counter_val);

        let result_text = format!("Processed: {} items", count);

        self.handle
            .0
            .send(touchportal_sdk::protocol::TouchPortalCommand::StateUpdate(
                touchportal_sdk::protocol::UpdateStateCommand::builder()
                    .state_id("text_state")
                    .value(&result_text)
                    .build()
                    .unwrap(),
            ))
            .await
            .ok();

        self.handle
            .0
            .send(touchportal_sdk::protocol::TouchPortalCommand::StateUpdate(
                touchportal_sdk::protocol::UpdateStateCommand::builder()
                    .state_id("counter_state")
                    .value(count.to_string())
                    .build()
                    .unwrap(),
            ))
            .await
            .ok();

        let color = match choice_field {
            ChoicesForChoiceField::Red => "Red",
            ChoicesForChoiceField::Green => "Green",
            ChoicesForChoiceField::Blue => "Blue",
            ChoicesForChoiceField::Dynamic(_) => "Red", // fallback
        };

        self.handle
            .0
            .send(touchportal_sdk::protocol::TouchPortalCommand::StateUpdate(
                touchportal_sdk::protocol::UpdateStateCommand::builder()
                    .state_id("color_state")
                    .value(color)
                    .build()
                    .unwrap(),
            ))
            .await
            .ok();

        tracing::info!(
            "Updated states: text={}, counter={}, color={}",
            result_text,
            count,
            color
        );

        Ok(())
    }

    #[tracing::instrument(skip(self), ret)]
    async fn on_select_choice_field_in_comprehensive_action(
        &mut self,
        instance: String,
        selected: ChoicesForChoiceField,
    ) -> eyre::Result<()> {
        tracing::info!(
            "Choice field selected in instance {}: {:?}",
            instance,
            selected
        );
        Ok(())
    }
}

impl Plugin {
    async fn new(
        _settings: PluginSettings,
        outgoing: TouchPortalHandle,
        info: InfoMessage,
    ) -> eyre::Result<Self> {
        tracing::info!(version = info.tp_version_string, "paired with TouchPortal");
        tracing::info!("All data types plugin - comprehensive API testing");

        let plugin = Self {
            handle: outgoing,
            counter: Arc::new(RwLock::new(0)),
        };

        // Start background task to cycle color states
        let counter = plugin.counter.clone();
        let handle = plugin.handle.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
            let colors = ["Red", "Green", "Blue"];
            let mut color_idx = 0;

            loop {
                interval.tick().await;

                let color = colors[color_idx % colors.len()];
                color_idx += 1;

                let _ = handle
                    .0
                    .send(touchportal_sdk::protocol::TouchPortalCommand::StateUpdate(
                        touchportal_sdk::protocol::UpdateStateCommand::builder()
                            .state_id("color_state")
                            .value(color)
                            .build()
                            .unwrap(),
                    ))
                    .await;

                let counter_val = counter.read().await;
                let text = format!("Auto-update #{}", *counter_val);
                drop(counter_val);

                let _ = handle
                    .0
                    .send(touchportal_sdk::protocol::TouchPortalCommand::StateUpdate(
                        touchportal_sdk::protocol::UpdateStateCommand::builder()
                            .state_id("text_state")
                            .value(&text)
                            .build()
                            .unwrap(),
                    ))
                    .await;

                tracing::debug!("Cycled color to: {}", color);
            }
        });

        Ok(plugin)
    }
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    // TODO: Add mock TouchPortal server support for all-data-types plugin testing
    // This plugin tests comprehensive action parameters, state updates, and background tasks
    // Priority: Add mock server with test scenarios for:
    // - comprehensive_action with various data types (text, number, switch, choice)
    // - choice field selection callbacks
    // - state updates verification
    // - background task state cycling verification

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .without_time()
        .with_ansi(false)
        .init();

    tracing::info!("all-data-types test plugin - mock support not implemented yet");
    Ok(())
}
