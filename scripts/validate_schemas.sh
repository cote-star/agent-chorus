#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STORE="$ROOT/fixtures/session-store"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

read_node_json="$TMP_DIR/read-node.json"
read_rust_json="$TMP_DIR/read-rust.json"
report_node_json="$TMP_DIR/report-node.json"
report_rust_json="$TMP_DIR/report-rust.json"
compare_node_json="$TMP_DIR/compare-node.json"
compare_rust_json="$TMP_DIR/compare-rust.json"
list_node_json="$TMP_DIR/list-node.json"
list_rust_json="$TMP_DIR/list-rust.json"
search_node_json="$TMP_DIR/search-node.json"
search_rust_json="$TMP_DIR/search-rust.json"
error_node_json="$TMP_DIR/error-node.json"
error_rust_json="$TMP_DIR/error-rust.json"

CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
node "$ROOT/scripts/read_session.cjs" read --agent=codex --id=codex-fixture --json > "$read_node_json"

CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
cargo run --quiet --manifest-path "$ROOT/cli/Cargo.toml" -- read --agent codex --id codex-fixture --json > "$read_rust_json"

CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
node "$ROOT/scripts/read_session.cjs" report --handoff="$ROOT/fixtures/handoff-report.json" --json > "$report_node_json"

CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
cargo run --quiet --manifest-path "$ROOT/cli/Cargo.toml" -- report --handoff "$ROOT/fixtures/handoff-report.json" --json > "$report_rust_json"

CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
node "$ROOT/scripts/read_session.cjs" compare \
  --source=codex:codex-fixture \
  --source=gemini:gemini-fixture \
  --source=claude:claude-fixture \
  --json > "$compare_node_json"

CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
cargo run --quiet --manifest-path "$ROOT/cli/Cargo.toml" -- compare \
  --source codex:codex-fixture \
  --source gemini:gemini-fixture \
  --source claude:claude-fixture \
  --json > "$compare_rust_json"

CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
node "$ROOT/scripts/read_session.cjs" list --agent=codex --cwd=/workspace/demo --json > "$list_node_json"

CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
cargo run --quiet --manifest-path "$ROOT/cli/Cargo.toml" -- list --agent codex --cwd /workspace/demo --json > "$list_rust_json"

CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
node "$ROOT/scripts/read_session.cjs" search "Codex fixture assistant output." --agent=codex --cwd=/workspace/demo --json > "$search_node_json"

CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
cargo run --quiet --manifest-path "$ROOT/cli/Cargo.toml" -- search "Codex fixture assistant output." --agent codex --cwd /workspace/demo --json > "$search_rust_json"

if CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
  CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
  CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
  node "$ROOT/scripts/read_session.cjs" read --agent=invalid-agent --json > "$error_node_json" 2>/dev/null; then
  echo "Expected node invalid-agent call to fail" >&2
  exit 1
fi

if CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
  CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
  CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
  cargo run --quiet --manifest-path "$ROOT/cli/Cargo.toml" -- read --agent invalid-agent --json > "$error_rust_json" 2>/dev/null; then
  echo "Expected rust invalid-agent call to fail" >&2
  exit 1
fi

if [[ "${CHORUS_SKIP_AJV:-0}" == "1" ]]; then
  node -e "JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8'));" "$read_node_json"
  node -e "JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8'));" "$read_rust_json"
  node -e "JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8'));" "$report_node_json"
  node -e "JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8'));" "$report_rust_json"
  node -e "JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8'));" "$compare_node_json"
  node -e "JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8'));" "$compare_rust_json"
  node -e "JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8'));" "$list_node_json"
  node -e "JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8'));" "$list_rust_json"
  node -e "JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8'));" "$search_node_json"
  node -e "JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8'));" "$search_rust_json"
  node -e "JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8'));" "$error_node_json"
  node -e "JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8'));" "$error_rust_json"
  echo "Schema validation skipped (CHORUS_SKIP_AJV=1); JSON parse sanity checks passed."
  exit 0
fi

AJV_CMD=(node "$ROOT/scripts/validate_schemas_ajv.cjs")

"${AJV_CMD[@]}" "$ROOT/schemas/handoff.schema.json" "$ROOT/fixtures/handoff-report.json"

"${AJV_CMD[@]}" "$ROOT/schemas/read-output.schema.json" "$read_node_json"
"${AJV_CMD[@]}" "$ROOT/schemas/read-output.schema.json" "$read_rust_json"

"${AJV_CMD[@]}" "$ROOT/schemas/report.schema.json" "$report_node_json"
"${AJV_CMD[@]}" "$ROOT/schemas/report.schema.json" "$report_rust_json"
"${AJV_CMD[@]}" "$ROOT/schemas/report.schema.json" "$compare_node_json"
"${AJV_CMD[@]}" "$ROOT/schemas/report.schema.json" "$compare_rust_json"
"${AJV_CMD[@]}" "$ROOT/schemas/list-output.schema.json" "$list_node_json"
"${AJV_CMD[@]}" "$ROOT/schemas/list-output.schema.json" "$list_rust_json"
"${AJV_CMD[@]}" "$ROOT/schemas/list-output.schema.json" "$search_node_json"
"${AJV_CMD[@]}" "$ROOT/schemas/list-output.schema.json" "$search_rust_json"
"${AJV_CMD[@]}" "$ROOT/schemas/error.schema.json" "$error_node_json"
"${AJV_CMD[@]}" "$ROOT/schemas/error.schema.json" "$error_rust_json"

echo "Schema validation complete for handoff/read/report/list/search/error outputs."
