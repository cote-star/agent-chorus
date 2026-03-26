# Skill Vision — from Slack (2026-03-26)

## Amit's original description

> .agent-context will sit inside repo. And there will be a skill to create it
> for the first time when a new repo gets published but it will be better to
> create .agent-context only for large repos with substantial amount of code
> and files. For small repos no point as agents can just scan everything and
> won't get lost or lead to many compactions.

## How it works (Amit's sequence)

1. We have a large repo
2. We use the .agent-context skill to create the structured context
3. This info gets mapped to AGENTS.md and CLAUDE.md of the repo but these are
   not full context — just 2-3 sentences on how agents should navigate the repo
   (max 100-200 tokens, or 300 depending on repo size)
4. Once created and merged with main branch it will create a hook and policy
5. Every time work gets merged with main the agent context gets updated but only
   the relevant part — not full context. For example if more files added or some
   features are added or new data schema added only the relevant parts get updated.
   Similarly stale information gets deleted. Agents only work on context file
   update — they don't touch the actual repo files without explicit approval
6. This way the context is always maintained with most up to date work
7. Sync policy can work with other branches but main is preferred

## Auto-research on first run

> Also the first time .agent-context skill is used it creates experiments and
> ground truth to auto-test itself with sub-agents and improves any part it
> deems necessary but this is for the first time. It follows the auto-research
> and improve mechanism.

## Update triggers (refined in Claude discussion)

Three distinct triggers, respecting the information boundary:

1. **Agent-opened PR** → agent already knows what changed → auto-prep
   .agent-context update as part of the PR (separate commit)
2. **Human-opened PR** → agent has no context → don't touch .agent-context
3. **Manual catchup** → human says "update context pack" → agent diffs since
   last seal, proposes patches, human approves each section

## Key design decisions

- Single skill, not separate init/update skills
- Agent-context changes in PRs go in a separate commit from code changes
- Manual catchup shows diff before applying — human approves each section
- Repo size check: warn if <50 files, suggest skipping
- Routing blocks in CLAUDE.md/AGENTS.md stay under 200 tokens
