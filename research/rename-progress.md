# Rename Progress: context-pack → agent-context

**Branch:** `refactor/agent-context-rename`
**Started:** 2026-04-01
**Last updated:** 2026-04-01

## Phase Status

| Phase | Description | Status | Notes |
|---|---|---|---|
| 1 | Deprecation alias (non-breaking) | NOT STARTED | |
| 2 | Internal rename (Rust + Node) | NOT STARTED | |
| 3 | Documentation | NOT STARTED | |
| 4 | Re-seal own context pack | NOT STARTED | |
| 5 | Remove old alias | NOT STARTED | Future (v1.0.0) |
| 6 | Research docs, demos, WIP assets | NOT STARTED | |

## Dependencies

| Dependency | Status | Notes |
|---|---|---|
| team_skills PR #10 | OPEN | agent-context skill |
| stream-models PR #392 | OPEN | agent-context pack for stream-models |

## Phase 1 — Deprecation alias

- [ ] Rust: add `agent-context` as primary, `context-pack` as hidden alias with warning
- [ ] Node: add `agent-context` routing, warn on old name
- [ ] Tests: new name works, old name warns
- [ ] CLI_REFERENCE.md: show new name, note deprecation
- [ ] Conformance: both implementations same deprecation message

## Phase 2 — Internal rename

- [ ] Rust: `context_pack.rs` → `agent_context.rs`, update mod/imports
- [ ] Node: `scripts/context_pack/` → `scripts/agent_context/`, update requires
- [ ] package.json: rename scripts, update files array
- [ ] CI: update ci.yml
- [ ] Git hooks: update .githooks/pre-push
- [ ] Test scripts: rename and update
- [ ] Conformance: full suite passes

## Phase 3 — Documentation

- [ ] CONTEXT_PACK.md → AGENT_CONTEXT.md
- [ ] README.md
- [ ] PROTOCOL.md
- [ ] CLAUDE.md / AGENTS.md / GEMINI.md
- [ ] CLI_REFERENCE.md
- [ ] CONTRIBUTING.md / DEVELOPMENT.md
- [ ] RELEASE_NOTES.md (add entry, keep historical)
- [ ] Skill: `skills/context-pack/` → `skills/agent-context/`

## Phase 4 — Re-seal own context pack

- [ ] Update `.agent-context/current/` files
- [ ] Validate references resolve
- [ ] Separate commit

## Phase 5 — Remove old alias (future)

- [ ] Remove `context-pack` command from Rust
- [ ] Remove `context-pack` command from Node
- [ ] Remove deprecation warning code

## Phase 6 — Remaining assets

- [ ] `research/context-pack-design-principles.md` → rename
- [ ] `research/context-pack-field-findings-2026-03-20.md` → rename
- [ ] `research/context-pack-next-version-agenda.md` → rename
- [ ] `research/context-pack-v2-design.md` → rename
- [ ] `docs/context-pack-presentation.md` → rename
- [ ] `docs/context-pack-results.md` → rename
- [ ] `docs/demo-context-pack.webp` → rename
- [ ] `docs/cold-start-context-pack-hero.webp` → rename
- [ ] `fixtures/demo/hero-context-pack.html` → rename
- [ ] `fixtures/demo/player-context-pack.html` → rename
- [ ] `wip/context-pack-skill/` → rename

## Changelog

| Date | Phase | What happened |
|---|---|---|
| 2026-04-01 | — | Plan created, branch created, tracking started |
