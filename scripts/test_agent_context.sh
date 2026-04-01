#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT
PASS=0
FAIL=0

CHORUS_BIN=""
if [[ -f "$ROOT/cli/target/debug/chorus" ]]; then
  CHORUS_BIN="$ROOT/cli/target/debug/chorus"
elif [[ -f "$ROOT/cli/target/release/chorus" ]]; then
  CHORUS_BIN="$ROOT/cli/target/release/chorus"
fi

# --- helpers ---

check() {
  local label="$1"
  if eval "$2"; then
    echo "PASS $label"
    PASS=$((PASS + 1))
  else
    echo "FAIL $label"
    FAIL=$((FAIL + 1))
  fi
}

make_repo() {
  local dir="$1"
  mkdir -p "$dir"
  git -C "$dir" init -q
  git -C "$dir" commit --allow-empty -m "init" -q
}

run_node_init() { node "$ROOT/scripts/agent_context/init.cjs" --cwd "$1" --force; }
run_rust_init() { "$CHORUS_BIN" context-pack init --cwd "$1" --force; }
run_node_seal() { node "$ROOT/scripts/agent_context/seal.cjs" --cwd "$1" --force; }
run_rust_seal() { "$CHORUS_BIN" context-pack seal --cwd "$1" --force; }

# Extract managed block content between markers (strips markers themselves)
extract_block() {
  local file="$1" marker="$2"
  sed -n "/<!-- ${marker}:start -->/,/<!-- ${marker}:end -->/p" "$file" \
    | grep -v "<!-- ${marker}:"
}

echo "=== Context-pack integration tests ==="

# --- Test 7: init-creates-agent-configs ---
T7="$TMP_DIR/t7"
make_repo "$T7"
run_node_init "$T7" >/dev/null 2>&1

check "init-creates-agent-configs" '
  [[ -f "$T7/CLAUDE.md" ]] &&
  [[ -f "$T7/AGENTS.md" ]] &&
  [[ -f "$T7/GEMINI.md" ]] &&
  grep -q "<!-- agent-chorus:context-pack:claude:start -->" "$T7/CLAUDE.md" &&
  grep -q "<!-- agent-chorus:context-pack:codex:start -->" "$T7/AGENTS.md" &&
  grep -q "<!-- agent-chorus:context-pack:gemini:start -->" "$T7/GEMINI.md"
'

check "init-creates-structured-files" '
  [[ -f "$T7/.agent-context/current/routes.json" ]] &&
  [[ -f "$T7/.agent-context/current/completeness_contract.json" ]] &&
  [[ -f "$T7/.agent-context/current/reporting_rules.json" ]] &&
  [[ -f "$T7/.agent-context/current/search_scope.json" ]]
'

# --- Test 8: init-idempotent ---
T8="$TMP_DIR/t8"
make_repo "$T8"
run_node_init "$T8" >/dev/null 2>&1
FIRST=$(cat "$T8/CLAUDE.md")
run_node_init "$T8" >/dev/null 2>&1
SECOND=$(cat "$T8/CLAUDE.md")

check "init-idempotent" '[[ "$FIRST" == "$SECOND" ]]'

# --- Test 9: init-preserves-existing-content ---
T9="$TMP_DIR/t9"
make_repo "$T9"
echo "# My Existing Project Notes" > "$T9/CLAUDE.md"
run_node_init "$T9" >/dev/null 2>&1

check "init-preserves-existing-content" '
  grep -q "<!-- agent-chorus:context-pack:claude:start -->" "$T9/CLAUDE.md" &&
  grep -q "# My Existing Project Notes" "$T9/CLAUDE.md"
'

# --- Test 10: seal-syncs-snapshot-metadata ---
T10="$TMP_DIR/t10"
make_repo "$T10"
run_node_init "$T10" >/dev/null 2>&1

# Fill template markers minimally so seal doesn't complain
for f in "$T10/.agent-context/current/"*.md; do
  sed -i '' 's/<!-- AGENT:.*-->/Filled by test/g' "$f" 2>/dev/null || true
done

run_node_seal "$T10" >/dev/null 2>&1
MANIFEST="$T10/.agent-context/current/manifest.json"

check "seal-syncs-snapshot-metadata" '
  [[ -f "$MANIFEST" ]] &&
  [[ -f "$T10/.agent-context/current/00_START_HERE.md" ]] &&
  SEAL_BRANCH=$(node -e "console.log(JSON.parse(require(\"fs\").readFileSync(\"$MANIFEST\",\"utf8\")).branch)" 2>/dev/null) &&
  grep -q "Branch at generation: \`$SEAL_BRANCH\`" "$T10/.agent-context/current/00_START_HERE.md"
'

# --- Test 10b: seal-markdown-only-mode ---
T10B="$TMP_DIR/t10b"
make_repo "$T10B"
run_node_init "$T10B" >/dev/null 2>&1
for f in "$T10B/.agent-context/current/"*.md; do
  sed -i '' 's/<!-- AGENT:.*-->/Filled by test/g' "$f" 2>/dev/null || true
done
rm -f \
  "$T10B/.agent-context/current/routes.json" \
  "$T10B/.agent-context/current/completeness_contract.json" \
  "$T10B/.agent-context/current/reporting_rules.json"
run_node_seal "$T10B" >/dev/null 2>&1

check "seal-markdown-only-mode" '
  [[ -f "$T10B/.agent-context/current/manifest.json" ]] &&
  MANIFEST_FILES=$(node -e "let m=JSON.parse(require(\"fs\").readFileSync(\"$T10B/.agent-context/current/manifest.json\",\"utf8\")); console.log((m.files||[]).map(f=>f.path).sort().join(\",\"))" 2>/dev/null) &&
  [[ "$MANIFEST_FILES" == "00_START_HERE.md,10_SYSTEM_OVERVIEW.md,20_CODE_MAP.md,30_BEHAVIORAL_INVARIANTS.md,40_OPERATIONS_AND_RELEASE.md" ]]
'

# --- Test 10c: seal-structured-invalid-ref-fails ---
T10C="$TMP_DIR/t10c"
make_repo "$T10C"
run_node_init "$T10C" >/dev/null 2>&1
for f in "$T10C/.agent-context/current/"*.md; do
  sed -i '' 's/<!-- AGENT:.*-->/Filled by test/g' "$f" 2>/dev/null || true
done
node -e '
  const fs = require("fs");
  const file = process.argv[1];
  const data = JSON.parse(fs.readFileSync(file, "utf8"));
  data.task_routes.lookup.pack_read_order = ["00_START_HERE.md", "missing-pack-file.md"];
  fs.writeFileSync(file, JSON.stringify(data, null, 2) + "\n");
' "$T10C/.agent-context/current/routes.json"

check "seal-structured-invalid-ref-fails" '
  ! run_node_seal "$T10C" >/dev/null 2>&1
'

# --- Test 11: node-rust-init-parity ---
if [[ -n "$CHORUS_BIN" ]]; then
  T11N="$TMP_DIR/t11-node"
  T11R="$TMP_DIR/t11-rust"
  make_repo "$T11N"
  make_repo "$T11R"
  run_node_init "$T11N" >/dev/null 2>&1
  run_rust_init "$T11R" >/dev/null 2>&1

  # Compare managed blocks (content between markers) for each agent config
  PARITY=true
  for marker in "agent-chorus:context-pack:claude" "agent-chorus:context-pack:codex" "agent-chorus:context-pack:gemini"; do
    file_suffix="${marker##*:}"
    case "$file_suffix" in
      claude) fname="CLAUDE.md" ;;
      codex)  fname="AGENTS.md" ;;
      gemini) fname="GEMINI.md" ;;
    esac
    NODE_BLOCK=$(extract_block "$T11N/$fname" "$marker" 2>/dev/null || echo "MISSING")
    RUST_BLOCK=$(extract_block "$T11R/$fname" "$marker" 2>/dev/null || echo "MISSING")
    if [[ "$NODE_BLOCK" != "$RUST_BLOCK" ]]; then
      PARITY=false
    fi
  done

  check "node-rust-init-parity" '$PARITY'
else
  echo "SKIP node-rust-init-parity (no Rust binary found — run cargo build first)"
fi

# --- Test 12: node-rust-seal-parity ---
if [[ -n "$CHORUS_BIN" ]]; then
  T12N="$TMP_DIR/t12-node"
  T12R="$TMP_DIR/t12-rust"
  make_repo "$T12N"
  make_repo "$T12R"
  run_node_init "$T12N" >/dev/null 2>&1
  run_rust_init "$T12R" >/dev/null 2>&1

  # Fill template markers for both
  for repo in "$T12N" "$T12R"; do
    for f in "$repo/.agent-context/current/"*.md; do
      sed -i '' 's/<!-- AGENT:.*-->/Filled by test/g' "$f" 2>/dev/null || true
    done
  done

  run_node_seal "$T12N" >/dev/null 2>&1
  run_rust_seal "$T12R" >/dev/null 2>&1

  MN="$T12N/.agent-context/current/manifest.json"
  MR="$T12R/.agent-context/current/manifest.json"

  # Compare manifest structure: same keys, same file list (mask timestamps and checksums)
  check "node-rust-seal-parity" '
    [[ -f "$MN" ]] && [[ -f "$MR" ]] &&
    NODE_KEYS=$(node -e "console.log(Object.keys(JSON.parse(require(\"fs\").readFileSync(\"$MN\",\"utf8\"))).sort().join(\",\"))" 2>/dev/null) &&
    RUST_KEYS=$(node -e "console.log(Object.keys(JSON.parse(require(\"fs\").readFileSync(\"$MR\",\"utf8\"))).sort().join(\",\"))" 2>/dev/null) &&
    [[ "$NODE_KEYS" == "$RUST_KEYS" ]] &&
    NODE_FILES=$(node -e "let m=JSON.parse(require(\"fs\").readFileSync(\"$MN\",\"utf8\")); console.log((m.files||[]).map(f=>f.path).sort().join(\",\"))" 2>/dev/null) &&
    RUST_FILES=$(node -e "let m=JSON.parse(require(\"fs\").readFileSync(\"$MR\",\"utf8\")); console.log((m.files||[]).map(f=>f.path).sort().join(\",\"))" 2>/dev/null) &&
    [[ "$NODE_FILES" == "$RUST_FILES" ]]
  '
else
  echo "SKIP node-rust-seal-parity (no Rust binary found — run cargo build first)"
fi

echo ""
echo "=== Results: $PASS passed, $FAIL failed ==="
if [[ "$FAIL" -gt 0 ]]; then
  exit 1
fi
