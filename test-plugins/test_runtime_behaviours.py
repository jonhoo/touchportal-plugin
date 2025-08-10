#!/usr/bin/env python3
"""
Test TouchPortal plugin runtime behaviors.

This script tests plugins that have mock support by running them with a timeout.
Plugins should exit gracefully via ClosePlugin, so timeouts are treated as failures.
"""

import argparse
import subprocess
import sys
import signal
import time
from pathlib import Path
from typing import List, Optional, Tuple

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


def show_usage() -> None:
    """Show usage information."""
    print("Usage: test_runtime_behaviours.py [plugin-name...]")
    print("")
    print("Test TouchPortal plugin runtime behaviors.")
    print("")
    print("Options:")
    print("  [plugin-name...]  Run tests only for specified plugins")
    print("  -h, --help        Show this help message")
    print("")
    print("Examples:")
    print("  test_runtime_behaviours.py                      # Test all plugins")
    print("  test_runtime_behaviours.py minimal-single       # Test only minimal-single plugin")
    print("  test_runtime_behaviours.py all-data-types no-events  # Test multiple specific plugins")
    print("")
    print("Available plugins:")
    
    # List available plugins
    for plugin in find_available_plugins():
        print(f"  - {plugin}")


def find_available_plugins() -> List[str]:
    """
    Find all available test plugin directories.
    
    Returns:
        List of plugin directory names
    """
    current_dir = Path.cwd()
    plugins = []
    
    for plugin_dir in current_dir.iterdir():
        if plugin_dir.is_dir() and (plugin_dir / "Cargo.toml").exists():
            plugins.append(plugin_dir.name)
    
    return plugins


def has_mock_support(plugin_dir: Path) -> bool:
    """
    Check if plugin has mock support by looking for mock server usage in main.rs.
    
    Args:
        plugin_dir: Path to the plugin directory
        
    Returns:
        True if plugin has mock support
    """
    main_rs_path = plugin_dir / "src" / "main.rs"
    
    if not main_rs_path.exists():
        return False
    
    try:
        with open(main_rs_path, 'r') as f:
            content = f.read()
        return "MockTouchPortalServer" in content
    except IOError:
        return False


def run_plugin_test(plugin_dir: Path, timeout_seconds: int = 30) -> Tuple[bool, str]:
    """
    Run a single plugin test.
    
    Args:
        plugin_dir: Path to the plugin directory
        timeout_seconds: Timeout in seconds
        
    Returns:
        Tuple of (success, status_message)
    """
    original_dir = Path.cwd()
    
    try:
        # Change to plugin directory
        import os
        os.chdir(plugin_dir)
        
        # Run the plugin with timeout
        try:
            result = subprocess.run(
                ["cargo", "run", "--quiet"],
                timeout=timeout_seconds,
                capture_output=True,
                text=True,
            )
            
            # If process completed within timeout, it should have exited successfully (code 0)
            if result.returncode == 0:
                return True, f"{Fore.GREEN}PASSED{Style.RESET_ALL}"
            else:
                return False, f"{Fore.RED}FAILED{Style.RESET_ALL} (exit code {result.returncode})"
                
        except subprocess.TimeoutExpired:
            # Timeout is a failure as plugins should exit gracefully
            return False, f"{Fore.RED}FAILED{Style.RESET_ALL} (timed out - plugin should exit gracefully)"
        
        except subprocess.CalledProcessError as e:
            return False, f"{Fore.RED}FAILED{Style.RESET_ALL} (cargo run failed: {e})"
    
    finally:
        # Always change back to original directory
        import os
        os.chdir(original_dir)


def main() -> None:
    """Main function."""
    parser = argparse.ArgumentParser(
        description="Test TouchPortal plugin runtime behaviors",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        add_help=False,  # We'll handle help manually to match bash script behavior
    )
    parser.add_argument(
        "plugins",
        nargs="*",
        help="Specific plugin names to test",
    )
    parser.add_argument(
        "-h", "--help",
        action="store_true",
        help="Show this help message",
    )
    
    args = parser.parse_args()
    
    if args.help:
        show_usage()
        return
    
    # Validate requested plugins if any were provided
    available_plugins = find_available_plugins()
    
    if args.plugins:
        unknown_plugins = []
        for plugin in args.plugins:
            plugin_dir = Path(plugin)
            if not plugin_dir.is_dir() or not (plugin_dir / "Cargo.toml").exists():
                unknown_plugins.append(plugin)
        
        if unknown_plugins:
            print(f"{Fore.RED}Error: Unknown plugin names: {', '.join(unknown_plugins)}{Style.RESET_ALL}")
            print("")
            show_usage()
            sys.exit(1)
        
        plugins_to_test = args.plugins
    else:
        plugins_to_test = available_plugins
    
    print("🧪 TouchPortal Plugin Runtime Behavior Tests")
    print("==================================")
    
    # Counters
    total_plugins = 0
    tested_plugins = 0
    skipped_plugins = 0
    failed_plugins = 0
    
    for plugin_name in plugins_to_test:
        plugin_dir = Path(plugin_name)
        total_plugins += 1
        
        print(f"Testing {plugin_name}... ", end="", flush=True)
        
        # Check if plugin has mock support
        if has_mock_support(plugin_dir):
            # Plugin has mock support, run the test
            success, status_message = run_plugin_test(plugin_dir)
            print(status_message)
            
            if success:
                tested_plugins += 1
            else:
                failed_plugins += 1
        else:
            # Plugin doesn't have mock support yet
            print(f"{Fore.YELLOW}SKIPPED{Style.RESET_ALL} (no mock support)")
            skipped_plugins += 1
    
    print("")
    print("==================================")
    print("📊 Test Summary:")
    print(f"  Total plugins: {total_plugins}")
    print(f"  {Fore.GREEN}Tested: {tested_plugins}{Style.RESET_ALL}")
    print(f"  {Fore.YELLOW}Skipped: {skipped_plugins}{Style.RESET_ALL}")
    print(f"  {Fore.RED}Failed: {failed_plugins}{Style.RESET_ALL}")
    
    if failed_plugins == 0:
        print(f"{Fore.GREEN}✅ All tests passed!{Style.RESET_ALL}")
        sys.exit(0)
    else:
        print(f"{Fore.RED}❌ Some tests failed{Style.RESET_ALL}")
        sys.exit(1)


if __name__ == "__main__":
    main()