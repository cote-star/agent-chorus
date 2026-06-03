# Context Pack Generation Guide

This guide tells AI agents how to fill in the context pack templates.

## Process
1. Read each file in `.agent-context/current/` in numeric order.
2. Fill the markdown templates with repository-derived content.
3. Fill the structured files (`routes.json`, `completeness_contract.json`, `reporting_rules.json`) with deterministic repo-specific rules.
4. After filling all sections, run `chorus agent-context seal` to finalize (manifest + snapshot).

## Quality Criteria
- Content must be factual and verifiable from the repository.
- Prefer concise bullets over long prose.
- Keep total word count under ~2000 words across all files.
- Do not include secrets or credentials.
- Keep structured artifacts explicit and deterministic; do not auto-generate them from freeform prose.
- If unsure, note `TBD` rather than inventing details.

## When to Update
- After significant architectural or contract changes.
- After adding new commands/APIs/features.
- When `chorus agent-context check-freshness` reports stale content.

## Update contract (read before any "update the pack" task)

The pack has **two halves**, and an update is not done until **both** are correct:

- **Markdown** (`00`–`40`, `acceptance_tests.md`) — prose that humans and agents read first. The `CLAUDE.md` / `AGENTS.md` routing blocks point only here, which biases editors toward updating *only* Markdown.
- **Structured JSON** (`search_scope.json`, `completeness_contract.json`, `reporting_rules.json`, `manifest.json`) — machine-read routing / scoping / authority consumed by search-and-verify agents (Codex, Cursor). **First-class, not optional.**

Rules:
1. When code changes, update **every applicable file in BOTH halves** — not just the Markdown. A stale `search_scope.json` verification shortcut or `completeness_contract.json` file family silently misleads agents with no visible error.
2. **Always finish with `chorus agent-context seal` then `chorus agent-context verify`.** Seal regenerates `manifest.json` and validates the structured files; verify confirms integrity. These are the only checks that inspect the JSON half.
3. **A `check-freshness` PASS does NOT mean the pack is correct.** Freshness only confirms the pack was *touched* relative to changed code — it does not compare pack content to code, and touching a single Markdown file satisfies it. Never treat a freshness PASS as "update complete."
4. Keep structured files on their current shapes — e.g. `search_scope.json` `verification_shortcuts` is an **object keyed by file path** (`"<path>": { look_for, check }`), not an array. Shape drift is rejected at seal time.
