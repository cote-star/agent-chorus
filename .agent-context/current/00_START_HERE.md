# Context Pack: Start Here

## Snapshot
- Repo: `agent-chorus`
- Branch at generation: `main`
- Generated at: `2026-03-12`

## Read Order (Token-Efficient)
1. Read this file.
2. Read `10_SYSTEM_OVERVIEW.md` for architecture and execution paths.
3. Read `30_BEHAVIORAL_INVARIANTS.md` before changing behavior.
4. Use `20_CODE_MAP.md` to deep dive only relevant files.
5. Use `40_OPERATIONS_AND_RELEASE.md` for tests, release, and maintenance.

## Fast Facts
- **Product**: Local-first CLI (`chorus`) for cross-agent session reading, comparison, and handoff across Codex, Claude, Gemini, and Cursor.
- **Dual implementation**: Node.js (`scripts/read_session.cjs`) and Rust (`cli/src/main.rs`) with conformance-tested parity.
- **Quality gate**: `npm run check` runs conformance, README examples, package contents, and JSON schema validation.
- **Core risk**: Any change to CLI output format or command flags must land in both implementations, schemas, and golden fixtures simultaneously.
- **Version**: 0.7.0 (npm `agent-chorus` + crate `agent-chorus`).

## Scope Rule
For "understand this repo end-to-end" requests:
- Start with `PROTOCOL.md` for the CLI contract and trust model.
- Read `docs/CLI_REFERENCE.md` for full command syntax and examples.
- Open code only when modifying a specific command or adapter.
- For agent integration, read `CLAUDE.md` or `AGENTS.md` (not both — they target different agents).
