#!/bin/bash

set -euo pipefail

# Use the generic package script
exec "$(dirname "$0")/../../scripts/package.sh" "$@"
