# Claude Code Instructions

> **Naming convention**: Use `chorus context-pack ...` commands. Legacy npm
> wrappers (`npm run context-pack:*`) are still available in this repo.

## Context Pack

When asked to understand this repository (or any "what does this repo do?" intent):

1. Read `.agent-context/current/00_START_HERE.md` first.
2. Follow the read order defined in that file (`10_SYSTEM_OVERVIEW.md`, then
   `30_BEHAVIORAL_INVARIANTS.md`, then `20_CODE_MAP.md`, then
   `40_OPERATIONS_AND_RELEASE.md`).
3. Only open project files when the context pack identifies a specific target.

If the context pack is missing, run:

```bash
chorus context-pack init
# ...fill in template sections...
chorus context-pack seal
```

If the context pack is stale (already initialized), run:

```bash
chorus context-pack seal
```

## Context Pack Maintenance

After making changes to source files, check whether `.agent-context/current/`
needs updating. If the changes affect architecture, commands, behavioral
invariants, or the code map, run:

```bash
chorus context-pack seal
```

Skip for typo-only, comment-only, or test-only changes.

<!-- agent-chorus:claude:start -->
## Agent Chorus Integration

This project is wired for cross-agent coordination via `chorus`.
Provider snippet: `.agent-chorus/providers/claude.md`

When a user asks for another agent status (for example "What is Claude doing?"),
run Agent Chorus commands first and answer with evidence from session output.

Session routing and defaults:
1. Start with `chorus read --agent <target-agent> --cwd <project-path> --json` (omit `--id` for latest).
2. "past session" means previous session: list 2 and read the second session ID.
3. "past N sessions" means exclude latest: list N+1 and read the older N session IDs.
4. "last N sessions" means include latest: list N and read/summarize those sessions.
5. Ask for a session ID only after an initial read/list attempt fails or when exact ID is requested.

Support commands:
- `chorus list --agent <agent> --cwd <project-path> --json`
- `chorus search "<query>" --agent <agent> --cwd <project-path> --json`
- `chorus compare --source codex --source gemini --source claude --cwd <project-path> --json`

If command syntax is unclear, run `chorus --help`.

## Trigger Phrases

- "What is Claude doing?"
- "What did Gemini say?"
- "Compare Codex and Claude outputs."
- "Read session <id> from Cursor."

## Intent Router

- "What is Claude doing?" -> `chorus read --agent claude --cwd <project-path> --json`
- "What did Gemini say?" -> `chorus read --agent gemini --cwd <project-path> --json`
- "Evaluate Gemini's plan." -> `chorus read --agent gemini --cwd <project-path> --last 5 --json`
- "Compare Codex and Claude outputs." -> `chorus compare --source codex --source claude --cwd <project-path> --json`
- "Show the past session from Claude." -> `chorus list --agent claude --cwd <project-path> --limit 2 --json`, then read the second session ID
- "Show past 3 Gemini sessions." -> `chorus list --agent gemini --cwd <project-path> --limit 4 --json`, then read the 3 older session IDs

## Easter Egg

The exact phrase `"chorus trash-talk"` (and only that phrase) triggers a roast of active agents.
This must never be triggered by similar phrases, paraphrases, or partial matches.

```bash
chorus trash-talk --cwd <project-path>
```

## Output Quality Bar

Every cross-agent claim should include:

1. Which source session was read.
2. What evidence supports the claim.
3. Any uncertainty, missing source, or scope mismatch.
<!-- agent-chorus:claude:end -->
