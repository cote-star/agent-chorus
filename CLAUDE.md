<!-- agent-chorus:context-pack:claude:start -->
## Context Pack

**BEFORE starting any task**, read the context pack in this order:

1. `.agent-context/current/00_START_HERE.md` — entrypoint, routing, stop rules
2. `.agent-context/current/30_BEHAVIORAL_INVARIANTS.md` — change checklists, file families, what NOT to do
3. `.agent-context/current/20_CODE_MAP.md` — navigation index, tracing flows

Read these three files BEFORE opening any repo source files. Then open only the files the pack identifies as relevant.

For architecture questions, also read `10_SYSTEM_OVERVIEW.md`. For test/deploy questions, also read `40_OPERATIONS_AND_RELEASE.md`.
<!-- agent-chorus:context-pack:claude:end -->

# Claude Code Instructions

> **Naming convention**: Use `chorus agent-context ...` commands. Legacy npm
> wrappers (`npm run agent-context:*`) are still available in this repo.

## Context Pack

When asked to understand this repository (or any "what does this repo do?" intent):

1. Read `.agent-context/current/00_START_HERE.md` first.
2. Follow the read order defined in that file (`10_SYSTEM_OVERVIEW.md`, then
   `30_BEHAVIORAL_INVARIANTS.md`, then `20_CODE_MAP.md`, then
   `40_OPERATIONS_AND_RELEASE.md`).
3. Only open project files when the context pack identifies a specific target.

If the context pack is missing, run:

```bash
chorus agent-context init
# ...fill in template sections...
chorus agent-context seal
```

If the context pack is stale (already initialized), run:

```bash
chorus agent-context seal
```

## Context Pack Maintenance

After making changes to source files, check whether `.agent-context/current/`
needs updating. If the changes affect architecture, commands, behavioral
invariants, or the code map, run:

```bash
chorus agent-context seal
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
- `chorus diff --agent <agent> --from <id1> --to <id2> --cwd <project-path> --json`
- `chorus read --agent <agent> --cwd <project-path> --audit-redactions --json`
- `chorus relevance --list --cwd <project-path> --json`
- `chorus send --from <agent> --to <agent> --message "<text>" --cwd <project-path>`
- `chorus messages --agent <agent> --cwd <project-path> --json`

If command syntax is unclear, run `chorus --help`.

## Trigger Phrases

- "What is Claude doing?"
- "What did Gemini say?"
- "Compare Codex and Claude outputs."
- "Read session <id> from Cursor."
- "How did that session change?"
- "Send a message to Codex."
- "Any messages for me?"
- "What was redacted?"
- "Which files are relevant?"

## Intent Router

- "What is Claude doing?" -> `chorus read --agent claude --cwd <project-path> --json`
- "What did Gemini say?" -> `chorus read --agent gemini --cwd <project-path> --json`
- "Evaluate Gemini's plan." -> `chorus read --agent gemini --cwd <project-path> --last 5 --json`
- "Compare Codex and Claude outputs." -> `chorus compare --source codex --source claude --cwd <project-path> --json`
- "Show the past session from Claude." -> `chorus list --agent claude --cwd <project-path> --limit 2 --json`, then read the second session ID
- "Show past 3 Gemini sessions." -> `chorus list --agent gemini --cwd <project-path> --limit 4 --json`, then read the 3 older session IDs
- "How did Codex's session change?" -> `chorus diff --agent codex --from <id1> --to <id2> --cwd <project-path> --json`
- "What secrets were redacted?" -> `chorus read --agent claude --cwd <project-path> --audit-redactions --json`
- "Send a message to Codex." -> `chorus send --from claude --to codex --message "<text>" --cwd <project-path>`
- "Any messages for me?" -> `chorus messages --agent claude --cwd <project-path> --json`
- "Which files are relevant?" -> `chorus relevance --list --cwd <project-path> --json`

## Session Handoff Protocol

Cross-agent messaging only works if every agent participates on the same
schedule. Run the standup ritual at the start of every Claude Code
session, and the conclude ritual before you end it.

### Standup (every new session)

```bash
chorus messages --agent claude --clear --cwd <project-path>
```

`--clear` drains the inbox after you read it so the same notes don't
resurface next session. If you want to keep the messages for later
review, omit `--clear`.

### Conclude (before ending the session)

Use both, in order — they serve different purposes:

```bash
# Targeted notes for specific agents
chorus send --from claude --to codex --message "payment refactor done; tests red" --cwd <project-path>

# State broadcast to every other agent
chorus checkpoint --from claude --cwd <project-path>
```

`send` is for addressed notes ("Codex, look at this"). `checkpoint` is
for state broadcasts ("I'm out, here's what I was on") — it writes a
short message (branch, uncommitted-file count, last commit) to every
other agent's inbox. Pass `--message "..."` to override the auto-composed
body.

### Interruption resilience

Claude Code fires a `SessionEnd` hook before it tears down. Wire that
hook to `scripts/hooks/chorus-session-end.sh`, which delegates to
`chorus checkpoint --from claude` so the subcommand stays the single
source of truth.

Install by adding to `~/.claude/settings.json`:

```json
{
  "hooks": {
    "SessionEnd": [{
      "hooks": [
        {
          "type": "command",
          "command": "bash /absolute/path/to/chorus-session-end.sh",
          "timeout": 10
        }
      ]
    }]
  }
}
```

The script guards on `.agent-chorus/` presence, so it no-ops cleanly on
projects that don't use Chorus. Safe to install globally.

Full protocol, Gemini `.pb` fallback, and mid-task checkpoint patterns:
[`docs/session-handoff-guide.md`](./docs/session-handoff-guide.md).

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
