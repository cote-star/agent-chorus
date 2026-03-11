#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STORE="$ROOT/fixtures/session-store"
GOLDEN="$ROOT/fixtures/golden"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

run_read_case() {
  local agent="$1"
  local session_id="$2"
  local label="$3"

  local node_out="$TMP_DIR/read-${agent}-node.json"
  local rust_out="$TMP_DIR/read-${agent}-rust.json"

  local node_cmd=(node "$ROOT/scripts/read_session.cjs" read "--agent=${agent}" "--id=${session_id}" --json)
  local rust_cmd=(cargo run --quiet --manifest-path "$ROOT/cli/Cargo.toml" -- read --agent "$agent" --id "$session_id" --json)

  if [[ "$agent" == "gemini" ]]; then
    node_cmd+=("--chats-dir=$STORE/gemini/tmp/demo/chats")
    rust_cmd+=(--chats-dir "$STORE/gemini/tmp/demo/chats")
  fi

  CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
  CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
  CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
  "${node_cmd[@]}" > "$node_out"

  CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
  CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
  CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
  "${rust_cmd[@]}" > "$rust_out"

  node "$ROOT/scripts/compare_read_output.cjs" "$node_out" "$rust_out" "read-${label}"

  # Golden file diff
  local golden_file="$GOLDEN/read-${agent}.json"
  if [[ -f "$golden_file" ]]; then
    node "$ROOT/scripts/compare_read_output.cjs" "$node_out" "$golden_file" "golden-read-${label}"
  fi
}

run_compare_case() {
  local node_out="$TMP_DIR/compare-node.json"
  local rust_out="$TMP_DIR/compare-rust.json"

  CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
  CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
  CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
  node "$ROOT/scripts/read_session.cjs" compare \
    --source=codex:codex-fixture \
    --source=gemini:gemini-fixture \
    --source=claude:claude-fixture \
    --json > "$node_out"

  CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
  CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
  CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
  cargo run --quiet --manifest-path "$ROOT/cli/Cargo.toml" -- compare \
    --source codex:codex-fixture \
    --source gemini:gemini-fixture \
    --source claude:claude-fixture \
    --json > "$rust_out"

  node "$ROOT/scripts/compare_read_output.cjs" "$node_out" "$rust_out" "compare"

  # Golden file diff
  if [[ -f "$GOLDEN/compare.json" ]]; then
    node "$ROOT/scripts/compare_read_output.cjs" "$node_out" "$GOLDEN/compare.json" "golden-compare"
  fi
}

run_report_case() {
  local handoff="$ROOT/fixtures/handoff-report.json"
  local node_out="$TMP_DIR/report-node.json"
  local rust_out="$TMP_DIR/report-rust.json"

  CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
  CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
  CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
  node "$ROOT/scripts/read_session.cjs" report --handoff="$handoff" --json > "$node_out"

  CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
  CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
  CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
  cargo run --quiet --manifest-path "$ROOT/cli/Cargo.toml" -- report --handoff "$handoff" --json > "$rust_out"

  node "$ROOT/scripts/compare_read_output.cjs" "$node_out" "$rust_out" "report"

  # Golden file diff
  if [[ -f "$GOLDEN/report.json" ]]; then
    node "$ROOT/scripts/compare_read_output.cjs" "$node_out" "$GOLDEN/report.json" "golden-report"
  fi
}

run_list_case() {
  local agent="$1"
  local label="$2"
  local cwd="$3"

  local node_out="$TMP_DIR/list-${agent}-node.json"
  local rust_out="$TMP_DIR/list-${agent}-rust.json"

  CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
  CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
  CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
  node "$ROOT/scripts/read_session.cjs" list --agent="$agent" --cwd="$cwd" --json > "$node_out"

  CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
  CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
  CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
  cargo run --quiet --manifest-path "$ROOT/cli/Cargo.toml" -- list --agent "$agent" --cwd "$cwd" --json > "$rust_out"

  node "$ROOT/scripts/compare_read_output.cjs" "$node_out" "$rust_out" "list-${label}"

  local golden_file="$GOLDEN/list-${agent}.json"
  if [[ -f "$golden_file" ]]; then
    node "$ROOT/scripts/compare_read_output.cjs" "$node_out" "$golden_file" "golden-list-${label}"
  fi
}

run_search_case() {
  local agent="$1"
  local label="$2"
  local query="$3"
  local cwd="$4"

  local node_out="$TMP_DIR/search-${agent}-node.json"
  local rust_out="$TMP_DIR/search-${agent}-rust.json"

  CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
  CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
  CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
  node "$ROOT/scripts/read_session.cjs" search "$query" --agent="$agent" --cwd="$cwd" --json > "$node_out"

  CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
  CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
  CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
  cargo run --quiet --manifest-path "$ROOT/cli/Cargo.toml" -- search "$query" --agent "$agent" --cwd "$cwd" --json > "$rust_out"

  node "$ROOT/scripts/compare_read_output.cjs" "$node_out" "$rust_out" "search-${label}"

  local golden_file="$GOLDEN/search-${agent}.json"
  if [[ -f "$golden_file" ]]; then
    node "$ROOT/scripts/compare_read_output.cjs" "$node_out" "$golden_file" "golden-search-${label}"
  fi
}

run_read_case codex codex-fixture Codex
run_read_case gemini gemini-fixture Gemini
run_read_case claude claude-fixture Claude
run_compare_case
run_report_case
run_list_case codex Codex /workspace/demo
run_search_case codex Codex "Codex fixture assistant output." /workspace/demo

echo "Conformance complete: Node and Rust outputs match for read/compare/report/list/search (including golden file diffs)."
