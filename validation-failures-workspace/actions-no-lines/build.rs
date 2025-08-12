mod plugin;

fn main() {
    let plugin = plugin::plugin();
    touchportal_sdk::codegen::export(&plugin);
}