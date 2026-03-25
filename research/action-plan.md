# Context Pack Research — Master Action Plan
**Owner:** Amit Prusty
**Started:** 2026-03-20
**Goal:** Design bulletproof context packs that make agents deliver higher quality, faster, more efficiently — and codify those learnings into agent-chorus and a standalone skill for broad adoption.

---

## How to use this plan

Work through phases in order. Each phase has a gate — don't move to the next until the gate condition is met.
Update status markers as you go: `[ ]` → `[>]` (in progress) → `[x]` (done).
Add notes inline when something is different from what was expected — those are the most valuable learnings.

---

## Phase 1 — Foundation ✅ COMPLETE

*Build the experiment infrastructure and run the first learning pass.*

- [x] Fill stream-models context pack (5 files, sealed)
- [x] Build experiment infrastructure: 3 branches, 6 tasks, schema, ground truth
- [x] Run first experiment (Claude + Codex × bare / chorus-only / context-pack)
- [x] Audit first run results — identify pitfalls
- [x] Write field findings to `research/context-pack-field-findings-2026-03-20.md`
- [x] Write design principles to `research/context-pack-design-principles.md`
- [x] Write master action plan (this file)

**Gate:** ✅ 6 pitfalls identified. Experiment infrastructure proven. Research notes written.

---

## Phase 2 — Close Known Gaps ✅ COMPLETE

*Fix what Run 1 revealed before running again.*

**Context pack fixes (stream-models):**
- [x] Add `databricks_prompt_reader.py` to CODE_MAP with inline risk note
- [x] Add `models/instructions/*.sql` to CODE_MAP as distinct high-impact path
- [x] Update BEHAVIORAL_INVARIANTS "New selector dimension" checklist row — full blast radius + silent failure warning
- [x] Add runtime prompt resolution note to SYSTEM_OVERVIEW — silent null failure mode documented
- [x] Reseal context pack, commit to `context-pack/init`

**Experiment design fixes:**
- [x] Remove chorus-only condition — narrow to bare vs context-pack
- [x] Replace H1 task: `trend-summary` (already existed) → `x-reach-score` (confirmed non-existent)
- [x] Fix `first_correct_file_hop` definition — concrete worked example added
- [x] Add `quality_self_score` (1–10) to result schema
- [x] Add session isolation enforcement to protocol (`--no-continue` instruction)
- [x] Wipe invalidated Run 1 results
- [x] Push both branches (`test/bare`, `context-pack/init`)

**Additional fixes (pre-run audit, 2026-03-20):**
- [x] 00_START_HERE: add Task-Type Routing section (impact analysis → BEHAVIORAL_INVARIANTS first; diagnosis → SYSTEM_OVERVIEW first)
- [x] 20_CODE_MAP: move incompleteness callout to before the table; add Approach column [Approach 1 / Approach 2 / Both] to every row
- [x] 30_BEHAVIORAL_INVARIANTS: add "New parameter threaded through client chain" checklist row (M2 pattern)
- [x] 30_BEHAVIORAL_INVARIANTS: add "Batch inference silent nulls" diagnostic checklist row (H2 pattern)
- [x] Reseal, commit, push `context-pack/init`

**Gate:** ✅ All known gaps closed. Branches pushed. Ready for Run 2.

---

## Experiment Runbook (reuse for every run)

**Repos and branches:**
- `~/sandbox/work/dsml/stream-models` → branch `test/bare`
- `~/sandbox/work/dsml/stream-models-context-pack` → branch `context-pack/init` (git worktree)

**Step 1 — Set up tmux session (run once, safe to re-run):**
```bash
bash ~/sandbox/work/dsml/stream-models-context-pack/tests/behaviour/scripts/setup-experiment.sh
```
Verifies branch state, kills any existing `experiment` session, creates a fresh 2×2 layout:
```
┌─────────────────────────┬─────────────────────────┐
│  Pane 1  CLAUDE  bare   │  Pane 3  CLAUDE  ctx    │
├─────────────────────────┼─────────────────────────┤
│  Pane 2  CODEX   bare   │  Pane 4  CODEX   ctx    │
└─────────────────────────┴─────────────────────────┘
```

**Step 2 — Fire Codex panes (Claude Code does this automatically):**
```bash
tmux send-keys -t experiment:1.2 'codex "read tests/behaviour/EXPERIMENT.md and follow the protocol exactly"' Enter
tmux send-keys -t experiment:1.4 'codex "read tests/behaviour/EXPERIMENT.md and follow the protocol exactly"' Enter
```

**Step 3 — Attach and manually start Claude in panes 1 and 3:**
```bash
tmux attach -t experiment
```
In **pane 1** (top-left, bare):
```
claude
> read tests/behaviour/EXPERIMENT.md and follow the protocol exactly
```
In **pane 3** (top-right, context-pack):
```
claude
> read tests/behaviour/EXPERIMENT.md and follow the protocol exactly
```

> **Note:** `--no-continue` does not exist as a flag. `claude` starts a fresh session by default — auto-resume only happens if you explicitly pass `-c/--continue`. Fresh pane from setup-experiment.sh = isolated session, no extra flag needed.

**Step 4 — After all 4 finish, collect and validate:**
```bash
bash ~/sandbox/work/dsml/stream-models-context-pack/tests/behaviour/scripts/collect-results.sh
```
Copies bare results from `stream-models` into the worktree, validates all 24 JSON files, commits to `context-pack/init`.

**Step 5 — Tell Claude Code "all runs complete"** to start reviewer grading pass.

**Notes:**
- `--no-continue` does not exist — `claude` starts fresh by default; `-c/--continue` is the opt-in resume flag
- Claude panes must be started manually (interactive); Codex can be automated via tmux send-keys
- `collect-results.sh` will exit non-zero if any of the 24 files are missing or invalid JSON
- Result files land at: `tests/behaviour/results/{agent}/{condition}/{task}.json`
- Bare results are written to `stream-models/tests/behaviour/results/` and must be copied across

---

## Phase 3 — Learning Run (Run 2) ✅ COMPLETE

**Results:**
- 24 files graded. Claude: 2→5 yes with context-pack. Codex: 2→2 (mini model, unmoved).
- 4/5 success criteria passed (SC3 partial — L2 tied, not strictly less).
- New findings: 4 new design principles (P7–P10), Codex model identified as capability ceiling variable.
- Codex will be re-run with full model before Phase 6. Mini results treated as baseline.

**New pitfalls documented:**
- prompt_sync.py universally excluded — needs human code review to confirm ground truth
- openai wrappers universally excluded (except claude/ctx) — context pack checklist row was the fix
- Codex self-scoring uncalibrated — reviewer grading non-optional

**Gate:** ✅ PASSED. 4/5 SC met, no critical new pitfalls block quality claim. Proceeding to Phase 4.

---

## Phase 4 — Improve agent-chorus Template

*Apply what we've learned to the product so every new context pack benefits.*

**`chorus context-pack init` template updates:**
- [x] CODE_MAP template: add inline `Risk` column with "Silent failure if missed" pattern
- [x] CODE_MAP template: add standard incompleteness note directing agents to grep + BEHAVIORAL_INVARIANTS
- [x] CODE_MAP template: add `[Approach 1] / [Approach 2] / [Both]` architecture tags example
- [x] SYSTEM_OVERVIEW template: add "Silent Failure Modes" subsection placeholder
- [x] BEHAVIORAL_INVARIANTS template: change-impact checklist rows must include full blast radius, not just primary files
- [x] 00_START_HERE template: Task-Type Routing section added (impact analysis → BEHAVIORAL_INVARIANTS first)
- [ ] Consider adding `50_CHANGE_IMPACT_REGISTRY.md` as optional 6th template file (deferred to next version)

**`chorus context-pack seal` validation updates:**
- [x] Check that CODE_MAP entries have non-empty Risk column
- [x] Check that BEHAVIORAL_INVARIANTS has at least one checklist row
- [x] Warn (not fail) if SYSTEM_OVERVIEW has no runtime behavior section

**agent-chorus SKILL.md update:**
- [x] Add explicit instruction: before answering impact analysis questions, read BEHAVIORAL_INVARIANTS first
- [x] Add explicit instruction: CODE_MAP is a navigation index, not an exhaustive impact list

**Gate:** ✅ PASSED. Template changes committed (v0.8.3). Seal validation updated. SKILL.md updated. Published to npm + cargo.

---

## Phase 5 — Apply Learnings Back to stream-models

*Update the stream-models context pack to match the improved template.*

- [ ] Review stream-models context pack against new template standards
- [ ] Add architecture tags (`[Approach 1]` / `[Approach 2]`) to CODE_MAP entries
- [ ] Evaluate whether `50_CHANGE_IMPACT_REGISTRY.md` is worth adding for stream-models
- [ ] Reseal, commit, push `context-pack/init`

**Gate:** stream-models context pack at template parity. Sealed and pushed.

---

## Phase 6 — Validation Run (Run 3)

*Freeze the context pack. Run a clean validation. Pre-commit to pass/fail criteria.*

**Rules for this run:**
- Context pack is frozen — no changes between run start and results
- Success criteria agreed before any results are seen
- This run is what gets presented to the team

**Merge policy — context-pack/init → main:**
- Do NOT merge to `main` until Phase 6 validation passes
- Do NOT install the pre-push hook on main until after merge
- Do NOT create a PR until the validation summary is written and pass criteria are met
- Phase 5 changes sit on `context-pack/init` locally only — remote was rolled back intentionally
- Rationale: the context pack must prove its value in this repo before it becomes part of the main branch contract

**Pre-run:**
- [x] Freeze `context-pack/init` branch (no further context pack edits until after grading)
- [x] Define and write down pass/fail criteria — written to `PHASE6_PASS_CRITERIA.md`

**Locked pass criteria (PHASE6_PASS_CRITERIA.md):**
- SC1: Claude context-pack quality score ≥ 8.0 (reviewer-graded, 5 dimensions)
- SC2: Claude context-pack completeness on M1 and M2: both ≥ partial
- SC3: Claude context-pack files_opened_count reduction vs bare: ≥ 40% on L1/L2
- SC4: At least one condition × agent achieves `yes` on H2
- SC5: Zero net new risk flags on context-pack vs bare

**External timing (required for Phase 6 — not self-reported):**
- [x] setup-experiment.sh logs start timestamps per pane; collect-results.sh logs end
- Total wall time: ~60 minutes (4 agents in parallel, 2026-03-25)

**Pre-run: resolve prompt_sync.py ground truth ambiguity**
- [x] Human code review: `_apply_filters()` uses generic dict matching — no change needed for new selectors
- [x] GROUND_TRUTH.md corrected (M1: 7→6 required files)

**Agents for Phase 6:**
- Claude: **Opus 4.6** (`claude-opus-4-6`) — upgraded from Sonnet for Run 3
- Codex: **gpt-5.4-high** — full/best model (mini was used in Run 2 — showed capability ceiling, not context-pack signal)

**Run (2026-03-25):**
- [x] Claude bare — fresh session
- [x] Claude context-pack — fresh session
- [x] Codex (full model) bare — fresh session
- [x] Codex (full model) context-pack — fresh session

**Grading:**
- [x] Full reviewer grading pass (all 24 files)
- [x] Evaluate against pre-committed pass criteria — **ALL 5 SC PASSED**
- [x] Compare external vs self-reported `duration_seconds` — wall time ~60min total
- [x] Write validation summary — `VALIDATION_SUMMARY.md` in stream-models-context-pack

**Results:** Claude ctx 6/6 yes, Codex ctx 5/6 yes. Claude bare 5/6, Codex bare 4/6. Zero net new risk flags. 57% file navigation reduction (L1+L2).

**Gate:** ✅ PASSED. All 5 SC met. Validation summary written. Decision: merge to main + present to team.

---

## Phase 6b — Structured Layer Experiment (Run 4) ✅ COMPLETE

*Test whether structured JSON artifacts improve Codex efficiency alongside Claude quality.*

**What we built:**
- [x] Structured constraint layer: routes.json, completeness_contract.json, reporting_rules.json
- [x] Agent-chorus CLI: init scaffolds + seal validates structured artifacts (Node + Rust parity)
- [x] Three-condition experiment infrastructure (bare / ctx / structured, 6-pane tmux)
- [x] Codex implemented the plan, Claude validated and corrected

**Run 4 results (2026-03-25):**
- [x] 36 results collected (2 agents × 3 conditions × 6 tasks)
- [x] Claude structured: 5/6 yes, 2.2 avg files, 12.5K tokens, 24s avg — revolutionary efficiency
- [x] Codex structured: 5/6 yes, 10.8 avg files, 14.4K tokens — quality held, files increased
- [x] Key finding: agents split into trust-and-follow (Claude) vs search-and-verify (Codex)
- [x] Stop rules don't work for Codex; search scope boundaries are the next hypothesis
- [x] v2 design document written: three-layer architecture (content / authority / navigation)
- [x] P11–P15 design principles documented

**Gate:** ✅ PASSED. Three-layer architecture validated. Trust-vs-verify agent taxonomy established. Ready for integration and generalization.

---

## Phase 7 — Integration and Release (v0.9.0)

*Ship the structured layer in agent-chorus. Fold remaining improvements into existing pack content.*

**agent-chorus CLI (v0.9.0):**
- [ ] Add search_scope.json scaffolding to init (Node + Rust)
- [ ] Add search_scope.json validation to seal
- [ ] Fold authority/derived/family markers into markdown templates (00_START_HERE stop rules, 20_CODE_MAP authority column, 30_BEHAVIORAL_INVARIANTS family semantics)
- [ ] Update AGENTS.md bootstrap template: imperative wording + search_scope.json reference
- [ ] Add negative guidance to templates ("do not enumerate _generated/ individually")
- [ ] Run full test suite (Rust + bash + npm check)
- [ ] Bump version to 0.9.0

**stream-models context pack update:**
- [ ] Fill search_scope.json with repo-specific search directories and verification shortcuts
- [ ] Add authority/derived markers to 20_CODE_MAP.md
- [ ] Add family semantics to 30_BEHAVIORAL_INVARIANTS.md
- [ ] Reseal and commit on context-pack/structured

**Publish:**
- [ ] npm-play publish
- [ ] cargo-play publish

**Gate:** v0.9.0 published. Stream-models pack updated with all three layers.

---

## Phase 8 — Generalize and Test on Second Repo

*Validate the three-layer architecture works beyond ML pipelines.*

**Second repo selection:**
- [ ] Pick a non-ML repo (agent-chorus itself is a candidate — CLI/library type)
- [ ] Run `chorus context-pack init` with v0.9.0 templates
- [ ] Fill the pack (markdown + structured artifacts)
- [ ] Run a lightweight experiment (Claude + Codex, 3-4 tasks, bare vs structured)

**Template variants:**
- [ ] Evaluate whether ML pipeline templates work for CLI/library repos
- [ ] If not, create template variants (see v2 design doc §Generalization)
- [ ] Document which template sections are universal vs repo-type-specific

**Gate:** Context pack works on 2 repo types. Template adaptations documented.

---

## Phase 9 — Showcase and Present

*Build the deliverables that communicate the story.*

**Aggregation:**
- [ ] Write aggregation script: all result JSONs → summary tables
- [ ] Include Run 3 + Run 4 data (4 runs total, 3 conditions)

**Presentation:**
- [ ] 15-minute deck: why → experiment design → two-architecture finding → results → how to adopt
- [ ] Lead with quality story (3/6 → 5/6 yes, zero risk flags)
- [ ] Show the headline stat: Claude M2 answered in 12 seconds with zero files opened
- [ ] Be honest about Codex: quality improved, efficiency is a model-level ceiling
- [ ] End with: install agent-chorus, run `context-pack init`, fill the pack

**Demo:**
- [ ] Side-by-side terminal: same task, bare vs structured
- [ ] Show Claude M2 (most dramatic) and Codex H1 (quality improvement)

**Gate:** Deck written. Demo recorded. Ready to present.

---

## Phase 10 — Document and Guide

*Turn the learnings into a reusable guide.*

- [ ] Write `docs/context-pack-guide.md` — how to fill a context pack well
- [ ] Include design principles (P1–P15), common pitfalls, worked examples
- [ ] Include the two-architecture insight and agent-specific bootstrap guidance
- [ ] Update agent-chorus README
- [ ] Write internal blog post summarizing the experiment program

**Gate:** Guide published. README updated.

---

## Status summary

| Phase | Name | Status |
|---|---|---|
| 1 | Foundation | ✅ Complete |
| 2 | Close Known Gaps | ✅ Complete |
| 3 | Learning Run (Run 2) | ✅ Complete |
| 4 | Improve agent-chorus Template | ✅ Complete (v0.8.3) |
| 5 | Apply Back to stream-models | ✅ Complete |
| 6 | Validation Run (Run 3) | ✅ Complete — ALL 5 SC PASSED |
| 6b | Structured Layer (Run 4) | ✅ Complete — three-layer architecture validated |
| 7 | Integration and Release (v0.9.0) | **▶ NEXT** |
| 8 | Generalize and Test on Second Repo | ⬜ After Phase 7 |
| 9 | Showcase and Present | ⬜ After Phase 8 |
| 10 | Document and Guide | ⬜ After Phase 9 |

---

## Research files

| File | Purpose |
|---|---|
| `action-plan.md` | This file — master sequenced plan |
| `context-pack-design-principles.md` | Accumulating universal principles from all case studies |
| `context-pack-field-findings-2026-03-20.md` | Detailed findings from stream-models Run 1 and Run 2 |
| `context-pack-next-version-agenda.md` | Next-version design agenda — staleness loop, generalisation, content gaps, init policy |
