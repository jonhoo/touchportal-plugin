#!/usr/bin/env python3
"""
TouchPortal Plugin Installer

This script installs a TouchPortal plugin by:
1. First ensuring the plugin is packaged (delegating to package.py)
2. Extracting the .tpp package file to a temporary location
3. Syncing the plugin files to TouchPortal's plugin directory
4. Cleaning up temporary files

The script modifies the user's system by installing files to the TouchPortal
plugin directory. Use package.py instead if you only want to create the .tpp file.
"""

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

# Import our common functions
from plugin_common import (
    get_plugin_config,
    check_requirements,
    log_step,
    log_info,
    log_error,
)


def show_help() -> None:
    """Show help information."""
    print("install.py - Install a TouchPortal plugin to your system")
    print("")
    print("Packages the plugin (if needed) and installs it to TouchPortal's plugin directory.")
    print("Modifies your system by copying files to ~/.config/TouchPortal/plugins/")


def ensure_plugin_packaged() -> None:
    """
    Ensure the plugin is packaged by running package.py.
    This handles rebuild checking automatically and only rebuilds if needed.
    """
    log_step("Ensuring plugin is packaged")
    
    # Get the directory where this script lives
    script_dir = Path(__file__).parent
    package_script = script_dir / "package.py"
    
    try:
        subprocess.run([sys.executable, str(package_script)], check=True)
    except subprocess.CalledProcessError as e:
        log_error(f"Failed to package plugin: {e}")
        sys.exit(1)


def install_plugin(plugin_name: str, tpp_file: str) -> None:
    """
    Install the plugin to TouchPortal's plugin directory.
    
    Args:
        plugin_name: Name of the plugin
        tpp_file: Path to the .tpp file
    """
    # TouchPortal expects plugins to be in ~/.config/TouchPortal/plugins/<plugin-name>/
    install_dir = Path.home() / ".config" / "TouchPortal" / "plugins" / plugin_name
    
    log_step("Installing plugin files")
    log_info(f"Destination: {install_dir}")
    
    # Create a temporary directory to extract the .tpp file
    # We extract first to avoid partial installations if the .tpp is corrupted
    with tempfile.TemporaryDirectory() as temp_dir:
        temp_path = Path(temp_dir)
        
        try:
            # Extract the .tpp file (which is a ZIP archive) to the temporary directory
            with zipfile.ZipFile(tpp_file, 'r') as zip_ref:
                zip_ref.extractall(temp_path)
            
            # Find the plugin directory inside the extracted archive
            # There should be exactly one directory containing the plugin files
            extracted_dirs = [d for d in temp_path.iterdir() if d.is_dir()]
            
            if not extracted_dirs:
                log_error(f"No plugin directory found in {tpp_file}")
                sys.exit(1)
            
            if len(extracted_dirs) > 1:
                log_error(f"Multiple directories found in {tpp_file}, expected exactly one")
                sys.exit(1)
            
            extracted_dir = extracted_dirs[0]
            
            # Create the target directory if it doesn't exist
            # This ensures we have a clean installation location
            install_dir.mkdir(parents=True, exist_ok=True)
            
            # Remove existing files in the install directory to ensure clean installation
            if install_dir.exists():
                for item in install_dir.iterdir():
                    if item.is_file():
                        item.unlink()
                    elif item.is_dir():
                        shutil.rmtree(item)
            
            # Copy all plugin files to the installation directory
            for item in extracted_dir.iterdir():
                dest_path = install_dir / item.name
                if item.is_file():
                    shutil.copy2(item, dest_path)
                elif item.is_dir():
                    shutil.copytree(item, dest_path)
            
            # Count installed files
            file_count = sum(1 for _ in install_dir.rglob('*') if _.is_file())
            log_info(f"Installed: {file_count} files")
            
        except zipfile.BadZipFile:
            log_error(f"Invalid .tpp file: {tpp_file}")
            sys.exit(1)
        except (OSError, shutil.Error) as e:
            log_error(f"Failed to install plugin files: {e}")
            sys.exit(1)


def main() -> None:
    """Main function."""
    parser = argparse.ArgumentParser(
        description="Install a TouchPortal plugin to your system",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--help-extended",
        action="store_true",
        help="Show extended help information",
    )
    
    args = parser.parse_args()
    
    if args.help_extended:
        show_help()
        return
    
    log_step("TouchPortal Plugin Installer")
    
    # First, we extract the plugin configuration from Cargo.toml metadata.
    # This gives us the plugin name and expected .tpp filename.
    plugin_name, _, tpp_file = get_plugin_config()
    
    # Verify all required tools are available before we start the installation.
    # We don't need rsync since we use Python's built-in file operations.
    check_requirements(["python3"])
    
    log_info(f"Plugin: {plugin_name}")
    log_info(f"Package: {tpp_file}")
    
    # Ensure the plugin is packaged first
    ensure_plugin_packaged()
    
    # Now we proceed with the actual installation
    install_plugin(plugin_name, tpp_file)
    
    log_step("Installation complete")
    print("")
    print("The plugin has been installed to TouchPortal.")
    print("Restart TouchPortal to load the new plugin.")


if __name__ == "__main__":
    main()