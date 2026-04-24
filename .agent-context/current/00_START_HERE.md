# Context Pack: Start Here

## Snapshot
- Repo: `agent-chorus`
- Branch at generation: `feat/v0.14-agent-context-hardening`
- Pack version: 0.14.0
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
- **Version**: 0.14.0 (npm `agent-chorus` + crate `agent-chorus`).

## What's New Since Last Seal (v0.13.0 → v0.14.0)
- **P1 — rich manifest + provenance.** Manifest carries head SHA, seal timestamp, tool versions and hashes; sealing/authoring chain is recorded so consumers can verify pack origin.
- **P2 — structural verifier.** `verify` now validates required sections and cross-file references beyond checksum integrity.
- **P3 — zone-aware freshness + suggest-patches.** Freshness is computed per pack zone, not globally; `check-freshness` emits targeted patch suggestions by affected section.
- **P4 — pre-edit awareness.** Authoring flows read the pack before editing so the agent sees invariants it is about to mutate.
- **P5 — count SSOT via handlebars.** Narrative counts expand from manifest via handlebars, eliminating prose/data drift.
- **P6 — hook intelligence + separate-commit enforcement.** Pre-push hook detects pack-only pushes, consumes `.last_freshness.json` to mark warnings addressed, and optional `verify --ci --enforce-separate-commits` fails on mixed `.agent-context/**` + code commits.
- **P7 — subagent reconciliation.** `diff --since-seal` lets a parent agent reconcile parallel subagent work.
- **P8 — hostile input & platform safety (F19–F23).** Path traversal, symlink escape, oversized files, non-UTF-8 sequences, and platform name collisions are rejected.
- **P9 — git edge cases (F24–F28).** Detached HEAD, submodules, worktrees, shallow clones, and grafted histories handled explicitly; `follow_symlinks: false` is the default seal behavior.
- **P10 — concurrency, atomic writes & recovery (F29–F33, F55).** Seal uses staging-dir + rename commit; stale lockfiles auto-recover; concurrent verify no longer races a seal.
- **P11 — schema version enforcement + install integrity (F34, F36, F37, F38).** Manifest pins a schema version; verify rejects unknown versions; install detects tampered / partial packs.
- **P12 — trust boundary & pack integrity.** Pack integrity validation runs on every seal.
- **P13 — authoring ergonomics (F46, F47, F50, F58).** Tiered adoption (`init --tier 1|2|3`), pack-file aliases, last-known-good pointer with `rollback --latest-good`, and the session-start freshness gate preamble.

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
