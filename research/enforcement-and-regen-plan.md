# Enforcement Gaps + Regeneration Plan

**Date:** 2026-04-08
**Status:** PLANNED
**Branch:** `refactor/agent-context-rename` (agent-chorus)

## Context

Codex review identified 6 enforcement gaps between the "agent-maintained" claim and shipped behavior. Additionally, both work repos need agent-context cleanup (chorus refs, stale content, merges from main). This plan addresses everything in order.

---

## Workstreams

### A. Agent-Chorus Engineering — Verify + Enforce

| Task | ID | Description | Status |
|---|---|---|---|
| Verify subcommand (Rust) | A1 | `chorus agent-context verify` — check pack freshness against code changes | NOT STARTED |
| Verify subcommand (Node) | A2 | Node parity for verify, conformance-tested | NOT STARTED |
| CI mode flag | A3 | `--ci` flag: exit code 0/1 for PR gates, JSON output for CI logs | NOT STARTED |
| Semantic diff mapping | A4 | Map changed source files → required pack sections, report which sections need update | NOT STARTED |
| Manifest provenance | A5 | Add `base_sha`, `head_sha`, `sections_updated`, `files_considered` to seal output | NOT STARTED |
| Separate-commit detection | A6 | In `verify --ci`: check that .agent-context changes are in their own commit (optional flag) | NOT STARTED |
| CI template | A7 | Reusable GitHub Actions step/workflow template for PR gates | NOT STARTED |
| Conformance tests | A8 | Tests for verify (Node + Rust parity), CI exit codes, manifest provenance | NOT STARTED |

**Files to create/modify in agent-chorus:**
- `cli/src/agent_context.rs` — add verify function + provenance to seal
- `cli/src/main.rs` — add Verify subcommand with CI flag
- `scripts/agent_context/verify.cjs` — update with semantic diff + CI mode
- `scripts/agent_context/seal.cjs` — add provenance fields to manifest
- `scripts/read_session.cjs` — add verify to help text and routing
- `scripts/test_agent_context.sh` — add verify tests
- `package.json` — add `agent-context:verify` script
- `.github/workflows/ci.yml` — add verify to CI matrix
- NEW: `templates/ci-agent-context.yml` — reusable GitHub Actions template

### B. Agent-Chorus Documentation Updates

| Task | ID | Description | Status |
|---|---|---|---|
| AGENT_CONTEXT.md | B1 | Add enforcement section (verify, CI gate, provenance) | NOT STARTED |
| README.md | B2 | Update maintenance section — reference enforcement, fix "automatic" wording | NOT STARTED |
| PROTOCOL.md | B3 | Add verify subcommand to CLI contract | NOT STARTED |
| CLI_REFERENCE.md | B4 | Add verify subcommand docs with examples | NOT STARTED |
| RELEASE_NOTES.md | B5 | Add enforcement entries to v0.10.0 | NOT STARTED |
| Presentation | B6 | Update with enforcement capabilities once built | NOT STARTED |
| agent-context-vs-skills.md | B7 | Fix future-state leakage (context-map ref), add enforcement to model | NOT STARTED |
| agent-context-map-design.md | B8 | Ensure clearly marked as "Status: Design" not current | NOT STARTED |
| context-pack-viz (index.html) | B9 | Add enforcement section, ensure numbers reconciled | NOT STARTED |

**Files to modify:**
- `AGENT_CONTEXT.md`
- `README.md`
- `PROTOCOL.md`
- `docs/CLI_REFERENCE.md`
- `docs/DEVELOPMENT.md`
- `RELEASE_NOTES.md`
- `docs/agent-context-presentation.md`
- `research/agent-context-vs-skills.md`
- `research/agent-context-map-design.md`
- `~/sandbox/play/context-pack-viz/index.html`

### C. Agent-Chorus Self-Update

| Task | ID | Description | Status |
|---|---|---|---|
| Re-seal own .agent-context | C1 | Update .agent-context/current/ to reflect verify subcommand + enforcement | NOT STARTED |
| Update skill definition | C2 | `skills/agent-context/SKILL.md` — reference verify in flows | NOT STARTED |

**Files to modify:**
- `.agent-context/current/00_START_HERE.md`
- `.agent-context/current/10_SYSTEM_OVERVIEW.md`
- `.agent-context/current/20_CODE_MAP.md`
- `.agent-context/current/30_BEHAVIORAL_INVARIANTS.md`
- `.agent-context/current/40_OPERATIONS_AND_RELEASE.md`
- `.agent-context/current/completeness_contract.json`
- `.agent-context/current/search_scope.json`
- `.agent-context/current/manifest.json`
- `skills/agent-context/SKILL.md`

### D. Team Skills Updates

| Task | ID | Description | Status |
|---|---|---|---|
| Update SKILL.md | D1 | Add CI setup step to Create flow, verify to Validate step, provenance to manifest description | NOT STARTED |
| Update Quality Bar | D2 | Add "CI verification passes" criterion | NOT STARTED |
| Update getting-started.md | D3 | Add CI setup instructions + enforcement section | NOT STARTED |
| Update architecture.md | D4 | Add enforcement layer to three-layer architecture | NOT STARTED |

**Files to modify in team_skills (branch: new branch from main):**
- `skills/agent-context/SKILL.md`
- `skills/agent-context/references/getting-started.md`
- `skills/agent-context/references/architecture.md`

### E. Stream-Models Agent Context Regeneration

| Task | ID | Description | Status |
|---|---|---|---|
| Rebase/merge with main | E1 | Branch has new merges (omiranda93/test-context, expand-modeling-tests) — ensure clean | NOT STARTED |
| Verify no chorus refs | E2 | Grep all .agent-context/, CLAUDE.md, AGENTS.md, GEMINI.md for chorus/seal | NOT STARTED |
| Update pack content | E3 | Regenerate sections that are stale from merged work (new tests, modeling changes) | NOT STARTED |
| Verify pack | E4 | All file references resolve, contracts match current files, quality bar passes | NOT STARTED |
| Update manifest | E5 | Refresh generated_at, head_sha, checksums | NOT STARTED |
| Push and update PR #392 | E6 | Push to feat/agent-context, update PR description if needed | NOT STARTED |

**Files to modify in stream-models:**
- `.agent-context/current/*` (all 10 files)
- `CLAUDE.md` (verify clean)
- `AGENTS.md` (verify clean)
- `GEMINI.md` (verify clean)

### F. Trust-Stream-Frontend Agent Context Cleanup + Regeneration

| Task | ID | Description | Status |
|---|---|---|---|
| Note: repo moved to ~/sandbox/work/trust-stream/trust-stream-frontend | — | Was at dsml/, now at trust-stream/ | CONFIRMED |
| Remove chorus artifacts | F1 | Delete .agent-context/GUIDE.md, history.jsonl, relevance.json, snapshots/ | NOT STARTED |
| Remove chorus HTML sentinels | F2 | Strip `<!-- agent-chorus:context-pack:*-->` from CLAUDE.md, GEMINI.md | NOT STARTED |
| Fix CLAUDE.md routing | F3 | Replace chorus block with clean imperative routing (P16) | NOT STARTED |
| Fix GEMINI.md routing | F4 | Replace chorus block with clean imperative routing (P16) | NOT STARTED |
| Create AGENTS.md | F5 | Add search-and-verify routing for Codex/Cursor (currently missing or stale AGENTS.MD) | NOT STARTED |
| Remove old AGENTS.MD if redundant | F6 | Check if AGENTS.MD has valuable content like stream-models did | NOT STARTED |
| Fix 40_OPERATIONS_AND_RELEASE.md | F7 | Replace chorus commands with agent-driven validation | NOT STARTED |
| Fix manifest.json | F8 | Remove chorus fields (build_reason: manual-seal, cargo_version, package_version) | NOT STARTED |
| Update pack content | F9 | Regenerate sections stale from recent main merges | NOT STARTED |
| Verify pack | F10 | All file references resolve, contracts match current files | NOT STARTED |
| Create/update PR | F11 | Push to feat/agent-context, create or update PR | NOT STARTED |

**Files to modify in trust-stream-frontend:**
- `.agent-context/GUIDE.md` (delete)
- `.agent-context/history.jsonl` (delete)
- `.agent-context/relevance.json` (delete)
- `.agent-context/snapshots/` (delete)
- `.agent-context/current/*` (all 10 files)
- `CLAUDE.md`
- `GEMINI.md`
- `AGENTS.md` (create)
- `AGENTS.MD` (check and handle)

---

## Execution Order

```
Phase 1: Agent-chorus engineering (A1-A8)
  ├── Rust verify subcommand (A1)
  ├── Node verify parity (A2)
  ├── CI mode + semantic diff (A3, A4)
  ├── Manifest provenance in seal (A5)
  ├── Separate-commit detection (A6)
  ├── CI template (A7)
  └── Conformance tests (A8)

Phase 2: Agent-chorus docs + self-update (B1-B9, C1-C2)
  ├── Update all docs (B1-B9)
  ├── Re-seal own pack (C1)
  └── Update own skill def (C2)

Phase 3: Team skills updates (D1-D4)
  ├── New branch from main
  ├── Update SKILL.md, quality bar, getting-started, architecture
  └── Push + PR

Phase 4: Stream-models regen (E1-E6)
  ├── Ensure branch is clean
  ├── Verify no chorus refs
  ├── Regenerate stale sections
  ├── Verify + update manifest
  └── Push to PR #392

Phase 5: Trust-stream-frontend cleanup + regen (F1-F11)
  ├── Remove chorus artifacts
  ├── Fix routing blocks
  ├── Create AGENTS.md
  ├── Fix operations + manifest
  ├── Regenerate stale sections
  ├── Verify
  └── Push + PR

Phase 6: Context pack viz final update (B9)
  └── Add enforcement section once all engineering is done
```

## Dependencies

- Phase 2 depends on Phase 1 (can't document what doesn't exist yet)
- Phase 3 depends on Phase 1 (skill describes what the engine does)
- Phases 4 and 5 can start after Phase 1 if enforcement is CLI-only (no CI gate needed for regen)
- Phase 6 depends on Phases 1-3 (viz should reflect final state)

## Total File Count Estimate

| Repo | Files to create | Files to modify | Files to delete |
|---|---|---|---|
| agent-chorus | 1 (CI template) | ~25 | 0 |
| team_skills | 0 | 3 | 0 |
| stream-models | 0 | ~12 | 0 |
| trust-stream-frontend | 1 (AGENTS.md) | ~12 | 4+ (chorus artifacts) |
| context-pack-viz | 0 | 1 | 0 |
| **Total** | **2** | **~53** | **4+** |
