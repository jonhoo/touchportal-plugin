#![allow(dead_code, unused_variables)]

use std::time::Duration;
use touchportal_sdk::protocol::{ActionInteractionMode, InfoMessage};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;

include!(concat!(env!("OUT_DIR"), "/entry.rs"));

#[derive(Debug)]
struct Plugin(TouchPortalHandle);

impl PluginCallbacks for Plugin {
    #[tracing::instrument(skip(self), ret)]
    async fn on_tp_pl_action_002(
        &mut self,
        mode: ActionInteractionMode,
        tp_pl_002_text: String,
        tp_pl_002_switch: bool,
        tp_pl_002_num: f64,
        tp_pl_002_choice: ChoicesForTpPl002Choice,
        tp_pl_002_dependent: ChoicesForTpPl002Dependent,
    ) -> eyre::Result<()> {
        Ok(())
    }

    #[tracing::instrument(skip(self), ret)]
    async fn on_select_tp_pl_002_choice_in_tp_pl_action_002(
        &mut self,
        instance: String,
        selected: ChoicesForTpPl002Choice,
    ) -> eyre::Result<()> {
        match selected {
            ChoicesForTpPl002Choice::X => {
                self.0
                    .update_choices_in_specific_tp_pl_002_dependent(instance, vec!["X1", "X2"])
                    .await
            }
            ChoicesForTpPl002Choice::Y => {
                self.0
                    .update_choices_in_specific_tp_pl_002_dependent(instance, vec!["Y1", "Y2"])
                    .await
            }
            ChoicesForTpPl002Choice::Dynamic(_) => unreachable!(),
        }
        Ok(())
    }

    #[tracing::instrument(skip(self), ret)]
    async fn on_select_tp_pl_002_dependent_in_tp_pl_action_002(
        &mut self,
        instance: String,
        selected: ChoicesForTpPl002Dependent,
    ) -> eyre::Result<()> {
        Ok(())
    }

    #[tracing::instrument(skip(self), ret)]
    async fn on_secondary(
        &mut self,
        mode: protocol::ActionInteractionMode,
        tp_pl_002_choice: ChoicesForTpPl002Choice,
    ) -> eyre::Result<()> {
        Ok(())
    }

    #[tracing::instrument(skip(self), ret)]
    async fn on_select_tp_pl_002_choice_in_secondary(
        &mut self,
        instance: String,
        selected: ChoicesForTpPl002Choice,
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

        let handle = outgoing.clone();
        tokio::spawn(async move {
            for i in 0.. {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let value = match i % 4 {
                    0 => ValuesForStateTpSidFruit::Apple,
                    1 => ValuesForStateTpSidFruit::Pears,
                    2 => ValuesForStateTpSidFruit::Bananas,
                    3 => ValuesForStateTpSidFruit::Grapes,
                    _ => unreachable!(),
                };
                outgoing.update_tp_sid_fruit(value).await;
                outgoing.update_tp_sid_count(format!("{i}")).await;
                if false {
                    outgoing.force_trigger_event_002().await;
                    outgoing.force_trigger_ev_counter().await;
                }
                if i > 20 {
                    outgoing.trigger_yoc("as_first", "On").await;
                }
            }
        });

        Ok(Self(handle))
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

    Plugin::run_dynamic("127.0.0.1:12136").await
}
