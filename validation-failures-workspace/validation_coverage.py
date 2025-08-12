#!/usr/bin/env python3
"""
Coverage collection script for validation failure tests.

This script creates a temporary workspace mirroring the validation-failures-workspace,
but transforms the build.rs files into main.rs files so the plugin definitions can run
as regular binaries for coverage collection.
"""

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path
from typing import List, Optional


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
    """Get list of all member plugins from Cargo.toml."""
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


def create_temporary_workspace(temp_dir: Path) -> None:
    """Create temporary workspace structure for coverage collection."""
    print(f"Creating temporary workspace in {temp_dir}")
    
    # Copy workspace Cargo.toml and Cargo.lock
    shutil.copy("Cargo.toml", temp_dir / "Cargo.toml")
    if Path("Cargo.lock").exists():
        shutil.copy("Cargo.lock", temp_dir / "Cargo.lock")
    
    # Get the target directory used by the main validation workspace
    try:
        result = subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version=1"],
            capture_output=True,
            text=True,
            check=True
        )
        import json
        metadata = json.loads(result.stdout)
        target_dir = metadata.get("target_directory", "target")
        print(f"  Using target directory: {target_dir}")
    except Exception as e:
        print(f"  WARNING: Could not get target directory, using default: {e}")
        target_dir = "target"
    
    # Create .cargo/config.toml to use the same target directory
    cargo_dir = temp_dir / ".cargo"
    cargo_dir.mkdir()
    
    with open(cargo_dir / "config.toml", "w") as f:
        f.write(f"""[build]
target-dir = "{target_dir}"
""")
    
    members = get_workspace_members()
    
    for member in members:
        plugin_dir = Path(member)
        temp_plugin_dir = temp_dir / member
        
        if not plugin_dir.exists():
            print(f"WARNING: Plugin directory {member} does not exist")
            continue
        
        print(f"  Processing {member}...")
        
        # Create plugin directory structure
        temp_plugin_dir.mkdir()
        (temp_plugin_dir / "src").mkdir()
        
        # Copy and fix plugin Cargo.toml (update SDK path references and convert build-dependencies to dependencies)
        plugin_cargo = plugin_dir / "Cargo.toml"
        if plugin_cargo.exists():
            with open(plugin_cargo, 'r') as f:
                cargo_content = f.read()
            
            # Fix SDK path references to use absolute path instead of copying SDK
            sdk_abs_path = (Path("../sdk").resolve()).as_posix()
            cargo_content = cargo_content.replace('path = "../../sdk"', f'path = "{sdk_abs_path}"')
            
            # Convert [build-dependencies] to [dependencies] for coverage collection
            # Validation failure tests only have build-dependencies since they run at build-time,
            # but for coverage collection we need to run the plugin definitions as regular binaries,
            # so we convert build-dependencies to regular dependencies in the temporary workspace
            cargo_content = cargo_content.replace('[build-dependencies]', '[dependencies]')
            
            with open(temp_plugin_dir / "Cargo.toml", 'w') as f:
                f.write(cargo_content)
        
        # Copy plugin.rs if it exists (to src/plugin.rs for main.rs module reference)
        plugin_rs = plugin_dir / "plugin.rs"
        if plugin_rs.exists():
            shutil.copy(plugin_rs, temp_plugin_dir / "src" / "plugin.rs")
        else:
            print(f"    WARNING: No plugin.rs found for {member}")
            continue
        
        # Create main.rs that uses the plugin definition
        main_rs_content = """mod plugin;

fn main() {
    let result = std::panic::catch_unwind(|| {
        plugin::plugin()
    });
    
    match result {
        Ok(_plugin) => println!("Plugin generated successfully"),
        Err(e) => {
            println!("Plugin validation failed as expected: {:?}", e);
            std::process::exit(0); // Exit successfully despite validation error
        }
    }
}"""
        
        with open(temp_plugin_dir / "src" / "main.rs", "w") as f:
            f.write(main_rs_content)


def get_package_name_from_cargo_toml(plugin_dir: Path) -> Optional[str]:
    """Extract package name from a plugin's Cargo.toml file."""
    cargo_toml = plugin_dir / "Cargo.toml"
    if not cargo_toml.exists():
        return None
    
    try:
        with open(cargo_toml, 'rb') as f:
            cargo_data = tomllib.load(f)
        return cargo_data.get("package", {}).get("name")
    except Exception:
        return None


def run_coverage_collection(temp_dir: Path, output_dir: Path) -> None:
    """Run coverage collection on all plugins in the temporary workspace."""
    print(f"Running coverage collection...")
    
    members = get_workspace_members()
    coverage_files = []
    
    for member in members:
        # Get the actual package name from Cargo.toml
        plugin_dir = Path(member)
        package_name = get_package_name_from_cargo_toml(plugin_dir)
        
        if not package_name:
            print(f"  ⚠️ Skipping {member}: could not find package name")
            continue
            
        print(f"  Collecting coverage for {member} (package: {package_name})...")
        
        coverage_file = output_dir / f"validation-{member}.lcov"
        temp_coverage_file = temp_dir / f"validation-{member}.lcov"
        
        try:
            # Run cargo llvm-cov run for this specific plugin using the package name
            # The coverage file will be created in the temp directory, then we'll move it
            result = subprocess.run([
                "cargo", "llvm-cov", "run", 
                "--lcov", "--output-path", str(temp_coverage_file),
                "-p", package_name
            ], 
            cwd=temp_dir,
            capture_output=True,
            text=True
            )
            
            # We expect some plugins might exit with errors, but coverage is still collected
            # Check if coverage file was generated in temp directory and move it
            if temp_coverage_file.exists():
                # Move the coverage file from temp directory to output directory
                shutil.move(str(temp_coverage_file), str(coverage_file))
                coverage_files.append(coverage_file)
                print(f"    ✓ Coverage collected: {coverage_file.name}")
            else:
                print(f"    ❌ ERROR: No coverage file generated for {member} at temp path: {temp_coverage_file}")
                if result.stderr:
                    print(f"    Cargo stderr: {result.stderr}")
                if result.stdout:
                    print(f"    Cargo stdout: {result.stdout}")
                # This is a hard error - we should have generated coverage
                raise RuntimeError(f"Coverage collection failed for {member}: no coverage file generated at {temp_coverage_file}")
        
        except subprocess.CalledProcessError as e:
            print(f"    ⚠️ Coverage collection failed for {member}: {e}")
        except Exception as e:
            print(f"    ERROR: Unexpected error for {member}: {e}")
    
    print(f"Coverage collection complete. Generated {len(coverage_files)} coverage files.")
    
    # Validate that we actually collected some coverage files
    if len(coverage_files) == 0:
        raise RuntimeError("ERROR: No coverage files were generated from any validation failure tests. This indicates coverage collection is not working properly.")


def main() -> None:
    """Main function."""
    parser = argparse.ArgumentParser(
        description="Collect code coverage from validation failure tests",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--coverage",
        action="store_true",
        help="Enable coverage collection (default behavior, kept for compatibility)"
    )
    
    args = parser.parse_args()
    
    print("🔬 Validation Failure Coverage Collection")
    print("========================================")
    
    # Ensure we're in the right directory
    script_dir = Path(__file__).parent
    if script_dir.resolve() != Path.cwd().resolve():
        os.chdir(script_dir)
    
    # Create output directory for coverage files
    output_dir = Path(".")
    output_dir.mkdir(exist_ok=True)
    
    # Create temporary workspace
    with tempfile.TemporaryDirectory(prefix="validation-coverage-") as temp_dir_str:
        temp_dir = Path(temp_dir_str)
        
        try:
            create_temporary_workspace(temp_dir)
            run_coverage_collection(temp_dir, output_dir)
            
        except KeyboardInterrupt:
            print(f"\n{Fore.YELLOW}Coverage collection interrupted{Style.RESET_ALL}")
            sys.exit(1)
        except Exception as e:
            print(f"{Fore.RED}ERROR: Coverage collection failed: {e}{Style.RESET_ALL}")
            sys.exit(1)
    
    print(f"{Fore.GREEN}✅ Validation coverage collection complete!{Style.RESET_ALL}")


if __name__ == "__main__":
    main()
