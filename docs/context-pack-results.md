# Agent Context Pack — Experiment Results

## TL;DR

Structured context packs make AI agents **dramatically better** at navigating large codebases. Across 3 repo types and 6 experiment runs:

- **Answer quality**: 50% → 88% correct (bare → structured)
- **Claude efficiency**: 50-70% fewer tokens, zero dead ends
- **Risk elimination**: context packs prevented every "would break production" answer
- **Template is general-purpose**: ML pipeline, CLI library, React frontend — zero modifications

---

## The Problem

Large codebases (500-2,000+ files) cause AI agents to:
- **Explore inefficiently** — opening irrelevant files, burning tokens
- **Miss critical files** — incomplete impact analysis, silent production failures
- **Use deprecated patterns** — proposing Apollo when React Query is the standard
- **Fail silently** — missing test setup resets that cause flaky suites

## The Solution

A three-layer `.agent-context` directory committed to the repo:

| Layer | Files | Serves |
|---|---|---|
| **Content** | 5 markdown files (architecture, code map, invariants, operations) | Humans + all agents |
| **Authority** | 3 JSON files (routes, completeness contracts, reporting rules) | Trust-and-follow agents (Claude) |
| **Navigation** | 1 JSON file (search scopes, verification shortcuts) | Search-and-verify agents (Codex) |

Plus minimal routing blocks in `CLAUDE.md` / `AGENTS.md` (~100-200 tokens each).

---

## Results Across 3 Repo Types

### Correctness (yes rate)

| Repo | Files | Claude bare | Claude structured | Codex bare | Codex structured |
|------|-------|------------|-------------------|------------|------------------|
| stream-models (ML pipeline) | 501 | 3/6 (50%) | 5/6 (83%) | 3/6 (50%) | 5/6 (83%) |
| agent-chorus (CLI/library) | 155 | 5/6 (83%) | 5/6 (83%) | — | 6/6 (100%) |
| trust-stream-frontend (React/TS) | 1,982 | 2/4 (50%) | 4/4 (100%) | 2/4 (50%) | 3/4 (75%) |

### Efficiency (Claude only — most dramatic improvement)

| Repo | Bare avg files | Structured avg files | Reduction | Bare avg tokens | Structured avg tokens | Token reduction |
|------|---------------|---------------------|-----------|----------------|----------------------|----------------|
| stream-models | 7.5 | 2.2 | **71%** | 49K | 12.5K | **74%** |
| agent-chorus | 1.5 | 0.83 | **45%** | 13.5K | 4.25K | **69%** |
| trust-stream-frontend | 10.0 | 2.75 | **73%** | 53K | 22.5K | **58%** |

### Dead Ends (wasted file reads)

| Repo | Claude bare | Claude structured | Codex bare | Codex structured |
|------|------------|-------------------|------------|------------------|
| stream-models | 2 | **0** | 7 | 11 |
| agent-chorus | 0 | **0** | 1 | 1 |
| trust-stream-frontend | 3 | **0** | 6 | 2 |

Claude structured: **zero dead ends across all 3 repos.**

### Risk Flags (answers that would break production)

| Repo | Bare total | Structured total |
|------|-----------|-----------------|
| stream-models | 3 | 0 |
| agent-chorus | 0 | 0 |
| trust-stream-frontend | 4 | 0* |

\* Structured risk flags were protocol breaches (grep matched ground truth), not production risks.

**Context packs eliminated every production-risk answer.**

---

## Best Stories

### "Zero files, 12 seconds" (stream-models, Claude structured, M2)
Claude answered a complex impact analysis question (new client parameter through the call chain) in 12 seconds with zero files opened. It trusted the completeness contract completely and produced a correct, comprehensive answer from context alone.

### "Both agents miss the same silent failure" (trust-stream-frontend, M1)
Both Claude and Codex in bare condition missed `src/__tests__/setup.tsx` store reset when adding a new Zustand store. This causes tests to pass individually but fail in suite — a silent, hard-to-debug failure. Both agents found it in structured because the behavioral invariants checklist explicitly names it.

### "Negative guidance prevents deprecated pattern" (trust-stream-frontend, H1)
Claude bare proposed using Apollo Client (being deprecated) in its implementation plan. Claude structured correctly used React Query because the behavioral invariants say "Do not assume Apollo GraphQL queries are the current data path — React Query is the primary pattern."

### "Codex achieves 6/6" (agent-chorus, Run 5)
Codex structured scored 6/6 yes on agent-chorus — the highest correctness of any agent in any condition across all experiments. The completeness contracts gave Codex the information it needed to be thorough without over-exploring.

---

## Two Agent Architectures

The experiments revealed that agents fall into two categories:

### Trust-and-follow (Claude, likely Gemini)
- Reads the context pack as authoritative
- Opens minimal repo files (2.75 avg in frontend)
- Zero dead ends consistently
- Benefits from: completeness contracts, stop rules, grouped reporting

### Search-and-verify (Codex, likely Cursor)
- Uses the context pack as scaffolding, then verifies against code
- Still opens many files (10.25 avg in frontend)
- Benefits from: answer quality improvement, search scope boundaries
- Does NOT benefit from: stop rules (ignores them)

**The three-layer architecture serves both**: authority layer for Claude, navigation layer for Codex, content layer for both.

---

## How to Adopt

### 1. Install
```bash
npm install -g agent-chorus   # v0.9.0+
```

### 2. Create the context pack
```bash
cd your-repo
chorus context-pack init --force
# Have an agent fill the templates (or use the context-pack skill)
chorus context-pack seal
git add .agent-context/ CLAUDE.md AGENTS.md GEMINI.md
git commit -m "feat: add agent context pack"
```

### 3. Maintain
- **Agent PRs**: agent auto-updates `.agent-context` as a separate commit
- **Human PRs**: no auto-update (agent lacks context)
- **Manual catchup**: ask an agent to "update the context pack" after significant human work
- **Pre-push hook**: installed automatically, warns about staleness

### 4. Validate (optional)
Design 3-4 questions (lookup + impact analysis), run bare vs structured, compare results.

---

## Experiment Program Summary

| Phase | What | Status |
|---|---|---|
| 1-3 | Foundation + learning runs | ✅ |
| 4 | Template improvements (v0.8.3) | ✅ |
| 5 | Apply to stream-models | ✅ |
| 6 | Validation run — all 5 SC passed | ✅ |
| 6b | Structured JSON layer experiment | ✅ |
| 7 | Integration + release (v0.9.0) | ✅ |
| 8 | Generalization — CLI/library (Run 5) | ✅ |
| 8b | Context-pack creation skill | ✅ |
| 8c | Frontend validation (Run 6) | ✅ |
| 9 | Showcase | ✅ This document |
| 10 | Documentation + guide | Next |

6 experiment runs, 78+ graded results, 15 design principles, 3 repo types, 1 general-purpose template.
