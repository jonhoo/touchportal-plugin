#!/usr/bin/env python3
"""
Common functions for TouchPortal plugin packaging and installation.
Import this module from plugin package.py and install.py scripts.
"""

import json
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Dict, List, Optional, Tuple

try:
    import colorama
    from colorama import Fore, Style
    colorama.init()
except ImportError:
    # Fallback if colorama not available
    class Fore:
        RED = ""
        GREEN = ""
        YELLOW = ""

    class Style:
        RESET_ALL = ""


def get_plugin_config() -> Tuple[str, str, str]:
    """
    Get plugin configuration from cargo metadata.

    Returns:
        Tuple of (plugin_name, crate_binary, tpp_file)

    Raises:
        SystemExit: If plugin configuration cannot be found
    """
    try:
        # Get current package metadata
        metadata_result = subprocess.run(
            ["cargo", "metadata", "--format-version=1", "--no-deps"],
            capture_output=True,
            text=True,
            check=True,
        )
        metadata = json.loads(metadata_result.stdout)

        # Get the current package ID using cargo pkgid
        pkgid_result = subprocess.run(
            ["cargo", "pkgid"],
            capture_output=True,
            text=True,
            check=True,
        )
        current_package_id = pkgid_result.stdout.strip()

        # Get the current package by matching the ID
        current_package = None
        for package in metadata["packages"]:
            if package["id"] == current_package_id:
                current_package = package
                break

        if not current_package:
            log_error(f"Could not find package with ID {current_package_id}")
            log_error("This usually means the current directory is not a valid Rust crate")
            sys.exit(1)

        # Extract plugin name from metadata
        metadata_dict = current_package.get("metadata") or {}
        plugin_metadata = metadata_dict.get("touchportal", {})
        plugin_name = plugin_metadata.get("plugin_name")
        if not plugin_name:
            log_error("package.metadata.touchportal.plugin_name not found in Cargo.toml")
            log_error("Please add the following to your Cargo.toml:")
            log_error("[package.metadata.touchportal]")
            log_error("plugin_name = \"YourPluginName\"")
            sys.exit(1)

        # Extract plugin binary name from metadata, fallback to default-run, then package name
        crate_binary = (
            plugin_metadata.get("plugin_binary") or
            current_package.get("default_run") or
            current_package["name"]
        )

        # Derive tpp filename
        tpp_file = f"{plugin_name}.tpp"

        return plugin_name, crate_binary, tpp_file

    except subprocess.CalledProcessError as e:
        log_error(f"Failed to get cargo metadata: {e}")
        if e.stderr:
            log_error("Cargo stderr output:")
            for line in e.stderr.strip().split('\n'):
                log_error(f"  {line}")
        log_error("Please ensure you're in a valid Rust crate directory with Cargo.toml")
        sys.exit(1)
    except json.JSONDecodeError as e:
        log_error(f"Failed to parse cargo metadata JSON: {e}")
        log_error("This usually indicates a problem with the cargo installation")
        sys.exit(1)


def check_requirements(tools: List[str]) -> None:
    """
    Check if required tools are available.

    Args:
        tools: List of tool names to check for

    Raises:
        SystemExit: If any required tools are missing
    """
    missing_tools = []

    for tool in tools:
        if not shutil.which(tool):
            missing_tools.append(tool)

    if missing_tools:
        log_error(f"Missing required tools: {', '.join(missing_tools)}")
        log_error("Please install the missing tools and try again.")
        sys.exit(1)


def log_step(message: str) -> None:
    """Log a major step with ==> prefix."""
    print(f"==> {message}")


def log_info(message: str) -> None:
    """Log informational message with indentation."""
    print(f"    {message}")


def log_error(message: str) -> None:
    """Log error message to stderr with ERROR prefix."""
    print(f"{Fore.RED}ERROR: {message}{Style.RESET_ALL}", file=sys.stderr)


def log_success(message: str) -> None:
    """Log success message in green."""
    print(f"{Fore.GREEN}{message}{Style.RESET_ALL}")


def log_warning(message: str) -> None:
    """Log warning message in yellow."""
    print(f"{Fore.YELLOW}{message}{Style.RESET_ALL}")