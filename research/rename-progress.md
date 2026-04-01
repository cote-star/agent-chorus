# Rename Progress: context-pack → agent-context

**Branch:** `refactor/agent-context-rename`
**Started:** 2026-04-01
**Last updated:** 2026-04-01

## Phase Status

| Phase | Description | Status | Notes |
|---|---|---|---|
| 1 | Deprecation alias (non-breaking) | DONE | 9/9 conformance |
| 2 | Internal rename (Rust + Node) | DONE | 9/9 conformance |
| 3 | Documentation | DONE | 9/9 conformance |
| 4 | Re-seal own context pack | DONE | 9/9 conformance |
| 5 | Remove old alias | DEFERRED | v1.0.0 — also update internal log prefixes and HTML sentinels |
| 6 | Research docs, demos, WIP assets | DONE | 9/9 conformance |

## Dependencies

| Dependency | Status | Notes |
|---|---|---|
| team_skills PR #10 | OPEN | agent-context skill |
| stream-models PR #392 | OPEN | agent-context pack for stream-models |

## Remaining for Phase 5 (v1.0.0)

- [ ] Remove `context-pack` command alias from Rust and Node
- [ ] Remove deprecation warning code
- [ ] Update `[context-pack]` log prefixes in agent_context.rs → `[agent-context]`
- [ ] Update `agent-chorus:context-pack:*` HTML sentinel markers → `agent-chorus:agent-context:*`
- [ ] Update `update_check.rs` command name check
- [ ] Update `teardown.rs` type field

## Changelog

| Date | Phase | What happened |
|---|---|---|
| 2026-04-01 | — | Plan created, branch created, tracking started |
| 2026-04-01 | 1 | DONE — Rust + Node alias, deprecation warning, 9/9 conformance |
| 2026-04-01 | 2 | DONE — Rust module + Node scripts dir renamed, all imports updated, 9/9 conformance |
| 2026-04-01 | 3 | DONE — CONTEXT_PACK.md, skill dir, all docs renamed, RELEASE_NOTES v0.10.0 entry, 9/9 conformance |
| 2026-04-01 | 4 | DONE — .agent-context/current/ files updated with new paths, 9/9 conformance |
| 2026-04-01 | 6 | DONE — research/, docs/, fixtures/demo/, wip/ all renamed + stale refs fixed, 9/9 conformance |
