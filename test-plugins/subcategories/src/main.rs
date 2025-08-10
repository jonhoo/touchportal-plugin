use touchportal_sdk::protocol::{ActionInteractionMode, InfoMessage};

include!(concat!(env!("OUT_DIR"), "/entry.rs"));

#[derive(Debug)]
struct Plugin;

impl PluginCallbacks for Plugin {
    #[tracing::instrument(skip(self), ret)]
    async fn on_media_action(&mut self, mode: ActionInteractionMode) -> eyre::Result<()> {
        tracing::info!("Media action executed with mode: {:?}", mode);
        Ok(())
    }

    #[tracing::instrument(skip(self), ret)]
    async fn on_settings_action(&mut self, mode: ActionInteractionMode) -> eyre::Result<()> {
        tracing::info!("Settings action executed with mode: {:?}", mode);
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
        Ok(Self)
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

    // Add test scenarios for both subcategory actions
    mock_server.add_test_scenario(
        touchportal_sdk::mock::TestScenario::new("Media Action Test")
            .with_action("media_action", Vec::<(&str, &str)>::new())
            .with_delay(std::time::Duration::from_millis(500)),
    );

    mock_server.add_test_scenario(
        touchportal_sdk::mock::TestScenario::new("Settings Action Test")
            .with_action("settings_action", Vec::<(&str, &str)>::new())
            .with_delay(std::time::Duration::from_millis(750)),
    );

    // Start mock server in background
    tokio::spawn(async move {
        if let Err(e) = mock_server.run_test_scenarios().await {
            tracing::error!("Mock server failed: {}", e);
        } else {
            tracing::info!("✅ Test PASSED: Subcategories plugin completed test scenarios");
        }
    });

    Plugin::run_dynamic(addr).await
}
