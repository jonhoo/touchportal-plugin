// for Plugin::run_dynamic
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
    mocks: touchportal_sdk::mock::MockExpectations,
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

        // Record this action call for mock verification (excluding choice field for now)
        self.mocks
            .check_action_call(
                "on_comprehensive_action",
                serde_json::json!({
                    "mode": mode,
                    "text_field": text_field,
                    "number_field": number_field,
                    "switch_field": switch_field,
                    "choice_field": choice_field,
                }),
            )
            .await?;

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

        // Record this action call for mock verification
        self.mocks
            .check_action_call(
                "on_select_choice_field_in_comprehensive_action",
                serde_json::json!({
                    "instance": instance,
                    "selected": selected,
                }),
            )
            .await?;

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
            mocks: touchportal_sdk::mock::MockExpectations::new(),
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
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .without_time()
        .with_ansi(false)
        .init();

    // Create mock TouchPortal server for testing
    let mut mock_server = touchportal_sdk::mock::MockTouchPortalServer::new().await?;
    let addr = mock_server.local_addr()?;

    // Add test scenarios for comprehensive action with various data types
    mock_server
        .expectations()
        .expect_action_call(
            "on_comprehensive_action",
            serde_json::json!({
                "mode": "Execute",
                "text_field": "test input",
                "number_field": 42.5,
                "switch_field": true,
                "choice_field": "Red",
            }),
        )
        .await;
    mock_server.add_test_scenario(
        touchportal_sdk::mock::TestScenario::new("Comprehensive Action Test 1")
            .with_action(
                "comprehensive_action",
                vec![
                    ("text_field", "test input"),
                    ("number_field", "42.5"),
                    ("switch_field", "On"),
                    ("choice_field", "Red"),
                ],
            )
            .with_delay(std::time::Duration::from_millis(500)),
    );

    mock_server
        .expectations()
        .expect_action_call(
            "on_comprehensive_action",
            serde_json::json!({
                "mode": "Execute",
                "text_field": "another test",
                "number_field": 100.0,
                "switch_field": false,
                "choice_field": "Blue"
            }),
        )
        .await;
    mock_server.add_test_scenario(
        touchportal_sdk::mock::TestScenario::new("Comprehensive Action Test 2")
            .with_action(
                "comprehensive_action",
                vec![
                    ("text_field", "another test"),
                    ("number_field", "100.0"),
                    ("switch_field", "Off"),
                    ("choice_field", "Blue"),
                ],
            )
            .with_delay(std::time::Duration::from_millis(500)),
    );

    // Test choice field selection (listChange event)
    mock_server
        .expectations()
        .expect_action_call(
            "on_select_choice_field_in_comprehensive_action",
            serde_json::json!({
                "instance": "mock-instance-123",
                "selected": "Green",
            }),
        )
        .await;
    mock_server.add_test_scenario(
        touchportal_sdk::mock::TestScenario::new("Choice Field Selection Test")
            .with_select_in_action(
                "comprehensive_action",
                "choice_field",
                "mock-instance-123",
                "Green",
            )
            .with_delay(std::time::Duration::from_millis(500)),
    );

    // Add final test scenario with state update validation
    mock_server.add_test_scenario(
        touchportal_sdk::mock::TestScenario::new("State Updates Validation")
            .with_delay(std::time::Duration::from_millis(1000))
            .with_assertions(|commands, _actions| {
                use touchportal_sdk::protocol::TouchPortalCommand;

                let state_updates = commands
                    .iter()
                    .filter(|cmd| matches!(cmd, TouchPortalCommand::StateUpdate(_)))
                    .count();

                if state_updates >= 4 {
                    tracing::info!(
                        "✅ Found {} state updates from comprehensive actions and background tasks",
                        state_updates
                    );
                    Ok(())
                } else {
                    eyre::bail!("Expected at least 4 state updates, got {}", state_updates)
                }
            }),
    );

    let expectations = mock_server.expectations().clone();

    // Start mock server in background
    tokio::spawn(async move {
        if let Err(e) = mock_server.run_test_scenarios().await {
            tracing::error!("Mock server failed: {}", e);
        } else {
            tracing::info!("✅ Test PASSED: All data types plugin completed test scenarios");
        }
    });

    let expectations_for_verification = expectations.clone();
    Plugin::run_dynamic_with_setup(addr, |mut plugin| {
        plugin.mocks = expectations;
        plugin
    })
    .await?;

    // Verify mock expectations after plugin completes
    expectations_for_verification.verify().await?;

    Ok(())
}
