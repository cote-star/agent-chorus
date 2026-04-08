# Agent Context vs Skills — When to Use Which

**Date:** 2026-04-08
**Status:** Reference

## The Short Version

**Skills** = reusable processes that work across repos ("how do we do X?")
**Agent context** = repo-specific truth that agents read every session ("what's in this repo, what's risky?")

Agent context absorbs repo-specific knowledge that was previously stuffed into stray skills, AGENTS.md files, or team wikis. Real skills — the ones that encode a reusable team process — stay as skills.

## The Clean Model

```
Agent context:  "When adding a Zustand store in this repo, also update
                 src/__tests__/setup.tsx (store reset). Silent failure
                 if missed — tests pass individually, fail in suite."

Skill:          "To add a Zustand store: create the store file, add the
                 provider to the app root, export from the barrel file,
                 add store reset to test setup."
```

The skill tells you the **process** (applicable to any React repo).
Agent context tells you the **specific files and risks in THIS repo**.

An agent with both knows the process AND the local landmines.

## The Stray Skill Problem

Without agent context, teams create skills to compensate for missing repo knowledge. These "stray skills" are repo-specific truths masquerading as reusable workflows:

| Stray skill pattern | What it really is | Where it belongs in agent context |
|---|---|---|
| "Always check setup.tsx when adding stores" | Behavioral invariant | `30_BEHAVIORAL_INVARIANTS.md` Update Checklist |
| "Important files in our frontend" | Navigation index | `20_CODE_MAP.md` High-Impact Paths |
| "Don't use Apollo — it's deprecated" | Negative guidance | `30_BEHAVIORAL_INVARIANTS.md` Negative Guidance |
| "Our CI pipeline runs these checks" | Operations | `40_OPERATIONS_AND_RELEASE.md` CI Checks |
| "The architecture is X → Y → Z" | System overview | `10_SYSTEM_OVERVIEW.md` Runtime Architecture |
| "When changing response formats, update the frontend" | Cross-repo invariant | `.agent-context-map/cross-repo-invariants.md` (planned; see design: `research/agent-context-map-design.md`) |

**When agent context exists, these stray skills become redundant.** The knowledge lives where it's always visible (loaded every session) instead of needing explicit trigger.

## What Agent Context Replaces

| Before (without agent context) | After (with agent context) |
|---|---|
| 744-line AGENTS.MD with migration instructions, file lists, pitfalls | `.agent-context/` for navigation + `docs/MIGRATION_PLAYBOOK.md` for reference |
| Per-repo "important files" skill | `20_CODE_MAP.md` with risk ratings and authority columns |
| "Don't do X" rules scattered across README, AGENTS.md, wiki | `30_BEHAVIORAL_INVARIANTS.md` Negative Guidance section |
| Tribal knowledge about CI/deploy | `40_OPERATIONS_AND_RELEASE.md` with exact commands |
| Agent burning 50K tokens re-discovering architecture | `10_SYSTEM_OVERVIEW.md` loaded in 800 tokens |

## What Agent Context Does NOT Replace

These are real skills with reusable processes — they should stay as skills:

| Skill type | Example | Why it's a skill, not context |
|---|---|---|
| Process patterns | `code-review`, `architecture-decision-record` | Same process works in any repo |
| Generators | `edelman-slides`, `api-documentation` | Output format is standardized across repos |
| Integrations | `jira-context`, `databricks-uc-metadata` | External system access pattern, not repo knowledge |
| Teaching | `backend-mentor`, `backend-pr-learner` | Pedagogy and mentoring approach, not file locations |
| Cross-repo workflows | `stream-model-migration`, `stream-prompt-registration` | Step-by-step process that spans systems |
| Meta-skill | `agent-context` (the skill that creates agent context) | The bridge — a reusable process for creating repo-specific truth |

## The Overlap Zone

Some content legitimately lives in both:

**Extension Recipes** (CODE_MAP) overlap with workflow skills. The recipe in agent context says "these 7 steps, these specific files." The skill says "here's the full process with syntax, examples, pitfalls."

Resolution: agent context has the **compact checklist** (files + order). The skill has the **full guide** (syntax, examples, edge cases, pitfalls). The agent reads the context first for routing, then the skill for depth if needed.

**Migration Playbooks** overlap with migration skills. The playbook has detailed step-by-step with examples. The agent context has the change checklists with file paths.

Resolution: same pattern. Agent context for the "what files" answer, skill/playbook for the "how exactly" answer. The playbook lives in `docs/`, referenced from CODE_MAP's Extension Recipe.

## Token Economics

| What | When loaded | Tokens | Frequency |
|---|---|---|---|
| Agent context | Every session | ~4500 | Always |
| Skill | When triggered | 500-2000 | On demand |
| Both together | Task that needs process + repo knowledge | ~5500-6500 | Occasionally |

Agent context is "always-on" overhead that pays back by preventing dead ends. Skills are "on-demand" depth that helps with specific tasks. The total token cost is manageable because they're complementary, not overlapping.

## Decision Framework

When someone proposes a new skill, ask:

1. **Is this repo-specific knowledge?** (file paths, invariants, architecture) → Agent context, not a skill
2. **Is this a reusable process?** (applies to multiple repos) → Skill
3. **Is this both?** → Agent context for the repo-specific parts, skill for the reusable process
4. **Would this content go stale if the repo changes?** → Agent context (semi-auto maintenance)
5. **Does this need explicit trigger or should agents always know it?** → Always-on = agent context, trigger = skill

## Implications for Team

1. **Before creating a new skill**, check if the knowledge belongs in agent context
2. **When agent context exists**, audit existing skills for stray repo knowledge that should migrate
3. **The `agent-context` skill is the only skill that creates agent context** — it's the bridge between the two systems
4. **Skills should reference agent context**, not duplicate it: "see the repo's `.agent-context/` for file paths and risks"
