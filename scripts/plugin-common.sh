#!/bin/bash

# Common functions for TouchPortal plugin packaging and installation
# Source this file from plugin package.sh and install.sh scripts

set -euo pipefail

# Get plugin configuration from cargo metadata
get_plugin_config() {
    local metadata current_package_id current_package
    metadata=$(cargo metadata --format-version=1 --no-deps)

    # Get the current package ID using cargo pkgid
    current_package_id=$(cargo pkgid)

    # Get the current package by matching the ID
    current_package=$(echo "$metadata" | jq --arg id "$current_package_id" '.packages[] | select(.id == $id)')

    # Extract plugin name from metadata
    plugin_name=$(echo "$current_package" | jq -r '.metadata.touchportal.plugin_name // empty')
    if [[ -z "$plugin_name" ]]; then
        log_error "package.metadata.touchportal.plugin_name not found in Cargo.toml"
        exit 1
    fi

    # Extract default-run binary name, fallback to package name
    crate_binary=$(echo "$current_package" | jq -r '.default_run // .name')

    # Derive tpp filename
    tpp_file="$plugin_name.tpp"

    # Export for use by calling scripts
    export plugin_name crate_binary tpp_file
}

# Check if required tools are available
# Usage: check_requirements tool1 tool2 tool3...
check_requirements() {
    local missing_tools=()

    for tool in "$@"; do
        if ! command -v "$tool" &> /dev/null; then
            missing_tools+=("$tool")
        fi
    done

    if [[ ${#missing_tools[@]} -gt 0 ]]; then
        log_error "Missing required tools: ${missing_tools[*]}"
        log_error "Please install the missing tools and try again."
        exit 1
    fi
}

# Logging functions
log_step() {
    echo "==> $1"
}

log_info() {
    echo "    $1"
}

log_error() {
    echo "ERROR: $1" >&2
}



