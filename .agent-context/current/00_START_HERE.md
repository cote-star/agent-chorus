# Context Pack: Start Here

## Snapshot
- Repo: `agent-chorus`
- Pack version: 0.16.0
- Generated at seal time (fields populated by `chorus agent-context seal`)

## Read Order — MANDATORY before starting work
1. Read this file completely.
2. Read `30_BEHAVIORAL_INVARIANTS.md` — change checklists, file families, negative guidance.
3. Read `20_CODE_MAP.md` — navigation index, tracing flows, extension recipe.

Do NOT open repo source files until you have read steps 1-3. These three files give you enough context to avoid common mistakes (wrong patterns, missing files, deprecated approaches).

Read on demand:
- `10_SYSTEM_OVERVIEW.md` — for architecture or diagnosis tasks.
- `40_OPERATIONS_AND_RELEASE.md` — for test, CI, or deploy tasks.

## Task-Type Routing
**Impact analysis** (list every file that must change): read `30_BEHAVIORAL_INVARIANTS.md` Update Checklist *before* `20_CODE_MAP.md` — the checklist has the full blast radius per change type. CODE_MAP alone is not exhaustive.
**Navigation / lookup** (find a file, find a value): start with `20_CODE_MAP.md` Quick Lookup Shortcuts.
**Planning** (add a new feature/module): follow the Extension Recipe in `20_CODE_MAP.md`, then cross-check the BEHAVIORAL_INVARIANTS checklist.
**Diagnosis** (unexpected output, broken parity): start with `10_SYSTEM_OVERVIEW.md` Silent Failure Modes, then the relevant invariant.

## Structured Routing
- If `routes.json` exists, use it as the authoritative task router before opening repo files.
- Use `completeness_contract.json` for "what must be included" and `reporting_rules.json` for "how to report it".
- Use `search_scope.json` for "where to search" — it bounds search directories and lists verification shortcuts.
- If the structured layer and markdown disagree, continue exploring and report the mismatch explicitly.

## Fast Facts
- **Product**: Local-first CLI (`chorus`) for cross-agent session reading, comparison, and handoff across Codex, Claude, Gemini, and Cursor.
- **Dual implementation**: Node.js (`scripts/read_session.cjs`) and Rust (`cli/src/main.rs`) with conformance-tested parity.
- **Quality gate**: `npm run check` runs conformance, README examples, package contents, schema validation, and agent-context tests.
- **Core risk**: Any change to CLI output format or command flags must land in both implementations, schemas, and golden fixtures simultaneously.
- **Session handoff**: `chorus checkpoint --from <agent>` (v0.12.0) plus `scripts/hooks/chorus-session-end.sh` broadcast state across agents on clean exit, crash, or window close — see `docs/session-handoff-guide.md`.
- **Session-start freshness gate (v0.14.0)**: routing blocks in `CLAUDE.md` / `AGENTS.md` / `GEMINI.md` now begin with a mandatory instruction to compare `head_sha_at_seal` against `git rev-parse HEAD` before reasoning. Agents MUST warn the user when they diverge.
- **Version**: 0.16.0 (npm `agent-chorus` + crate `agent-chorus`, published 2026-06-03).
- **What's new in 0.16.0**: UAT gap close — Cursor IDE (app) SQLite adapter alongside the cursor-agent CLI JSONL surface; `--history=on-demand` contracted default with on-demand recall semantics; `cwd_mismatch` structured field on fallback; doctor honesty pass (info severity, env-override-dangling check, git-aware hooks checks, stale-snippet detection); codex search extractor fix (`read ⊆ search` invariant now CI-enforced for every adapter); per-subcommand `--help` overhaul including handoff JSON schema in `chorus report --help`; uniform `--tool-calls` NOT_AVAILABLE warning for gemini/hermes; Node parser rejects unknown flags. `RELEASE_NOTES.md` has the full v0.16.0 entry.
- **Known issues (v0.16.1 pending)**: Codex UAT 2026-06-03 found 6 defects v0.16.0 shipped with — Gemini latest-read selection on empty-assistant sessions, `chorus checkpoint` silent-no-op invariant violation, `chorus report` missing-mode error path, cursor `timeline` missing `source` field, `read ⊆ search` invariant edge cases on Codex + Claude with adversarial queries, hermes missing from `schemas/read-output.schema.json` enum. v0.16.1 patch scope pending decision.

## What's New Since Last Seal (v0.14.1 → v0.16.0)

**v0.15.0 (cursor native + cleanup):**
- Native cursor-agent CLI adapter reading `~/.cursor/projects/*/agent-transcripts/*.jsonl` directly (no external bridge required). Per-session cwd recovered via `.workspace-trusted` → `workspacePath` or filesystem-validated demangle.
- Provisional Hermes adapter scaffold (claude-like JSONL under `~/.hermes/sessions`, format unconfirmed).
- Hardened agent-context update contract in `GUIDE.md` + seal drift-guard.
- Bridge fully decommissioned locally; chorus is now standalone.

**v0.16.0 (UAT gap close — see invariants 20-26 in 30_BEHAVIORAL_INVARIANTS.md):**
- N1 Cursor IDE (app) SQLite adapter (`~/.cursor/chats/<hash>/<uuid>/store.db`), merged with the v0.15.0 JSONL surface into one `--agent cursor` view. Each cursor entry carries `source: "cli" | "app"`. Requires Node ≥ 22.5; graceful CLI-only fallback on older Node.
- N2 Codex search extractor fix — codex `search` now walks the real `response_item.payload` / `event_msg.payload` envelopes that real sessions use; pre-fix it walked a top-level `{role, content}` schema that never existed in any session and returned empty for every query.
- N3 Doctor self-consistency — `info` severity added, hooks-path vs pre-push contradiction resolved.
- N4 Per-subcommand `--help` leads with the subcommand. `chorus report --help` includes the full handoff JSON schema with a copy-pasteable example.
- N6 `--tool-calls` parity — claude/codex/cursor (both surfaces) render `[TOOL: <name>]` blocks; gemini/hermes emit a uniform `NOT_AVAILABLE` warning while still setting `included_tool_calls: true`.
- N7 `--history=on-demand` contract (default) / `none` (metadata-only alias) / `eager` (reserved + warning). Closes the 2.5x token-inflation finding from the field study.
- F1 `cwd_mismatch` structured field on read output when `--cwd` doesn't match any session. Stderr also echoes the fallback.
- F2 `env_override_dangling` doctor check for stale `CHORUS_*_DIR` env vars pointing at non-existent directories.
- F3 Git-aware hooks checks — doctor reports `info: cwd is not a git repository` rather than falsely claiming a global hook is installed.
- F4 `read ⊆ search` invariant CI-enforced for every adapter.
- F5 Provider snippets carry the History contract (top of managed block).
- F6/F7/F8 Fixture coverage gaps closed — claude/codex tool-call fixtures with real `tool_use`/`function_call` entries, live hermes fixture, SQLite redaction fixture.
- F9/F10/F11/F13 Hygiene — Rust `report --help` / `doctor --help` parity with Node, `node:sqlite` ExperimentalWarning suppression, Node parser rejects unknown flags, cursor_app dead-code annotations.
- R2 defect fixes — Rust `cwd_matches_project` no longer treats `cwd: "/"` as wildcard, `relative_path` symlink fix, stale-snippet detection in doctor, History contract promoted to top of managed block.

## Carry forward from v0.13.0 → v0.14.0 (still load-bearing)
- P1-P13 hardening (manifest + provenance, structural verifier, zone-aware freshness, pre-edit awareness, count SSOT, hook intelligence, subagent reconciliation, hostile input handling, git edge cases, concurrency safety, schema-version enforcement, pack integrity, authoring ergonomics). See invariants 16-19.

## Scope Rule
- Start with `PROTOCOL.md` for the CLI contract and trust model.
- Read `docs/CLI_REFERENCE.md` for full command syntax and examples.
- Open code only when modifying a specific command or adapter.
- For agent integration, read `CLAUDE.md` or `AGENTS.md` (not both — they target different agents).

## Stop Rules
- Lookup tasks close after the authoritative file + exact value + one supporting chain if requested.
- Impact analysis closes after the update checklist is satisfied — do not grep for more files beyond the checklist.
- Node/Rust parity is always required: never answer "change file X" without also checking if the other implementation needs the same change.
- Do not enumerate fixture files individually — report as `fixtures/golden/*.json` family.
