#![allow(dead_code, unused_variables)]

use std::time::Duration;
use touchportal_plugin::protocol::{ActionInteractionMode, InfoMessage};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;

include!(concat!(env!("OUT_DIR"), "/entry.rs"));

#[derive(Debug)]
struct Plugin;

impl PluginMethods for Plugin {
    #[tracing::instrument(skip(self), ret)]
    async fn on_tp_pl_action_002(
        &mut self,
        mode: ActionInteractionMode,
        tp_pl_002_text: String,
        tp_pl_002_switch: bool,
        tp_pl_002_num: f64,
        tp_pl_002_choice: ChoicesFor_tp_pl_002_choice,
    ) -> eyre::Result<()> {
        Ok(())
    }
}

impl Plugin {
    async fn new(
        settings: PluginSettings,
        mut outgoing: TouchPortalHandle,
        info: InfoMessage,
    ) -> eyre::Result<Self> {
        tracing::info!(version = info.tp_version_string, "paired with TouchPortal");
        tracing::debug!(settings = ?settings, "got settings");

        tokio::spawn(async move {
            for i in 0.. {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let value = match i % 4 {
                    0 => ValuesForState_tp_sid_fruit::Apple,
                    1 => ValuesForState_tp_sid_fruit::Pears,
                    2 => ValuesForState_tp_sid_fruit::Bananas,
                    3 => ValuesForState_tp_sid_fruit::Grapes,
                    _ => unreachable!(),
                };
                outgoing.update_tp_sid_fruit(value).await;
                outgoing.update_tp_sid_count(format!("{i}")).await;
                // outgoing.trigger_event002().await;
                // outgoing.trigger_ev_counter().await;
                if i > 20 {
                    outgoing.trigger_yoc().await;
                }
            }
        });

        Ok(Self)
    }
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::TRACE.into())
                .from_env_lossy(),
        )
        .without_time() // done by TouchPortal's logs
        .with_ansi(false)
        .init();

    let plugin = Plugin::run_dynamic("127.0.0.1:12136").await?;

    Ok(())
}
