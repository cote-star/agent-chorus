#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STORE="$ROOT/fixtures/session-store"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

read_node_json="$TMP_DIR/read-node.json"
read_rust_json="$TMP_DIR/read-rust.json"
compare_node_json="$TMP_DIR/compare-node.json"
compare_rust_json="$TMP_DIR/compare-rust.json"
report_node_json="$TMP_DIR/report-node.json"
report_rust_json="$TMP_DIR/report-rust.json"
list_node_json="$TMP_DIR/list-node.json"
list_rust_json="$TMP_DIR/list-rust.json"
search_node_json="$TMP_DIR/search-node.json"
search_rust_json="$TMP_DIR/search-rust.json"

CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
node "$ROOT/scripts/read_session.cjs" read --agent=codex --cwd=/workspace/demo --json > "$read_node_json"

CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
cargo run --quiet --manifest-path "$ROOT/cli/Cargo.toml" -- read --agent codex --cwd /workspace/demo --json > "$read_rust_json"

CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
node "$ROOT/scripts/read_session.cjs" compare \
  --source=codex \
  --source=gemini \
  --source=claude \
  --cwd=/workspace/demo \
  --json > "$compare_node_json"

CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
cargo run --quiet --manifest-path "$ROOT/cli/Cargo.toml" -- compare \
  --source codex \
  --source gemini \
  --source claude \
  --cwd /workspace/demo \
  --json > "$compare_rust_json"

CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
node "$ROOT/scripts/read_session.cjs" report \
  --handoff="$ROOT/fixtures/handoff-report.json" \
  --json > "$report_node_json"

CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
cargo run --quiet --manifest-path "$ROOT/cli/Cargo.toml" -- report \
  --handoff "$ROOT/fixtures/handoff-report.json" \
  --json > "$report_rust_json"

CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
node "$ROOT/scripts/read_session.cjs" list \
  --agent=codex \
  --cwd=/workspace/demo \
  --json > "$list_node_json"

CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
cargo run --quiet --manifest-path "$ROOT/cli/Cargo.toml" -- list \
  --agent codex \
  --cwd /workspace/demo \
  --json > "$list_rust_json"

CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
node "$ROOT/scripts/read_session.cjs" search "Codex fixture assistant output." \
  --agent=codex \
  --cwd=/workspace/demo \
  --json > "$search_node_json"

CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
cargo run --quiet --manifest-path "$ROOT/cli/Cargo.toml" -- search "Codex fixture assistant output." \
  --agent codex \
  --cwd /workspace/demo \
  --json > "$search_rust_json"

node -e "JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8'));" "$read_node_json"
node -e "JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8'));" "$read_rust_json"
node -e "JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8'));" "$compare_node_json"
node -e "JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8'));" "$compare_rust_json"
node -e "JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8'));" "$report_node_json"
node -e "JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8'));" "$report_rust_json"
node -e "JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8'));" "$list_node_json"
node -e "JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8'));" "$list_rust_json"
node -e "JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8'));" "$search_node_json"
node -e "JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8'));" "$search_rust_json"

node "$ROOT/scripts/compare_read_output.cjs" "$read_node_json" "$read_rust_json" "readme-read"
node "$ROOT/scripts/compare_read_output.cjs" "$compare_node_json" "$compare_rust_json" "readme-compare"
node "$ROOT/scripts/compare_read_output.cjs" "$report_node_json" "$report_rust_json" "readme-report"
node "$ROOT/scripts/compare_read_output.cjs" "$list_node_json" "$list_rust_json" "readme-list"
node "$ROOT/scripts/compare_read_output.cjs" "$search_node_json" "$search_rust_json" "readme-search"

echo "README command checks complete."
