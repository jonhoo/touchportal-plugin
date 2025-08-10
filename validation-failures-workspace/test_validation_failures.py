#!/usr/bin/env python3
"""
Script to test that all validation-failure plugins fail compilation with expected errors.
Usage: ./test_validation_failures.py [plugin-name1] [plugin-name2] ...
If plugin names are provided, only those plugins will be tested.
"""

import argparse
import re
import subprocess
import sys
from pathlib import Path
from typing import List, Optional, Tuple

import tomllib

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


def get_workspace_members() -> List[str]:
    """
    Get list of all member plugins from Cargo.toml.
    
    Returns:
        List of plugin names
        
    Raises:
        SystemExit: If Cargo.toml cannot be read
    """
    cargo_toml_path = Path("Cargo.toml")
    
    try:
        with open(cargo_toml_path, 'rb') as f:
            cargo_data = tomllib.load(f)
        
        workspace = cargo_data.get("workspace", {})
        members = workspace.get("members", [])
        
        if not members:
            print(f"{Fore.RED}ERROR: No workspace members found in Cargo.toml{Style.RESET_ALL}")
            sys.exit(1)
        
        return members
        
    except FileNotFoundError:
        print(f"{Fore.RED}ERROR: Cargo.toml not found in current directory{Style.RESET_ALL}")
        sys.exit(1)
    except Exception as e:
        print(f"{Fore.RED}ERROR: Failed to parse Cargo.toml: {e}{Style.RESET_ALL}")
        sys.exit(1)


def get_package_name(plugin_dir: Path) -> Optional[str]:
    """
    Extract package name from the plugin's Cargo.toml.
    
    Args:
        plugin_dir: Path to the plugin directory
        
    Returns:
        Package name or None if not found
    """
    cargo_toml_path = plugin_dir / "Cargo.toml"
    
    if not cargo_toml_path.exists():
        return None
    
    try:
        with open(cargo_toml_path, 'rb') as f:
            cargo_data = tomllib.load(f)
        
        package = cargo_data.get("package", {})
        return package.get("name")
        
    except Exception:
        return None


def find_available_plugins() -> List[str]:
    """
    Find all available plugins in the current workspace.
    
    Returns:
        List of available plugin names
    """
    plugins = []
    for plugin_dir in Path(".").iterdir():
        if plugin_dir.is_dir() and (plugin_dir / "Cargo.toml").exists():
            plugins.append(plugin_dir.name)
    return plugins


def test_plugin(plugin: str) -> Tuple[str, bool]:
    """
    Test a single plugin for validation failures.
    
    Args:
        plugin: Plugin name to test
        
    Returns:
        Tuple of (status_message, success)
    """
    plugin_dir = Path(plugin)
    
    if not plugin_dir.exists():
        return f"{Fore.RED}ERROR: Plugin directory {plugin} does not exist{Style.RESET_ALL}", False
    
    package_name = get_package_name(plugin_dir)
    if not package_name:
        return f"{Fore.RED}ERROR: {plugin}/Cargo.toml not found or invalid{Style.RESET_ALL}", False
    
    # Check if this is an uncaught validation test (no expected-error.txt)
    expected_error_file = plugin_dir / "expected-error.txt"
    if not expected_error_file.exists():
        print(f"⚠️  UNCAUGHT VALIDATION TEST: This plugin tests a validation gap that is not currently caught by the SDK")
        print(f"Running: cargo check -p {package_name}")
        
        # For uncaught tests, we expect them to compile successfully
        try:
            subprocess.run(
                ["cargo", "check", "-p", package_name],
                capture_output=True,
                check=True,
                text=True,
            )
            return f"{Fore.GREEN}✓{Style.RESET_ALL} Plugin {plugin} compiled successfully (expected - validation gap)", True
        except subprocess.CalledProcessError:
            return (f"{Fore.RED}⚠️{Style.RESET_ALL}  Plugin {plugin} failed compilation - validation may have been implemented!\n"
                   "This uncaught test should be moved to proper validation test with expected-error.txt"), False
    
    # Read expected error
    try:
        with open(expected_error_file, 'r') as f:
            expected_error = f.read().strip()
    except IOError:
        return f"{Fore.RED}ERROR: Could not read expected-error.txt for {plugin}{Style.RESET_ALL}", False
    
    print(f"Expected error: {expected_error}")
    print(f"Running: cargo check -p {package_name}")
    
    # Run cargo check and capture stderr
    try:
        result = subprocess.run(
            ["cargo", "check", "-p", package_name],
            capture_output=True,
            check=True,
            text=True,
        )
        # If we reach here, compilation succeeded when it should have failed
        return f"{Fore.RED}ERROR{Style.RESET_ALL}: Plugin {plugin} compiled successfully, but it should have failed!", False
        
    except subprocess.CalledProcessError as e:
        actual_error = e.stderr
        
        # Check if the expected error is contained in the actual error output
        if expected_error in actual_error:
            return f"{Fore.GREEN}✓{Style.RESET_ALL} Plugin {plugin} failed with expected error", True
        else:
            return (f"{Fore.RED}✗{Style.RESET_ALL} Plugin {plugin} failed with unexpected error:\n"
                   f"Actual error: {actual_error}\n"
                   f"Expected error: {expected_error}"), False


def main() -> None:
    """Main function."""
    parser = argparse.ArgumentParser(
        description="Test that all validation-failure plugins fail compilation with expected errors",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "plugins",
        nargs="*",
        help="Specific plugin names to test (if not provided, all plugins will be tested)",
    )
    
    args = parser.parse_args()
    
    print("🧪 TouchPortal Validation Failure Test Suite")
    print("==============================================")
    
    if args.plugins:
        if len(args.plugins) == 1:
            print(f"Testing specific validation failure plugin: {args.plugins[0]}...")
        else:
            print(f"Testing specific validation failure plugins: {', '.join(args.plugins)}...")
    else:
        print("Testing all validation failure plugins...")
    
    # Change to the script directory
    script_dir = Path(__file__).parent
    original_dir = Path.cwd()
    
    try:
        script_dir = script_dir.resolve()
        original_dir.resolve()
        if script_dir != original_dir:
            import os
            os.chdir(script_dir)
    except OSError as e:
        print(f"{Fore.RED}ERROR: Failed to change to script directory: {e}{Style.RESET_ALL}")
        sys.exit(1)
    
    # Get list of all member plugins from Cargo.toml
    all_plugins = get_workspace_members()
    
    # If specific plugins requested, filter to just those
    if args.plugins:
        plugins = []
        for plugin in args.plugins:
            if plugin in all_plugins:
                plugins.append(plugin)
            else:
                print(f"{Fore.RED}ERROR: Unknown plugin '{plugin}'.{Style.RESET_ALL}")
                print("Available plugins:")
                for available_plugin in all_plugins:
                    print(f"  - {available_plugin}")
                sys.exit(1)
    else:
        plugins = all_plugins
    
    total_plugins = 0
    passed_plugins = 0
    failed_plugins = 0
    uncaught_plugins = 0
    
    for plugin in plugins:
        print("")
        print(f"=== Testing plugin: {plugin} ===")
        total_plugins += 1
        
        expected_error_file = Path(plugin) / "expected-error.txt"
        is_uncaught = not expected_error_file.exists()
        
        status_message, success = test_plugin(plugin)
        print(status_message)
        
        if success:
            if is_uncaught:
                uncaught_plugins += 1
            else:
                passed_plugins += 1
        else:
            failed_plugins += 1
    
    print("")
    print("==============================================")
    print("📊 Test Summary:")
    print(f"  Total plugins: {total_plugins}")
    print(f"  {Fore.GREEN}Passed: {passed_plugins}{Style.RESET_ALL}")
    print(f"  {Fore.YELLOW}Uncaught: {uncaught_plugins}{Style.RESET_ALL}")
    print(f"  {Fore.RED}Failed: {failed_plugins}{Style.RESET_ALL}")
    
    if failed_plugins == 0:
        print(f"{Fore.GREEN}✅ All tests passed!{Style.RESET_ALL}")
        if uncaught_plugins > 0:
            print(f"Note: {uncaught_plugins} validation gaps were confirmed as still uncaught by the SDK")
        sys.exit(0)
    else:
        print(f"{Fore.RED}❌ Some tests failed{Style.RESET_ALL}")
        sys.exit(1)


if __name__ == "__main__":
    main()