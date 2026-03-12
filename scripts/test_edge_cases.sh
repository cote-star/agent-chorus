#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STORE="$ROOT/fixtures/session-store"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT
PASS=0
FAIL=0

ENV_VARS=(
  CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions"
  CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp"
  CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects"
)

run_node() {
  env "${ENV_VARS[@]}" node "$ROOT/scripts/read_session.cjs" "$@"
}

run_rust() {
  env "${ENV_VARS[@]}" cargo run --quiet --manifest-path "$ROOT/cli/Cargo.toml" -- "$@"
}

expect_success() {
  local label="$1"; shift
  local node_out="$TMP_DIR/${label}-node.json"
  local rust_out="$TMP_DIR/${label}-rust.json"

  if run_node "$@" > "$node_out" 2>/dev/null; then
    :
  else
    echo "FAIL $label (node exited non-zero)"
    FAIL=$((FAIL + 1))
    return
  fi

  if run_rust "$@" > "$rust_out" 2>/dev/null; then
    :
  else
    echo "FAIL $label (rust exited non-zero)"
    FAIL=$((FAIL + 1))
    return
  fi

  if node "$ROOT/scripts/compare_read_output.cjs" "$node_out" "$rust_out" "$label" > /dev/null 2>&1; then
    echo "PASS $label"
    PASS=$((PASS + 1))
  else
    echo "FAIL $label (parity mismatch)"
    FAIL=$((FAIL + 1))
  fi
}

expect_error() {
  local label="$1"
  local expected_code="$2"
  shift 2
  local node_out="$TMP_DIR/${label}-node-err.json"
  local rust_out="$TMP_DIR/${label}-rust-err.json"

  # Node
  if run_node "$@" --json > "$node_out" 2>/dev/null; then
    echo "FAIL $label (node should have failed)"
    FAIL=$((FAIL + 1))
    return
  fi

  # Rust
  if run_rust "$@" --json > "$rust_out" 2>/dev/null; then
    echo "FAIL $label (rust should have failed)"
    FAIL=$((FAIL + 1))
    return
  fi

  # Check error code
  local node_code rust_code
  node_code=$(node -e "console.log(JSON.parse(require('fs').readFileSync('$node_out','utf8')).error_code || 'MISSING')" 2>/dev/null || echo "PARSE_ERROR")
  rust_code=$(node -e "console.log(JSON.parse(require('fs').readFileSync('$rust_out','utf8')).error_code || 'MISSING')" 2>/dev/null || echo "PARSE_ERROR")

  if [[ "$node_code" == "$expected_code" && "$rust_code" == "$expected_code" ]]; then
    echo "PASS $label (error_code=$expected_code)"
    PASS=$((PASS + 1))
  else
    echo "FAIL $label (expected=$expected_code, node=$node_code, rust=$rust_code)"
    FAIL=$((FAIL + 1))
  fi
}

echo "=== Edge-case tests ==="

# Malformed JSONL: should succeed with warnings about skipped lines
expect_success "codex-malformed" read --agent codex --id codex-malformed --json

# No CWD: should succeed
expect_success "codex-no-cwd" read --agent codex --id codex-no-cwd --json

# Mixed schema: should succeed
expect_success "codex-mixed-schema" read --agent codex --id codex-mixed-schema --json

# Gemini history format: should succeed
expect_success "gemini-history" read --agent gemini --id gemini-history-format --chats-dir "$STORE/gemini/tmp/demo/chats" --json

# Claude redaction stress: should succeed and redact all secrets
expect_success "claude-redaction-stress" read --agent claude --id claude-redaction-stress --json

# Claude no assistant: should succeed (fallback to raw lines)
expect_success "claude-no-assistant" read --agent claude --id claude-no-assistant --json

# Multi-message with --last
expect_success "codex-multi-last2" read --agent codex --id codex-multi --last 2 --json

# Unsupported agent: should fail with UNSUPPORTED_AGENT
expect_error "unsupported-agent" "UNSUPPORTED_AGENT" read --agent foobar

# Not found: should fail with NOT_FOUND
expect_error "not-found" "NOT_FOUND" read --agent codex --id nonexistent-session-xyz

echo ""
echo "=== Results: $PASS passed, $FAIL failed ==="
if [[ "$FAIL" -gt 0 ]]; then
  exit 1
fi
