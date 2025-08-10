use serde_json;
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
    async fn on_simple_action(
        &mut self,
        mode: ActionInteractionMode,
        text_input: String,
    ) -> eyre::Result<()> {
        tracing::info!("Processing text: {} with mode: {:?}", text_input, mode);

        // Record this action call for mock verification
        self.mocks
            .check_action_call(
                "on_simple_action",
                serde_json::json!({
                    "mode": mode,
                    "text_input": text_input
                }),
            )
            .await?;

        let result = format!("Processed: {}", text_input);

        self.handle
            .0
            .send(touchportal_sdk::protocol::TouchPortalCommand::StateUpdate(
                touchportal_sdk::protocol::UpdateStateCommand::builder()
                    .state_id("result_state")
                    .value(&result)
                    .build()
                    .unwrap(),
            ))
            .await
            .ok();

        Ok(())
    }

    #[tracing::instrument(skip(self), ret)]
    async fn on_counter_action(&mut self, mode: ActionInteractionMode) -> eyre::Result<()> {
        // Record this action call for mock verification
        self.mocks
            .check_action_call(
                "on_counter_action",
                serde_json::json!({
                    "mode": mode
                }),
            )
            .await?;

        let mut counter_val = self.counter.write().await;
        *counter_val += 1;
        let count = *counter_val;
        drop(counter_val);

        tracing::info!("Counter incremented to: {} with mode: {:?}", count, mode);

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

        self.handle
            .0
            .send(touchportal_sdk::protocol::TouchPortalCommand::StateUpdate(
                touchportal_sdk::protocol::UpdateStateCommand::builder()
                    .state_id("result_state")
                    .value(format!("Counter: {}", count))
                    .build()
                    .unwrap(),
            ))
            .await
            .ok();

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
        Ok(Self {
            handle: outgoing,
            counter: Arc::new(RwLock::new(0)),
            mocks: touchportal_sdk::mock::MockExpectations::new(),
        })
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

    // Set up expectations for simple action
    mock_server
        .expectations()
        .expect_action_call(
            "on_simple_action",
            serde_json::json!({
                "mode": "Execute",
                "text_input": "Hello World"
            }),
        )
        .await;

    mock_server
        .expectations()
        .expect_action_call(
            "on_simple_action",
            serde_json::json!({
                "mode": "Execute",
                "text_input": "Test input"
            }),
        )
        .await;

    // Set up expectations for counter action
    mock_server
        .expectations()
        .expect_action_call(
            "on_counter_action",
            serde_json::json!({
                "mode": "Execute"
            }),
        )
        .await;

    mock_server
        .expectations()
        .expect_action_call(
            "on_counter_action",
            serde_json::json!({
                "mode": "Execute"
            }),
        )
        .await;

    let expectations = mock_server.expectations().clone();

    // Add test scenarios for actions and state updates
    mock_server.add_test_scenario(
        touchportal_sdk::mock::TestScenario::new("Simple Action Test 1")
            .with_action("simple_action", vec![("text_input", "Hello World")])
            .with_delay(std::time::Duration::from_millis(500)),
    );

    mock_server.add_test_scenario(
        touchportal_sdk::mock::TestScenario::new("Counter Action Test 1")
            .with_action("counter_action", Vec::<(&str, &str)>::new())
            .with_delay(std::time::Duration::from_millis(500)),
    );

    mock_server.add_test_scenario(
        touchportal_sdk::mock::TestScenario::new("Simple Action Test 2")
            .with_action("simple_action", vec![("text_input", "Test input")])
            .with_delay(std::time::Duration::from_millis(500)),
    );

    mock_server.add_test_scenario(
        touchportal_sdk::mock::TestScenario::new("Counter Action Test 2")
            .with_action("counter_action", Vec::<(&str, &str)>::new())
            .with_delay(std::time::Duration::from_millis(500))
            .with_assertions(|commands, _actions| {
                use touchportal_sdk::protocol::TouchPortalCommand;

                let state_updates = commands
                    .iter()
                    .filter(|cmd| matches!(cmd, TouchPortalCommand::StateUpdate(_)))
                    .count();

                if state_updates >= 4 {
                    tracing::info!(
                        "✅ Found {} state updates from action executions",
                        state_updates
                    );
                    Ok(())
                } else {
                    eyre::bail!("Expected at least 4 state updates, got {}", state_updates)
                }
            }),
    );

    // Start mock server in background
    tokio::spawn(async move {
        if let Err(e) = mock_server.run_test_scenarios().await {
            tracing::error!("Mock server failed: {}", e);
        } else {
            tracing::info!("✅ Test PASSED: No-events plugin completed test scenarios");
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
