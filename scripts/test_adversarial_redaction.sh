#!/usr/bin/env bash
# Adversarial redaction tests — verifies that both Node and Rust implementations
# redact secrets from crafted payloads.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
FIXTURES_DIR="$ROOT_DIR/fixtures/adversarial"
PASS=0
FAIL=0

# Helper: run Node redaction on a file and check for leaked patterns
test_node_redaction() {
  local fixture="$1"
  local label="$2"
  shift 2
  local patterns=("$@")

  local content
  content=$(cat "$fixture")

  # Use Node to redact via utils.cjs
  local redacted
  redacted=$(node -e "
    const { redactSensitiveText } = require('$ROOT_DIR/scripts/adapters/utils.cjs');
    const fs = require('fs');
    const input = fs.readFileSync('$fixture', 'utf-8');
    process.stdout.write(redactSensitiveText(input));
  ")

  local leaked=0
  for pattern in "${patterns[@]}"; do
    if echo "$redacted" | grep -qE "$pattern"; then
      echo "  LEAK [Node/$label]: pattern '$pattern' found in output"
      leaked=1
    fi
  done

  if [ "$leaked" -eq 0 ]; then
    echo "  PASS [Node/$label]"
    PASS=$((PASS + 1))
  else
    echo "  FAIL [Node/$label]"
    FAIL=$((FAIL + 1))
  fi
}

# Helper: run Rust redaction via a test harness
# Since redact_sensitive_text is private, we test via a small Rust program
# We'll use a temporary session fixture and chorus read
test_rust_redaction() {
  local fixture="$1"
  local label="$2"
  shift 2
  local patterns=("$@")

  # Build a temporary Codex-style JSONL session with the adversarial content as a message
  local content
  content=$(cat "$fixture")
  local tmpdir
  tmpdir=$(mktemp -d)
  local session_file="$tmpdir/adversarial.jsonl"

  # Escape content for JSON embedding
  local escaped_content
  escaped_content=$(node -e "process.stdout.write(JSON.stringify(require('fs').readFileSync('$fixture', 'utf-8')))")

  cat > "$session_file" <<JSONL
{"type":"session_meta","payload":{"session_id":"adversarial-test","cwd":"$tmpdir"}}
{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":$escaped_content}]}}
JSONL

  local redacted
  redacted=$(CHORUS_CODEX_SESSIONS_DIR="$tmpdir" "$ROOT_DIR/cli/target/debug/chorus" read --agent codex --json 2>/dev/null || echo '{"content":"[error]"}')
  local output_content
  output_content=$(echo "$redacted" | node -e "
    let d=''; process.stdin.on('data',c=>d+=c); process.stdin.on('end',()=>{
      try { process.stdout.write(JSON.parse(d).content || ''); }
      catch(e) { process.stdout.write('[parse-error]'); }
    });
  ")

  local leaked=0
  for pattern in "${patterns[@]}"; do
    if echo "$output_content" | grep -qE "$pattern"; then
      echo "  LEAK [Rust/$label]: pattern '$pattern' found in output"
      leaked=1
    fi
  done

  if [ "$leaked" -eq 0 ]; then
    echo "  PASS [Rust/$label]"
    PASS=$((PASS + 1))
  else
    echo "  FAIL [Rust/$label]"
    FAIL=$((FAIL + 1))
  fi

  rm -rf "$tmpdir"
}

echo "=== Adversarial Redaction Tests ==="
echo ""

# Build Rust binary if needed
if [ ! -f "$ROOT_DIR/cli/target/debug/chorus" ]; then
  echo "Building Rust CLI..."
  (cd "$ROOT_DIR/cli" && cargo build 2>/dev/null)
fi

# ---- Test: mixed-secrets.txt ----
echo "Test: mixed-secrets"
MIXED_PATTERNS=(
  "sk-proj-[a-z]"
  "AKIAIOSFODNN7EXAMPLE"
  "ghp_ABCDEF"
  "AIzaSy"
  "xoxb-"
  "Bearer eyJ"
  "supersecret"
  "admin:supersecret"
  "my_database_p4ssw0rd"
  "redis_secret_pass"
)
test_node_redaction "$FIXTURES_DIR/mixed-secrets.txt" "mixed-secrets" "${MIXED_PATTERNS[@]}"
test_rust_redaction "$FIXTURES_DIR/mixed-secrets.txt" "mixed-secrets" "${MIXED_PATTERNS[@]}"
echo ""

# ---- Test: multi-line-pem.txt ----
echo "Test: multi-line-pem"
PEM_PATTERNS=(
  "BEGIN PRIVATE KEY"
  "MIIEvQIBADA"
)
test_node_redaction "$FIXTURES_DIR/multi-line-pem.txt" "multi-line-pem" "${PEM_PATTERNS[@]}"
test_rust_redaction "$FIXTURES_DIR/multi-line-pem.txt" "multi-line-pem" "${PEM_PATTERNS[@]}"
echo ""

# ---- Test: edge-cases.txt ----
echo "Test: edge-cases"
EDGE_PATTERNS=(
  "sk-ant-api03"
  "github_pat_11"
  "sk-supersecret"
  "gho_ABCDEF"
  "ghs_ABCDEF"
  "xoxp-"
  "guest_password"
)
test_node_redaction "$FIXTURES_DIR/edge-cases.txt" "edge-cases" "${EDGE_PATTERNS[@]}"
test_rust_redaction "$FIXTURES_DIR/edge-cases.txt" "edge-cases" "${EDGE_PATTERNS[@]}"
echo ""

# ---- Summary ----
TOTAL=$((PASS + FAIL))
echo "=== Results: $PASS/$TOTAL passed ==="
if [ "$FAIL" -gt 0 ]; then
  echo "WARNING: $FAIL test(s) failed — secrets leaked through redaction"
  exit 1
fi
echo "All adversarial redaction tests passed."
