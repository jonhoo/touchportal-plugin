#!/bin/bash

set -euo pipefail

# Use the generic install script
exec "$(dirname "$0")/../../scripts/install.sh" "$@"