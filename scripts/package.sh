#!/bin/bash
# TouchPortal Plugin Packager
#
# This script packages a TouchPortal plugin by:
# 1. Reading plugin configuration from Cargo.toml metadata
# 2. Checking if a rebuild is needed by comparing file modification times
# 3. Building the plugin binary and extracting the generated entry.tp
# 4. Creating a .tpp package file containing the binary and plugin definition
#
# The script is designed to be efficient by only rebuilding when source files change.

set -euo pipefail

# Show help if requested
if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
    echo "package.sh - Build a TouchPortal plugin into a .tpp package file"
    echo ""
    echo "Creates a .tpp package from the plugin in the current directory."
    echo "Only rebuilds if source files have changed since the last build."
    exit 0
fi

# Source common functions for logging and plugin config
# shellcheck source=plugin-common.sh
source "$(dirname "$0")/plugin-common.sh"

# Variables set by get_plugin_config function
# shellcheck disable=SC2154
declare plugin_name crate_binary tpp_file

# ==============================================================================
# Main Packaging Logic
# ==============================================================================

echo "==> TouchPortal Plugin Packager"

# First, we extract the plugin configuration from Cargo.toml metadata.
# This gives us the plugin name, binary name, and output .tpp filename.
get_plugin_config

# Verify all required tools are available before we start building.
# This prevents partial builds when dependencies are missing.
check_requirements cargo jq zip

echo "    Plugin: $plugin_name"
echo "    Binary: $crate_binary"
echo "    Output: $tpp_file"

# Check if we need to rebuild by comparing source file times to the .tpp file.
# We only rebuild if source files are newer than the existing package.
echo "==> Checking if rebuild is needed"

source_time=$(find . \( -name "*.rs" -o -name "Cargo.toml" -o -name "build.rs" \) -exec stat -c %Y {} \; 2>/dev/null | sort -n | tail -1)
source_time=${source_time:-0}

if [[ -f "$tpp_file" ]]; then
    package_time=$(stat -c %Y "$tpp_file" 2>/dev/null || echo 0)
else
    package_time=0
fi

if [[ $source_time -le $package_time ]]; then
    echo "==> Package up to date: $tpp_file"
    exit 0
fi

# If we reach here, we need to build the plugin.
echo "==> Building plugin binary: $crate_binary"

# Build the plugin and capture JSON output to extract file paths.
# We use --message-format=json to get structured data about the build artifacts.
build_json=$(cargo build --release --bin "$crate_binary" --message-format=json)

# Extract the executable path from the build artifacts.
# The compiler-artifact message contains the path to the built binary.
plugin_exe=$(echo "$build_json" | jq -r "select(.reason == \"compiler-artifact\" and .target.name == \"$crate_binary\").executable")

if [[ -z "$plugin_exe" || "$plugin_exe" == "null" ]]; then
    echo "ERROR: Failed to find executable path for $crate_binary" >&2
    exit 1
fi

# Extract the build script output directory to find entry.tp.
# The build script generates this file as part of the plugin definition.
# Look for the local package (path+file://) in the build output
out_dir=$(echo "$build_json" | jq -r "select(.reason == \"build-script-executed\") | select(.package_id | startswith(\"path+file://\")).out_dir")
out_dir=$(dirname "$out_dir")
entry_tp="$out_dir/out/entry.tp"

if [[ ! -f "$entry_tp" ]]; then
    echo "ERROR: entry.tp not found at $entry_tp" >&2
    exit 1
fi

echo "    Built: $plugin_exe"
echo "    Entry: $entry_tp"

# Now we create the .tpp package file.
# A .tpp file is simply a ZIP archive containing the plugin directory.
echo "==> Creating .tpp package: $tpp_file"

# Create a temporary directory for staging the plugin files.
temp_dir=$(mktemp -d)
plugin_dir="$temp_dir/$plugin_name"
mkdir -p "$plugin_dir"

# Copy the essential files: the binary and the plugin definition.
cp "$plugin_exe" "$entry_tp" "$plugin_dir/"

# Create the ZIP package and move it to the current directory.
current_dir=$(pwd)
(
    cd "$temp_dir"
    zip -r "$plugin_name.tpp" "$plugin_name" >/dev/null
    mv "$plugin_name.tpp" "$current_dir/$tpp_file"
)

# Clean up the temporary directory.
rm -rf "$temp_dir"

echo "    Created: $tpp_file"
echo "==> Packaging complete"
