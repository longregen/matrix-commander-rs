#!/usr/bin/env bash
# Combined test runner: CLI sandbox tests + browser integration tests.
# Run inside the sandbox:
#   nix run .#sandbox -- --run bash tests/test-all.sh
set -euo pipefail

PROJECT_ROOT="${PROJECT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"

echo "=== Phase 1: CLI sandbox tests ==="
SKIP_CLEANUP=1 bash "$PROJECT_ROOT/tests/test-sandbox.sh"

echo ""
echo "=== Phase 2: Browser integration tests ==="
mkdir -p "${HOME:-/tmp/home}"
python "$PROJECT_ROOT/tests/test-browser.py"
