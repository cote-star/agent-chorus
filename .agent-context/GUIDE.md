# Context Pack Generation Guide

This guide tells AI agents how to fill in the context pack templates.

## Process
1. Read each file in `.agent-context/current/` in numeric order.
2. For each `<!-- AGENT: ... -->` block, replace it with repository-derived content.
3. After filling all sections, run `bridge context-pack seal` to finalize (manifest + snapshot).

## Quality Criteria
- Content must be factual and verifiable from the repository.
- Prefer concise bullets over long prose.
- Keep total word count under ~2000 words across all files.
- Do not include secrets or credentials.
- If unsure, note `TBD` rather than inventing details.

## When to Update
- After significant architectural or contract changes.
- After adding new commands/APIs/features.
- When `bridge context-pack check-freshness` reports stale content.
