# TouchPortal Rust SDK & Plugins

[![Crates.io](https://img.shields.io/crates/v/touchportal-sdk.svg)](https://crates.io/crates/touchportal-sdk)
[![Documentation](https://docs.rs/touchportal-sdk/badge.svg)](https://docs.rs/touchportal-sdk/)
[![codecov](https://codecov.io/gh/jonhoo/touchportal-plugin/graph/badge.svg?token=5F2CLHGAEM)](https://codecov.io/gh/jonhoo/touchportal-plugin)
[![Dependency status](https://deps.rs/repo/github/jonhoo/touchportal-plugin/status.svg)](https://deps.rs/repo/github/jonhoo/touchportal-plugin)

[TouchPortal](https://www.touch-portal.com/) is a macro deck software
that allows you to control your Windows, macOS, or Linux computer from
your phone, tablet, or second screen. Plugins extend TouchPortal's
capabilities by adding custom actions, states, and events.

This repository contains:
- **TouchPortal SDK for Rust** - A library for writing TouchPortal plugins in Rust
- **Production plugins** - Real-world plugins built with the SDK

## Production Plugins

To install any of these:

1. Download the latest `.tpp` file from the [releases page](https://github.com/jonhoo/touchportal-plugin/releases)
2. Extract the `.tpp` file (it's a zip archive) to your TouchPortal plugins directory:
   - **Windows**: `%APPDATA%\TouchPortal\plugins\`
   - **macOS**: `~/Documents/TouchPortal/plugins/`
   - **Linux**: `~/.config/TouchPortal/plugins/`
3. Restart TouchPortal

Alternatively, build and install from source:

```bash
cd plugins/youtube/
python3 ../../scripts/package.py
python3 ../../scripts/install.py # just unzips the .tpp into the above plugins/ directory.
```

### YouTube Live Plugin

Control YouTube live streams from TouchPortal with support for:
- Starting/stopping live streams
- Updating stream titles and descriptions
- Monitoring stream status and viewer counts
- OAuth2 authentication with YouTube API


## TouchPortal SDK for Rust

The `touchportal-sdk` crate provides a type-safe, async framework for building TouchPortal plugins.
It features:

- **Code generation**: Automatically generates type-safe Rust interfaces from plugin definitions
- **Compile-time validation**: Builder patterns with extensive validation catch configuration errors at build time
- **Type-safe API**: Generated enums and structs eliminate runtime string-based errors
- **State management**: Automatic state updates and event triggering
- **Cross-platform**: Works on Windows, macOS, and Linux

### Documentation

- [API Documentation](https://docs.rs/touchportal-sdk) - Complete SDK reference
- [TouchPortal API](https://www.touch-portal.com/api/) - Official TouchPortal plugin API

### Quick Start Example

First, define your plugin in `build.rs`:

```rust
use touchportal_sdk::*;

fn main() -> eyre::Result<()> {
    // Define your plugin as per
    // https://www.touch-portal.com/api/index.php?section=description_file
    let plugin = PluginDescription::builder()
        .api(ApiVersion::V4_3)
        .version(1)
        .name("Example Plugin")
        .id("com.example.plugin")
        .configuration(
            PluginConfiguration::builder()
                .color_dark(HexColor::from_u24(0x333333))
                .color_light(HexColor::from_u24(0x666666))
                .parent_category(PluginCategory::Misc)
                .build()
                .unwrap(),
        )
        .plugin_start_cmd(format!(
            "%TP_PLUGIN_FOLDER%Example/{}{}",
            std::env::var("CARGO_PKG_NAME").unwrap(),
            std::env::consts::EXE_SUFFIX
        ))
        .category(/* ... */)
        .category(/* ... */)
        .build()?;

    // Export plugin files and generated code
    plugin.export()?;

    Ok(())
}
```

Then implement your plugin logic in `main.rs`:

```rust
// Include the generated plugin interface.
//
// This expects there to be a `Plugin` type that implements the
// auto-generated `PluginCallbacks` trait.
include!(concat!(env!("OUT_DIR"), "/entry.rs"));

struct Plugin;

impl PluginCallbacks for Plugin {
    // PluginCallbacks will hold function signatures for every incoming
    // message you may receive from TouchPortal.
    //
    // You can start with it empty and let the compiler or your IDE
    // guide you.
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    Plugin::run_dynamic("127.0.0.1:12136").await?;
}
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines on:
- Modifying the SDK
- Adding test plugins
- Building and packaging plugins
- Running validation tests

## License

Licensed under either of

 * Apache License, Version 2.0
   ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
 * MIT license
   ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
