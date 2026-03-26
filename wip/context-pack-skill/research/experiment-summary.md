# Experiment Summary — Informing Skill Design

## What we tested

5 experiment runs across 2 repo types, 2 agents, 3 conditions, 78 total result files.

## Results that matter for skill design

### The structured layer works
- Claude structured: 50-70% token reduction, near-zero dead ends across both repos
- Codex structured: quality improved (3/6 bare → 5-6/6 structured), efficiency mixed
- Templates generalized from ML pipeline to CLI/library with zero modifications

### Two agent architectures
- **Trust-and-follow** (Claude): uses context pack as authority, opens minimal files
- **Search-and-verify** (Codex): uses context pack as scaffolding, still verifies against code
- Implication for skill: the generated pack must serve both — strong contracts for Claude, scoped search boundaries for Codex

### What the pack must contain (non-negotiable)
1. `00_START_HERE.md` — entrypoint with task-type routing and stop rules
2. `10_SYSTEM_OVERVIEW.md` — architecture + silent failure modes
3. `20_CODE_MAP.md` — navigation index with risk + authority columns
4. `30_BEHAVIORAL_INVARIANTS.md` — change checklists + file families + negative guidance
5. `40_OPERATIONS_AND_RELEASE.md` — validation, CI, deploy
6. `routes.json` — task-type → entrypoint mapping
7. `completeness_contract.json` — required files per change pattern
8. `reporting_rules.json` — grouping semantics + stop conditions
9. `search_scope.json` — search directories + verification shortcuts

### What makes a pack good vs mediocre
- **Good**: explicit file paths in checklists, named patterns in contracts, line-range verification shortcuts
- **Mediocre**: generic descriptions without file paths, empty JSON arrays, vague stop rules
- The skill must fill arrays with real repo content, not leave empty scaffolds

### Auto-testing works
- 3-4 well-designed questions (lookup + impact analysis) are enough to validate a pack
- Ground truth can be derived from the repo by the same agent that fills the pack
- A bare-vs-pack comparison on 2-3 questions catches most quality issues

### Repo size threshold
- agent-chorus (~130 files): significant benefit from context pack
- Likely threshold: >50 files OR >3 distinct subsystems
- Below threshold: agents can scan everything efficiently without the pack overhead
