#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
FIXTURES="$ROOT_DIR/fixtures"
GOLDEN="$FIXTURES/golden"
EMPTY_DIR="$(mktemp -d)"

PASS=0
FAIL=0

compare() {
  local label="$1" actual="$2" golden="$3"
  if diff -u "$golden" "$actual" > /dev/null 2>&1; then
    echo "PASS  $label"
    PASS=$((PASS + 1))
  else
    echo "FAIL  $label"
    diff -u "$golden" "$actual" || true
    FAIL=$((FAIL + 1))
  fi
}

# --- Scenario 1: No agents ---
echo "=== Scenario: no agents ==="

CHORUS_CODEX_SESSIONS_DIR="$EMPTY_DIR" \
CHORUS_CLAUDE_PROJECTS_DIR="$EMPTY_DIR" \
CHORUS_GEMINI_TMP_DIR="$EMPTY_DIR" \
CHORUS_CURSOR_DATA_DIR="$EMPTY_DIR" \
  node "$ROOT_DIR/scripts/read_session.cjs" trash-talk --cwd /tmp/nowhere \
  > "$EMPTY_DIR/node-none.txt" 2>&1
compare "node  trash-talk-none" "$EMPTY_DIR/node-none.txt" "$GOLDEN/trash-talk-none.txt"

CHORUS_CODEX_SESSIONS_DIR="$EMPTY_DIR" \
CHORUS_CLAUDE_PROJECTS_DIR="$EMPTY_DIR" \
CHORUS_GEMINI_TMP_DIR="$EMPTY_DIR" \
CHORUS_CURSOR_DATA_DIR="$EMPTY_DIR" \
  cargo run --quiet --manifest-path "$ROOT_DIR/cli/Cargo.toml" -- trash-talk --cwd /tmp/nowhere \
  > "$EMPTY_DIR/rust-none.txt" 2>&1
compare "rust  trash-talk-none" "$EMPTY_DIR/rust-none.txt" "$GOLDEN/trash-talk-none.txt"

# --- Scenario 2: Single agent (Codex only) ---
echo "=== Scenario: single agent ==="

CHORUS_CODEX_SESSIONS_DIR="$FIXTURES/session-store/codex/sessions" \
CHORUS_CLAUDE_PROJECTS_DIR="$EMPTY_DIR" \
CHORUS_GEMINI_TMP_DIR="$EMPTY_DIR" \
CHORUS_CURSOR_DATA_DIR="$EMPTY_DIR" \
  node "$ROOT_DIR/scripts/read_session.cjs" trash-talk --cwd /workspace/demo \
  > "$EMPTY_DIR/node-single.txt" 2>&1
compare "node  trash-talk-single" "$EMPTY_DIR/node-single.txt" "$GOLDEN/trash-talk-single.txt"

CHORUS_CODEX_SESSIONS_DIR="$FIXTURES/session-store/codex/sessions" \
CHORUS_CLAUDE_PROJECTS_DIR="$EMPTY_DIR" \
CHORUS_GEMINI_TMP_DIR="$EMPTY_DIR" \
CHORUS_CURSOR_DATA_DIR="$EMPTY_DIR" \
  cargo run --quiet --manifest-path "$ROOT_DIR/cli/Cargo.toml" -- trash-talk --cwd /workspace/demo \
  > "$EMPTY_DIR/rust-single.txt" 2>&1
compare "rust  trash-talk-single" "$EMPTY_DIR/rust-single.txt" "$GOLDEN/trash-talk-single.txt"

# --- Scenario 3: Multiple agents (Codex + Claude) ---
echo "=== Scenario: multi agent ==="

CHORUS_CODEX_SESSIONS_DIR="$FIXTURES/session-store/codex/sessions" \
CHORUS_CLAUDE_PROJECTS_DIR="$FIXTURES/session-store/claude/projects" \
CHORUS_GEMINI_TMP_DIR="$FIXTURES/session-store/gemini/tmp" \
CHORUS_CURSOR_DATA_DIR="$EMPTY_DIR" \
  node "$ROOT_DIR/scripts/read_session.cjs" trash-talk --cwd /workspace/demo \
  > "$EMPTY_DIR/node-multi.txt" 2>&1
compare "node  trash-talk-multi" "$EMPTY_DIR/node-multi.txt" "$GOLDEN/trash-talk-multi.txt"

CHORUS_CODEX_SESSIONS_DIR="$FIXTURES/session-store/codex/sessions" \
CHORUS_CLAUDE_PROJECTS_DIR="$FIXTURES/session-store/claude/projects" \
CHORUS_GEMINI_TMP_DIR="$FIXTURES/session-store/gemini/tmp" \
CHORUS_CURSOR_DATA_DIR="$EMPTY_DIR" \
  cargo run --quiet --manifest-path "$ROOT_DIR/cli/Cargo.toml" -- trash-talk --cwd /workspace/demo \
  > "$EMPTY_DIR/rust-multi.txt" 2>&1
compare "rust  trash-talk-multi" "$EMPTY_DIR/rust-multi.txt" "$GOLDEN/trash-talk-multi.txt"

# --- Summary ---
rm -rf "$EMPTY_DIR"
echo ""
echo "Results: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ]
