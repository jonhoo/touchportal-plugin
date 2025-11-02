// for Plugin::run_dynamic
#![allow(dead_code)]

use touchportal_sdk::protocol::{ActionInteractionMode, InfoMessage};

include!(concat!(env!("OUT_DIR"), "/entry.rs"));

#[derive(Debug)]
struct Plugin {
    mocks: touchportal_sdk::mock::MockExpectations,
}

impl PluginCallbacks for Plugin {
    type SelfTriggered = ();

    #[tracing::instrument(skip(self), ret)]
    async fn on_media_action(&mut self, mode: ActionInteractionMode) -> eyre::Result<()> {
        tracing::info!("Media action executed with mode: {:?}", mode);

        // Record this action call for mock verification
        self.mocks
            .check_action_call(
                "on_media_action",
                serde_json::json!({
                    "mode": mode
                }),
            )
            .await?;

        Ok(())
    }

    #[tracing::instrument(skip(self), ret)]
    async fn on_settings_action(&mut self, mode: ActionInteractionMode) -> eyre::Result<()> {
        tracing::info!("Settings action executed with mode: {:?}", mode);

        // Record this action call for mock verification
        self.mocks
            .check_action_call(
                "on_settings_action",
                serde_json::json!({
                    "mode": mode
                }),
            )
            .await?;

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
        _outgoing: TouchPortalHandle,
        info: InfoMessage,
        mocks: touchportal_sdk::mock::MockExpectations,
    ) -> eyre::Result<Self> {
        tracing::info!(version = info.tp_version_string, "paired with TouchPortal");

        Ok(Self { mocks })
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

    // Set up expectations for action calls
    mock_server
        .expectations()
        .expect_action_call(
            "on_media_action",
            serde_json::json!({
                "mode": "Execute"
            }),
        )
        .await;
    mock_server.add_test_scenario(
        touchportal_sdk::mock::TestScenario::new("Media Action Test")
            .with_action("media_action", Vec::<(&str, &str)>::new())
            .with_delay(std::time::Duration::from_millis(500)),
    );

    mock_server
        .expectations()
        .expect_action_call(
            "on_settings_action",
            serde_json::json!({
                "mode": "Execute"
            }),
        )
        .await;
    mock_server.add_test_scenario(
        touchportal_sdk::mock::TestScenario::new("Settings Action Test")
            .with_action("settings_action", Vec::<(&str, &str)>::new())
            .with_delay(std::time::Duration::from_millis(500)),
    );

    // Expect both actions to be called again in the comprehensive test
    mock_server
        .expectations()
        .expect_action_call(
            "on_media_action",
            serde_json::json!({
                "mode": "Execute"
            }),
        )
        .await;
    mock_server
        .expectations()
        .expect_action_call(
            "on_settings_action",
            serde_json::json!({
                "mode": "Execute"
            }),
        )
        .await;
    mock_server.add_test_scenario(
        touchportal_sdk::mock::TestScenario::new("Both Actions Test")
            .with_action("media_action", Vec::<(&str, &str)>::new())
            .with_action("settings_action", Vec::<(&str, &str)>::new())
            .with_delay(std::time::Duration::from_millis(300)),
    );

    // Take expectations for injection into plugin
    let expectations = mock_server.take_expectations();

    // Start mock server in background
    tokio::spawn(async move {
        if let Err(e) = mock_server.run_test_scenarios().await {
            tracing::error!("Mock server failed: {}", e);
        } else {
            tracing::info!("✅ Test PASSED: Subcategories plugin completed test scenarios");
        }
    });

    let expectations_for_verification = expectations.clone();
    let result = Plugin::run_dynamic_with(addr, async move |settings, outgoing, info, _self_trigger| {
        Plugin::new(settings, outgoing, info, expectations).await
    })
    .await;

    // Verify mock expectations after plugin completes
    expectations_for_verification.verify().await?;

    result
}
