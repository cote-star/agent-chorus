# Skill Test Plan: trust-stream-frontend

**Repo:** `Edelman-DxI/trust-stream-frontend`
**Type:** React/TypeScript frontend (Vite, Vitest, Storybook, Playwright)
**Size:** 1,982 files (1,709 TS/TSX, 582 test files)
**Location:** `/Users/e059303/sandbox/work/dsml/trust-stream-frontend`

This is the third repo type we're testing (after ML pipeline and CLI/library).
It's also the first test of the skill itself — an agent using `SKILL.md` to
create a context pack from scratch.

---

## Phase 1 — Skill Execution: Create

**Goal:** Use the context-pack skill to create `.agent-context` for
trust-stream-frontend. Validate the skill produces a complete, sealable pack.

**Steps:**
1. [ ] Read `skills/context-pack/SKILL.md` and follow the Create flow
2. [ ] Run `chorus context-pack init --force` in the repo
3. [ ] Fill all 9 files (5 markdown + 4 JSON) by reading the repo
4. [ ] Seal the pack: `chorus context-pack seal --force`
5. [ ] Self-test: generate 3 questions + ground truth, evaluate pack quality
6. [ ] Commit the pack

**Success criteria:**
- Seal passes without errors
- All markdown files have content (no unfilled template markers)
- All JSON artifacts have repo-specific entries (no all-empty arrays)
- Self-test confirms pack helps on at least 2 of 3 questions
- CLAUDE.md/AGENTS.md routing blocks under 200 tokens

**Timing:** Record how long the full Create flow takes (target: 5-10 min).

---

## Phase 2 — Validation Experiment

**Goal:** Run a controlled experiment to confirm the skill-generated pack
actually improves agent performance.

**Design:**
- 2 conditions: bare vs structured (skill-generated pack)
- 2 agents: Claude Opus 4.6, Codex gpt-5.4-high
- 4 tasks (scaled down from 6 — this is validation, not research):
  - L1: Lookup — find a specific configuration value or component
  - M1: Impact analysis — list files that must change for a specific feature
  - H1: Planning — implementation plan for a new feature
  - H2: Diagnosis — debug a specific frontend issue

**Setup:**
- Create `test/bare` branch (strip .agent-context)
- Create `test/structured` branch (keep full pack)
- Set up 2×2 tmux experiment

**Pass criteria:**
- At least one agent shows improvement in structured vs bare
- No risk flags on structured condition
- Claude structured uses fewer tokens than Claude bare

---

## Phase 3 — Evaluate Template Generalization

**Goal:** Assess whether the v0.9.0 templates needed adaptation for a
React/TypeScript frontend.

**Questions to answer:**
- Did the template sections map naturally to frontend concepts?
- Were any sections irrelevant (e.g., "Silent Failure Modes" for a frontend)?
- Were any sections missing that a frontend repo needs?
- Did the structured JSON artifacts (routes, contracts, scopes) adapt well?

**Deliverable:** Notes in `evolution/01-frontend-test.md` documenting:
- What worked out of the box
- What needed adaptation
- What's missing for frontend repos
- Recommendation: template variants or universal template?

---

## Phase 4 — Document Findings

**Goal:** Update skill design based on findings.

- [ ] Update `evolution/` log
- [ ] Update `SKILL.md` if the flow needs adjustments
- [ ] Update `research/experiment-summary.md` with trust-stream-frontend data
- [ ] Note any template improvements needed for v0.9.1

---

## Repo Quick Reference

```
trust-stream-frontend/
  src/
    components/     # React components (largest dir)
    pages/          # Route pages
    hooks/          # Custom React hooks
    api/            # API client layer
    services/       # Business logic services
    stores/         # State management (Zustand likely)
    queries/        # React Query definitions
    types/          # TypeScript type definitions
    context/        # React context providers
    lib/            # Utility libraries
    loaders/        # Route loaders
    constants/      # App constants
  tests/            # Playwright E2E tests
  mocks/            # MSW mock handlers
  .storybook/       # Storybook configuration
  .cursor/          # Cursor AI skills (already has some)
  .github/          # CI workflows
  docs/             # Documentation
```

**Stack:** React 18+, TypeScript, Vite, Vitest, Storybook, Playwright,
TailwindCSS, React Query, MSW, Vercel deployment.

**Key patterns to document in pack:**
- Component architecture (atomic? feature-based?)
- API client → React Query → component data flow
- Testing strategy (unit + Storybook + E2E)
- State management approach
- Routing structure
- CI/CD pipeline (Vercel + GitHub Actions)
