#![allow(dead_code, unused_variables)]

use touchportal_plugin::protocol::InfoMessage;

include!(concat!(env!("OUT_DIR"), "/entry.rs"));

struct Plugin;

impl PluginMethods for Plugin {
    async fn on_tp_pl_action_002(
        &mut self,
        tp_pl_002_text: String,
        tp_pl_002_switch: bool,
        tp_pl_002_num: f64,
        tp_pl_002_choice: ChoicesFor_tp_pl_002_choice,
    ) -> eyre::Result<()> {
        eprintln!("on_tp_pl_action_002({tp_pl_002_text:?}, {tp_pl_002_switch:?}, {tp_pl_002_num:?}, {tp_pl_002_choice:?}");
        Ok(())
    }
    async fn on_close(&mut self, eof: bool) -> eyre::Result<()> {
        eprintln!("on_close({eof:?})");
        Ok(())
    }
}

impl Plugin {
    async fn new(
        settings: PluginSettings,
        tx: TouchPortalHandle,
        info: InfoMessage,
    ) -> eyre::Result<Self> {
        eprintln!("{settings:?}");
        eprintln!("{info:?}");
        Ok(Self)
    }
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let plugin = Plugin::run_dynamic("127.0.0.1:12136").await?;

    Ok(())
}
