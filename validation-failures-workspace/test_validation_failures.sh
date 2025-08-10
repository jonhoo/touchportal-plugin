#!/bin/bash
set -euo pipefail

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# Script to test that all validation-failure plugins fail compilation with expected errors
# Usage: ./test_validation_failures.sh [plugin-name1] [plugin-name2] ...
# If plugin names are provided, only those plugins will be tested

echo "🧪 TouchPortal Validation Failure Test Suite"
echo "=============================================="

if [ $# -gt 0 ]; then
    if [ $# -eq 1 ]; then
        echo "Testing specific validation failure plugin: $1..."
    else
        echo "Testing specific validation failure plugins: $*..."
    fi
else
    echo "Testing all validation failure plugins..."
fi

WORKSPACE_DIR="$(dirname "$0")"
cd "$WORKSPACE_DIR"

# Get list of all member plugins from Cargo.toml
ALL_PLUGINS_STR=$(grep -A 100 "members = \[" Cargo.toml | grep -E '^\s*"[^"]+",?' | sed 's/.*"\([^"]*\)".*/\1/' | sed '/^\s*$/d')
readarray -t ALL_PLUGINS <<< "$ALL_PLUGINS_STR"

# If specific plugins requested, filter to just those
if [ $# -gt 0 ]; then
    PLUGINS=()
    for plugin in "$@"; do
        # Check if the specific plugin exists in the list
        if printf '%s\n' "${ALL_PLUGINS[@]}" | grep -qx "$plugin"; then
            PLUGINS+=("$plugin")
        else
            echo "ERROR: Plugin '$plugin' not found in workspace members."
            echo "Available plugins:"
            printf '%s\n' "${ALL_PLUGINS[@]}" | sed 's/^/  - /'
            exit 1
        fi
    done
else
    PLUGINS=("${ALL_PLUGINS[@]}")
fi

total_plugins=0
passed_plugins=0
failed_plugins=0
uncaught_plugins=0

for plugin in "${PLUGINS[@]}"; do
    echo ""
    echo "=== Testing plugin: $plugin ==="
    total_plugins=$((total_plugins + 1))

    if [ ! -d "$plugin" ]; then
        echo "ERROR: Plugin directory $plugin does not exist"
        failed_plugins=$((failed_plugins + 1))
        continue
    fi

    # Extract package name from the plugin's Cargo.toml
    if [ -f "$plugin/Cargo.toml" ]; then
        package_name=$(grep "^name = " "$plugin/Cargo.toml" | sed 's/name = "\(.*\)"/\1/')
    else
        echo "ERROR: $plugin/Cargo.toml not found"
        failed_plugins=$((failed_plugins + 1))
        continue
    fi

    # Check if this is an uncaught validation test (no expected-error.txt)
    if [ ! -f "$plugin/expected-error.txt" ]; then
        echo "⚠️  UNCAUGHT VALIDATION TEST: This plugin tests a validation gap that is not currently caught by the SDK"
        echo "Running: cargo check -p $package_name"

        # For uncaught tests, we expect them to compile successfully
        if cargo check -p "$package_name" > /dev/null 2>&1; then
            echo -e "${GREEN}✓${NC} Plugin $plugin compiled successfully (expected - validation gap)"
            uncaught_plugins=$((uncaught_plugins + 1))
        else
            echo -e "${RED}⚠️${NC}  Plugin $plugin failed compilation - validation may have been implemented!"
            echo "This uncaught test should be moved to proper validation test with expected-error.txt"
            failed_plugins=$((failed_plugins + 1))
        fi
        continue
    fi

    expected_error=$(cat "$plugin/expected-error.txt" | tr -d '\n')
    echo "Expected error: $expected_error"

    echo "Running: cargo check -p $package_name"

    # Capture stderr from cargo check
    if actual_error=$(cargo check -p "$package_name" 2>&1); then
        echo -e "${RED}ERROR${NC}: Plugin $plugin compiled successfully, but it should have failed!"
        failed_plugins=$((failed_plugins + 1))
        continue
    fi

    # Check if the expected error is contained in the actual error output
    if echo "$actual_error" | grep -F "$expected_error" > /dev/null; then
        echo -e "${GREEN}✓${NC} Plugin $plugin failed with expected error"
        passed_plugins=$((passed_plugins + 1))
    else
        echo -e "${RED}✗${NC} Plugin $plugin failed with unexpected error:"
        echo "Actual error: $actual_error"
        echo ""
        echo "Expected error: $expected_error"
        failed_plugins=$((failed_plugins + 1))
    fi
done

echo
echo "=============================================="
echo "📊 Test Summary:"
echo "  Total plugins: $total_plugins"
echo -e "  ${GREEN}Passed: $passed_plugins${NC}"
echo -e "  ${YELLOW}Uncaught: $uncaught_plugins${NC}"
echo -e "  ${RED}Failed: $failed_plugins${NC}"

if [ $failed_plugins -eq 0 ]; then
    echo -e "${GREEN}✅ All tests passed!${NC}"
    if [ $uncaught_plugins -gt 0 ]; then
        echo "Note: $uncaught_plugins validation gaps were confirmed as still uncaught by the SDK"
    fi
    exit 0
else
    echo -e "${RED}❌ Some tests failed${NC}"
    exit 1
fi
