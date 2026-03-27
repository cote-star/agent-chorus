# Context Pack — Next Version Improvement Agenda
**Created:** 2026-03-23
**Last reviewed:** 2026-03-27
**Status:** Living document — items marked ✅ are resolved, remaining items are future work
**Scope:** Improvements to agent-chorus `context-pack` subsystem

---

## How to use this document

These are the findings and gaps that need design decisions before being built.
Each item has a severity rating:

- 🔴 **Critical** — undermines the core value proposition if not fixed
- 🟡 **Important** — reduces effectiveness or limits adoption
- 🟢 **Enhancement** — improves quality or breadth of use cases

Work through Critical items first. Items within a category are independent unless noted.

---

## Category 1 — Staleness Loop (🔴 Critical)

The biggest structural gap in the current design. A context pack that goes stale silently is
worse than no context pack — it gives agents confident but outdated navigation.

### 1.1 — Hook is advisory-only; it does not update anything

**Current state:** `sync_main.cjs` (the `pre-push` hook handler) detects context-relevant file
changes and prints an advisory: *"Update pack content with your agent, then run
`chorus context-pack seal`."* It does not call an LLM, does not write any files, does not
trigger a seal.

**Gap:** The warning is only useful if a human reads it and acts. In practice, pushes happen
under time pressure. The advisory gets ignored. The context pack drifts.

**What it should do:** Detect which context pack *sections* are affected by the changed files,
then either (a) trigger an agent to update only those sections, or (b) mark specific sections as
stale in `manifest.json` so agents reading the pack know which parts to verify before trusting.

**Design question:** Auto-update requires an LLM call in a pre-push hook — which is slow and
blocks the push. Better approach may be async: push succeeds, a post-push CI job or background
agent runs the targeted section update and commits it back.

---

### 1.2 — Hook not installed during `chorus context-pack init` ✅ FIXED in v0.8.2

**Current state:** Fixed. `init` now auto-installs the pre-push hook. `seal` warns if missing.

**Gap:** The stream-models repo has had a context pack since early 2026-03-20 and the hook was
never installed. There is no warning that it isn't installed. The entire staleness-detection
system was silently inactive.

**What it should do:** `chorus context-pack init` should install the hook automatically as part
of setup. If the hook can't be installed (non-git dir, permissions), it should warn explicitly —
not silently skip.

**Also:** `chorus context-pack seal` should check whether the hook is installed and warn if not.
One line: *"[context-pack] WARN: pre-push hook is not installed — run
`chorus context-pack install-hooks` to enable staleness detection."*

---

### 1.3 — No mapping from source files to context pack sections

**Current state:** `sync_main.cjs` can detect which source files changed. It has no knowledge of
which context pack section those files belong to. It therefore warns at the pack level ("the
pack may be stale") not at the section level ("CODE_MAP row for `models/src/modeling/clients/`
may be stale").

**Gap:** Section-level staleness is what agents need. "The pack might be stale" is too coarse to
act on. "CODE_MAP and BEHAVIORAL_INVARIANTS may be stale; SYSTEM_OVERVIEW and OPERATIONS are
likely fine" lets an agent read selectively and verify only the affected parts.

**What it should do:** `.agent-context/relevance.json` (already exists) maps file patterns to
relevance. Extend it or add a parallel `section-map.json` that maps file patterns to context
pack sections. `sync_main.cjs` then outputs section-level staleness markers, written into
`manifest.json` as `stale_sections: ["20_CODE_MAP", "30_BEHAVIORAL_INVARIANTS"]`.

---

### 1.4 — No CI/CD hook for GitHub-hosted repos

**Current state:** Hook is local-only (`pre-push`). For repos with GitHub Actions CI, there is
no workflow that checks or updates the context pack on merge to main.

**Gap:** Pre-push hooks are local — they only fire if the developer has the hook installed. A
force-push or a merge via GitHub UI bypasses the hook entirely.

**What it should do:** `chorus context-pack init` should optionally scaffold a
`.github/workflows/context-pack-freshness.yml` that runs `chorus context-pack check-freshness`
on every PR and `chorus context-pack seal` (or marks stale sections) on merge to main.

---

## Category 2 — Init and Setup Policy (🔴 Critical)

### 2.1 — `chorus context-pack init` does not set up the full sync policy

**Current state:** `init` creates the 5 template files and a `manifest.json`. It does not:
- Install the pre-push hook
- Create `relevance.json` (or guide the user through it)
- Scaffold a CI workflow
- Explain the ongoing maintenance commitment to the user

**Gap:** After `init`, the user has a context pack but no mechanism to keep it fresh. They don't
know this. The pack will go stale on the first commit that touches covered files.

**What init should do (in order):**
1. Create the 5 template files (current)
2. Create a starter `relevance.json` with sensible defaults for the repo type detected
3. Install the pre-push hook
4. Optionally scaffold `.github/workflows/context-pack-freshness.yml`
5. Print a summary: "Hook installed. Relevance rules written to `.agent-context/relevance.json`.
   Edit that file to tune what triggers a staleness check. Run `seal` after filling in the
   templates."

---

### 2.2 — No repo-type detection at init time ✅ RESOLVED (not needed)

**Resolution (2026-03-27):** Runs 5 and 6 validated that the same template works across
ML pipeline (501 files), CLI/library (155 files), and React/TS frontend (1,982 files)
with zero modifications. Template variants are not needed — the template sections are
universal, the content adapts per repo. The skill fills repo-specific content.

Monorepo support remains unvalidated.

---

## Category 3 — Content Gaps in the 5-File Design (🟡 Important)

These are structural omissions — information classes that matter for agent quality but have no
home in the current file structure.

### 3.1 — "Why" is not captured anywhere

**Current state:** Every file captures *what* and *how*. Nothing captures *why* things are
designed the way they are.

**Examples from stream-models:**
- Why does the dual-approach (Approach 1 / Approach 2) architecture still coexist? (migration
  in progress — context agents need to avoid applying Approach 1 patterns to Approach 2 work)
- Why does `_apply_filters()` use generic dict matching? (intentional, dimension-agnostic design)
- Why are prompt UDFs in Unity Catalog rather than inline? (runtime constraint, not tech debt)

**Gap:** Agents that don't know *why* make wrong assumptions. They may refactor intentional
patterns, flag working code as broken, or pick the wrong approach for a new feature.

**Options:**
- Add a "Design Decisions" subsection to `10_SYSTEM_OVERVIEW.md`
- Add a `Why` column to `20_CODE_MAP.md` rows (alongside existing Risk column)
- Template note: the most important "why" to capture is any non-obvious constraint that
  an agent would likely try to "fix"

---

### 3.2 — What is NOT tested / safety net gaps not documented

**Current state:** Context pack documents what exists. It doesn't say what catches breakage.

**Gap:** Agents don't know that the Unity Catalog UDF integration has no unit test, or that
`batch_inference` has no integration test against a real Databricks cluster. Before recommending
a change to those paths, that matters — the agent should flag it, not assume CI will catch it.

**What to add:** A "Test Coverage" subsection in `40_OPERATIONS_AND_RELEASE.md`:
- What is tested (unit, integration, e2e)
- What is NOT tested and why (cost, infra, manual-only)
- Which paths have no safety net — changes there need extra review

---

### 3.3 — Dead ends, anti-patterns, and deprecated paths have no dedicated surface ✅ RESOLVED in v0.9.0

**Resolution (2026-03-26):** Added "Negative Guidance" section to `30_BEHAVIORAL_INVARIANTS.md`
template. Lists explicit "do not" rules (do not enumerate generated files, do not use deprecated
patterns, do not open test files for blast radius). Validated in Runs 5 and 6 — Claude bare
proposed deprecated Apollo, Claude structured used React Query because negative guidance said
"Apollo is being deprecated."

---

### 3.4 — Data and schema contracts between pipeline steps not captured

**Current state:** Context pack points to schema files but doesn't distill what flows between
steps. Agents navigating a pipeline need to know the contract at each handoff.

**Gap:** For stream-models, what shape must the DataFrame be entering `batch_inference`? What
does `prompt_sync` emit that `register_prompt` consumes? Without this, agents making changes
to one step may introduce schema mismatches that only surface at runtime.

**What to add:** For pipeline/data repos, `10_SYSTEM_OVERVIEW.md` should include a step-by-step
contract table: step name → inputs (schema/type) → outputs → what breaks silently if wrong.

---

### 3.5 — Dynamic context: "what changed recently" has no home

**Current state:** Context pack is sealed at a point in time. It has no mechanism to reflect
recent changes, active work, or known fragile areas.

**Gap:** "This area changed last week, be careful" is exactly the context that prevents
regression. An agent working on `batch_inference` today would benefit from knowing it was
heavily refactored 2 weeks ago. That's in git log but not surfaced anywhere in the context pack.

**Options (in order of implementation cost):**
1. Auto-generate a "Recent Changes" section in `00_START_HERE.md` during `seal` — last 30 days,
   grouped by context pack section. Low cost, high value.
2. Add a `known_fragile` list to `manifest.json` that the hook updates when relevant files
   change — agents reading the pack see it immediately.
3. Surface active PRs/branches that touch context-relevant files (requires GitHub integration).

---

## Category 4 — Seal Validation (🟡 Important)

### 4.1 — `seal` validates structure, not content quality

**Current state:** `seal` checks that required files exist and manifest is up to date. It does
not check whether the *content* of those files is useful.

**Gap:** A context pack where every CODE_MAP row has `Risk: TBD` and every BEHAVIORAL_INVARIANTS
checklist row says `TODO` passes seal. It will give agents false confidence.

**What seal should check (warnings, not failures):**
- CODE_MAP: non-empty Risk column on every row
- BEHAVIORAL_INVARIANTS: at least one checklist row with explicit file paths (not just
  descriptions)
- SYSTEM_OVERVIEW: at least one runtime behavior section
- `00_START_HERE`: Fast Facts filled in (not template placeholder text)

**Also:** After seal runs, print a content quality summary: "3 CODE_MAP rows have empty Risk
column. 0 BEHAVIORAL_INVARIANTS checklist rows name explicit file paths. Consider filling these
before relying on this pack for impact analysis tasks."

---

### 4.2 — Seal does not check hook installation ✅ FIXED in v0.8.3

Covered in 1.2. Both Node and Rust `seal` now warn if hook is not installed.

---

## Category 5 — Generalizability Research (🟢 Enhancement)

### 5.1 — Current design validated on only one repo type ✅ RESOLVED

**Resolution (2026-03-26/27):** Validated on 3 repo types with zero template modifications:
- ML pipeline (stream-models, 501 files) — Runs 1-4
- CLI/library (agent-chorus, 155 files) — Run 5
- React/TS frontend (trust-stream-frontend, 1,982 files) — Run 6 + field test

All template sections mapped naturally to all three repo types. "Silent Failure Modes" was
relevant even for frontend (Auth0 tokens, Zustand persistence, MSW handler sync). "File
Families" adapted to components, hooks, page objects. No variants needed.

**Remaining gap:** Monorepo support not tested.

---

### 5.2 — Design principles doc should declare its scope explicitly

**Current state:** `context-pack-design-principles.md` reads as universal principles. P1–P10 are
written without qualification.

**Gap:** Several principles are stream-models specific (P3 — "Silent Failure Modes" applies to
async pipelines; P6 — coexisting architectures apply to repos mid-migration). A reader applying
P3 to a stateless REST API would add a section that doesn't apply.

**What to add:** Each principle should declare its applicability:
- `Scope: all repos` — always apply
- `Scope: pipeline/service repos` — apply when there is runtime state or async execution
- `Scope: repos with coexisting architectures` — apply during active migration periods

---

## Category 6 — Agent Adoption Gap (🔴 Critical, added from Run 3 findings)

Run 3 (2026-03-23) showed Claude Opus achieved 5/6 yes, 0 dead ends, 38 files opened.
Codex gpt-5.4-high achieved 4/6 yes, 17 dead ends, 100 files opened — essentially the same
quality as its bare condition (4/6 yes, 24 dead ends, 98 files). **The context pack did not
help Codex.**

### 6.1 — CLAUDE.md wired but no equivalent for Codex/other agents

**Current state:** `chorus context-pack init` adds context pack reading instructions to `CLAUDE.md`
only. Codex reads `AGENTS.md`, `CODEX.md`, or codex-specific config. No equivalent wiring exists.

**Impact:** Codex never received the instruction to read `00_START_HERE.md` first. It treated
the context-pack branch as a bare codebase, ignoring the index entirely. This is likely the
single largest cause of the Codex performance gap.

**Fix:** `chorus context-pack init` must wire all supported agent config files:
- `CLAUDE.md` (Claude Code)
- `AGENTS.md` (generic, read by multiple agents)
- `CODEX.md` or `.codex/instructions.md` (Codex-specific, if format exists)
- `.cursorrules` or `.cursor/rules` (Cursor)
- Any other agent config file that context-pack users need

### 6.2 — Search-first agents ignore structured read order

**Evidence from Run 3:** Codex used Bash in every context-pack task (Claude used it in zero).
Codex's tool_calls show heavy Grep/Glob usage even with context pack available. No evidence
Codex read BEHAVIORAL_INVARIANTS — it explicitly rejected the M2 checklist guidance.

**Hypothesis:** GPT-based agents prefer search over structured reading. The 5-file read order
design assumes agents will follow instructions sequentially. Codex does not.

**Options to test:**
1. **Inline critical info** — repeat key checklists in CODE_MAP (redundancy for search-first agents)
2. **Single-file mode** — consolidate 5 files into one `CONTEXT.md` to reduce cross-referencing
3. **Stronger enforcement** — experiment prompt says "You MUST read BEHAVIORAL_INVARIANTS before any Grep"
4. **Agent-specific START_HERE** — different routing for different agent architectures

### 6.3 — Context pack reading instructions must be agent-agnostic

**Current state:** The CLAUDE.md instruction says "Read `.agent-context/current/00_START_HERE.md` first."
This is Claude-specific syntax. Other agents need the same instruction in their config format.

**Design question:** Should `chorus context-pack init` maintain N separate agent config files,
or write one universal file (like `AGENTS.md`) that all agents read? The risk of N files is
staleness across configs. The risk of one file is agents that don't read `AGENTS.md`.

---

## Open Questions (not resolved by experiment data)

| Question | Why it matters | How to resolve |
|---|---|---|
| Does a context pack help Codex when running the full/best model? | Run 3 used gpt-5.4-high — showed zero correctness improvement (4/6 both conditions). Likely not model ceiling — likely agent config gap (see 6.1). | Fix CODEX.md wiring, re-run |
| Is there a repo complexity threshold below which a context pack adds no value? | Overhead may exceed value for <50 file repos | Run experiment on a simple repo |
| Does a grep-first index format help Codex? | P5/P9 suggest Codex needs a different entry point | Design and test a Codex-specific `00_START_HERE` variant |
| Is the Codex gap caused by missing CODEX.md or by architectural preference for search? | If CODEX.md, fix is simple wiring. If architectural, need redesigned format. | Fix 6.1 first, re-run, then test 6.2 options if gap persists |
| Can section-level staleness markers be generated accurately without an LLM? | If yes, the sync hook can be fast and synchronous | Prototype `section-map.json` approach and test on stream-models commit history |
| What is the right cadence for a full context pack refresh? | Some sections (CODE_MAP) drift fast; others (SYSTEM_OVERVIEW) are stable | Instrument seal timestamps per section, analyse in Run 3+ |

---

## Relationship to Phase 4 (current sprint)

Phase 4 (in `action-plan.md`) covers immediate template improvements: CODE_MAP Risk column,
BEHAVIORAL_INVARIANTS blast radius, SYSTEM_OVERVIEW silent failures, seal validation, SKILL.md.

This document covers the *next version* design agenda — items that need design decisions and
possibly new primitives before implementation. Nothing in this document blocks Phase 4.

After Phase 6 (validation run), revisit this document to decide what moves into the roadmap.
