# Plugins

This directory contains actual, real TouchPortal plugins:

- **youtube/** - Plugin for interacting with the YouTube Live API,
  allowing control of live streams from TouchPortal.

## Packaging and installing plugins

To package and install a plugin, cd into the plugin's directory and run:

```bash
# Package the plugin into a .tpp file for TouchPortal (safe for automation)
python3 ../../scripts/package.py

# Install the plugin to TouchPortal (DO NOT run automatically - modifies user system)
python3 ../../scripts/install.py
```

The packaging script includes smart rebuild detection and only rebuilds when source files have changed. The `.tpp` file will be created in the plugin's directory and can be installed elsewhere if needed.
