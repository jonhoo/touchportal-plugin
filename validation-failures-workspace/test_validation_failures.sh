#!/bin/bash
set -e

# Script to test that all validation-failure plugins fail compilation with expected errors

echo "Testing validation failure plugins..."

WORKSPACE_DIR="$(dirname "$0")"
cd "$WORKSPACE_DIR"

# Get list of all member plugins from Cargo.toml
PLUGINS=$(grep -A 100 "members = \[" Cargo.toml | grep -E '^\s*"[^"]+",?' | sed 's/.*"\([^"]*\)".*/\1/' | sed '/^\s*$/d')

TOTAL=0
PASSED=0
FAILED=0
UNCAUGHT=0

for plugin in $PLUGINS; do
    echo ""
    echo "=== Testing plugin: $plugin ==="
    TOTAL=$((TOTAL + 1))

    if [ ! -d "$plugin" ]; then
        echo "ERROR: Plugin directory $plugin does not exist"
        FAILED=$((FAILED + 1))
        continue
    fi

    # Extract package name from the plugin's Cargo.toml
    if [ -f "$plugin/Cargo.toml" ]; then
        package_name=$(grep "^name = " "$plugin/Cargo.toml" | sed 's/name = "\(.*\)"/\1/')
    else
        echo "ERROR: $plugin/Cargo.toml not found"
        FAILED=$((FAILED + 1))
        continue
    fi

    # Check if this is an uncaught validation test (no expected-error.txt)
    if [ ! -f "$plugin/expected-error.txt" ]; then
        echo "⚠️  UNCAUGHT VALIDATION TEST: This plugin tests a validation gap that is not currently caught by the SDK"
        echo "Running: cargo check -p $package_name"

        # For uncaught tests, we expect them to compile successfully
        if cargo check -p "$package_name" 2>&1 > /dev/null; then
            echo "✓ Plugin $plugin compiled successfully (expected - validation gap)"
            UNCAUGHT=$((UNCAUGHT + 1))
        else
            echo "⚠️  Plugin $plugin failed compilation - validation may have been implemented!"
            echo "This uncaught test should be moved to proper validation test with expected-error.txt"
            FAILED=$((FAILED + 1))
        fi
        continue
    fi

    expected_error=$(cat "$plugin/expected-error.txt" | tr -d '\n')
    echo "Expected error: $expected_error"

    echo "Running: cargo check -p $package_name"

    # Capture stderr from cargo check
    if actual_error=$(cargo check -p "$package_name" 2>&1); then
        echo "ERROR: Plugin $plugin compiled successfully, but it should have failed!"
        FAILED=$((FAILED + 1))
        continue
    fi

    # Check if the expected error is contained in the actual error output
    if echo "$actual_error" | grep -F "$expected_error" > /dev/null; then
        echo "✓ Plugin $plugin failed with expected error"
        PASSED=$((PASSED + 1))
    else
        echo "✗ Plugin $plugin failed with unexpected error:"
        echo "Actual error: $actual_error"
        echo ""
        echo "Expected error: $expected_error"
        FAILED=$((FAILED + 1))
    fi
done

echo ""
echo "=== Summary ==="
echo "Total plugins tested: $TOTAL"
echo "Validation tests passed: $PASSED"
echo "Uncaught validation gaps: $UNCAUGHT"
echo "Failed tests: $FAILED"

if [ $FAILED -eq 0 ]; then
    echo ""
    echo "All tests completed successfully! ✓"
    if [ $UNCAUGHT -gt 0 ]; then
        echo "Note: $UNCAUGHT validation gaps were confirmed as still uncaught by the SDK"
    fi
    exit 0
else
    echo ""
    echo "Some tests failed! ✗"
    exit 1
fi
