# Feature Tests

This workspace contains feature test plugins for the TouchPortal SDK. These
plugins are used for testing SDK functionality, serving as examples, and
experimenting with new features. They are not intended for actual use.

Available feature test plugins:
- **stress/** - Kitchen sink plugin that stress-tests various SDK features
- **all-data-types/** - Tests all supported data field types
- **minimal-single/** - Minimal plugin with single action
- **boundary-values/** - Tests edge cases and boundary conditions
- **no-actions/** - Plugin with no actions (events/states only)
- **no-events/** - Plugin with no events (actions/settings only)
- **subcategories/** - Tests action subcategory organization

## Running Feature Tests

Run all feature tests:
```bash
python3 run_feature_tests.py
```

Run specific feature tests:
```bash
python3 run_feature_tests.py stress minimal-single
```

## Manual Testing

Each plugin can be built independently:

```bash
cd stress/  # or any other feature test plugin
RUST_LOG=trace cargo run
```

For TouchPortal integration testing:
```bash
python3 ../../scripts/package.py
python3 ../../scripts/install.py  # Only if you want to install to TouchPortal
```
