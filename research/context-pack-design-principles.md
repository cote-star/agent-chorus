# Context Pack Design Principles
**Status:** Living document — updated after each experiment case study
**Last updated:** 2026-03-25 (stream-models, Runs 1–4; P11–P15 added from structured layer experiments)

---

## Purpose

This document accumulates design principles for context packs derived from real experiments.
Each principle is labelled with its source and its **scope** — not all principles apply to all repo types.

**Scope legend:**
- `[all repos]` — apply regardless of repo type, size, or stack
- `[pipeline/service]` — apply when there is runtime state, async execution, or shared infrastructure
- `[coexisting architectures]` — apply when a repo has multiple active patterns or is mid-migration
- `[complex repos]` — apply when the repo has >100 files or multiple distinct subsystems

> **Validation status:** P1–P10 are derived from stream-models (ML pipeline, 501 files). P11–P15
> are derived from the same repo but specifically from cross-agent comparison (Claude Opus 4.6 vs
> Codex gpt-5.4-high) across 4 experiment runs. Principles marked `[all repos]` are plausible
> generalisations but not yet validated on other repo types.

---

## Principles

### P1 — The index must declare its own incompleteness
**Scope:** `[all repos]`
**Source:** stream-models Run 1 (M1 impact analysis task)
**What happened:** CODE_MAP listed ~13 high-impact paths. Agents treated this as a complete list and stopped searching when the list ran out. Bare agents found 2 additional critical files by exploring freely.
**Principle:** CODE_MAP must include an explicit note that it is a navigation index, not an exhaustive impact registry. For tasks that require listing all affected files, agents must be directed to grep and to the BEHAVIORAL_INVARIANTS checklist — not just trust the map.
**Template fix:** Add standard note above CODE_MAP table: *"This table is a navigation index, not a complete impact list. For impact analysis tasks, cross-reference `30_BEHAVIORAL_INVARIANTS.md` and verify with grep."*

---

### P2 — Risk callouts belong inline, not only in a separate file
**Scope:** `[all repos]`
**Source:** stream-models Run 1 (M1 task, SQL UDF miss)
**What happened:** BEHAVIORAL_INVARIANTS had the correct checklist but agents doing impact analysis never cross-referenced it mid-task. The risk information existed but was in the wrong location relative to where agents were navigating.
**Principle:** The highest-risk paths in CODE_MAP need an inline warning at the point of navigation. A separate invariants file is necessary but not sufficient — the first-line risk signal must be where the agent is looking.
**Template fix:** Add a `Risk` column to the CODE_MAP table. Entries where a miss causes silent production failure should say: **"Silent failure if missed"** — not just "High".

---

### P3 — Runtime behavior is as important as code structure
**Scope:** `[pipeline/service]` — applies when there is async execution, shared infrastructure, or silent failure modes. Less applicable to stateless libraries or simple CLIs.
**Source:** stream-models Run 1 (H2 diagnosis task)
**What happened:** No agent in any condition correctly identified prompt selector miss as the top root cause of silent null rows. The context pack documented code structure but not what happens at runtime when a selector has no match.
**Principle:** SYSTEM_OVERVIEW must document runtime failure modes, not just architecture. Any code path where a silent failure can occur (null return, silent drop, no error log) must be called out explicitly — these are the things hardest to find by reading code and most valuable to have written down.
**Template fix:** Add a "Silent Failure Modes" subsection to SYSTEM_OVERVIEW for any system with async processing, UDF execution, or schema validation at runtime.

---

### P4 — Change impact completeness matters more than navigation speed
**Scope:** `[complex repos]` — most important when a change to one file has non-obvious downstream effects. In small repos with low coupling, the index can be exhaustive.
**Source:** stream-models Run 1 (M1/M2 vs L1/L2)
**What happened:** Context pack dramatically improved navigation speed (L2: 5 hops → 1). But on impact analysis tasks the context pack agents were *less complete* than bare agents because the index didn't list all affected files.
**Principle:** For production codebases, quality of impact analysis is the highest-value metric. A context pack that makes agents faster but less complete on change impact tasks is net-negative — it increases production risk while reducing tokens. Speed is secondary to completeness on complex tasks.
**Template fix:** Consider a dedicated `50_CHANGE_IMPACT_REGISTRY.md` for repos where change blast radius is complex — a lookup table: change type → complete file list → invariants to check → commands to run.

---

### P5 — Claude and Codex navigate differently; one format may not serve both
**Scope:** `[all repos]` — agent-format mismatch is independent of repo type.
**Source:** stream-models Run 1 (aggregate efficiency comparison)
**What happened:** Claude followed the structured index closely (L2: 5 hops → 1, dramatic win). Codex opened 22 files on the same task regardless of the context pack — barely different from its bare behavior.
**Principle:** Context packs as structured prose tables are Claude-idiomatic. Codex appears to do grep-first, read-later exploration. A format that leads with concrete grep patterns and file paths before explanatory prose may serve both agents better.
**Open question:** Does Codex benefit from a different index format entirely, or just a different entry point in `00_START_HERE.md`?

---

### P6 — Coexisting architectures need explicit boundary documentation
**Scope:** `[coexisting architectures]` — specifically for repos mid-migration or with multiple active patterns sharing the same codebase. Less relevant for greenfield or fully-migrated repos.
**Source:** stream-models Run 1 (general navigation confusion)
**What happened:** Stream-models has two active approaches (Approach 1 model-centric, Approach 2 prompt-centric) sharing the same codebase. Agents sometimes applied reasoning from one approach to files that belonged to the other.
**Principle:** Repos with multiple coexisting architectural patterns need an explicit "which approach does this file belong to" reference. Not just "here are the important files" but "here's the boundary between approaches and why it matters before you make a change."
**Template fix:** In CODE_MAP, tag each entry with its approach where relevant: `[Approach 1]`, `[Approach 2]`, `[Both]`.

---

### P7 — Checklist rows in BEHAVIORAL_INVARIANTS prevent specific exclusion errors
**Scope:** `[all repos]` — any repo with multi-file change patterns benefits from explicit blast-radius rows.
**Source:** stream-models Run 2 (M2 task, openai wrappers)
**What happened:** Every agent in every condition excluded `openai_responses.py` and `openai_async_responses.py` from the M2 required file list — except claude/context-pack. The context pack had an explicit "New parameter threaded through client call chain" checklist row that listed both files by name. That one row was the difference between a partial and a yes.
**Principle:** BEHAVIORAL_INVARIANTS checklist rows are the most targeted intervention available. A well-written row can prevent a specific, systematic exclusion error that affects all agents in all conditions. The checklist must name files explicitly, not describe patterns generically.
**Template fix:** Each change-type row must name the full blast radius by file path, not just by description. "All client files" is not enough — list them.

---

### P8 — Zero dead ends is the strongest single efficiency signal
**Scope:** `[all repos]`
**Source:** stream-models Run 2 (aggregate dead-end comparison)
**What happened:** Claude/bare had 6 dead ends across all tasks. Claude/context-pack had 0. Every file opened with context pack was relevant. Files-opened count dropped from 6.5 to 3.3 average. But the dead-end elimination is the cleaner signal — it means the context pack isn't just helping agents go faster, it's eliminating wasted work entirely.
**Principle:** Track dead ends (files opened that turned out to be irrelevant) as the primary efficiency metric, not files-opened count alone. A context pack that reduces files opened from 10 to 8 may be marginal; one that eliminates all dead ends is transformative.
**Template fix:** In `00_START_HERE.md`, make the entry point directive explicit enough that agents never need to open a file to discover it's the wrong one. The directive should point to the exact file, not the directory.

---

### P9 — Model capability caps context pack effectiveness
**Scope:** `[all repos]`
**Source:** stream-models Run 2 (Codex mini vs Claude Sonnet 4.6)
**What happened:** Codex (mini model) showed zero grade improvement from context pack (2 yes in both conditions) and near-identical file exploration behaviour (M1: 25 bare vs 24 ctx). Claude (Sonnet 4.6) showed a 2→5 yes improvement. The context pack was identical; the agents were not.
**Principle:** A context pack's effectiveness is bounded by the model's ability to follow structured instructions and synthesise indexed information. Below a capability threshold, context packs don't constrain exploration — agents just read the context pack and then explore anyway. Test with the best available model before concluding a context pack doesn't help.
**Implication:** Context pack design should target capable models first. Weaker models may need a simpler, more imperative format ("open exactly these files, in this order, stop when you have the answer").

---

### P10 — Agent self-scores are not a reliable quality signal
**Scope:** `[all repos]` — applies to any experiment or quality dashboard that uses agent self-reporting.
**Source:** stream-models Run 2 (Codex self-scoring pattern)
**What happened:** Codex self-reported "partial" on L1 and L2 (reviewer grade: yes), and scored 8.2/10 in both bare and context-pack despite identical reviewer grades. Codex's self-scores did not track actual quality changes. Claude's self-scores were better calibrated — the 8.5 ctx vs 7.2 bare gap correctly tracked the real grade lift.
**Principle:** Agent self-scores are a useful signal for within-agent confidence tracking but must not be used as a proxy for quality in cross-condition comparisons. Reviewer grading against a ground truth is non-optional for any experiment that makes quality claims.
**Implication for agent-chorus:** Any future "quality dashboard" that relies on agent self-scores needs a reviewer correction layer, or it will produce misleading comparisons across agents.

---

### P11 — Content is universal; routing is agent-specific
**Scope:** `[all repos]`
**Source:** stream-models Run 4 (three-condition: bare / ctx / structured)
**What happened:** The same markdown content helped Claude dramatically (7.5 → 2.2 files avg) but did not reduce Codex's file count (7.3 → 10.8). Adding structured JSON artifacts (routes, contracts, reporting rules) helped both agents' answer quality equally (5/6 yes each) but through completely different mechanisms — Claude used them as authoritative answers, Codex used them as search scaffolding.
**Principle:** One context pack, multiple bootstrap paths. The content layer (markdown) is shared. The authority layer (JSON contracts) serves trust-and-follow agents. The navigation layer (search scopes) serves search-and-verify agents. Agent-specific bootstrap wording in CLAUDE.md / AGENTS.md / GEMINI.md routes each agent to the right layer.

---

### P12 — Authority layer for trust-agents; navigation layer for verify-agents
**Scope:** `[all repos]`
**Source:** stream-models Run 4 (structured condition analysis)
**What happened:** Claude-structured M2 answered in 12 seconds with zero files opened — it trusted the completeness_contract.json entirely. Codex-structured M2 opened 13 files and had 4 dead ends despite having the same contract — it verified every file in the contract against code. The same artifact served completely different purposes for the two agent types.
**Principle:** Don't try to make one structured layer serve both agent architectures. Trust-agents need completeness contracts (what must be in the answer). Verify-agents need search scope boundaries (where to look, where not to look). These are separate concerns with separate artifacts.

---

### P13 — Bound the search space, don't prescribe the stop point
**Scope:** `[all repos]`
**Source:** stream-models Run 4 (Codex structured dead ends: 11, up from 6 in ctx)
**What happened:** reporting_rules.json included `stop_after` and `optional_verify_budget` rules. Codex ignored them — dead ends actually increased. Codex's exploration is not linear (search → find → verify → stop). It continuously searches, reads, cross-references. Stop rules don't match this architecture.
**Principle:** For search-and-verify agents, the effective constraint is WHERE they search, not WHEN they stop. `search_directories` and `exclude_from_search` are the right primitives. `stop_after` and `verify_budget` are ignored. Design accordingly.

---

### P14 — Verification shortcuts must include line ranges
**Scope:** `[complex repos]`
**Source:** stream-models Run 4 (Codex structured H2: 10 files, 0 dead ends)
**What happened:** Codex H2 in structured condition had zero dead ends (vs 2 in ctx). The completeness_contract.json listed specific code paths, which Codex used as a checklist to verify rather than exploring freely. But it still read 10 full files because the contract named files, not line ranges.
**Principle:** Verification shortcuts should include approximate line ranges or function names, not just file paths. `"_base.py:131-135: _create_empty_result"` lets an agent check a specific location. `"_base.py"` forces a full-file read. Line ranges go stale faster but save the most exploration.

---

### P15 — Derived files are evidence, never edit targets
**Scope:** `[all repos]` — any repo with generated/compiled/bundled output files.
**Source:** stream-models Run 4 (Codex M1: enumerated 38 _generated/*.yml files individually)
**What happened:** Despite reporting_rules.json containing `"never_enumerate_individually": ["models/assets_gen/_generated/*.yml"]`, Codex listed every generated file as a change target. The rule was present but not forceful enough — Codex saw the files on disk and included them.
**Principle:** Generated/derived files must be excluded at the search scope level (`exclude_from_search`), not just the reporting level. If an agent can see the files, it will list them. Preventing enumeration requires preventing discovery.

---

## Open questions (not yet resolved by experiment data)

- **What is the right size for a context pack?** Five markdown files + 3-4 JSON artifacts is the current shape. May need adaptation for monorepos.
- ~~Does context pack help Codex when running the full/best model?~~ **Resolved (Run 3):** Yes — Codex full model went from 4/6 bare to 5/6 ctx.
- ~~Is prompt_sync.py actually required for M1?~~ **Resolved (human review):** No — generic dict matching handles any selector. Ground truth corrected.
- **Is there a repo complexity threshold below which a context pack adds no value?** Simple repos with <50 files may not benefit. Needs testing.
- ~~Does a context pack format tuned for grep-first agents help?~~ **Partially resolved (Run 4):** Structured JSON helped Codex answer quality (5/6) and token efficiency (14.4K avg) but did NOT reduce file opens or dead ends. A navigation-scoping layer (search_scope.json) is the hypothesized next fix.
- **Is Cursor trust-and-follow or search-and-verify?** Needs a Cursor experiment run.
- **Is Gemini trust-and-follow?** Likely, but needs validation.
- **Can search_scope.json be auto-generated from completeness_contract.json?** The search directories could be inferred from file paths. But explicit is better for v1.

---

## Case studies

| Date | Repo | Agents | Run | Key finding |
|---|---|---|---|---|
| 2026-03-20 | stream-models | Claude, Codex | Run 1 (invalidated) | 6 pitfalls identified; context pack gaps cause completeness regression on M1/M2; chorus-only inflates Claude tokens 2.5x |
| 2026-03-20 | stream-models | Claude Sonnet 4.6, Codex mini | Run 2 (complete) | Post-fix: Claude 2→5 yes, 0 dead ends, 61% duration reduction. Codex mini unmoved — model capability ceiling. 4/5 success criteria passed. |
| 2026-03-25 | stream-models | Claude Opus 4.6, Codex gpt-5.4-high | Run 3 (validation) | Claude 6/6 yes, Codex 5/6 yes. All 5 SC passed. 57% file navigation reduction. Full model unlocked Codex improvement. |
| 2026-03-25 | stream-models | Claude Opus 4.6, Codex gpt-5.4-high | Run 4 (structured) | Three conditions. Structured: Claude 2.2 avg files (down from 7.5 bare), 12.5K tokens, 24s avg. Codex 5/6 yes but 10.8 files (up from 7.3 bare). Stop rules don't work for Codex; search scope boundaries are the next hypothesis. |
