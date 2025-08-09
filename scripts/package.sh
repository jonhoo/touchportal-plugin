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
# Use exact package ID matching to avoid conflicts with local dependencies
# (e.g., SDK build scripts, patch.crates-io sections)
current_package_id=$(cargo pkgid)
out_dir=$(echo "$build_json" | jq -r --arg id "$current_package_id" 'select(.reason == "build-script-executed") | select(.package_id == $id).out_dir')

if [[ -z "$out_dir" || "$out_dir" == "null" ]]; then
    echo "ERROR: Failed to find build script output directory for package $current_package_id" >&2
    echo "       This usually means the package has no build.rs or the build failed" >&2
    exit 1
fi

out_dir=$(dirname "$out_dir")
entry_tp="$out_dir/out/entry.tp"

if [[ ! -f "$entry_tp" ]]; then
    echo "ERROR: entry.tp not found at $entry_tp" >&2
    echo "       This usually means the build script failed to generate the TouchPortal plugin definition" >&2
    exit 1
fi

echo "    Built: $plugin_exe"
echo "    Entry: $entry_tp"

# Validate that the plugin_start_cmd in entry.tp matches our build configuration.
# This ensures TouchPortal will be able to find and execute the plugin correctly.
echo "==> Validating plugin_start_cmd consistency"

# Helper function to validate a single plugin start command
validate_plugin_start_cmd() {
    local cmd_name="$1"
    local cmd_value="$2"
    local is_os_specific="$3"  # true for OS-specific commands

    if [[ -z "$cmd_value" || "$cmd_value" == "null" ]]; then
        return 0  # Skip validation for absent optional commands
    fi

    echo "    Validating $cmd_name: $cmd_value"

    # Extract the path portion (before any space-separated arguments)
    local cmd_path
    cmd_path=$(echo "$cmd_value" | cut -d' ' -f1)

    # Validate the directory structure: should start with %TP_PLUGIN_FOLDER% followed by plugin name
    if [[ ! "$cmd_path" =~ ^%TP_PLUGIN_FOLDER%([^/]+)/(.+)$ ]]; then
        echo "ERROR: $cmd_name has invalid format: $cmd_value" >&2
        echo "       Expected format: %TP_PLUGIN_FOLDER%<plugin_name>/<binary_name> [args...]" >&2
        return 1
    fi

    local expected_plugin_dir="${BASH_REMATCH[1]}"
    local expected_binary_name="${BASH_REMATCH[2]}"

    # Validate that the plugin directory matches the metadata plugin_name
    if [[ "$expected_plugin_dir" != "$plugin_name" ]]; then
        echo "ERROR: Plugin directory mismatch in $cmd_name" >&2
        echo "       Expected: $plugin_name" >&2
        echo "       Found: $expected_plugin_dir" >&2
        echo "       This usually means the build.rs hardcoded directory doesn't match Cargo.toml metadata" >&2
        return 1
    fi

    # For OS-specific commands, we allow different exe suffixes since they target different platforms
    # For the main command, we validate against the current platform's built binary
    if [[ "$is_os_specific" == "true" ]]; then
        # For OS-specific commands, just validate that the base binary name matches (ignoring suffixes)
        local expected_base_name="$expected_binary_name"
        local actual_base_name
        actual_base_name=$(basename "$plugin_exe")

        # Strip known extensions for comparison
        expected_base_name="${expected_base_name%.exe}"
        actual_base_name="${actual_base_name%.exe}"

        if [[ "$expected_base_name" != "$actual_base_name" ]]; then
            echo "ERROR: Binary base name mismatch in $cmd_name" >&2
            echo "       Expected: $expected_base_name (ignoring OS suffix)" >&2
            echo "       Built: $actual_base_name (ignoring OS suffix)" >&2
            echo "       This usually means the build.rs hardcoded binary name doesn't match Cargo.toml metadata" >&2
            return 1
        fi
    else
        # For main plugin_start_cmd, validate exact match with current platform binary
        local actual_binary_name
        actual_binary_name=$(basename "$plugin_exe")
        if [[ "$expected_binary_name" != "$actual_binary_name" ]]; then
            echo "ERROR: Binary name mismatch in $cmd_name" >&2
            echo "       Expected: $expected_binary_name" >&2
            echo "       Built: $actual_binary_name" >&2
            echo "       This usually means the build.rs hardcoded binary name doesn't match Cargo.toml metadata" >&2
            return 1
        fi
    fi

    return 0
}

# Extract and validate all plugin start command variants
plugin_start_cmd=$(jq -r '.plugin_start_cmd' "$entry_tp")
plugin_start_cmd_windows=$(jq -r '.plugin_start_cmd_windows // empty' "$entry_tp")
plugin_start_cmd_mac=$(jq -r '.plugin_start_cmd_mac // empty' "$entry_tp")
plugin_start_cmd_linux=$(jq -r '.plugin_start_cmd_linux // empty' "$entry_tp")

# The main plugin_start_cmd is required
if [[ -z "$plugin_start_cmd" || "$plugin_start_cmd" == "null" ]]; then
    echo "ERROR: plugin_start_cmd not found in entry.tp" >&2
    exit 1
fi

# Validate all present plugin start commands
validate_plugin_start_cmd "plugin_start_cmd" "$plugin_start_cmd" "false" || exit 1
validate_plugin_start_cmd "plugin_start_cmd_windows" "$plugin_start_cmd_windows" "true" || exit 1
validate_plugin_start_cmd "plugin_start_cmd_mac" "$plugin_start_cmd_mac" "true" || exit 1
validate_plugin_start_cmd "plugin_start_cmd_linux" "$plugin_start_cmd_linux" "true" || exit 1

echo "    All plugin_start_cmd validations passed"

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
