# Test plugins

This directory contains test and development TouchPortal plugins. These
are primarily used for testing the SDK itself, serving as examples,
experimenting with new features. They are not intended for actual use.

- **stress/** - Kitchen sink plugin aimed at stress-testing the SDK with
  various features and edge cases

## Using the test plugins

Each test plugin can be built independently by navigating to its
directory and running:

```bash
RUST_LOG=trace cargo test
```

You can also package and install the plugin for manual testing with
TouchPortal using:

```bash
python3 ../scripts/package.py
python3 ../scripts/install.py  # Only if you want to install to TouchPortal
```
