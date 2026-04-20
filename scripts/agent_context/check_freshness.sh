#!/usr/bin/env bash
# Non-blocking freshness check for agent-context updates (also serves the context-pack alias).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
node "$ROOT/scripts/agent_context/check_freshness.cjs" "$@"
