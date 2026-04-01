#!/usr/bin/env bash
# Non-blocking freshness check for context-pack updates.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
node "$ROOT/scripts/agent_context/check_freshness.cjs" "$@"
