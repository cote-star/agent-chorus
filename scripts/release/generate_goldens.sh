#!/usr/bin/env bash
# Regenerate the golden fixtures under fixtures/golden/ for v0.13 parity tests.
#
# Goldens are produced from the Rust binary (cli/target/debug/chorus) using the
# deterministic fixture session-store at fixtures/session-store/, then scrubbed
# by scripts/scrub_parity_output.cjs to remove machine-specific values
# (timestamps with sub-second precision, absolute paths, version strings).
#
# Usage:
#   bash scripts/release/generate_goldens.sh
#
# Re-run whenever the fixture session-store or the CLI output schema changes.
# Pinned session IDs are listed below; any drift in those IDs needs this
# script updated before re-running.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
STORE="$ROOT/fixtures/session-store"
GOLDEN="$ROOT/fixtures/golden"
SCRUB="$ROOT/scripts/scrub_parity_output.cjs"
BIN="$ROOT/cli/target/debug/chorus"

if [[ ! -x "$BIN" ]]; then
  echo "Building debug binary..." >&2
  (cd "$ROOT/cli" && cargo build --quiet)
fi

export CHORUS_CODEX_SESSIONS_DIR="$STORE/codex/sessions"
export CHORUS_GEMINI_TMP_DIR="$STORE/gemini/tmp"
export CHORUS_CLAUDE_PROJECTS_DIR="$STORE/claude/projects"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

# Pinned session ids (deterministic fixture store):
PIN_CODEX="codex-fixture"
PIN_CLAUDE="claude-fixture"
PIN_GEMINI="gemini-fixture"

# Deterministic tempdirs for doctor/setup (must not exist on disk so tests
# produce predictable "warn" states).
DOCTOR_CWD="$TMP_DIR/doctor-cwd"
SETUP_CWD="$TMP_DIR/setup-cwd"
mkdir -p "$DOCTOR_CWD" "$SETUP_CWD"

# --- summary goldens ---
# Pin --cwd so the golden isn't machine-specific (gemini sessions have no cwd
# in the raw file; the CLI falls back to the process cwd unless told otherwise).
for pair in "codex:$PIN_CODEX" "claude:$PIN_CLAUDE" "gemini:$PIN_GEMINI"; do
  agent="${pair%%:*}"
  sid="${pair##*:}"
  raw="$TMP_DIR/summary-$agent.raw.json"
  "$BIN" summary --agent "$agent" --id "$sid" --cwd /workspace/demo --json > "$raw"
  node "$SCRUB" "$raw" "$GOLDEN/summary-$agent.json" summary
done

# --- timeline golden (schema-shape + sorted entries) ---
raw="$TMP_DIR/timeline.raw.json"
"$BIN" timeline --cwd /workspace/demo --limit 6 --json > "$raw"
node "$SCRUB" "$raw" "$GOLDEN/timeline.json" timeline

# --- doctor golden (predictable empty-cwd state) ---
raw="$TMP_DIR/doctor.raw.json"
"$BIN" doctor --cwd "$DOCTOR_CWD" --json > "$raw" || true
node "$SCRUB" "$raw" "$GOLDEN/doctor.json" doctor

# --- setup dry-run golden ---
raw="$TMP_DIR/setup.raw.json"
"$BIN" setup --dry-run --cwd "$SETUP_CWD" --json > "$raw"
node "$SCRUB" "$raw" "$GOLDEN/setup.json" setup

# --- read --include-user goldens (codex, claude, gemini) ---
for pair in "codex:$PIN_CODEX" "claude:$PIN_CLAUDE" "gemini:$PIN_GEMINI"; do
  agent="${pair%%:*}"
  sid="${pair##*:}"
  raw="$TMP_DIR/read-$agent-include-user.raw.json"
  "$BIN" read --agent "$agent" --id "$sid" --include-user --json > "$raw"
  node "$SCRUB" "$raw" "$GOLDEN/read-$agent-include-user.json" read
done

# --- read --tool-calls goldens (codex, claude; gemini is a no-op per notes) ---
for pair in "codex:$PIN_CODEX" "claude:$PIN_CLAUDE"; do
  agent="${pair%%:*}"
  sid="${pair##*:}"
  raw="$TMP_DIR/read-$agent-tool-calls.raw.json"
  "$BIN" read --agent "$agent" --id "$sid" --tool-calls --json > "$raw"
  node "$SCRUB" "$raw" "$GOLDEN/read-$agent-tool-calls.json" read
done

# --- sidecar provenance note ---
cat > "$GOLDEN/.generated.json" <<EOF
{
  "generator": "scripts/release/generate_goldens.sh",
  "generated_at": "$(date -u +"%Y-%m-%dT%H:%M:%SZ")",
  "pinned_sessions": {
    "codex": "$PIN_CODEX",
    "claude": "$PIN_CLAUDE",
    "gemini": "$PIN_GEMINI"
  },
  "fixture_session_store": "fixtures/session-store",
  "doctor_cwd_strategy": "empty tempdir (warn states predictable)",
  "setup_cwd_strategy": "empty tempdir dry-run",
  "scrubber": "scripts/scrub_parity_output.cjs",
  "notes": [
    "Gemini --tool-calls skipped: no-op per adapter (no tool-call schema).",
    "Cursor goldens skipped: JSON sessions not discoverable on generating machine (SQLite-only fallback).",
    "Volatile fields scrubbed: timestamps (sub-second), absolute paths, version string, update_status detail, cwd prefix."
  ]
}
EOF

echo "Goldens regenerated under $GOLDEN"
