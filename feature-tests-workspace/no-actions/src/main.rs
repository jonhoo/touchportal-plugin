use std::sync::Arc;
use tokio::sync::RwLock;
use touchportal_sdk::protocol::InfoMessage;
use tracing_subscriber::EnvFilter;

include!(concat!(env!("OUT_DIR"), "/entry.rs"));

#[derive(Debug)]
struct Plugin {
    handle: TouchPortalHandle,
    counter: Arc<RwLock<i32>>,
}

impl PluginCallbacks for Plugin {
    type SelfTriggered = ();

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
    ) -> eyre::Result<Self> {
        tracing::info!(version = info.tp_version_string, "paired with TouchPortal");
        tracing::info!("No-actions plugin - events and states only");

        let plugin = Self {
            handle: outgoing,
            counter: Arc::new(RwLock::new(0)),
        };

        // Start background task to update states
        let counter = plugin.counter.clone();
        let handle = plugin.handle.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
            loop {
                interval.tick().await;

                let mut counter_val = counter.write().await;
                *counter_val += 1;
                let count = *counter_val;
                drop(counter_val);

                let _ = handle
                    .0
                    .send(touchportal_sdk::protocol::TouchPortalCommand::StateUpdate(
                        touchportal_sdk::protocol::UpdateStateCommand::builder()
                            .state_id("counter_state")
                            .value(count.to_string())
                            .build()
                            .unwrap(),
                    ))
                    .await;

                let status = if count % 10 == 0 {
                    "Error"
                } else if count % 3 == 0 {
                    "Active"
                } else {
                    "Inactive"
                };

                let _ = handle
                    .0
                    .send(touchportal_sdk::protocol::TouchPortalCommand::StateUpdate(
                        touchportal_sdk::protocol::UpdateStateCommand::builder()
                            .state_id("status_state")
                            .value(status)
                            .build()
                            .unwrap(),
                    ))
                    .await;

                tracing::debug!("Updated states: counter={}, status={}", count, status);
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

    // Add test scenario to verify state updates from background task
    mock_server.add_test_scenario(
        touchportal_sdk::mock::TestScenario::new("Background State Updates Test")
            .with_delay(std::time::Duration::from_secs(3)) // Wait for several state updates
            .with_assertions(|commands, _actions| {
                use touchportal_sdk::protocol::TouchPortalCommand;

                let state_updates = commands
                    .iter()
                    .filter(|cmd| matches!(cmd, TouchPortalCommand::StateUpdate(_)))
                    .count();

                if state_updates >= 2 {
                    tracing::info!(
                        "✅ Found {} state updates from background task",
                        state_updates
                    );
                    Ok(())
                } else {
                    eyre::bail!("Expected at least 2 state updates, got {}", state_updates)
                }
            }),
    );

    // Start mock server in background
    tokio::spawn(async move {
        if let Err(e) = mock_server.run_test_scenarios().await {
            tracing::error!("Mock server failed: {}", e);
        }
    });

    Plugin::run_dynamic_with(addr, async move |settings, outgoing, info, _self_trigger| {
        Plugin::new(settings, outgoing, info).await
    })
    .await
}
