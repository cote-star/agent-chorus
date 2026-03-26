# Ground Truth — Reviewer Use Only

> **AGENTS: DO NOT READ THIS FILE.**
> This document contains the correct answers used to grade experiment results.
> Reading it during your experiment run will invalidate your results.

---

## L1 — Redaction Patterns: Where are JWT tokens redacted?

**Correct answer:**

Both implementations redact JWT tokens using pattern matching on the `eyJ` prefix followed by two base64url-encoded segments separated by dots.

| Implementation | File | Function | Line |
|---|---|---|---|
| Rust | `cli/src/agents.rs` | `redact_jwt_tokens()` | line 1345 |
| Node | `scripts/adapters/utils.cjs` | `redactSecrets()` | JWT regex within the patterns array |

The Rust implementation scans for `eyJ` prefix and walks the token char-by-char to find the full `header.payload.signature` structure. The Node implementation uses a regex pattern matching `eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+`.

**Authoritative files:** `cli/src/agents.rs` (Rust), `scripts/adapters/utils.cjs` (Node).

**Grading:**
- `yes`: Both files cited, both function/pattern locations correct, JWT pattern described.
- `partial`: One implementation found but not the other, or correct file but wrong function.
- `no`: Wrong files or unable to find the redaction logic.

**Risk flag:** Set if agent claims JWT redaction doesn't exist or cites only one implementation without mentioning parity requirement.

---

## L2 — Output Version: Where is `chorus_output_version` set?

**Correct answer:**

| Implementation | File | Line | Value |
|---|---|---|---|
| Rust | `cli/src/main.rs` | line 527 | hardcoded `1` in JSON output construction |
| Node | `scripts/read_session.cjs` | line 1533 | `Object.assign({ chorus_output_version: 1 }, result)` |
| Schema | `schemas/read-output.schema.json` | lines 9-12 | defined as integer, description: "Output format version" |

The version is hardcoded to `1` in both implementations. The schema defines it but does not enforce a specific value.

**Grading:**
- `yes`: Both implementation files + schema cited with correct locations.
- `partial`: One implementation found, or correct files but missing the schema.
- `no`: Wrong files or unable to find the version.

---

## M1 — New Agent Adapter: Add "windsurf"

**Correct answer — files that must change:**

| # | File | Change | Required? |
|---|---|---|---|
| 1 | `cli/src/main.rs` | Add `Windsurf` variant to `AgentType` enum (line 408) + `as_str()` match arm (line 417) | Yes |
| 2 | `cli/src/agents.rs` | Add `read_session_windsurf()`, `list_sessions_windsurf()`, `search_sessions_windsurf()` functions + match arms in dispatch functions | Yes |
| 3 | `scripts/read_session.cjs` | Add `'windsurf'` to agents array (line 2547) + handler case in read/list/search switch statements | Yes |
| 4 | `scripts/adapters/windsurf.cjs` | Create new adapter file with `readSession()`, `listSessions()`, `searchSessions()` exports | Yes |
| 5 | `fixtures/session-store/windsurf/` | Create test session fixture data | Yes |
| 6 | `fixtures/golden/read-windsurf.json` | Create golden output for conformance testing | Yes |
| 7 | `scripts/conformance.sh` | Add `expect_success "read-windsurf"` and `expect_success "golden-read-windsurf"` | Yes |
| 8 | `PROTOCOL.md` | Add windsurf to supported agents list | Yes |
| 9 | `docs/CLI_REFERENCE.md` | Add windsurf to agent documentation | Yes |

**Grading:**
- `yes`: All 9 items identified with correct file paths and descriptions.
- `partial`: 5-8 items. Common misses: `PROTOCOL.md`, `docs/CLI_REFERENCE.md`, `conformance.sh`.
- `no`: Fewer than 5 items.

**Risk flag:** Set if agent misses both `cli/src/main.rs` AgentType enum AND `scripts/read_session.cjs` agents array — these are the registration points. Missing either means the agent name isn't recognized.

---

## M2 — New CLI Flag: Add `--output-format yaml` to `chorus read`

**Correct answer — files that must change:**

| # | File | Change | Required? |
|---|---|---|---|
| 1 | `cli/src/main.rs` | Add `output_format: OutputFormat` field to `Read` command struct (around line 28); define `OutputFormat` enum with `Json`, `Text`, `Yaml` | Yes |
| 2 | `scripts/read_session.cjs` | Add `--output-format` arg parsing; add YAML serialization branch in output formatting | Yes |
| 3 | `cli/Cargo.toml` | Add YAML serialization dependency (e.g., `serde_yaml`) | Yes |
| 4 | `package.json` or Node dependency | Add YAML serialization library (e.g., `js-yaml`) — or use built-in if available | Conditional |
| 5 | `schemas/read-output.schema.json` | Either extend for YAML or document that YAML output follows the same schema | Yes |
| 6 | `fixtures/golden/` | Add golden output for YAML format conformance testing | Yes |
| 7 | `scripts/conformance.sh` | Add parity test for `--output-format yaml` | Yes |
| 8 | `PROTOCOL.md` | Document new flag | Yes |
| 9 | `docs/CLI_REFERENCE.md` | Document new flag with examples | Yes |

**Grading:**
- `yes`: 7+ items including both implementations, dependency, conformance, and docs.
- `partial`: 4-6 items. Common miss: YAML dependency in Cargo.toml/package.json.
- `no`: Fewer than 4 items.

**Risk flag:** Set if agent suggests changing only one implementation without the other — parity invariant violation.

---

## H1 — Implementation Plan: `chorus audit` command

**Correct answer — the plan must include:**

**Files to create:**
1. `cli/src/audit.rs` — new Rust module: scan all sessions for un-redacted secrets using the existing `redact_sensitive_text_with_audit()` function, return findings per session.
2. `scripts/audit.cjs` — Node implementation mirroring the Rust module.
3. `schemas/audit-output.schema.json` — output schema for audit results.
4. `fixtures/golden/audit.json` — golden output for conformance.

**Files to modify:**
5. `cli/src/main.rs` — Add `Audit` variant to `Commands` enum, add dispatch in `main()`.
6. `scripts/read_session.cjs` — Add `audit` case in command switch.
7. `scripts/conformance.sh` — Add audit parity test.
8. `PROTOCOL.md` — Document the audit command.
9. `docs/CLI_REFERENCE.md` — Document with examples.

**Key design decisions:**
- Must use `redact_sensitive_text_with_audit()` (already exists, returns findings) rather than re-implementing redaction.
- Must scan all agents' sessions, not just one.
- Output should include: session_id, agent, finding_count, finding_types, sample_locations.

**Grading:**
- `yes`: Plan includes files to create and modify, mentions `redact_sensitive_text_with_audit`, covers both implementations, and includes validation criteria.
- `partial`: Files correct but missing the existing audit function, or missing one implementation.
- `no`: Plan missing >3 files or fundamentally wrong approach.

**Risk flag:** Set if agent proposes re-implementing redaction logic instead of using `redact_sensitive_text_with_audit()`.

---

## H2 — Diagnosis: Why would conformance fail on `chorus read` for Claude sessions after adding a new redaction pattern?

**Scenario:** A developer adds a new redaction pattern (e.g., Anthropic API keys `sk-ant-*`) to the Rust implementation in `cli/src/agents.rs` but forgets to add it to `scripts/adapters/utils.cjs`. The conformance suite (`scripts/conformance.sh`) starts failing.

**Correct answer — diagnosis chain:**

| # | Root Cause | Location | How to Confirm |
|---|---|---|---|
| 1 | **Redaction pattern only in Rust, not Node** — Rust redacts the new pattern, Node does not. Output differs on any session containing the pattern. | `cli/src/agents.rs` (`redact_sensitive_text`) vs `scripts/adapters/utils.cjs` (`redactSecrets`) | Diff the two pattern sets; grep fixture sessions for `sk-ant-` |
| 2 | **Conformance comparator detects output diff** — `scripts/compare_read_output.cjs` compares JSON output field-by-field after canonicalization. The `content` field will differ where Rust shows `[REDACTED]` and Node shows the raw key. | `scripts/compare_read_output.cjs` | Run conformance with `--verbose` or diff the two output files |
| 3 | **Golden fixture mismatch** — if golden files were regenerated from Node output (which doesn't redact), the Rust output won't match the golden. If regenerated from Rust, Node won't match. | `fixtures/golden/read-claude.json` | Check which implementation generated the current golden file |

**Key insight:** The conformance suite catches this because it runs BOTH implementations and compares output. The failure surfaces as a content mismatch in the session text, not as a structural error.

**Grading:**
- `yes`: Identifies the root cause (pattern in one impl, not the other), cites both files, and explains the conformance comparison mechanism.
- `partial`: Identifies the parity issue but doesn't trace through the comparison mechanism.
- `no`: Doesn't identify parity as the root cause.

**Risk flag:** Set if agent suggests fixing by disabling the conformance check rather than adding the missing pattern.
