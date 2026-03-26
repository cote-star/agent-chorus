# Context Pack: Start Here

## Snapshot
- Repo: `agent-chorus`
- Branch at generation: `main`
- HEAD commit: `879c7265e5b318faee3ce775a14aba4d81b14d9a`
- Generated at: `2026-03-26T15:22:33Z`

## Read Order (Token-Efficient)
1. Read this file.
2. Read `10_SYSTEM_OVERVIEW.md` for architecture and execution paths.
3. Read `30_BEHAVIORAL_INVARIANTS.md` before changing behavior.
4. Use `20_CODE_MAP.md` to deep dive only relevant files.
5. Use `40_OPERATIONS_AND_RELEASE.md` for tests, release, and maintenance.

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
- **Version**: 0.9.0 (npm `agent-chorus` + crate `agent-chorus`).

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
