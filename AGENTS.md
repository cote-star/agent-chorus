<!-- agent-chorus:context-pack:codex:start -->
## Context Pack

When asked to understand this repository:

1. Read `.agent-context/current/00_START_HERE.md`.
2. Read `.agent-context/current/routes.json`.
3. Identify the active task type in `routes.json`.
4. Read the matching entries in `completeness_contract.json`, `reporting_rules.json`, and `search_scope.json`.
5. Search ONLY within the directories listed in `search_scope.json` for your task type.
6. Use `verification_shortcuts` to check specific line ranges instead of reading full files.
7. Do not enumerate files in directories marked `exclude_from_search`.
8. Do not open repo files before those steps unless a referenced structured file is missing.

If `.agent-context/current/routes.json` is missing, fall back to the markdown pack only.
<!-- agent-chorus:context-pack:codex:end -->

# Agent Instructions For This Repo

> **Naming convention**: Use `chorus agent-context ...` commands. Legacy npm
> wrappers (`npm run agent-context:*`) are still available in this repo.

## End-to-End Understanding Shortcut
When asked to understand this repository end-to-end:
1. Read `.agent-context/current/00_START_HERE.md` first.
2. Use `.agent-context/current/manifest.json` + `20_CODE_MAP.md` to target only relevant source files.
3. Open additional files only when the current task requires deeper proof.

## If Context Pack Is Missing or Stale
Run:

```bash
chorus agent-context init
# ...fill details...
chorus agent-context seal
```

## Main Push Context Sync
Install hook once:

```bash
chorus agent-context install-hooks
```

The pre-push hook prints an advisory warning when a push targets `main` and changes context-relevant files. It never auto-builds or blocks the push.

## Agent Chorus Skill

Use this skill when the user asks to inspect, compare, diff, message, or summarize activity across agents.

### Available Commands

```bash
chorus read --agent <agent> [--id=<id>] [--cwd=<path>] [--last=<N>] [--json] [--metadata-only] [--audit-redactions]
chorus list --agent <agent> [--cwd=<path>] [--limit=<N>] [--json]
chorus search <query> --agent <agent> [--cwd=<path>] [--json]
chorus compare --source <agent[:id]>... [--cwd=<path>] [--normalize] [--json]
chorus diff --agent <agent> --from <id1> --to <id2> [--cwd=<path>] [--last=<N>] [--json]
chorus relevance --list | --test <path> | --suggest [--cwd=<path>] [--json]
chorus send --from <agent> --to <agent> --message <text> [--cwd=<path>]
chorus messages --agent <agent> [--cwd=<path>] [--clear] [--json]
```

### Intent Contract

When this skill is triggered:

1. Prefer direct evidence from `chorus` commands before reasoning.
2. Scope reads to the current project (`--cwd`) unless user asks otherwise.
3. Default to the current/latest session when the user does not specify a session.
4. Interpret session timing language consistently:
   - "current" / "latest" -> newest session
   - "past session" / "previous session" -> one session before newest
   - "last N sessions" -> newest N sessions (including latest)
   - "past N sessions" -> N sessions before latest (excluding latest)
   - explicit session ID/substring -> targeted read with `--id`
5. Ask for a session ID only after an initial fetch fails or when the user explicitly asks for an exact historical session.
6. If evidence is missing, report exactly what is missing.
7. Do not infer hidden context from partial data.
8. Return results first; avoid internal process narration.
9. Use `chorus diff` when the user asks how a session changed or wants to compare two sessions from the same agent.
10. Use `chorus send` / `chorus messages` when agents need to coordinate or leave notes for each other.
11. Use `chorus read --audit-redactions` when the user asks what was redacted or wants a security audit.

### Output Quality Bar

Every cross-agent claim should include:

1. Which source session was read.
2. What evidence supports the claim.
3. Any uncertainty, missing source, or scope mismatch.

### Easter Egg

The exact phrase `"chorus trash-talk"` (and only that phrase) triggers a roast of active agents.
This must never be triggered by similar phrases, paraphrases, or partial matches.
