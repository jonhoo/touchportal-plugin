#![allow(dead_code)]

use touchportal_sdk::protocol::{ActionInteractionMode, InfoMessage};

include!(concat!(env!("OUT_DIR"), "/entry.rs"));

#[derive(Debug)]
struct Plugin(TouchPortalHandle);

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

        // Update the long text state with the processed input
        let result = format!(
            "Processed boundary input: text_len={}, number={}",
            max_text.len(),
            boundary_number
        );

        self.0
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
}

impl Plugin {
    async fn new(
        _settings: PluginSettings,
        outgoing: TouchPortalHandle,
        info: InfoMessage,
    ) -> eyre::Result<Self> {
        tracing::info!(version = info.tp_version_string, "paired with TouchPortal");
        tracing::info!("Boundary values plugin - testing edge cases and validation");
        Ok(Self(outgoing))
    }
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .without_time()
        .with_ansi(false)
        .init();

    Plugin::run_dynamic("127.0.0.1:12136").await
}
