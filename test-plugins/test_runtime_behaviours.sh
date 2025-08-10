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

# Usage function
show_usage() {
    echo "Usage: $0 [plugin-name...]"
    echo ""
    echo "Test TouchPortal plugin runtime behaviors."
    echo ""
    echo "Options:"
    echo "  [plugin-name...]  Run tests only for specified plugins"
    echo "  -h, --help        Show this help message"
    echo ""
    echo "Examples:"
    echo "  $0                      # Test all plugins"
    echo "  $0 minimal-single       # Test only minimal-single plugin"
    echo "  $0 all-data-types no-events  # Test multiple specific plugins"
    echo ""
    echo "Available plugins:"
    for plugin_dir in */; do
        if [[ -d "$plugin_dir" ]]; then
            plugin_name=$(basename "$plugin_dir")
            echo "  - $plugin_name"
        fi
    done
}

# Check for help flag
if [[ "${1:-}" == "-h" ]] || [[ "${1:-}" == "--help" ]]; then
    show_usage
    exit 0
fi

# Store requested plugins
requested_plugins=("$@")

# Validate requested plugins if any were provided
if [[ ${#requested_plugins[@]} -gt 0 ]]; then
    invalid_plugins=()
    for plugin in "${requested_plugins[@]}"; do
        if [[ ! -d "$plugin" ]] || [[ ! -f "$plugin/Cargo.toml" ]]; then
            invalid_plugins+=("$plugin")
        fi
    done

    if [[ ${#invalid_plugins[@]} -gt 0 ]]; then
        echo -e "${RED}Error: Invalid plugin names: ${invalid_plugins[*]}${NC}"
        echo ""
        show_usage
        exit 1
    fi
fi

echo "🧪 TouchPortal Plugin Runtime Behavior Tests"
echo "=================================="

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

# Determine which plugins to test
if [[ ${#requested_plugins[@]} -gt 0 ]]; then
    # Test only requested plugins
    for plugin_name in "${requested_plugins[@]}"; do
        plugin_dir="$plugin_name/"
        run_plugin_test "$plugin_dir"
    done
else
    # Find all test plugin directories
    for plugin_dir in */; do
        if [[ ! -d "$plugin_dir" ]]; then
            continue
        fi

        run_plugin_test "$plugin_dir"
    done
fi

echo
echo "=================================="
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
