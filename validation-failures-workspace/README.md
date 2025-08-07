# Validation Failures Workspace

This workspace contains TouchPortal plugins that are **expected to fail compilation** due to intentional validation errors in their `build.rs` files. These plugins test that the TouchPortal SDK properly catches invalid configurations at build time.

## Purpose

The SDK includes build-time validation to catch common configuration errors early. This workspace ensures that:

1. Invalid plugin configurations are properly detected
2. Clear, helpful error messages are provided
3. The validation logic doesn't have false positives/negatives

## Structure

Each plugin in this workspace:

- **Contains intentional validation errors** in `build.rs`
- **Has an `expected-error.txt` file** with the exact error message that should be produced
- **Is isolated from the main codebase** so `cargo check --all` in the main workspace doesn't fail

## Running Tests

To test that all validation failures work as expected:

```bash
cd validation-failures-workspace
./test_validation_failures.sh
```

The script will:
1. Attempt to compile each plugin
2. Verify that compilation fails with the expected error message
3. Report which plugins passed/failed the validation test

## Adding New Validation Tests

To add a new validation failure test:

1. Create a new plugin directory
2. Add the plugin to the `members` list in `Cargo.toml`
3. Write a `build.rs` with intentional validation errors
4. Create an `expected-error.txt` file with the exact error message expected
5. Run `./test_validation_failures.sh` to verify it works

## Current Test Cases

Each validation test plugin has a comment at the top of its `build.rs` explaining what validation error it's designed to trigger.

## Notes

- These plugins should **never** compile successfully
- If a plugin starts compiling, it means either:
  - The validation was fixed (good) and the test should be updated
  - The validation was accidentally removed (bad) and needs to be restored
- All plugins use relative paths (`../../../sdk`) to reference the main SDK