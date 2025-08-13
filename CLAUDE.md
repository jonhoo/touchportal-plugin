# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build and Development Commands

### Building and testing the SDK

While inside the `sdk/` directory:

```bash
# Run the tests
cargo test
```

#### Snapshot Testing with Insta

The SDK uses [insta](https://docs.rs/insta/) for snapshot testing of JSON serialization. To update snapshots after making changes:

```bash
cd sdk/
cargo insta test       # Run tests and generate new snapshots
```

This will create `.snap.new` files in `snapshots/` for any tests whose snapshot output changed. Review the `.new` files, and if they are correct, remove the `.new` suffix, overwriting the corresponding `.snap` file.

### Building the plugins

Plugins are built and installed using Python scripts in the `scripts/` directory:
- `scripts/package.py` - Builds the plugin and creates a `.tpp` file (safe to run in automated environments)
- `scripts/install.py` - Installs the plugin to `~/.config/TouchPortal/plugins/` (modifies user system, should not be run automatically)

The packaging script includes smart rebuild detection and only rebuilds when source files have changed.

### Production Plugins (plugins/ directory)

Production plugins are in the `plugins/` directory and are intended for real-world use.

#### The YouTube Live Plugin

While inside the `plugins/youtube/` directory:

```bash
# Package the plugin into a .tpp file for TouchPortal (safe for automation)
python3 ../../scripts/package.py

# Install the plugin to TouchPortal (DO NOT run automatically - modifies user system)
python3 ../../scripts/install.py
```

### Feature Tests (feature-tests-workspace/ directory)

Feature test plugins are in the `feature-tests-workspace/` directory and are used for testing the SDK itself, serving as examples, and experimenting with new features. They are not intended for actual use.

#### The Stress Test Plugin

While inside the `feature-tests-workspace/stress/` directory:

```bash
# Package the plugin into a .tpp file for TouchPortal (safe for automation)
python3 ../../scripts/package.py

# Install the plugin to TouchPortal (DO NOT run automatically - modifies user system)
python3 ../../scripts/install.py
```

### Development

```bash
# Build with debug information
cargo build --all

# Check code without building
cargo check --all-targets
```

The TouchPortal plugin binary should only be run by TouchPortal itself, since it requires the TouchPortal host process. The YouTube plugin includes a separate CLI binary for testing and debugging:

```bash
# Run the CLI binary (default) for testing/debugging
RUST_LOG=trace cargo run --release

# Run the TouchPortal plugin binary (should only be run by TouchPortal)
RUST_LOG=trace cargo run --release --bin touchportal-youtube-live
```

## Architecture Overview

This is an SDK that allows writing **TouchPortal plugins** written in Rust that integrates with the TouchPortal automation platform, as well as two plugins using that SDK.
**Only `touchportal-sdk` is published to crates.io, as a library.**

### Project Structure

- **`sdk/`** - TouchPortal plugin framework/SDK
  - Provides types and builders for TouchPortal plugin definitions
  - Code generation for plugin interfaces based on their plugin definitions
  - Protocol handling for TouchPortal communication
- **`plugins/`** - Production TouchPortal plugins for real-world use
  - **`youtube/`** - Plugin to interact with the YouTube Live API
    - `src/bin/touchportal-youtube-live.rs` - TouchPortal plugin binary
    - `src/bin/youtube-live-cli.rs` - CLI tool for testing and debugging
    - `src/lib.rs` - Shared code between binaries
    - `build.rs` - Build-time plugin definition and code generation
    - `Cargo.toml` - Plugin dependencies and metadata
- **`feature-tests-workspace/`** - Feature test plugins for SDK testing
  - **`stress/`** - Kitchen sink plugin aimed at stress-testing the SDK
    - `src/main.rs` - Plugin runtime and business logic
    - `build.rs` - Build-time plugin definition and code generation
    - `Cargo.toml` - Plugin dependencies and metadata

### Key Architecture Components

1. **Plugin Definition** (`build.rs` for each plugin):
   - Defines the plugin structure using a builder pattern:
     - Plugin metadata (name, ID, version)
     - Settings (text, switch, choice, number types)
     - Actions with dynamic implementation
     - Events with state bindings
     - States that track plugin data

2. **Code Generation**: The framework generates:
   - `entry.rs` - Rust code with typed interfaces
   - `entry.tp` - JSON plugin description for TouchPortal
   - You can look at these generated files for a plugin using these commands:
     ```bash
     # To read the generated entry.rs
     cat "$(dirname "$(cargo check --message-format=json | jq -r 'select(.reason == "build-script-executed") | select(.package_id | contains("#touchportal-")).out_dir')")"/out/entry.rs

     # To read the generated entry.tp
     cat "$(dirname "$(cargo check --message-format=json | jq -r 'select(.reason == "build-script-executed") | select(.package_id | contains("#touchportal-")).out_dir')")"/out/entry.tp
     ```

3. **Plugin Runtime** (`main.rs` for each plugin):
   - Implements the generated `PluginCallbacks` trait with action handlers
   - Manages async communication with TouchPortal via TCP (port 12136)
   - Reacts to events, updates states, and triggers events

### Plugin SDK Features

- **Type-safe API**: Generated enums for choices and states
- **Builder patterns**: For constructing plugin definitions since the protocol may gain fields and features over time
- **Async communication**: Using tokio for TouchPortal protocol
- **State management**: Automatic state updates and event triggering
- **Multiple data types**: Text, switch, number, choice inputs
- **Cross-platform**: Windows/Mac/Linux plugin commands supported

### TouchPortal Integration

The plugin communicates with TouchPortal using a JSON-based protocol over TCP. The framework handles:
- Plugin registration and pairing
- Action execution with typed parameters
- State updates that reflect in TouchPortal UI
- Event triggering based on plugin logic
- The TouchPortal API is documented at <https://www.touch-portal.com/api/>.
  To explore it, follow the hrefs in the `<a>` tags on <https://www.touch-portal.com/api/index.php?section=intro>.
- A good implementation of the TouchPortal SDK in Java lives at <https://github.com/ChristopheCVB/TouchPortalPluginSDK>.

## Working with This Codebase

- The plugin definition in `build.rs` determines the TouchPortal interface
- Action handlers in `main.rs` implement the actual functionality
- The framework automatically generates type-safe interfaces
- Testing of the SDK and plugin is manual beyond ensuring that `cargo check` works in the plugin directory.
- Build-time validation is tested via unit tests in the SDK.
- When writing `validate` functions for builders, avoid unnecessary `if let Some` on fields that are not optional; the builder will ensure that they are set to `Some` before `validate` is called, so we can use expect.

## Error Handling & Debugging

### Build Script Debugging

```bash
# If build.rs fails, check the build script output
cargo build --verbose

# Inspect generated files (even if plugin build failed but build script succeeded)
OUT_DIR=$(cargo check --message-format=json | jq -r 'select(.reason == "build-script-executed") | select(.package_id | contains("#touchportal-")).out_dir')
cat "$OUT_DIR"/entry.rs  # Generated Rust code
cat "$OUT_DIR"/entry.tp  # Generated TouchPortal plugin description
```

### Common Build Issues

- Build scripts use `.unwrap()` extensively to produce compile-time errors that point to the exact line in `build.rs` where the issue occurs
- Generated code includes `include!(concat!(env!("OUT_DIR"), "/entry.rs"))` - build failures prevent this inclusion
- If the build script succeeded but plugin compilation failed, the generated files may contain key details to understand the failure

## Logging & Debugging

### Plugin Runtime Debugging

```bash
# Enable comprehensive tracing (already documented)
RUST_LOG=trace cargo run --release

# Plugin uses structured logging with tracing - key log points:
# - TouchPortal connection establishment
# - Plugin pairing info at main.rs:44 (both plugins)
# - Protocol message tracing (send/recv) in generated code
```

### Action Handler Debugging

- All action handlers use `#[tracing::instrument(skip(self), ret)]` to log inputs and return values while avoiding logging `self`
- Error contexts use `eyre::Result<T>` with `.context()` calls for detailed error chains

## Protocol Testing

### Manual Protocol Testing

```bash
# The SDK connects to 127.0.0.1:12136 by default
# You can create a simple TCP server to test protocol communication
# Key protocol messages documented in sdk/src/protocol/incoming.rs and outgoing.rs
```

### Plugin State Inspection

- State updates visible in tracing logs with `RUST_LOG=trace`
- Action handlers are instrumented to show parameter values and return results

## Mock Testing System

- **Action Verification**: Use `mocks.check_action_call("callback_name", json_args)` in action callbacks and `mock_server.expectations().expect_action_call()` in main for explicit verification
- **Mock Injection**: Use `Plugin::run_dynamic_with_setup(addr, |mut plugin| { plugin.mocks = expectations; plugin })` to inject MockExpectations from `main`
- **Automatic Protocol**: MockTouchPortalServer automatically tests for Pair (plugin → mock) and sends ClosePlugin (mock → plugin) at the end
- **Test Runner**: `python3 ./run_feature_tests.py` treats timeouts as failures since plugins should exit gracefully via ClosePlugin

## Code Generation Notes

- **Important Reminder**: Some feature test plugins may fail to compile because of bugs in our code generation rather than errors you make, since bugs in code generation cause compile-time errors.

## Build-Time Validation

The SDK includes comprehensive build-time validation to catch invalid plugin configurations early, with validation rules tested via unit tests in the SDK.

### Key Validation Rules

- **Event-State Consistency**: Events referencing states must have matching data types and choice sets
- **Data Field Consistency**: Same data field IDs must have identical definitions across all actions
- **Choice Validation**: Choice data fields must have initial values that exist in their valid choices
- **Type Safety**: Choice events cannot reference text states and vice versa

### Testing Validation Logic

When making changes to validation logic in the SDK:

1. Run the SDK unit tests: `cd sdk && cargo test`
2. Add new unit tests for any new validation rules

The SDK unit tests help ensure that changes don't accidentally break or weaken build-time error detection.
