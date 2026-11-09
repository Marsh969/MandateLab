#!/usr/bin/env bash
# Wrapper used via NEAR_SANDBOX_BIN_PATH in containers whose hard open-file
# limit is < 65535. Usage: see scripts/sandbox/README.md
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
: "${NEAR_SANDBOX_REAL_BIN:?set NEAR_SANDBOX_REAL_BIN to the downloaded near-sandbox binary}"
exec env LD_PRELOAD="$here/build/libnofile_clamp.so${LD_PRELOAD:+:$LD_PRELOAD}" "$NEAR_SANDBOX_REAL_BIN" "$@"
