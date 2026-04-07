# Agent Context Map — Multi-Repo Context Routing

**Date:** 2026-04-07
**Status:** Design
**Author:** Amit Prusty

## Problem

An agent working on a multi-repo feature (e.g., "add a new trust score model with a frontend dashboard") currently:

1. Opens repo A, reads its `.agent-context` — good context, efficient work
2. Switches to repo B, reads its `.agent-context` — repo A's context gets compacted
3. Makes a change in repo B that breaks an invariant in repo A — didn't remember
4. Burns tokens re-reading repo A's pack to verify
5. Misses a cross-repo dependency entirely because neither pack mentions the other

**Result:** Dead ends, lost context, redundant token spend, and missed cross-repo invariants.

## Insight

You don't need all packs loaded at once. You need a **thin routing layer** (~300-500 tokens) that tells the agent:
- Which repos matter for this task
- How they connect (data flow, API contracts, shared schemas)
- What changes in repo A require changes in repo B
- What order to read them in

The agent reads the map first, then loads only the relevant repo packs one at a time with the cross-repo invariants in working memory.

## Design: `.agent-context-map/`

A single directory in a central location (e.g., a shared repo or the primary repo of a feature area) containing:

```
.agent-context-map/
├── MAP.md                    # Human + agent readable — the entry point
├── repos.json                # Machine readable — repo index with locations
├── flows.json                # Machine readable — cross-repo data/API flows
└── cross-repo-invariants.md  # The killer feature — what changes cascade across repos
```

### MAP.md (~200-300 tokens)

The routing entrypoint. An agent reads this BEFORE any individual repo's `.agent-context`.

```markdown
# Agent Context Map

## Repos in this stack

| Repo | Role | Location | Has .agent-context |
|---|---|---|---|
| stream-models | LLMOps pipeline — models, prompts, validation, deployment | dsml/stream-models | Yes |
| trust-stream-frontend | Dashboard — React/Next.js frontend for trust scores | dsml/trust-stream-frontend | Yes |
| team_skills | AI agent skills — team conventions and templates | cross-team-repos/team_skills | No (docs-only) |

## Task routing — which repos do I need?

| Task type | Primary repo | Also touch | Read order |
|---|---|---|---|
| New model/prompt | stream-models | — | stream-models only |
| New dashboard view | trust-stream-frontend | — | trust-stream-frontend only |
| New model + dashboard | stream-models | trust-stream-frontend | stream-models first (defines the data contract), then trust-stream-frontend |
| Validation threshold change | stream-models | trust-stream-frontend (if threshold is displayed) | stream-models first |
| Shared component/schema change | stream-models | trust-stream-frontend | stream-models first (source of truth), then trust-stream-frontend (consumer) |
| New team skill | team_skills | — | team_skills only |

## Cross-repo invariants — READ BEFORE MULTI-REPO WORK

See `cross-repo-invariants.md` for the full list. Key ones:

1. **Response format changes in stream-models → frontend must update** — if you add/change a Pydantic model in `response_formats.py`, the frontend component that renders that model's output must be updated.
2. **New model in stream-models → frontend route needed** — every model that produces displayable scores needs a corresponding route/component in trust-stream-frontend.
3. **Prompt selector changes cascade** — changing selectors in `prompts.yml` affects the frontend's filter/grouping logic if it uses the same dimensions.
```

### repos.json

```json
{
  "schema_version": 1,
  "repos": [
    {
      "id": "stream-models",
      "role": "LLMOps pipeline — models, prompts, validation, deployment",
      "path": "dsml/stream-models",
      "has_agent_context": true,
      "pack_path": ".agent-context/current/",
      "primary_for": ["model", "prompt", "validation", "deployment"],
      "data_produces": ["trust_scores", "relevancy_scores", "topic_tags", "sentiment_scores"],
      "api_surface": ["databricks_workflows", "mlflow_model_registry", "mlflow_prompt_registry"]
    },
    {
      "id": "trust-stream-frontend",
      "role": "Dashboard — React/Next.js frontend for trust scores",
      "path": "dsml/trust-stream-frontend",
      "has_agent_context": true,
      "pack_path": ".agent-context/current/",
      "primary_for": ["dashboard", "visualization", "filtering", "export"],
      "data_consumes": ["trust_scores", "relevancy_scores", "topic_tags", "sentiment_scores"],
      "api_surface": ["next_api_routes", "snowflake_queries"]
    },
    {
      "id": "team_skills",
      "role": "AI agent skills — team conventions and templates",
      "path": "cross-team-repos/team_skills",
      "has_agent_context": false,
      "primary_for": ["skills", "conventions", "templates"]
    }
  ]
}
```

### flows.json

```json
{
  "schema_version": 1,
  "data_flows": [
    {
      "name": "model_scores_to_dashboard",
      "producer": "stream-models",
      "consumer": "trust-stream-frontend",
      "contract": "Databricks tables → Snowflake sync → Next.js API routes",
      "schema_owner": "stream-models (Pydantic response formats)",
      "breaks_if": "Response format fields renamed or removed without frontend update"
    },
    {
      "name": "prompt_selectors_to_filters",
      "producer": "stream-models",
      "consumer": "trust-stream-frontend",
      "contract": "Prompt selector dimensions (action, domain, persona, market) used as filter axes in dashboard",
      "schema_owner": "stream-models (prompts.yml selectors)",
      "breaks_if": "New selector dimension added without frontend filter update"
    }
  ],
  "shared_concepts": [
    {
      "name": "model_action_key",
      "defined_in": "stream-models (specs + actions.yaml)",
      "used_in": ["stream-models (inference, validation)", "trust-stream-frontend (route params, display labels)"],
      "breaks_if": "Key renamed without updating both repos"
    }
  ]
}
```

### cross-repo-invariants.md

The most valuable file. Equivalent to `30_BEHAVIORAL_INVARIANTS.md` but across repos.

```markdown
# Cross-Repo Invariants

## Change Cascades

| Change in | Repo | Must also update | Repo | Why |
|---|---|---|---|---|
| New response format | stream-models | Rendering component | trust-stream-frontend | Frontend parses model output JSON — new fields need display logic |
| New model/prompt producing scores | stream-models | Dashboard route + component | trust-stream-frontend | Every scorable model needs a UI entry point |
| Rename model_action_key | stream-models | Route params, display labels | trust-stream-frontend | Frontend uses action keys for routing and display |
| New selector dimension | stream-models | Filter/grouping UI | trust-stream-frontend | Dashboard filters mirror prompt selector axes |
| Validation threshold change | stream-models | Threshold display (if shown) | trust-stream-frontend | Dashboard may show pass/fail status based on thresholds |
| Schema migration (Snowflake) | trust-stream-frontend | Table bindings in specs | stream-models | Model specs reference table names that must match |

## Single-Repo Tasks (no cascade)

These do NOT require cross-repo changes:
- Adding prompts to existing models (stream-models only)
- CSS/layout changes (trust-stream-frontend only)
- Adding a new team skill (team_skills only)
- Validation preset tuning without threshold display changes (stream-models only)
```

## How It Works in Practice

### Agent workflow for a multi-repo task

1. Agent reads `MAP.md` (~300 tokens) — learns which repos matter and in what order
2. Agent reads `cross-repo-invariants.md` (~200 tokens) — learns what cascades
3. Agent opens primary repo, reads its `.agent-context` — does the main work
4. Agent checks cross-repo invariants — "does my change trigger a cascade?"
5. If yes: opens secondary repo, reads its `.agent-context`, makes the dependent change
6. If no: done

**Token budget:** ~500 tokens for the map + ~4500 tokens for one repo's pack = ~5000 tokens before opening any source files. This fits comfortably in one context window alongside actual code work.

### Without the map (current state)

1. Agent reads repo A's pack (~4500 tokens)
2. Does work in repo A
3. User says "now update the frontend"
4. Agent reads repo B's pack (~4500 tokens)
5. Repo A's pack gets compacted
6. Agent misses a cross-repo invariant because it forgot repo A's response format details
7. User catches the bug in review

### With the map

1. Agent reads map (~500 tokens) — knows both repos are needed
2. Reads cross-repo invariants — knows response format changes cascade
3. Reads repo A's pack, does the work, notes the response format change
4. Reads repo B's pack, knows exactly what to update (the invariant told it)
5. No surprises in review

## Where Does the Map Live?

**Option A: Central repo** (e.g., `cross-team-repos/agent-context-map/`)
- Pro: single source of truth, easy to find
- Con: yet another repo to maintain

**Option B: Primary repo of the stack** (e.g., `stream-models/.agent-context-map/`)
- Pro: co-located with the data contract owner
- Con: secondary repos don't know about it unless told

**Option C: In each repo, with cross-references**
- Pro: each repo is self-contained
- Con: N copies to keep in sync

**Recommendation: Option B** — the map lives in the repo that owns the data contracts (stream-models for this stack). Other repos reference it. The map is small enough (~4 files) that staleness is manageable.

## Staleness Management

| What goes stale | How fast | Detection |
|---|---|---|
| Repo list | Glacial (new repos are rare) | Manual — add when onboarding a repo |
| Data flows | Slow (API contracts change infrequently) | PR review — any schema change should check flows.json |
| Cross-repo invariants | Medium (new cascades emerge with new features) | Agent PR — agent that discovers a cascade adds it |
| Task routing table | Slow | Manual — update when feature patterns change |

## Relationship to Existing Layers

```
.agent-context-map/          ← NEW: cross-repo routing (this design)
  MAP.md                       reads in ~300 tokens, routes to repos
  cross-repo-invariants.md     prevents cross-repo blind spots

.agent-context/              ← EXISTING: per-repo navigation
  00_START_HERE.md             reads in ~4500 tokens total
  ...9 files...                routes to source files within the repo

source files                 ← the actual code
```

The map is Layer 0 — it sits above individual repo packs and routes between them. It does not replace any per-repo content.

## Open Questions

1. **Should the map be auto-generated from repo packs?** The `data_produces` / `data_consumes` fields in repos.json could be inferred from completeness_contract.json entries. But cross-repo invariants require human knowledge of how repos interact.

2. **How does an agent discover the map?** Options: routing block in CLAUDE.md that says "for multi-repo work, read the context map first"; or a convention that the map always lives at a known path.

3. **Should cross-repo invariants be bidirectional?** Currently written as "change in A → must update B." Should B's `.agent-context` also reference the map? Probably yes — add a "cross-repo dependencies" section to each repo's START_HERE.

4. **Scale limit?** This design works for 2-5 repos in a feature stack. For 20+ repos (monorepo-scale), the map itself becomes too large. At that scale, you need a map-of-maps or a query interface.

## Next Steps

1. Create `.agent-context-map/` in stream-models (the data contract owner)
2. Fill MAP.md, repos.json, flows.json, cross-repo-invariants.md for the stream-models + trust-stream-frontend stack
3. Add a routing block to both repos' CLAUDE.md/AGENTS.md pointing to the map
4. Test: give an agent a multi-repo task and compare with/without map
5. If effective, add to the agent-context skill as an optional "multi-repo" flow
