use touchportal_sdk::protocol::InfoMessage;
use tracing_subscriber::EnvFilter;

include!(concat!(env!("OUT_DIR"), "/entry.rs"));

#[derive(Debug)]
struct Plugin;

impl PluginCallbacks for Plugin {
    // No required methods - all have default implementations for empty categories
}

impl Plugin {
    async fn new(
        _settings: PluginSettings,
        _outgoing: TouchPortalHandle,
        info: InfoMessage,
    ) -> eyre::Result<Self> {
        tracing::info!(version = info.tp_version_string, "paired with TouchPortal");
        tracing::info!("Empty categories plugin - should not display any categories in UI");
        Ok(Self)
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
