#!/bin/bash
# TouchPortal Plugin Installer
#
# This script installs a TouchPortal plugin by:
# 1. First ensuring the plugin is packaged (delegating to package.sh)
# 2. Extracting the .tpp package file to a temporary location
# 3. Syncing the plugin files to TouchPortal's plugin directory
# 4. Cleaning up temporary files
#
# The script modifies the user's system by installing files to the TouchPortal
# plugin directory. Use package.sh instead if you only want to create the .tpp file.

set -euo pipefail

# Show help if requested
if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
    echo "install.sh - Install a TouchPortal plugin to your system"
    echo ""
    echo "Packages the plugin (if needed) and installs it to TouchPortal's plugin directory."
    echo "Modifies your system by copying files to ~/.config/TouchPortal/plugins/"
    exit 0
fi

# Source common functions for logging, configuration, and dependency checking
# shellcheck source=plugin-common.sh
source "$(dirname "$0")/plugin-common.sh"

# Variables set by get_plugin_config function
# shellcheck disable=SC2154
declare plugin_name tpp_file

# ==============================================================================
# Main Installation Logic
# ==============================================================================

echo "==> TouchPortal Plugin Installer"

# First, we extract the plugin configuration from Cargo.toml metadata.
# This gives us the plugin name and expected .tpp filename.
get_plugin_config

# Verify all required tools are available before we start the installation.
# This includes unzip for extracting the .tpp file and rsync for reliable file copying.
check_requirements unzip rsync

echo "    Plugin: $plugin_name"
echo "    Package: $tpp_file"

# Ensure the plugin is packaged first by delegating to package.sh.
# This handles rebuild checking automatically and only rebuilds if needed.
echo "==> Ensuring plugin is packaged"
"$(dirname "$0")/package.sh"

# Now we proceed with the actual installation to TouchPortal's plugin directory.
# TouchPortal expects plugins to be in ~/.config/TouchPortal/plugins/<plugin-name>/
install_dir="$HOME/.config/TouchPortal/plugins/$plugin_name"

echo "==> Installing plugin files"
echo "    Destination: $install_dir"

# Create a temporary directory to extract the .tpp file.
# We extract first to avoid partial installations if the .tpp is corrupted.
temp_dir=$(mktemp -d)
here=$(pwd)

# Extract the .tpp file (which is a ZIP archive) to the temporary directory.
# We change to the temp directory to avoid path conflicts during extraction.
(
    cd "$temp_dir"
    unzip -q "$here/$tpp_file"

    # Find the plugin directory inside the extracted archive.
    # There should be exactly one directory containing the plugin files.
    extracted_dir=$(find . -maxdepth 1 -type d -not -name "." | head -1)

    if [[ -z "$extracted_dir" ]]; then
        echo "ERROR: No plugin directory found in $tpp_file" >&2
        exit 1
    fi

    # Create the target directory if it doesn't exist.
    # This ensures we have a clean installation location.
    mkdir -p "$install_dir"

    # Use rsync to reliably copy all plugin files to the installation directory.
    # The -a flag preserves permissions and handles directory structures correctly.
    # The trailing slashes ensure we copy contents, not the directory itself.
    rsync -a "$extracted_dir/" "$install_dir/"
)

# Clean up the temporary directory to avoid leaving artifacts on the system.
rm -rf "$temp_dir"

echo "    Installed: $(find "$install_dir" -type f | wc -l) files"
echo "==> Installation complete"
echo ""
echo "The plugin has been installed to TouchPortal."
echo "Restart TouchPortal to load the new plugin."
