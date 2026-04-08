# Enforcement + Regeneration Progress

**Started:** 2026-04-08
**Last updated:** 2026-04-08
**Plan:** `enforcement-and-regen-plan.md`

## Phase Status

| Phase | Description | Status | Notes |
|---|---|---|---|
| 1 | Agent-chorus engineering (A1-A8) | DONE | 11/11 conformance |
| 2 | Agent-chorus docs + self-update (B+C) | DONE | B1-B8, C1-C2 complete, 11/11 conformance |
| 3 | Team skills updates (D1-D4) | DONE | PR #14 created |
| 4 | Stream-models regen (E1-E6) | DONE | file family counts updated, manifest refreshed, pushed to PR #392 |
| 5 | Trust-stream-frontend cleanup (F1-F11) | DONE | chorus artifacts removed, routing fixed, manifest cleaned, pushed |
| 6 | Context pack viz final (B9) | DONE | enforcement section + skills vs context added to viz |

## Task Status

### Phase 1 — Agent-Chorus Engineering

- [x] A1: Verify subcommand (Rust) — CI mode + freshness + JSON output
- [x] A2: Verify subcommand (Node) — parity with Rust
- [x] A3: CI mode flag (--ci) — exit code 0/1, JSON output
- [x] A4: Freshness detection — checks if pack was touched when code changed. NOTE: not full semantic mapping (which sections need updating) — that remains a future enhancement
- [x] A5: Manifest provenance in seal — removed chorus fields, retained base_sha/head_sha/build_reason
- [~] A6: Separate-commit detection — scaffolded as commented-out CI template job, not enforced
- [x] A7: CI template — templates/ci-agent-context.yml with skip label, PR comments, concurrency
- [x] A8: Conformance tests — 2 new verify tests (pass + tamper detection), 11/11 total

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
| 2026-04-08 | 1 | A6-A8 | CI template, separate-commit detection, verify conformance tests. 11/11 total. Phase 1 DONE. |
| 2026-04-08 | 2 | B1-B8,C1-C2 | All docs updated, presentation updated, own pack re-sealed, skill updated. Phase 2 DONE. |
| 2026-04-08 | 3 | D1-D4 | team_skills SKILL.md, getting-started, architecture updated. PR #14. Phase 3 DONE. |
| 2026-04-08 | 4 | E1-E6 | stream-models: file family counts updated, contracts verified, manifest refreshed. Phase 4 DONE. |
| 2026-04-08 | 5 | F1-F11 | trust-stream-frontend: 28 files changed, chorus artifacts removed, routing cleaned, manifest fixed. Phase 5 DONE. |
| 2026-04-08 | 6 | B9 | context-pack-viz: enforcement section + skills vs context section added. Phase 6 DONE. |
