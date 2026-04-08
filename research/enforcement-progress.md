# Enforcement + Regeneration Progress

**Started:** 2026-04-08
**Last updated:** 2026-04-08
**Plan:** `enforcement-and-regen-plan.md`

## Phase Status

| Phase | Description | Status | Notes |
|---|---|---|---|
| 1 | Agent-chorus engineering (A1-A8) | IN PROGRESS | A1-A5 done, A6-A8 remaining |
| 2 | Agent-chorus docs + self-update (B+C) | NOT STARTED | depends on Phase 1 |
| 3 | Team skills updates (D1-D4) | NOT STARTED | depends on Phase 1 |
| 4 | Stream-models regen (E1-E6) | NOT STARTED | can start after Phase 1 |
| 5 | Trust-stream-frontend cleanup (F1-F11) | NOT STARTED | can start after Phase 1 |
| 6 | Context pack viz final (B9) | NOT STARTED | depends on Phases 1-3 |

## Task Status

### Phase 1 — Agent-Chorus Engineering

- [x] A1: Verify subcommand (Rust) — CI mode + freshness + JSON output
- [x] A2: Verify subcommand (Node) — parity with Rust
- [x] A3: CI mode flag (--ci) — exit code 0/1, JSON output
- [x] A4: Semantic diff mapping — freshness via relevance patterns + changed file list
- [x] A5: Manifest provenance in seal — removed chorus fields, retained base_sha/head_sha/build_reason
- [ ] A6: Separate-commit detection
- [ ] A7: CI template (GitHub Actions)
- [ ] A8: Conformance tests

### Phase 2 — Agent-Chorus Docs + Self-Update

- [ ] B1: AGENT_CONTEXT.md — enforcement section
- [ ] B2: README.md — maintenance claims
- [ ] B3: PROTOCOL.md — verify subcommand
- [ ] B4: CLI_REFERENCE.md — verify docs
- [ ] B5: RELEASE_NOTES.md — enforcement entries
- [ ] B6: Presentation — enforcement capabilities
- [ ] B7: agent-context-vs-skills.md — fix future-state leakage
- [ ] B8: agent-context-map-design.md — confirm marked as design
- [ ] B9: context-pack-viz — enforcement section (Phase 6)
- [ ] C1: Re-seal own .agent-context
- [ ] C2: Update skills/agent-context/SKILL.md

### Phase 3 — Team Skills

- [ ] D1: Update SKILL.md — CI setup, verify step
- [ ] D2: Update Quality Bar — CI verification criterion
- [ ] D3: Update getting-started.md — CI instructions
- [ ] D4: Update architecture.md — enforcement layer

### Phase 4 — Stream-Models Regen

- [ ] E1: Rebase/merge with main
- [ ] E2: Verify no chorus refs
- [ ] E3: Update stale pack sections
- [ ] E4: Verify pack (references resolve, contracts match)
- [ ] E5: Update manifest
- [ ] E6: Push + update PR #392

### Phase 5 — Trust-Stream-Frontend Cleanup

- [ ] F1: Remove chorus artifacts (GUIDE.md, history.jsonl, relevance.json, snapshots/)
- [ ] F2: Strip chorus HTML sentinels from CLAUDE.md, GEMINI.md
- [ ] F3: Fix CLAUDE.md routing (imperative, P16)
- [ ] F4: Fix GEMINI.md routing (imperative, P16)
- [ ] F5: Create AGENTS.md (search-and-verify routing)
- [ ] F6: Handle old AGENTS.MD (check for valuable content)
- [ ] F7: Fix 40_OPERATIONS_AND_RELEASE.md (replace chorus commands)
- [ ] F8: Fix manifest.json (remove chorus fields)
- [ ] F9: Update stale pack sections
- [ ] F10: Verify pack
- [ ] F11: Push + create/update PR

### Phase 6 — Viz Final

- [ ] B9: context-pack-viz enforcement section

## Changelog

| Date | Phase | Task | What happened |
|---|---|---|---|
| 2026-04-08 | — | — | Plan created, progress tracker created |
| 2026-04-08 | 1 | A1-A5 | verify subcommand enhanced (Rust+Node), CI mode, freshness, manifest cleaned. 9/9 conformance. |
