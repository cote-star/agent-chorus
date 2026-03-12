#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PASS=0
FAIL=0
TMP_DIR="$ROOT/.smoke-test-tmp"
rm -rf "$TMP_DIR"
mkdir -p "$TMP_DIR"
git init -q "$TMP_DIR"
trap 'rm -rf "$TMP_DIR"' EXIT

smoke() {
  local label="$1"; shift
  if "$@" > /dev/null 2>&1; then
    echo "PASS $label"
    PASS=$((PASS + 1))
  else
    echo "FAIL $label"
    FAIL=$((FAIL + 1))
  fi
}

smoke_fail() {
  local label="$1"; shift
  if "$@" > /dev/null 2>&1; then
    echo "FAIL $label (expected failure)"
    FAIL=$((FAIL + 1))
  else
    echo "PASS $label (expected failure)"
    PASS=$((PASS + 1))
  fi
}

echo "=== CLI Smoke Tests ==="

# Help
smoke "help"           node "$ROOT/scripts/read_session.cjs" --help
smoke "help-cp"        node "$ROOT/scripts/read_session.cjs" context-pack --help

# Doctor (the critical regression test)
smoke "doctor"         node "$ROOT/scripts/read_session.cjs" doctor --cwd "$TMP_DIR"
smoke "doctor-json"    node "$ROOT/scripts/read_session.cjs" doctor --cwd "$TMP_DIR" --json

# Setup
smoke "setup-dry"      node "$ROOT/scripts/read_session.cjs" setup --cwd "$TMP_DIR" --dry-run

# Context-pack init + seal cycle
smoke "cp-init"        node "$ROOT/scripts/read_session.cjs" context-pack init --cwd "$TMP_DIR" --force
smoke "cp-seal-force"  node "$ROOT/scripts/read_session.cjs" context-pack seal --cwd "$TMP_DIR" --force

# Context-pack build (legacy) — should succeed after init
smoke "cp-build"       node "$ROOT/scripts/read_session.cjs" context-pack build --cwd "$TMP_DIR" --force

# Doctor with context-pack present (exercises the fixed code path)
smoke "doctor-with-pack" node "$ROOT/scripts/read_session.cjs" doctor --cwd "$TMP_DIR" --json

echo ""
echo "=== Results: $PASS passed, $FAIL failed ==="
if [[ "$FAIL" -gt 0 ]]; then
  exit 1
fi
