// for Plugin::run_dynamic
#![allow(dead_code)]

use touchportal_sdk::protocol::{ActionInteractionMode, InfoMessage};

include!(concat!(env!("OUT_DIR"), "/entry.rs"));

#[derive(Debug)]
struct Plugin {
    handle: TouchPortalHandle,
    mocks: touchportal_sdk::mock::MockExpectations,
}

impl PluginCallbacks for Plugin {
    #[tracing::instrument(skip(self), ret)]
    async fn on_boundary_action(
        &mut self,
        mode: ActionInteractionMode,
        max_text: String,
        boundary_number: f64,
    ) -> eyre::Result<()> {
        tracing::info!(
            "Boundary action executed with mode: {:?}, text: '{}', number: {}",
            mode,
            max_text,
            boundary_number
        );

        // Record this action call for mock verification
        self.mocks
            .check_action_call(
                "on_boundary_action",
                serde_json::json!({
                    "mode": mode,
                    "max_text": max_text,
                    "boundary_number": boundary_number
                }),
            )
            .await?;

        // Update the long text state with the processed input
        let result = format!(
            "Processed boundary input: text_len={}, number={}",
            max_text.len(),
            boundary_number
        );

        self.handle
            .0
            .send(touchportal_sdk::protocol::TouchPortalCommand::StateUpdate(
                touchportal_sdk::protocol::UpdateStateCommand::builder()
                    .state_id("long_text_state")
                    .value(&result)
                    .build()
                    .unwrap(),
            ))
            .await
            .ok();

        Ok(())
    }

    #[tracing::instrument(skip(self), ret)]
    async fn on_settings_changed(&mut self, settings: PluginSettings) -> eyre::Result<()> {
        tracing::info!(?settings, "plugin settings changed");
        Ok(())
    }
}

impl Plugin {
    async fn new(
        _settings: PluginSettings,
        outgoing: TouchPortalHandle,
        info: InfoMessage,
        mocks: touchportal_sdk::mock::MockExpectations,
    ) -> eyre::Result<Self> {
        tracing::info!(version = info.tp_version_string, "paired with TouchPortal");
        tracing::info!("Boundary values plugin - testing edge cases and validation");
        Ok(Self {
            handle: outgoing,
            mocks,
        })
    }
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .without_time()
        .with_ansi(false)
        .init();

    // Create mock TouchPortal server for testing
    let mut mock_server = touchportal_sdk::mock::MockTouchPortalServer::new().await?;
    let addr = mock_server.local_addr()?;

    // Set up expectations for boundary action with various edge case values
    let max_text = "X".repeat(1000);
    mock_server
        .expectations()
        .expect_action_call(
            "on_boundary_action",
            serde_json::json!({
                "mode": "Execute",
                "max_text": max_text,
                "boundary_number": 999999.99
            }),
        )
        .await;
    mock_server.add_test_scenario(
        touchportal_sdk::mock::TestScenario::new("Maximum Boundary Values Test")
            .with_action(
                "boundary_action",
                vec![
                    ("max_text", max_text.as_str()),
                    ("boundary_number", "999999.99"),
                ],
            )
            .with_delay(std::time::Duration::from_millis(500)),
    );

    mock_server
        .expectations()
        .expect_action_call(
            "on_boundary_action",
            serde_json::json!({
                "mode": "Execute",
                "max_text": "",
                "boundary_number": -999999.99
            }),
        )
        .await;
    mock_server.add_test_scenario(
        touchportal_sdk::mock::TestScenario::new("Minimum Boundary Values Test")
            .with_action(
                "boundary_action",
                vec![("max_text", ""), ("boundary_number", "-999999.99")],
            )
            .with_delay(std::time::Duration::from_millis(500)),
    );

    mock_server
        .expectations()
        .expect_action_call(
            "on_boundary_action",
            serde_json::json!({
                "mode": "Execute",
                "max_text": "Normal text",
                "boundary_number": 0.0
            }),
        )
        .await;
    mock_server.add_test_scenario(
        touchportal_sdk::mock::TestScenario::new("Normal Boundary Values Test")
            .with_action(
                "boundary_action",
                vec![("max_text", "Normal text"), ("boundary_number", "0.0")],
            )
            .with_delay(std::time::Duration::from_millis(500))
            .with_assertions(|commands, _actions| {
                use touchportal_sdk::protocol::TouchPortalCommand;

                let state_updates = commands
                    .iter()
                    .filter(|cmd| matches!(cmd, TouchPortalCommand::StateUpdate(_)))
                    .count();

                if state_updates >= 3 {
                    tracing::info!(
                        "✅ Found {} state updates from boundary value actions",
                        state_updates
                    );
                    Ok(())
                } else {
                    eyre::bail!("Expected at least 3 state updates, got {}", state_updates)
                }
            }),
    );

    let expectations = mock_server.expectations().clone();

    // Start mock server in background
    tokio::spawn(async move {
        if let Err(e) = mock_server.run_test_scenarios().await {
            tracing::error!("Mock server failed: {}", e);
        } else {
            tracing::info!("✅ Test PASSED: Boundary values plugin completed test scenarios");
        }
    });

    let expectations_for_verification = expectations.clone();
    Plugin::run_dynamic_with(addr, async move |settings, outgoing, info| {
        Plugin::new(settings, outgoing, info, expectations).await
    })
    .await?;

    // Verify mock expectations after plugin completes
    expectations_for_verification.verify().await?;

    Ok(())
}
