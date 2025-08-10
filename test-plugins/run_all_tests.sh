#!/bin/bash

set -euo pipefail

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# Counters
total_plugins=0
tested_plugins=0
skipped_plugins=0
failed_plugins=0

echo "🧪 Running TouchPortal Plugin Test Suite"
echo "========================================"

# Function to run a single plugin test
run_plugin_test() {
    local plugin_dir="$1"
    local plugin_name
    plugin_name=$(basename "$plugin_dir")
    
    total_plugins=$((total_plugins + 1))
    
    echo -n "Testing $plugin_name... "
    
    # Check if plugin has mock support by looking for mock server usage in main.rs
    if grep -q "MockTouchPortalServer" "$plugin_dir/src/main.rs" 2>/dev/null; then
        # Plugin has mock support, run the test
        cd "$plugin_dir"
        
        # Set timeout to prevent hanging tests - timeout is a failure as plugins should exit gracefully
        if timeout 30s cargo run --quiet 2>/dev/null; then
            echo -e "${GREEN}PASSED${NC}"
            tested_plugins=$((tested_plugins + 1))
        else
            # Check if it was a timeout
            if [ $? -eq 124 ]; then
                echo -e "${RED}FAILED${NC} (timed out - plugin should exit gracefully)"
                failed_plugins=$((failed_plugins + 1))
            else
                echo -e "${RED}FAILED${NC}"
                failed_plugins=$((failed_plugins + 1))
            fi
        fi
        cd - > /dev/null
    else
        # Plugin doesn't have mock support yet
        echo -e "${YELLOW}SKIPPED${NC} (no mock support)"
        skipped_plugins=$((skipped_plugins + 1))
    fi
}

# Find all test plugin directories
for plugin_dir in */; do
    # Skip if not a directory or doesn't have Cargo.toml
    if [[ ! -d "$plugin_dir" ]] || [[ ! -f "$plugin_dir/Cargo.toml" ]]; then
        continue
    fi
    
    # Skip the stress plugin as requested
    if [[ "$plugin_dir" == "stress/" ]]; then
        echo "Skipping stress plugin (too complex for basic testing)"
        continue
    fi
    
    run_plugin_test "$plugin_dir"
done

echo
echo "========================================"
echo "📊 Test Summary:"
echo "  Total plugins: $total_plugins"
echo -e "  ${GREEN}Tested: $tested_plugins${NC}"
echo -e "  ${YELLOW}Skipped: $skipped_plugins${NC}"
echo -e "  ${RED}Failed: $failed_plugins${NC}"

if [ $failed_plugins -eq 0 ]; then
    echo -e "${GREEN}✅ All tests passed!${NC}"
    exit 0
else
    echo -e "${RED}❌ Some tests failed${NC}"
    exit 1
fi
