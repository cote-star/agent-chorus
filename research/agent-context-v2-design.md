# Context Pack v2 — Design Document

**Owner:** Amit Prusty
**Date:** 2026-03-25
**Status:** Draft — informed by Runs 1–4 across 4 phases of experimentation

---

## Executive Summary

Context packs dramatically improve agent answer quality and navigation efficiency, but
the current design implicitly assumes a single agent interaction model. Four experiment
runs across two agent architectures (Claude Opus 4.6, Codex gpt-5.4-high) reveal that
agents fall into two distinct categories — trust-and-follow vs search-and-verify — and
that the optimal context pack serves both by separating content from routing from
constraint.

This document proposes a three-layer context pack architecture that generalizes across
repo types and agent families.

---

## Evidence Base

### Experiment history

| Run | Date | Agents | Conditions | Key finding |
|-----|------|--------|-----------|-------------|
| 1 | 2026-03-20 | Claude Sonnet, Codex mini | bare / ctx | Infrastructure proven; 6 pitfalls identified |
| 2 | 2026-03-20 | Claude Sonnet, Codex mini | bare / ctx | Claude 2→5 yes; Codex unmoved (model ceiling) |
| 3 | 2026-03-25 | Claude Opus 4.6, Codex gpt-5.4-high | bare / ctx | Claude 6/6 yes; Codex 5/6 yes; all SC passed |
| 4 | 2026-03-25 | Claude Opus 4.6, Codex gpt-5.4-high | bare / ctx / structured | Structured layer: Claude 5/6 yes in 24s avg; Codex 5/6 yes but more files opened |

### Run 4 — the critical finding

| Metric | Claude bare | Claude ctx | Claude struct | Codex bare | Codex ctx | Codex struct |
|--------|------------|------------|--------------|------------|-----------|-------------|
| Yes count | 3/6 | 5/6 | 5/6 | 3/6 | 5/6 | 5/6 |
| Avg files | 7.5 | 4.7 | **2.2** | 7.3 | 8.5 | **10.8** |
| Avg tokens | 49K | 23K | **12.5K** | 21.5K | 20K | **14.4K** |
| Avg duration | 175s | 70s | **24s** | 129s | 71s | **112s** |
| Dead ends | 2 | 1 | **0** | 7 | 6 | **11** |
| Risk flags | 2 | 0 | 0 | 1 | 0 | 0 |

**Claude-structured M2 answered in 12 seconds with zero files opened** — pure context-
derived answer from the completeness contract.

**Codex-structured opened more files and hit more dead ends than Codex-bare** but
maintained quality gains and reduced token usage. The stop rules in reporting_rules.json
were not effective for Codex.

---

## The Two Agent Architectures

### Trust-and-follow (Claude, likely Gemini)

1. Reads the index or instruction
2. Follows the prescribed read order
3. Trusts the completeness contract
4. Stops when the contract says sufficient
5. Opens repo files only to extract specific values

**What helps:** Authoritative completeness lists. Stop conditions. Grouped reporting
rules. The more precise the contract, the fewer files Claude opens.

**What hurts:** Vague or incomplete contracts — Claude will trust a wrong contract and
produce a wrong answer confidently.

### Search-and-verify (Codex, likely Cursor)

1. Reads the index as one signal among many
2. Greps the repo to build its own understanding
3. Cross-references structured artifacts against code
4. Continues reading until its internal confidence threshold is met
5. Overrides the contract when code suggests otherwise

**What helps:** Scoped search boundaries. Relevance filters that prevent enumeration of
derived files. Completeness contracts that prevent dropping pass-through files.

**What hurts:** Stop rules (Codex doesn't stop). Read-order prescriptions (Codex reads
in grep-result order). Verify budgets (Codex's budget is "until I'm satisfied").

---

## Three-Layer Architecture

### Layer 1 — Content Layer (universal, human-readable)

The existing markdown pack. Serves humans, serves both agent types, provides the
semantic foundation.

```
.agent-context/current/
  00_START_HERE.md              # Entrypoint, snapshot, read order, task-type routing
  10_SYSTEM_OVERVIEW.md         # Architecture, runtime behavior, silent failure modes
  20_CODE_MAP.md                # Navigation index, risk column, approach tags
  30_BEHAVIORAL_INVARIANTS.md   # Change-impact checklists, blast radius per change type
  40_OPERATIONS_AND_RELEASE.md  # Deploy, test, CI, maintenance
```

**Design principles for content:**
- P1: Index must declare its own incompleteness
- P2: Risk callouts belong inline, not in separate files
- P3: Runtime behavior is as important as code structure
- P6: Coexisting architectures need explicit boundary documentation

### Layer 2 — Authority Layer (serves trust-and-follow agents)

Machine-readable contracts that enable an agent to answer from context alone when the
contract is strong enough.

```
.agent-context/current/
  routes.json                   # Task-type → entrypoint mapping
  completeness_contract.json    # Required files per change pattern
  reporting_rules.json          # Grouping semantics, stop conditions
```

**routes.json** maps task intent to a pack read path:
```json
{
  "task_routes": {
    "lookup": {
      "pack_read_order": ["00_START_HERE.md", "20_CODE_MAP.md", "reporting_rules.json"],
      "named_patterns": {
        "deployment_thresholds": {
          "authoritative_files": ["models/src/validation/presets/trust_score_v1.py"],
          "supporting_files": ["models/src/validation/components/metrics/thresholds.py"]
        }
      }
    }
  }
}
```

**completeness_contract.json** defines what must appear in the answer:
```json
{
  "task_families": {
    "impact_analysis": {
      "named_patterns": {
        "new_selector_dimension": {
          "contractually_required_files": [
            "models/prompts/prompts.yml",
            "models/src/schemas/prompts.py",
            "..."
          ],
          "required_file_families": [
            "models/instructions/*.sql",
            "models/assets_gen/_specs/*.prompt.yml"
          ]
        }
      }
    }
  }
}
```

**reporting_rules.json** defines how to present answers:
```json
{
  "global_rules": {
    "grouped_reporting_default": true,
    "authoritative_vs_derived_paths": [
      {"pattern": "models/assets_gen/_generated/*.yml", "role": "derived"},
      {"pattern": "models/assets_gen/_specs/*.yml", "role": "authoritative"}
    ]
  },
  "task_families": {
    "impact_analysis": {
      "stop_after": "Stop after blast radius is complete and families are grouped.",
      "stop_unless": ["pack and code disagree", "structured artifact references missing file"],
      "never_enumerate_individually": ["models/assets_gen/_generated/*.yml"]
    }
  }
}
```

### Layer 3 — Navigation Layer (serves search-and-verify agents)

This is the **missing layer** that Run 4 proved is needed. It does not try to stop
Codex from reading — it constrains *where* Codex searches and *how* it interprets
what it finds.

**Proposed new artifact:** `search_scope.json`

```json
{
  "schema_version": 1,
  "task_families": {
    "impact_analysis": {
      "named_patterns": {
        "new_selector_dimension": {
          "search_directories": [
            "models/src/schemas/",
            "models/src/steps/",
            "models/src/stream_models/llm/instruction/",
            "models/instructions/",
            "models/assets_gen/_specs/"
          ],
          "exclude_from_search": [
            "models/assets_gen/_generated/",
            "models/tests/",
            "models/validation/notebooks/"
          ],
          "verification_shortcuts": {
            "models/src/steps/prompt_sync.py": "line 42: _apply_filters uses generic dict matching — no change needed",
            "models/src/steps/batch_inference.py": "lines 43-68: check inference_params construction"
          },
          "derived_file_policy": "Do not list _generated/ files as change targets. They are regenerated by `python tools/dabgen/generate.py`."
        }
      }
    },
    "diagnosis": {
      "named_patterns": {
        "batch_inference_nulls": {
          "search_directories": [
            "models/src/steps/",
            "models/src/modeling/clients/",
            "models/src/modeling/",
            "models/src/validation/components/extractors/"
          ],
          "verification_shortcuts": {
            "models/src/modeling/clients/_base.py": "lines 131-135: _create_empty_result; lines 233-255: retry loop",
            "models/src/steps/shared/prompt_reading.py": "lines 32-41: get_prompt selector match"
          }
        }
      }
    }
  }
}
```

**Key differences from Layer 2:**
- `search_directories` replaces `stop_after` — bounds WHERE Codex looks instead of WHEN it stops
- `exclude_from_search` prevents enumeration of derived/test files
- `verification_shortcuts` give Codex specific line ranges to check instead of reading entire files
- `derived_file_policy` is explicit prose that Codex can reason about

---

## Agent-Specific Bootstrap

The content is universal. The routing is agent-specific.

### CLAUDE.md (trust-and-follow)
```markdown
## Context Pack

When asked to understand this repository:

1. Read `.agent-context/current/00_START_HERE.md` first.
2. Follow the read order defined in that file.
3. Only open project files when the context pack identifies a specific target.
```

### AGENTS.md (search-and-verify — Codex)
```markdown
## Context Pack

When asked to understand this repository:

1. Read `.agent-context/current/00_START_HERE.md`.
2. Read `.agent-context/current/routes.json`.
3. Identify the active task type in routes.json.
4. Read the matching entries in `completeness_contract.json` and `search_scope.json`.
5. Search ONLY within the directories listed in `search_scope.json` for your task type.
6. Use `verification_shortcuts` to check specific line ranges instead of reading full files.
7. Do not enumerate files in directories marked `exclude_from_search`.
```

### GEMINI.md (likely trust-and-follow, needs testing)
```markdown
## Context Pack

When asked to understand this repository:

1. Read `.agent-context/current/00_START_HERE.md` first.
2. Follow the read order defined in that file.
3. Only open project files when the context pack identifies a specific target.
```

### .cursorrules (likely search-and-verify, needs testing)
```
# Context Pack
When understanding this repository, read .agent-context/current/routes.json first
and follow the search_scope.json boundaries for your task type.
```

---

## Generalization Across Repo Types

The three-layer architecture must work for different repo shapes. The **structure** is
universal; the **content** adapts.

### Template variants by repo type

| Repo type | System Overview emphasis | Code Map emphasis | Invariants emphasis |
|-----------|------------------------|-------------------|---------------------|
| ML pipeline | Runtime behavior, silent failures, data flow | Prompt specs, validation presets, generated files | Selector dimensions, schema changes, prompt registration |
| Web app | Request lifecycle, auth flow, state management | Routes, components, API endpoints, DB models | API contract changes, migration patterns, auth changes |
| Library | Public API surface, versioning, backward compat | Module structure, exports, type hierarchy | Breaking changes, deprecation patterns, semver rules |
| Monorepo | Package boundaries, shared dependencies, build graph | Package index, cross-package imports, shared libs | Cross-package changes, version bump cascades, CI matrix |

### What stays universal

- **File structure** (00–40 markdown + JSON artifacts)
- **routes.json schema** (task_routes with named_patterns)
- **completeness_contract.json schema** (contractually_required_files, required_file_families)
- **reporting_rules.json schema** (groupable_families, never_enumerate_individually)
- **search_scope.json schema** (search_directories, exclude_from_search, verification_shortcuts)
- **Agent bootstrap wording** (trust-and-follow vs search-and-verify)
- **seal validation rules** (file reference checks, derived-file policy enforcement)

### What adapts per repo

- **Named patterns** in completeness_contract.json (repo-specific change types)
- **Search directories** in search_scope.json (repo-specific directory structure)
- **Verification shortcuts** (repo-specific line references — these go stale fastest)
- **Groupable families** (e.g., `_specs/*.yml` for ML pipeline, `src/components/*.tsx` for React app)
- **Derived file policies** (e.g., `_generated/` for ML pipeline, `dist/` for web app)

---

## Staleness Management

The single biggest adoption risk. From the next-version agenda:

> "Hook is advisory-only; pack drifts silently." — Severity: Critical

### Staleness tiers

| Tier | What goes stale | Detection | Mitigation |
|------|----------------|-----------|------------|
| **Fast** | verification_shortcuts (line numbers) | Any file edit changes line counts | Auto-invalidate shortcuts when referenced file changes; seal warns |
| **Medium** | completeness_contract (file lists) | New files added, old files renamed | CI check: `seal --verify` flags referenced files that don't exist |
| **Slow** | Content layer (architecture, invariants) | Major refactors, new subsystems | Freshness check on `git diff --stat` since last seal; warn if >N files changed |
| **Glacial** | routes.json (task type routing) | Almost never changes unless repo purpose changes | Manual review on major version bumps |

### Proposed freshness protocol

1. **Pre-push hook** (already exists): runs `chorus context-pack check-freshness`
2. **CI check** (new): `chorus context-pack seal --verify` validates all file references resolve
3. **Section-level staleness markers** (new): each markdown section gets a `last_verified: <commit>` annotation
4. **Auto-invalidation** (new): verification_shortcuts in search_scope.json are cleared for any file whose `git diff` shows line changes since last seal

---

## Design Principles (updated from P1–P10)

### Universal (all repos)

- **P1: Index must declare its own incompleteness.** The CODE_MAP is not exhaustive. Say so.
- **P2: Risk callouts belong inline.** Don't separate risk into a different file.
- **P3: Runtime behavior matters as much as code structure.** Document silent failures.
- **P11: Content is universal; routing is agent-specific.** One pack, multiple bootstrap paths.
- **P12: Authority layer for trust-agents; navigation layer for verify-agents.** Don't try to make one layer serve both.
- **P13: Bound the search space, don't prescribe the stop point.** Codex ignores stop rules but respects scope boundaries.
- **P14: Verification shortcuts must include line ranges, not just file paths.** This is what lets Codex skip full-file reads.
- **P15: Derived files are evidence, never edit targets.** Enforce in seal validation.

### Pipeline/service repos

- **P6: Coexisting architectures need explicit boundary documentation.**
- **P9: Checklist completeness > navigation speed.** A complete blast radius with more files read is better than a fast incomplete answer.

### Complex repos

- **P10: Self-score calibration is unreliable.** Reviewer grading is mandatory.

---

## Implementation Plan

### Phase 1: Add search_scope.json to agent-chorus (v0.9.0)

- [ ] Define JSON schema for search_scope.json
- [ ] Add to `init` scaffolding (Node + Rust)
- [ ] Add to `seal` validation (verify search_directories exist, verify shortcut files exist)
- [ ] Add parity tests
- [ ] Update AGENTS.md bootstrap template to reference search_scope.json

### Phase 2: Fill stream-models search_scope.json

- [ ] Populate search_directories for all 4 task families
- [ ] Add verification_shortcuts for key files (prompt_reading.py, _base.py, etc.)
- [ ] Add exclude_from_search for _generated/, tests/, legacy notebooks

### Phase 3: Validation run (Run 5)

- [ ] Three conditions: bare / structured-v1 (current) / structured-v2 (with search_scope)
- [ ] Primary target: Codex file count and dead ends should decrease
- [ ] Secondary target: Claude should not regress
- [ ] New success criterion: Codex structured files_opened ≤ Codex bare

### Phase 4: Generalize templates

- [ ] Create template variants: ML pipeline, web app, library, monorepo
- [ ] Test on at least one non-ML repo (agent-chorus itself is a good candidate)
- [ ] Update `init` to detect repo type and select template

### Phase 5: Staleness automation

- [ ] Implement auto-invalidation for verification_shortcuts
- [ ] Add `seal --verify` CI mode
- [ ] Add section-level staleness markers to markdown

---

## Open Questions

1. **Should search_scope.json be generated from completeness_contract.json?** The
   search directories could be inferred from the contractually_required_files paths.
   But explicit is better for v1.

2. **How do we handle repos where the agent adds new search_scope entries?** If Codex
   finds a relevant directory not in search_scope, should it update the file? Or just
   report the gap?

3. **Is Cursor search-and-verify or trust-and-follow?** Need a Cursor experiment run
   to classify it before designing the `.cursorrules` bootstrap.

4. **Should the content layer be auto-generated?** Current design is explicit
   maintenance. An auto-generation mode (agent reads repo, fills markdown + JSON) is
   the Phase 7 standalone skill. But auto-generated content may be less precise than
   human-curated content. The experiment data shows precision matters more for trust-
   agents.

5. **What is the right granularity for verification_shortcuts?** Line ranges go stale
   fastest. Function names are more stable but less precise. The right answer may be
   both: `"file.py": "function _apply_filters (approx line 42): uses generic dict matching"`.

---

## Files in this research program

| File | Purpose |
|------|---------|
| `action-plan.md` | Master sequenced plan (Phases 1–9) |
| `context-pack-design-principles.md` | P1–P15 with scope tags |
| `context-pack-field-findings-2026-03-20.md` | Run 1 + Run 2 findings |
| `context-pack-next-version-agenda.md` | Next-version improvement items |
| `context-pack-v2-design.md` | This document — v2 architecture |
| `handoff-2026-03-23.md` | Previous handoff |
