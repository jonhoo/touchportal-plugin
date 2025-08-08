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
    // No required methods - all have default implementations for plugins with no actions
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
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
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

    Plugin::run_dynamic("127.0.0.1:12136").await
}
