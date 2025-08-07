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

for plugin in $PLUGINS; do
    echo ""
    echo "=== Testing plugin: $plugin ==="
    TOTAL=$((TOTAL + 1))
    
    if [ ! -d "$plugin" ]; then
        echo "ERROR: Plugin directory $plugin does not exist"
        FAILED=$((FAILED + 1))
        continue
    fi
    
    if [ ! -f "$plugin/expected-error.txt" ]; then
        echo "ERROR: expected-error.txt not found for plugin $plugin"
        FAILED=$((FAILED + 1))
        continue
    fi
    
    expected_error=$(cat "$plugin/expected-error.txt" | tr -d '\n')
    echo "Expected error: $expected_error"
    
    echo "Running: cargo check -p $plugin"
    
    # Capture stderr from cargo check
    if actual_error=$(cargo check -p "$plugin" 2>&1); then
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
echo "Passed: $PASSED"
echo "Failed: $FAILED"

if [ $FAILED -eq 0 ]; then
    echo "All validation failure tests passed! ✓"
    exit 0
else
    echo "Some validation failure tests failed! ✗"
    exit 1
fi