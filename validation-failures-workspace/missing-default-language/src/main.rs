// This plugin is expected to fail at build time due to validation errors

include!(concat!(env!("OUT_DIR"), "/entry.rs"));

fn main() {
    println!("This should never run");
}
