# Agent Context — Team Presentation

**Duration:** 15 minutes
**Audience:** Engineering team
**Goal:** Explain what agent context is, show the evidence, get adoption

---

## Slide 1: The Problem (2 min)

**"AI agents get lost in large repos"**

When you ask Claude or Codex to work on a 500+ file repo:
- They open 10-18 files per task, many irrelevant
- They miss critical files in impact analysis → silent production bugs
- They propose deprecated patterns (Apollo instead of React Query)
- They burn 50-100K tokens exploring

**This costs time, money, and trust.**

---

## Slide 2: The Solution (2 min)

**A `.agent-context` directory in your repo**

```
.agent-context/current/
  00_START_HERE.md           ← "Read this first"
  10_SYSTEM_OVERVIEW.md      ← Architecture + silent failure modes
  20_CODE_MAP.md             ← Navigation index with risk ratings
  30_BEHAVIORAL_INVARIANTS.md ← Change checklists + what NOT to do
  routes.json                ← Task routing for agents
  completeness_contract.json ← "These files MUST be in your answer"
  search_scope.json          ← "Search HERE, not THERE"
```

Plus 2-3 imperative sentences in `CLAUDE.md` / `AGENTS.md` / `GEMINI.md` pointing agents to the pack.

**One-time setup. Auto-maintained on agent PRs.**

---

## Slide 3: The Evidence — Quality (3 min)

**Correct answers: bare vs structured**

```
stream-models (ML pipeline, 501 files)
  Claude:  50% → 83%    Codex:  50% → 83%

agent-chorus (CLI/library, 155 files)
  Claude:  83% → 83%    Codex:   — → 100%

trust-stream-frontend (React/TS, 1,982 files)
  Claude:  50% → 100%   Codex:  50% → 75%
```

**Agent context cuts incorrect answers in half.**

---

## Slide 4: The Evidence — Efficiency (2 min)

**Claude with agent context (averages across all repos):**

| Metric | Bare | Structured | Change |
|--------|------|-----------|--------|
| Files opened | 6-10 | 1-3 | **-70%** |
| Tokens used | 40-53K | 4-22K | **-60%** |
| Dead ends | 2-3 | 0 | **-100%** |
| Duration | 90-180s | 25-45s | **-65%** |

**The "zero files" moment:** Claude answered a complex impact analysis in 12 seconds with zero files opened. Pure context.

---

## Slide 5: The Evidence — Safety (1 min)

**Risk flags: answers that would break production if acted on**

| Condition | Risk flags |
|-----------|-----------|
| Bare | 7 total across all repos |
| Structured | **0** |

Examples prevented:
- Missing `schemas/inference.py` → parameter silently dropped
- Missing `setup.tsx` store reset → flaky test suite
- Using Apollo (deprecated) instead of React Query

**Agent context eliminated every production-risk answer.**

---

## Slide 6: How It Works — Two Agent Types (2 min)

**Claude (trust-and-follow):**
Reads the pack → trusts the completeness contracts → opens minimal files → done.

**Codex (search-and-verify):**
Reads the pack → still greps the repo → but now knows WHAT to look for and WHERE to stop.

**Same pack, different layers serve each agent:**
- Markdown → both agents + humans
- JSON contracts → Claude trusts them as authoritative
- Search scopes → Codex uses them to focus exploration

**Key insight from Codex experiments (P13):** Don't prescribe when to stop — bound where to search. `search_directories` and `exclude_from_search` work; `stop_after` rules are ignored.

---

## Slide 7: Agent Context vs Skills (2 min)

**Skills = reusable process** (how do we do X?)
**Agent context = repo-specific truth** (what's in this repo, what's risky, what to avoid?)

| | Skill | Agent Context |
|---|---|---|
| **Scope** | Cross-repo (works anywhere) | This repo only |
| **Trigger** | On demand | Always-on (every session) |
| **Content** | How to do a task | What's here + what's dangerous |
| **Token cost** | Loaded when needed | ~4500 tokens per session |
| **Maintenance** | Manual, versioned | Semi-auto (agent PRs) |

**The stray skill problem:** Without agent context, teams create skills that are really repo knowledge:
- "Always check setup.tsx when adding stores" → that's a **behavioral invariant**
- "Important files in our frontend" → that's a **code map**
- "Don't use Apollo" → that's **negative guidance**
- "Our CI pipeline" → that's **operations**

**Agent context absorbs these.** The stray skills die.

**What stays as skills:**
- Process patterns (code review, ADR, PR review)
- Generators (slides, documentation)
- Integrations (Jira, Databricks metadata)
- Teaching (backend mentor, PR learner)
- Cross-repo workflows (model migration, prompt registration)

**The clean model:**
```
Skill:  "How to add a Zustand store" (reusable process)
Context: "When adding a store, also update src/__tests__/setup.tsx" (this repo's invariant)
```

The skill tells you the process. Agent context tells you the files and risks. Both together = an agent that knows how AND where.

---

## Slide 8: The Headline Story (1 min)

**trust-stream-frontend, M1 task: "Add a new Zustand store"**

Both Claude and Codex in **bare** mode missed `src/__tests__/setup.tsx` — the store reset that prevents flaky tests. No error tells you it's missing. Tests pass individually, fail in suite.

Both agents in **structured** mode found it — because the behavioral invariants checklist says:

> "Zustand store schema change → `src/__tests__/setup.tsx` (store reset). Silent failure if missed — tests pass individually but fail in suite."

**That one line in the agent context prevents a week of debugging flaky tests.**

---

## Slide 9: How to Get It (2 min)

**Self-contained — no external CLI needed.**

Install the `agent-context` skill from team_skills:

```bash
npx skills add Edelman-DxI/team_skills --skill agent-context --agent cursor claude-code codex
```

Then open a session in your repo and say:

> "Create a context pack for this repo"

The agent reads the repo, fills all 9 files, validates, self-tests, and commits. Takes ~10-15 minutes for a large repo.

**Agents keep it fresh:**
- Agent PRs include `.agent-context` updates as a separate commit
- `chorus agent-context verify --ci` as a PR gate — fails if code changed but pack wasn't updated
- Pre-push hook warns about staleness (advisory, never blocks)
- After human-only work: "update the context pack" — agent diffs and proposes per-section patches

Full guide: `team_skills/skills/agent-context/references/getting-started.md`

---

## Slide 10: What's Next (1 min)

**Already done:**
- [x] `agent-context` skill in team_skills (PR #10 merged)
- [x] `.agent-context` created for stream-models (PR #392)
- [x] `.agent-context` created for trust-stream-frontend
- [x] 16 design principles (P1–P16) validated across 3 repo types
- [x] Getting started guide for teammates
- [x] Standardized naming: `.agent-context` everywhere (CLI, skill, directory)
- [x] `chorus agent-context verify --ci` for PR enforcement gates
- [x] CI template for teams to copy (`templates/ci-agent-context.yml`)

**Coming next:**
- [ ] **Agent Context Map** — cross-repo routing layer (~500 tokens tells the agent which repos matter, how they connect, what cascades across repo boundaries)
- [ ] **Adopt on 2-3 more team repos** (start with the ones agents use most)
- [ ] **Cross-repo invariants** — "change X in stream-models → must update Y in trust-stream-frontend"
- [ ] **Live demo** — end-to-end prompt registration on Databricks dev workspace

**Agent context is infrastructure for AI-assisted development.
The more repos have it, the better every agent works.**

---

## Appendix: Research Program

- **7 experiment runs** across 3 repo types (including P16 field test)
- **78+ graded results** against ground truth
- **16 design principles** derived from data (P1–P16)
- **3 layers** validated: content (markdown), authority (JSON contracts), navigation (search scopes)
- **1 template** — works for ML pipelines, CLI tools, React frontends with zero modifications
- **Naming convention:** `.agent-context/` is the standard directory name across all repos
- **CLI:** `chorus agent-context` (v0.10.0) — `context-pack` still works as deprecated alias

Full research: `agent-chorus/research/`
Skill: `team_skills/skills/agent-context/`
Getting started: `team_skills/skills/agent-context/references/getting-started.md`
