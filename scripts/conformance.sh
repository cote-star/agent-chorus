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

# -----------------------------------------------------------------------------
# v0.13 parity tests (summary / timeline / doctor / setup / read-flags).
#
# These subcommands either were Node-only or had flags that Node supported but
# Rust did not before v0.13. The tests here gate against silent regressions.
#
# NOTE: `--format json` is intentionally NOT tested for Rust-vs-Node parity.
# Node has a known bug (scripts/read_session.cjs ~line 1759) where
# `--format json` falls through to the text renderer and emits TEXT instead of
# valid JSON. Rust treats `--format json` as an alias for `--json` (correct).
# Until the Node bug is fixed, `--format json` parity is excluded.
#
# All parity-sensitive output is piped through scripts/scrub_parity_output.cjs
# which normalizes known-drift fields (sub-second timestamps, absolute paths,
# version/update strings, timeline insertion order) before diff.
# -----------------------------------------------------------------------------

SCRUB="$ROOT/scripts/scrub_parity_output.cjs"

# Helper: run one parity test. Captures both Node and Rust outputs, scrubs,
# diffs them against each other AND against the golden fixture if present.
run_parity_case() {
  local kind="$1"      # summary|timeline|doctor|setup|read
  local label="$2"     # human-readable label for PASS/FAIL print
  local golden="$3"    # filename under fixtures/golden (empty string to skip)
  shift 3
  # Remaining args split by "::" separator into node_cmd (left) and rust_cmd (right)
  local sep_idx=0
  local args=("$@")
  for ((i=0; i<${#args[@]}; i++)); do
    if [[ "${args[$i]}" == "::" ]]; then
      sep_idx=$i
      break
    fi
  done
  if [[ $sep_idx -eq 0 ]]; then
    echo "run_parity_case missing :: separator between node and rust args" >&2
    exit 1
  fi

  local node_args=("${args[@]:0:$sep_idx}")
  local rust_args=("${args[@]:$((sep_idx + 1))}")

  local node_out="$TMP_DIR/parity-${label}-node.json"
  local rust_out="$TMP_DIR/parity-${label}-rust.json"
  local node_scrubbed="$TMP_DIR/parity-${label}-node.scrubbed.json"
  local rust_scrubbed="$TMP_DIR/parity-${label}-rust.scrubbed.json"

  CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
  CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
  CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
  node "$ROOT/scripts/read_session.cjs" "${node_args[@]}" > "$node_out"

  CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
  CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
  CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
  cargo run --quiet --manifest-path "$ROOT/cli/Cargo.toml" -- "${rust_args[@]}" > "$rust_out"

  node "$SCRUB" "$node_out" "$node_scrubbed" "$kind"
  node "$SCRUB" "$rust_out" "$rust_scrubbed" "$kind"

  if ! diff -u "$node_scrubbed" "$rust_scrubbed" >/dev/null; then
    echo "FAIL parity-${label}: Node vs Rust mismatch after scrub"
    diff -u "$node_scrubbed" "$rust_scrubbed" || true
    exit 1
  fi
  echo "PASS parity-${label} (Node=Rust)"

  if [[ -n "$golden" && -f "$GOLDEN/$golden" ]]; then
    if ! diff -u "$GOLDEN/$golden" "$rust_scrubbed" >/dev/null; then
      echo "FAIL golden-${label}: Rust output drifted from $golden"
      diff -u "$GOLDEN/$golden" "$rust_scrubbed" || true
      exit 1
    fi
    echo "PASS golden-${label} (Rust=$golden)"
  fi
}

# Setup for doctor/setup tests: deterministic empty tempdirs so checks return
# predictable warn states. Must NOT overlap with $TMP_DIR itself because the
# scrubber needs the original tempdir path to strip it out.
DOCTOR_TMP_CWD="$(mktemp -d)"
SETUP_TMP_CWD="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR" "$DOCTOR_TMP_CWD" "$SETUP_TMP_CWD"' EXIT

# --- summary parity (codex, claude, gemini; cursor skipped — see fixture note) ---
# --cwd is pinned to /workspace/demo so the gemini golden (which has no cwd
# embedded in the session file) isn't machine-specific.
run_parity_case summary summary-codex summary-codex.json \
  summary --agent=codex --id=codex-fixture --cwd=/workspace/demo --json :: \
  summary --agent codex --id codex-fixture --cwd /workspace/demo --json

run_parity_case summary summary-claude summary-claude.json \
  summary --agent=claude --id=claude-fixture --cwd=/workspace/demo --json :: \
  summary --agent claude --id claude-fixture --cwd /workspace/demo --json

run_parity_case summary summary-gemini summary-gemini.json \
  summary --agent=gemini --id=gemini-fixture --cwd=/workspace/demo --json :: \
  summary --agent gemini --id gemini-fixture --cwd /workspace/demo --json

# --- timeline parity (schema-shape, entries sorted by agent+session_id) ---
run_parity_case timeline timeline timeline.json \
  timeline --cwd=/workspace/demo --limit 6 --json :: \
  timeline --cwd /workspace/demo --limit 6 --json

# --- doctor parity (empty-tempdir warn states, scrubbed paths) ---
run_parity_case doctor doctor doctor.json \
  doctor --cwd="$DOCTOR_TMP_CWD" --json :: \
  doctor --cwd "$DOCTOR_TMP_CWD" --json

# --- setup dry-run parity ---
run_parity_case setup setup setup.json \
  setup --cwd="$SETUP_TMP_CWD" --dry-run --json :: \
  setup --cwd "$SETUP_TMP_CWD" --dry-run --json

# --- read --include-user parity (codex, claude, gemini) ---
# Gemini and Cursor --tool-calls are no-ops (no tool-call schema); only
# --include-user is meaningful for gemini here.
run_parity_case read read-codex-include-user read-codex-include-user.json \
  read --agent=codex --id=codex-fixture --include-user --json :: \
  read --agent codex --id codex-fixture --include-user --json

run_parity_case read read-claude-include-user read-claude-include-user.json \
  read --agent=claude --id=claude-fixture --include-user --json :: \
  read --agent claude --id claude-fixture --include-user --json

run_parity_case read read-gemini-include-user read-gemini-include-user.json \
  read --agent=gemini --id=gemini-fixture --include-user --json :: \
  read --agent gemini --id gemini-fixture --include-user --json

# --- read --tool-calls parity (codex, claude) ---
run_parity_case read read-codex-tool-calls read-codex-tool-calls.json \
  read --agent=codex --id=codex-fixture --tool-calls --json :: \
  read --agent codex --id codex-fixture --tool-calls --json

run_parity_case read read-claude-tool-calls read-claude-tool-calls.json \
  read --agent=claude --id=claude-fixture --tool-calls --json :: \
  read --agent claude --id claude-fixture --tool-calls --json

run_read_case codex codex-fixture Codex
run_read_case gemini gemini-fixture Gemini
run_read_case claude claude-fixture Claude
run_compare_case
run_report_case
run_list_case codex Codex /workspace/demo
run_search_case codex Codex "Codex fixture assistant output." /workspace/demo

# --- Gemini list parity: proves .jsonl files are indexed and cwd is not null ---
#
# The Gemini list path had two pre-existing bugs fixed in v0.14.0:
#   1. filter only matched *.json — .jsonl sessions (newer Gemini CLI) were
#      silently excluded.
#   2. cwd was hardcoded null in listings.
# This case scans the fixture store's tmp base (no --cwd scope) and asserts
# that Node and Rust both emit the same shape, that the .jsonl fixture is
# present, and that the named scope `demo` bubbles up as the cwd hint.
run_gemini_list_case() {
  local node_out="$TMP_DIR/list-gemini-node.json"
  local rust_out="$TMP_DIR/list-gemini-rust.json"

  CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
  CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
  CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
  node "$ROOT/scripts/read_session.cjs" list --agent=gemini --limit=20 --json > "$node_out"

  CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
  CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
  CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
  cargo run --quiet --manifest-path "$ROOT/cli/Cargo.toml" -- list --agent gemini --limit 20 --json > "$rust_out"

  # Node-vs-Rust shape equivalence.
  node "$ROOT/scripts/compare_read_output.cjs" "$node_out" "$rust_out" "list-gemini"

  # Behavior assertions: .jsonl indexed, cwd inferred from named scope.
  if ! node -e "
    const rows = JSON.parse(require('fs').readFileSync(process.argv[1], 'utf-8'));
    const jsonl = rows.find(r => r.file_path && r.file_path.endsWith('.jsonl'));
    if (!jsonl) { console.error('FAIL list-gemini-jsonl: no .jsonl session indexed'); process.exit(1); }
    if (jsonl.cwd !== 'demo') { console.error('FAIL list-gemini-cwd: .jsonl cwd should be demo, got ' + JSON.stringify(jsonl.cwd)); process.exit(1); }
    const json = rows.find(r => r.file_path && r.file_path.endsWith('.json') && !r.file_path.endsWith('.jsonl'));
    if (!json) { console.error('FAIL list-gemini-json: no .json session indexed'); process.exit(1); }
    if (json.cwd !== 'demo') { console.error('FAIL list-gemini-cwd-json: .json cwd should be demo, got ' + JSON.stringify(json.cwd)); process.exit(1); }
    console.log('PASS list-gemini-assertions (.jsonl indexed, cwd=demo)');
  " "$rust_out"; then
    exit 1
  fi
}

run_gemini_list_case

# --- Gemini .jsonl read parity: proves the line-delimited parser works ---
#
# Newer Gemini CLI writes sessions as .jsonl. v0.14.0 indexes them in list
# but read() originally rejected .jsonl (single-document parser only).
# This case reads the .jsonl fixture and checks Rust/Node byte-identical
# output across the default path, --include-user, and --last 2.
run_gemini_jsonl_read_case() {
  local node_out="$TMP_DIR/read-gemini-jsonl-node.json"
  local rust_out="$TMP_DIR/read-gemini-jsonl-rust.json"

  CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
  CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
  CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
  node "$ROOT/scripts/read_session.cjs" read \
    --agent=gemini --id=gemini-jsonl-fixture \
    --chats-dir="$STORE/gemini/tmp/demo/chats" --json > "$node_out"

  CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
  CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
  CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
  cargo run --quiet --manifest-path "$ROOT/cli/Cargo.toml" -- read \
    --agent gemini --id gemini-jsonl-fixture \
    --chats-dir "$STORE/gemini/tmp/demo/chats" --json > "$rust_out"

  node "$ROOT/scripts/compare_read_output.cjs" "$node_out" "$rust_out" "read-gemini-jsonl"

  # Behavior assertion: reached the .jsonl parser (not json) and returned the
  # expected assistant text plus non-null session id.
  if ! node -e "
    const r = JSON.parse(require('fs').readFileSync(process.argv[1], 'utf-8'));
    if (!r.source || !r.source.endsWith('.jsonl')) { console.error('FAIL read-gemini-jsonl: source is not .jsonl, got ' + r.source); process.exit(1); }
    if (r.session_id !== 'gemini-jsonl-fixture') { console.error('FAIL read-gemini-jsonl-session-id: ' + r.session_id); process.exit(1); }
    if (r.message_count !== 2) { console.error('FAIL read-gemini-jsonl-count (expected 2 after dedupe): ' + r.message_count); process.exit(1); }
    if (!String(r.content).includes('Second jsonl assistant answer')) { console.error('FAIL read-gemini-jsonl-content: ' + r.content); process.exit(1); }
    console.log('PASS read-gemini-jsonl-assertions');
  " "$rust_out"; then
    exit 1
  fi

  # --include-user parity
  local node_iu="$TMP_DIR/read-gemini-jsonl-iu-node.json"
  local rust_iu="$TMP_DIR/read-gemini-jsonl-iu-rust.json"

  CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
  CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
  CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
  node "$ROOT/scripts/read_session.cjs" read \
    --agent=gemini --id=gemini-jsonl-fixture --include-user \
    --chats-dir="$STORE/gemini/tmp/demo/chats" --json > "$node_iu"

  CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
  CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
  CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
  cargo run --quiet --manifest-path "$ROOT/cli/Cargo.toml" -- read \
    --agent gemini --id gemini-jsonl-fixture --include-user \
    --chats-dir "$STORE/gemini/tmp/demo/chats" --json > "$rust_iu"

  node "$ROOT/scripts/compare_read_output.cjs" "$node_iu" "$rust_iu" "read-gemini-jsonl-include-user"

  # --last 2 parity
  local node_l2="$TMP_DIR/read-gemini-jsonl-last2-node.json"
  local rust_l2="$TMP_DIR/read-gemini-jsonl-last2-rust.json"

  CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
  CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
  CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
  node "$ROOT/scripts/read_session.cjs" read \
    --agent=gemini --id=gemini-jsonl-fixture --last=2 \
    --chats-dir="$STORE/gemini/tmp/demo/chats" --json > "$node_l2"

  CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions" \
  CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp" \
  CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects" \
  cargo run --quiet --manifest-path "$ROOT/cli/Cargo.toml" -- read \
    --agent gemini --id gemini-jsonl-fixture --last 2 \
    --chats-dir "$STORE/gemini/tmp/demo/chats" --json > "$rust_l2"

  node "$ROOT/scripts/compare_read_output.cjs" "$node_l2" "$rust_l2" "read-gemini-jsonl-last2"
}

run_gemini_jsonl_read_case

echo "Conformance complete: Node and Rust outputs match for read/compare/report/list/search, plus v0.13 summary/timeline/doctor/setup/read-flags (including golden file diffs)."
