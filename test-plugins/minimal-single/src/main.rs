use serde_json;
use touchportal_sdk::protocol::{ActionInteractionMode, InfoMessage};
use tracing_subscriber::EnvFilter;

include!(concat!(env!("OUT_DIR"), "/entry.rs"));

#[derive(Debug)]
struct Plugin {
    mocks: touchportal_sdk::mock::MockExpectations,
}

impl PluginCallbacks for Plugin {
    #[tracing::instrument(skip(self), ret)]
    async fn on_single_action(&mut self, mode: ActionInteractionMode) -> eyre::Result<()> {
        tracing::info!("Single action executed with mode: {:?}", mode);

        // Record this action call for mock verification
        self.mocks
            .check_action_call(
                "on_single_action",
                serde_json::json!({
                    "mode": mode
                }),
            )
            .await;

        Ok(())
    }
}

impl Plugin {
    async fn new(
        _settings: PluginSettings,
        _outgoing: TouchPortalHandle,
        info: InfoMessage,
    ) -> eyre::Result<Self> {
        tracing::info!(version = info.tp_version_string, "paired with TouchPortal");
        Ok(Self {
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

    // Add a simple test scenario to trigger the single action
    // The action callback will log when it is called, which serves as verification
    mock_server.add_test_scenario(
        touchportal_sdk::mock::TestScenario::new("Single Action Test")
            .with_action("single_action", Vec::<(&str, &str)>::new())
            .with_delay(std::time::Duration::from_millis(300)),
    );

    // Start mock server in background
    tokio::spawn(async move {
        if let Err(e) = mock_server.run_test_scenarios().await {
            tracing::error!("Mock server failed: {}", e);
        }
    });

    Plugin::run_dynamic(addr).await
}
