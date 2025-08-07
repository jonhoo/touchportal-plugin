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

    Plugin::run_dynamic("127.0.0.1:12136").await
}
