---
name: agent-chorus
description: This skill should be used when the user asks "What is Claude doing?", "What did Gemini say?", "What is Codex working on?", "Compare Codex and Claude outputs.", "Read session from Cursor.", "How did that session change?", "Send a message to Codex.", "Any messages for me?", "What was redacted?", "Which files are relevant?", "Summarize this session.", "Show a timeline of agent activity.", or any request to inspect, compare, diff, search, summarize, or coordinate activity across Codex, Claude, Gemini, or Cursor agents using chorus.
version: 0.12.2
---

# Agent Chorus Skill

Use this skill to inspect, compare, diff, message, or summarize activity across AI coding agents (Codex, Claude, Gemini, Cursor) via the `chorus` CLI.

## Commands

```bash
chorus read --agent <agent> [--id=<id>] [--cwd=<path>] [--last=<N>] [--include-user] [--tool-calls] [--format=<fmt>] [--json] [--metadata-only] [--audit-redactions]
chorus list --agent <agent> [--cwd=<path>] [--limit=<N>] [--json]
chorus search <query> --agent <agent> [--cwd=<path>] [--json]
chorus compare --source <agent[:id]>... [--cwd=<path>] [--normalize] [--json]
chorus diff --agent <agent> --from <id1> --to <id2> [--cwd=<path>] [--last=<N>] [--json]
chorus summary --agent <agent> [--id=<id>] [--cwd=<path>] [--json]
chorus timeline [--agent <agent>]... [--cwd=<path>] [--limit=<N>] [--json]
chorus relevance --list | --test <path> | --suggest [--cwd=<path>] [--json]
chorus send --from <agent> --to <agent> --message <text> [--cwd=<path>]
chorus messages --agent <agent> [--cwd=<path>] [--clear] [--json]
```

Agents: `codex`, `claude`, `gemini`, `cursor`

## Intent Contract

When this skill is triggered:

1. Prefer direct evidence from `chorus` commands before reasoning.
2. Scope reads to the current project (`--cwd`) unless the user asks otherwise.
3. Default to the current/latest session when the user does not specify a session.
4. Interpret session timing language consistently:
   - "current" / "latest" → newest session
   - "past session" / "previous session" → one session before newest: `list --limit 2`, read the second ID
   - "past N sessions" → N sessions before latest (excluding latest): `list --limit N+1`, read the older N IDs
   - "last N sessions" → newest N sessions (including latest): `list --limit N`, read all
   - explicit session ID/substring → targeted read with `--id`
5. Ask for a session ID only after an initial fetch fails or when the user explicitly asks for an exact historical session.
6. If evidence is missing, report exactly what is missing.
7. Do not infer hidden context from partial data.
8. Return results first; avoid internal process narration.
9. Use `chorus diff` when the user asks how a session changed or wants to compare two sessions from the same agent.
10. Use `chorus send` / `chorus messages` when agents need to coordinate or leave notes for each other.
11. Use `chorus read --audit-redactions` when the user asks what was redacted or wants a security audit.
12. Use `chorus read --include-user` when checking what an agent is actively working on (status checks). Omit it for output-only handoff reads.
13. Use `chorus read --tool-calls` when the user needs to see which files were read/edited or which commands were run in a session.
14. Use `chorus summary` for a quick structured digest of a session (files referenced, tool call counts, duration) without reading full content.
15. Use `chorus timeline` for a cross-agent chronological view of activity in a project.

## Intent Router

| User phrase | Command |
|---|---|
| "What is Claude doing?" | `chorus read --agent claude --cwd <path> --include-user --json` |
| "What did Gemini say?" | `chorus read --agent gemini --cwd <path> --json` |
| "What is Codex working on?" | `chorus read --agent codex --cwd <path> --include-user --json` |
| "Evaluate Gemini's plan." | `chorus read --agent gemini --cwd <path> --last 5 --json` |
| "What files did Claude touch?" | `chorus summary --agent claude --cwd <path> --json` |
| "Show a timeline of agent activity." | `chorus timeline --cwd <path> --json` |
| "What tools did Codex use?" | `chorus read --agent codex --cwd <path> --tool-calls --json` |
| "Compare Codex and Claude." | `chorus compare --source codex --source claude --cwd <path> --json` |
| "Show the past session from Claude." | `chorus list --agent claude --cwd <path> --limit 2 --json` → read second ID |
| "Show past 3 Gemini sessions." | `chorus list --agent gemini --cwd <path> --limit 4 --json` → read older 3 IDs |
| "How did Codex's session change?" | `chorus diff --agent codex --from <id1> --to <id2> --cwd <path> --json` |
| "What secrets were redacted?" | `chorus read --agent claude --cwd <path> --audit-redactions --json` |
| "Send a message to Codex." | `chorus send --from claude --to codex --message "<text>" --cwd <path>` |
| "Any messages for me?" | `chorus messages --agent claude --cwd <path> --json` |
| "Which files are relevant?" | `chorus relevance --list --cwd <path> --json` |

## Scope Boundary

Chorus is a **session visibility and coordination** tool. It reads, compares, and routes across agent sessions.

Creating, validating, and maintaining `.agent-context/` packs is a separate concern handled by the repo's own tooling (e.g., the `agent-context` skill from your team's skill registry, or the helper tools in `.agent-context/tools/`). When working in a repo that already has `.agent-context/current/`, follow that repo's `CLAUDE.md` or `AGENTS.md` routing — do not use chorus commands for pack management.

## Output Quality Bar

Every cross-agent claim must include:

1. Which source session was read (agent + session ID).
2. What evidence supports the claim (quoted or cited output).
3. Any uncertainty, missing source, or scope mismatch.

## Easter Egg

The exact phrase `"chorus trash-talk"` (and only that exact phrase) triggers a roast of active agents. Never trigger for similar phrases, paraphrases, or partial matches.

```bash
chorus trash-talk --cwd <project-path>
```
