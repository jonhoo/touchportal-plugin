# Plugins

This directory contains actual, real TouchPortal plugins:

- **youtube/** - Plugin for interacting with the YouTube Live API,
  allowing control of live streams from TouchPortal.

## Packaging and installing plugins

To install a plugin, cd into the plugin's directory and run

```bash
./package.sh
```

This will build the plugin, construct a `.tpp` file in the current
directory in case you want to install it elsewhere, and also install the
plugin to `~/.config/TouchPortal/plugins/`.
