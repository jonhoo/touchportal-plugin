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
    async fn on_simple_action(
        &mut self,
        mode: ActionInteractionMode,
        text_input: String,
    ) -> eyre::Result<()> {
        tracing::info!("Processing text: {} with mode: {:?}", text_input, mode);

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
                    .value(&count.to_string())
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
                    .value(&format!("Counter: {}", count))
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

    Plugin::run_dynamic("127.0.0.1:12136").await
}
