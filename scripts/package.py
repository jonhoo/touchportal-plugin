#!/usr/bin/env python3
"""
TouchPortal Plugin Packager

This script packages a TouchPortal plugin by:
1. Reading plugin configuration from Cargo.toml metadata
2. Checking if a rebuild is needed by comparing file modification times
3. Building the plugin binary and extracting the generated entry.tp
4. Creating a .tpp package file containing the binary and plugin definition

The script is designed to be efficient by only rebuilding when source files change.
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path
from typing import Dict, List, Optional, Tuple

# Import our common functions
from plugin_common import (
    get_plugin_config,
    check_requirements,
    log_step,
    log_info,
    log_error,
    log_success,
)


def get_newest_source_time() -> float:
    """
    Get the modification time of the newest source file.

    Returns:
        Unix timestamp of the newest source file, or 0 if no files found
    """
    source_patterns = ["*.rs", "Cargo.toml", "build.rs"]
    newest_time = 0

    for pattern in source_patterns:
        for file_path in Path(".").glob(f"**/{pattern}"):
            if file_path.is_file():
                mtime = file_path.stat().st_mtime
                if mtime > newest_time:
                    newest_time = mtime

    return newest_time


def get_file_mtime(file_path: Path) -> float:
    """
    Get modification time of a file, returning 0 if file doesn't exist.

    Args:
        file_path: Path to the file

    Returns:
        Unix timestamp or 0 if file doesn't exist
    """
    try:
        return file_path.stat().st_mtime
    except FileNotFoundError:
        return 0


def build_plugin(crate_binary: str) -> Tuple[str, str]:
    """
    Build the plugin and return paths to the executable and entry.tp.

    Args:
        crate_binary: Name of the binary to build

    Returns:
        Tuple of (plugin_exe_path, entry_tp_path)

    Raises:
        SystemExit: If build fails or artifacts cannot be found
    """
    log_step(f"Building plugin binary: {crate_binary}")

    try:
        # Build the plugin and capture JSON output
        result = subprocess.run(
            ["cargo", "build", "--release", "--bin", crate_binary, "--message-format=json"],
            capture_output=True,
            text=True,
            check=True,
        )

        # Parse the JSON output line by line
        plugin_exe = None
        out_dir = None
        current_package_id = subprocess.run(
            ["cargo", "pkgid"], capture_output=True, text=True, check=True
        ).stdout.strip()

        for line in result.stdout.strip().split('\n'):
            if not line:
                continue
            try:
                message = json.loads(line)

                # Extract executable path from compiler-artifact messages
                if (message.get("reason") == "compiler-artifact" and
                    message.get("target", {}).get("name") == crate_binary):
                    plugin_exe = message.get("executable")

                # Extract build script output directory
                if (message.get("reason") == "build-script-executed" and
                    message.get("package_id") == current_package_id):
                    out_dir = message.get("out_dir")

            except json.JSONDecodeError:
                continue  # Skip non-JSON lines

        if not plugin_exe:
            log_error(f"Failed to find executable path for {crate_binary}")
            sys.exit(1)

        if not out_dir:
            log_error(f"Failed to find build script output directory for package {current_package_id}")
            log_error("This usually means the package has no build.rs or the build failed")
            sys.exit(1)

        # Construct path to entry.tp
        out_dir_parent = Path(out_dir).parent
        entry_tp = out_dir_parent / "out" / "entry.tp"

        if not entry_tp.exists():
            log_error(f"entry.tp not found at {entry_tp}")
            log_error("This usually means the build script failed to generate the TouchPortal plugin definition")
            sys.exit(1)

        log_info(f"Built: {plugin_exe}")
        log_info(f"Entry: {entry_tp}")

        return str(plugin_exe), str(entry_tp)

    except subprocess.CalledProcessError as e:
        log_error(f"Failed to build plugin: {e}")
        sys.exit(1)


def validate_plugin_start_cmd(cmd_name: str, cmd_value: Optional[str], is_os_specific: bool,
                            plugin_name: str, plugin_exe: str) -> bool:
    """
    Validate a plugin start command.

    Args:
        cmd_name: Name of the command (for error messages)
        cmd_value: Command value to validate
        is_os_specific: True for OS-specific commands
        plugin_name: Expected plugin name
        plugin_exe: Path to the built executable

    Returns:
        True if validation passes
    """
    if not cmd_value or cmd_value == "null":
        return True  # Skip validation for absent optional commands

    log_info(f"Validating {cmd_name}: {cmd_value}")

    # Extract the path portion (before any space-separated arguments)
    cmd_path = cmd_value.split(' ', 1)[0]

    # Validate the directory structure: should start with %TP_PLUGIN_FOLDER% followed by plugin name
    pattern = r"^%TP_PLUGIN_FOLDER%([^/]+)/(.+)$"
    match = re.match(pattern, cmd_path)

    if not match:
        log_error(f"{cmd_name} has invalid format: {cmd_value}")
        log_error("Expected format: %TP_PLUGIN_FOLDER%<plugin_name>/<binary_name> [args...]")
        return False

    expected_plugin_dir = match.group(1)
    expected_binary_name = match.group(2)

    # Validate that the plugin directory matches the metadata plugin_name
    if expected_plugin_dir != plugin_name:
        log_error(f"Plugin directory mismatch in {cmd_name}")
        log_error(f"Expected: {plugin_name}")
        log_error(f"Found: {expected_plugin_dir}")
        log_error("This usually means the build.rs hardcoded directory doesn't match Cargo.toml metadata")
        return False

    # For OS-specific commands, allow different exe suffixes since they target different platforms
    # For the main command, validate against the current platform's built binary
    if is_os_specific:
        # For OS-specific commands, just validate that the base binary name matches (ignoring suffixes)
        expected_base_name = expected_binary_name
        actual_base_name = Path(plugin_exe).name

        # Strip known extensions for comparison
        if expected_base_name.endswith('.exe'):
            expected_base_name = expected_base_name[:-4]
        if actual_base_name.endswith('.exe'):
            actual_base_name = actual_base_name[:-4]

        if expected_base_name != actual_base_name:
            log_error(f"Binary base name mismatch in {cmd_name}")
            log_error(f"Expected: {expected_base_name} (ignoring OS suffix)")
            log_error(f"Built: {actual_base_name} (ignoring OS suffix)")
            log_error("This usually means the build.rs hardcoded binary name doesn't match Cargo.toml metadata")
            return False
    else:
        # For main plugin_start_cmd, validate exact match with current platform binary
        actual_binary_name = Path(plugin_exe).name
        if expected_binary_name != actual_binary_name:
            log_error(f"Binary name mismatch in {cmd_name}")
            log_error(f"Expected: {expected_binary_name}")
            log_error(f"Built: {actual_binary_name}")
            log_error("This usually means the build.rs hardcoded binary name doesn't match Cargo.toml metadata")
            return False

    return True


def validate_entry_tp(entry_tp_path: str, plugin_name: str, plugin_exe: str) -> None:
    """
    Validate that the plugin_start_cmd in entry.tp matches our build configuration.

    Args:
        entry_tp_path: Path to the entry.tp file
        plugin_name: Expected plugin name
        plugin_exe: Path to the built executable

    Raises:
        SystemExit: If validation fails
    """
    log_step("Validating plugin_start_cmd consistency")

    try:
        with open(entry_tp_path, 'r') as f:
            entry_data = json.load(f)

        # Extract and validate all plugin start command variants
        plugin_start_cmd = entry_data.get("plugin_start_cmd")
        plugin_start_cmd_windows = entry_data.get("plugin_start_cmd_windows", "")
        plugin_start_cmd_mac = entry_data.get("plugin_start_cmd_mac", "")
        plugin_start_cmd_linux = entry_data.get("plugin_start_cmd_linux", "")

        # The main plugin_start_cmd is required
        if not plugin_start_cmd:
            log_error("plugin_start_cmd not found in entry.tp")
            sys.exit(1)

        # Validate all present plugin start commands
        validations = [
            ("plugin_start_cmd", plugin_start_cmd, False),
            ("plugin_start_cmd_windows", plugin_start_cmd_windows, True),
            ("plugin_start_cmd_mac", plugin_start_cmd_mac, True),
            ("plugin_start_cmd_linux", plugin_start_cmd_linux, True),
        ]

        for cmd_name, cmd_value, is_os_specific in validations:
            if not validate_plugin_start_cmd(cmd_name, cmd_value, is_os_specific, plugin_name, plugin_exe):
                sys.exit(1)

        log_info("All plugin_start_cmd validations passed")

    except (FileNotFoundError, json.JSONDecodeError) as e:
        log_error(f"Failed to validate entry.tp: {e}")
        sys.exit(1)


def create_tpp_package(plugin_name: str, plugin_exe: str, entry_tp: str, tpp_file: str) -> None:
    """
    Create the .tpp package file.

    Args:
        plugin_name: Name of the plugin
        plugin_exe: Path to the plugin executable
        entry_tp: Path to the entry.tp file
        tpp_file: Name of the output .tpp file
    """
    log_step(f"Creating .tpp package: {tpp_file}")

    # Create a temporary directory for staging the plugin files
    with tempfile.TemporaryDirectory() as temp_dir:
        plugin_dir = Path(temp_dir) / plugin_name
        plugin_dir.mkdir(parents=True)

        # Copy the essential files: the binary and the plugin definition
        shutil.copy2(plugin_exe, plugin_dir)
        shutil.copy2(entry_tp, plugin_dir)

        # Create the ZIP package
        tpp_path = Path(tpp_file)
        with zipfile.ZipFile(tpp_path, 'w', zipfile.ZIP_DEFLATED) as zipf:
            for file_path in plugin_dir.rglob('*'):
                if file_path.is_file():
                    # Calculate the archive path relative to temp_dir
                    arcname = file_path.relative_to(temp_dir)
                    zipf.write(file_path, arcname)

    log_info(f"Created: {tpp_file}")


def main() -> None:
    """Main function."""
    parser = argparse.ArgumentParser(
        description="""Build a TouchPortal plugin into a .tpp package file.

Creates a .tpp package from the plugin in the current directory.
Only rebuilds if source files have changed since the last build.""",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        add_help=True,
    )

    args = parser.parse_args()

    log_step("TouchPortal Plugin Packager")

    # First, we extract the plugin configuration from Cargo.toml metadata.
    # This gives us the plugin name, binary name, and output .tpp filename.
    plugin_name, crate_binary, tpp_file = get_plugin_config()

    # Verify all required tools are available before we start building.
    # This prevents partial builds when dependencies are missing.
    check_requirements(["cargo", "zip"])

    log_info(f"Plugin: {plugin_name}")
    log_info(f"Binary: {crate_binary}")
    log_info(f"Output: {tpp_file}")

    # Check if we need to rebuild by comparing source file times to the .tpp file.
    # We only rebuild if source files are newer than the existing package.
    log_step("Checking if rebuild is needed")

    source_time = get_newest_source_time()
    tpp_path = Path(tpp_file)
    package_time = get_file_mtime(tpp_path)

    if source_time <= package_time:
        log_step(f"Package up to date: {tpp_file}")
        return

    # If we reach here, we need to build the plugin.
    plugin_exe, entry_tp = build_plugin(crate_binary)

    # Validate that the plugin_start_cmd in entry.tp matches our build configuration.
    # This ensures TouchPortal will be able to find and execute the plugin correctly.
    validate_entry_tp(entry_tp, plugin_name, plugin_exe)

    # Now we create the .tpp package file.
    # A .tpp file is simply a ZIP archive containing the plugin directory.
    create_tpp_package(plugin_name, plugin_exe, entry_tp, tpp_file)

    log_step("Packaging complete")


if __name__ == "__main__":
    main()