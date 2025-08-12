#!/usr/bin/env python3
"""
Test TouchPortal plugin features.

This script tests feature test plugins that have mock support by running them with a timeout.
Plugins should exit gracefully via ClosePlugin, so timeouts are treated as failures.
"""

import argparse
import re
import shlex
import subprocess
import sys
import signal
import time
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


def show_usage() -> None:
    """Show usage information."""
    print("Usage: run_feature_tests.py [options] [plugin-name...]")
    print("")
    print("Test TouchPortal plugin features.")
    print("")
    print("Options:")
    print("  [plugin-name...]  Run tests only for specified plugins")
    print("  --coverage        Enable coverage collection during tests")
    print("  -h, --help        Show this help message")
    print("")
    print("Examples:")
    print("  run_feature_tests.py                      # Test all plugins")
    print("  run_feature_tests.py --coverage           # Test all plugins with coverage")
    print("  run_feature_tests.py minimal-single       # Test only minimal-single plugin")
    print("  run_feature_tests.py --coverage minimal-single  # Test with coverage")
    print("  run_feature_tests.py all-data-types no-events  # Test multiple specific plugins")
    print("")
    print("Available plugins:")

    # List available plugins
    for plugin in find_available_plugins():
        print(f"  - {plugin}")


def find_available_plugins() -> List[str]:
    """
    Find all available feature test plugin directories.

    Returns:
        List of plugin directory names
    """
    current_dir = Path.cwd()
    plugins = []

    for plugin_dir in current_dir.iterdir():
        if plugin_dir.is_dir() and (plugin_dir / "Cargo.toml").exists():
            plugins.append(plugin_dir.name)

    return plugins


def setup_coverage_env() -> Optional[Dict[str, str]]:
    """
    Set up coverage environment variables using cargo llvm-cov show-env.
    
    Returns:
        Dictionary of environment variables for coverage, or None if setup failed
    """
    try:
        # Get coverage environment variables
        result = subprocess.run(
            ["cargo", "llvm-cov", "show-env", "--export-prefix"],
            capture_output=True,
            text=True,
        )
        
        if result.returncode != 0:
            return None
            
        # Parse the output to extract environment variables
        env_vars = {}
        export_pattern = re.compile(r'^export\s+([^=]+)=(.*)$')
        
        for line in result.stdout.strip().split('\n'):
            line = line.strip()
            if not line:
                continue
                
            match = export_pattern.match(line)
            if match:
                key = match.group(1).strip()
                value_part = match.group(2)
                
                # Use shlex to properly parse the value, handling quotes and escapes
                try:
                    # shlex.split handles proper shell quoting
                    parsed_values = shlex.split(value_part)
                    if parsed_values:
                        # For environment variables, we expect a single value
                        env_vars[key] = parsed_values[0]
                except ValueError:
                    # If shlex fails to parse, fall back to the raw value
                    # This handles edge cases where the value might not be properly quoted
                    env_vars[key] = value_part
        
        return env_vars
        
    except (subprocess.CalledProcessError, FileNotFoundError):
        return None


def clean_coverage_workspace() -> bool:
    """
    Clean the workspace for coverage collection.
    
    Returns:
        True if successful, False otherwise
    """
    try:
        result = subprocess.run(
            ["cargo", "llvm-cov", "clean", "--workspace"],
            capture_output=True,
            text=True,
        )
        return result.returncode == 0
    except (subprocess.CalledProcessError, FileNotFoundError):
        return False


def generate_coverage_report_for_plugin(plugin_name: str, coverage_env: Optional[dict] = None) -> bool:
    """
    Generate coverage report for a specific plugin.
    
    Args:
        plugin_name: Name of the plugin to generate coverage for
        coverage_env: Environment variables for coverage
        
    Returns:
        True if successful, False otherwise
    """
    plugin_dir = Path(plugin_name)
    output_path = f"coverage-{plugin_name}.lcov"
    
    try:
        original_dir = Path.cwd()
        import os
        os.chdir(plugin_dir)
        
        # Prepare environment
        env = dict(os.environ)
        if coverage_env:
            env.update(coverage_env)
        
        result = subprocess.run(
            ["cargo", "llvm-cov", "report", "--lcov", "--output-path", f"../{output_path}"],
            capture_output=True,
            text=True,
            env=env,
        )
        
        os.chdir(original_dir)
        
        return result.returncode == 0
        
    except (subprocess.CalledProcessError, FileNotFoundError, OSError):
        return False


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


def run_plugin_test(plugin_dir: Path, timeout_seconds: int = 30, enable_coverage: bool = False, coverage_env: Optional[dict] = None) -> Tuple[bool, str]:
    """
    Run a single plugin test.

    Args:
        plugin_dir: Path to the plugin directory
        timeout_seconds: Timeout in seconds (applies only to execution, not build)
        enable_coverage: Whether to collect coverage data
        coverage_env: Environment variables for coverage (from cargo llvm-cov show-env)

    Returns:
        Tuple of (success, status_message)
    """
    original_dir = Path.cwd()

    try:
        # Change to plugin directory
        import os
        os.chdir(plugin_dir)

        # Prepare environment - use coverage env if provided, otherwise use current env
        env = dict(os.environ)
        if enable_coverage and coverage_env:
            env.update(coverage_env)

        # Step 1: Build the plugin first (without timeout to handle dependency compilation)
        build_cmd = ["cargo", "build", "--quiet"]

        try:
            build_result = subprocess.run(
                build_cmd,
                capture_output=True,
                text=True,
                env=env,
            )

            if build_result.returncode != 0:
                return False, f"{Fore.RED}FAILED{Style.RESET_ALL} (build failed: {build_result.stderr.strip()})"

        except subprocess.CalledProcessError as e:
            return False, f"{Fore.RED}FAILED{Style.RESET_ALL} (build command failed: {e})"

        # Step 2: Run the plugin with timeout (now that it's built)
        run_cmd = ["cargo", "run", "--quiet"]

        try:
            result = subprocess.run(
                run_cmd,
                timeout=timeout_seconds,
                capture_output=True,
                text=True,
                env=env,
            )

            # If process completed within timeout, it should have exited successfully (code 0)
            if result.returncode == 0:
                if enable_coverage:
                    return True, f"{Fore.GREEN}PASSED{Style.RESET_ALL} (coverage data collected)"
                else:
                    return True, f"{Fore.GREEN}PASSED{Style.RESET_ALL}"
            else:
                return False, f"{Fore.RED}FAILED{Style.RESET_ALL} (exit code {result.returncode})"

        except subprocess.TimeoutExpired:
            # Timeout is a failure as plugins should exit gracefully
            return False, f"{Fore.RED}FAILED{Style.RESET_ALL} (timed out - plugin should exit gracefully)"

        except subprocess.CalledProcessError as e:
            return False, f"{Fore.RED}FAILED{Style.RESET_ALL} (execution failed: {e})"

    finally:
        # Always change back to original directory
        import os
        os.chdir(original_dir)


def main() -> None:
    """Main function."""
    parser = argparse.ArgumentParser(
        description="Test TouchPortal plugin feature tests",
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
    parser.add_argument(
        "--coverage",
        action="store_true",
        help="Enable coverage collection during tests",
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

    print("🧪 TouchPortal Plugin Feature Tests")
    print("==================================")

    # Set up coverage environment if needed
    coverage_env = None
    if args.coverage:
        print("Setting up coverage environment... ", end="", flush=True)
        coverage_env = setup_coverage_env()
        if coverage_env is None:
            print(f"{Fore.RED}FAILED{Style.RESET_ALL} (could not set up coverage environment)")
            sys.exit(1)
        
        # Clean workspace for accurate coverage
        if not clean_coverage_workspace():
            print(f"{Fore.RED}FAILED{Style.RESET_ALL} (could not clean coverage workspace)")
            sys.exit(1)
            
        print(f"{Fore.GREEN}DONE{Style.RESET_ALL}")

    # Counters
    total_plugins = 0
    tested_plugins = 0
    skipped_plugins = 0
    failed_plugins = 0
    successfully_tested_plugins = []

    for plugin_name in plugins_to_test:
        plugin_dir = Path(plugin_name)
        total_plugins += 1

        print(f"Testing {plugin_name}... ", end="", flush=True)

        # Check if plugin has mock support
        if has_mock_support(plugin_dir):
            # Plugin has mock support, run the test
            success, status_message = run_plugin_test(
                plugin_dir, 
                enable_coverage=args.coverage,
                coverage_env=coverage_env
            )
            print(status_message)

            if success:
                tested_plugins += 1
                successfully_tested_plugins.append(plugin_name)
            else:
                failed_plugins += 1
        else:
            # Plugin doesn't have mock support yet
            print(f"{Fore.YELLOW}SKIPPED{Style.RESET_ALL} (no mock support)")
            skipped_plugins += 1

    # Generate coverage reports if coverage was enabled and tests were successful
    if args.coverage and failed_plugins == 0 and successfully_tested_plugins:
        print("")
        print("Generating coverage reports... ", end="", flush=True)
        coverage_success = True
        for plugin_name in successfully_tested_plugins:
            if not generate_coverage_report_for_plugin(plugin_name, coverage_env):
                coverage_success = False
        
        if coverage_success:
            coverage_files = [f"coverage-{plugin}.lcov" for plugin in successfully_tested_plugins]
            print(f"{Fore.GREEN}DONE{Style.RESET_ALL} (saved {', '.join(coverage_files)})")
        else:
            print(f"{Fore.RED}FAILED{Style.RESET_ALL} (coverage report generation failed)")
            failed_plugins += 1  # Treat coverage failure as a test failure

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