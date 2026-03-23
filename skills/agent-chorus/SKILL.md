---
name: agent-chorus
description: This skill should be used when the user asks "What is Claude doing?", "What did Gemini say?", "What is Codex working on?", "Compare Codex and Claude outputs.", "Read session from Cursor.", "How did that session change?", "Send a message to Codex.", "Any messages for me?", "What was redacted?", "Which files are relevant?", or any request to inspect, compare, diff, search, summarize, or coordinate activity across Codex, Claude, Gemini, or Cursor agents using chorus.
version: 0.8.3
---

# Agent Chorus Skill

Use this skill to inspect, compare, diff, message, or summarize activity across AI coding agents (Codex, Claude, Gemini, Cursor) via the `chorus` CLI.

## Commands

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

## Intent Router

| User phrase | Command |
|---|---|
| "What is Claude doing?" | `chorus read --agent claude --cwd <path> --json` |
| "What did Gemini say?" | `chorus read --agent gemini --cwd <path> --json` |
| "What is Codex working on?" | `chorus read --agent codex --cwd <path> --json` |
| "Evaluate Gemini's plan." | `chorus read --agent gemini --cwd <path> --last 5 --json` |
| "Compare Codex and Claude." | `chorus compare --source codex --source claude --cwd <path> --json` |
| "Show the past session from Claude." | `chorus list --agent claude --cwd <path> --limit 2 --json` → read second ID |
| "Show past 3 Gemini sessions." | `chorus list --agent gemini --cwd <path> --limit 4 --json` → read older 3 IDs |
| "How did Codex's session change?" | `chorus diff --agent codex --from <id1> --to <id2> --cwd <path> --json` |
| "What secrets were redacted?" | `chorus read --agent claude --cwd <path> --audit-redactions --json` |
| "Send a message to Codex." | `chorus send --from claude --to codex --message "<text>" --cwd <path>` |
| "Any messages for me?" | `chorus messages --agent claude --cwd <path> --json` |
| "Which files are relevant?" | `chorus relevance --list --cwd <path> --json` |

## Context Pack Usage

When working in a repo that has `.agent-context/current/`:

1. **Impact analysis tasks** (list all files that must change): read `30_BEHAVIORAL_INVARIANTS.md` Update Checklist *before* `20_CODE_MAP.md`. The checklist has the full blast radius per change type. CODE_MAP is a navigation index — it is not exhaustive.
2. **Navigation tasks** (find a file, find a value): start with `20_CODE_MAP.md`.
3. **Diagnosis tasks** (silent failures, unexpected output): start with `10_SYSTEM_OVERVIEW.md` Silent Failure Modes section.
4. Never treat CODE_MAP as a complete list of affected files for a given change — always cross-reference BEHAVIORAL_INVARIANTS and verify with grep.

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
