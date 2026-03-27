# Context Pack: Start Here

## Snapshot
- Repo: `agent-chorus`
- Branch at generation: `main`
- HEAD commit: `729dfeb19789bbb3bc8dc8100bb97bf0db7527d2`
- Generated at: `2026-03-27T12:42:49Z`

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
- **Quality gate**: `npm run check` runs conformance, README examples, package contents, schema validation, and context-pack tests.
- **Core risk**: Any change to CLI output format or command flags must land in both implementations, schemas, and golden fixtures simultaneously.
- **Version**: 0.9.1 (npm `agent-chorus` + crate `agent-chorus`).

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
