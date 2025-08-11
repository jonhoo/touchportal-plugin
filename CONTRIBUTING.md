# Contributing to TouchPortal SDK

This guide provides essential information for contributing to the TouchPortal SDK and plugins.

## Quick Start

### Installing an Existing Plugin

To install and use an existing production plugin:

```bash
cd plugins/youtube/  # or any plugin directory
python3 ../../scripts/package.py      # Creates .tpp file
python3 ../../scripts/install.py      # Installs to TouchPortal
```

### Building and Testing

```bash
# Build the SDK and production plugins
cargo build --all

# Run SDK tests
cd sdk/ && cargo test

# Test SDK features
cd feature-tests-workspace/ && python3 run_feature_tests.py

# Test validation (build-time error detection)
cd validation-failures-workspace/ && python3 test_validation_failures.py
```

## Project Structure

- **`sdk/`** - The TouchPortal plugin framework (published to crates.io)
- **`plugins/`** - Production plugins for real-world use (e.g., YouTube Live)
- **`feature-tests-workspace/`** - Feature test plugins for SDK testing
- **`validation-failures-workspace/`** - Validation test plugins that intentionally fail compilation

## Common Development Tasks

### Modifying the SDK

1. Make changes in the `sdk/` directory
2. Test with feature tests: `cd feature-tests-workspace && python3 run_feature_tests.py`
3. Verify validation works: `cd validation-failures-workspace && python3 test_validation_failures.py`
4. Run SDK tests: `cd sdk && cargo test`

### Adding a Feature Test

Feature tests demonstrate SDK functionality and serve as examples:

1. Create new plugin directory in `feature-tests-workspace/`
2. Add plugin name to `members` list in `feature-tests-workspace/Cargo.toml`
3. Create `build.rs` with plugin definition using SDK builders
4. Create `src/main.rs` with plugin implementation
5. Test with: `python3 run_feature_tests.py your-plugin-name`

### Adding a Validation Test

Validation tests ensure the SDK catches invalid plugin configurations:

1. Create new plugin directory in `validation-failures-workspace/`
2. Add plugin name to `members` list in `validation-failures-workspace/Cargo.toml`
3. Create `build.rs` with intentional validation errors
4. Create `expected-error.txt` with the exact error message expected
5. Create `src/main.rs` (minimal, since build.rs will fail first)
6. Test with: `python3 test_validation_failures.py your-test-name`

## Architecture Overview

### Plugin Definition (build.rs)

Plugins are defined at build-time using builder patterns:

```rust
use touchportal_sdk::{reexport::HexColor, *};

fn main() {
    // as per https://www.touch-portal.com/api/index.php?section=description_file
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
        .category(/* .. */
        .build()
        .unwrap()

    touchportal_sdk::codegen::export(&plugin).unwrap();
}
```

### Plugin Runtime (main.rs)

Plugin runtime implements the auto-generated `PluginCallbacks` trait for
a type named `Plugin`:

```rust
use touchportal_sdk::protocol::{ActionInteractionMode, InfoMessage};
use tracing_subscriber::EnvFilter;

include!(concat!(env!("OUT_DIR"), "/entry.rs"));

#[derive(Debug)]
struct Plugin {
    // Plugin state fields
}

impl PluginCallbacks for Plugin {
    // The set of methods in here will depend in your PluginDescription.
    // Let your IDE or the compiler guide you.
    #[tracing::instrument(skip(self), ret)]
    async fn on_my_action(&mut self, mode: ActionInteractionMode) -> eyre::Result<()> {
        tracing::info!("My action executed with mode: {:?}", mode);
        // Action implementation here
        Ok(())
    }
}

impl Plugin {
    async fn new(
        settings: PluginSettings,
        outgoing: TouchPortalHandle,
        info: InfoMessage,
    ) -> eyre::Result<Self> {
        tracing::info!(version = info.tp_version_string, "paired with TouchPortal");
        Ok(Self {
            // Initialize plugin state
        })
    }
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .without_time() // done by TouchPortal logging
        .with_ansi(false) // not supported by TouchPortal logging
        .init();

    Plugin::run_dynamic("127.0.0.1:12136").await
}
```

**Note**: Feature test plugins typically include mock testing
infrastructure for automated testing. For production plugins intended to
connect to real TouchPortal, use
`Plugin::run_dynamic("127.0.0.1:12136").await` as shown above. See the
`feature-tests-workspace/` plugins for examples of mock testing setup.
You may want to have a separate mock testing binary for your production
plugins.

## Code Style Guidelines

### Rust

- Consistently use `.context()` before `?` for error propagation
- Prefer `expect()` over `unwrap()` with clear messages
- Follow API guidelines: https://rust-lang.github.io/api-guidelines/checklist.html

### Error Handling

```rust
// Good: simple present tense; completes the sentence "while attempting to ..."
let result = some_operation()
    .context("load configuration")?;

// Also good: explains why the expect could never fail
let value = optional_value
    .expect("value must be set by builder validation");
```

## Resources

- [TouchPortal API Documentation](https://www.touch-portal.com/api/)
- [TouchPortal Discord](https://discord.gg/MgxQb8r)
- [SDK Documentation](https://docs.rs/touchportal-sdk) (when published)
- [Java SDK Implementation](https://github.com/ChristopheCVB/TouchPortalPluginSDK) (for reference)
