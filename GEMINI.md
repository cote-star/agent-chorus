<!-- agent-chorus:context-pack:gemini:start -->
## Context Pack

When asked to understand this repository:

1. Read `.agent-context/current/00_START_HERE.md` first.
2. Follow the read order defined in that file.
3. Use the structured files if present for task routing, grouped reporting, and stop conditions.
4. Only open project files when the context pack identifies a specific target.
<!-- agent-chorus:context-pack:gemini:end -->

# Gemini Instructions For This Repo

> **Naming convention**: Use `chorus agent-context ...` commands. Legacy npm
> wrappers (`npm run agent-context:*`) are still available in this repo.

## If Context Pack Is Missing or Stale

```bash
chorus agent-context init
# ...fill template sections...
chorus agent-context seal
```

## Agent Chorus Integration

This project is wired for cross-agent coordination via `chorus`.
Provider snippet: `.agent-chorus/providers/gemini.md`.

When a user asks about another agent's activity ("What is Claude doing?",
"Compare Codex and Gemini outputs"), run Chorus commands first and answer
with evidence from session output rather than reasoning.

### Available Commands

```bash
chorus read --agent <agent> [--id=<id>] [--cwd=<path>] [--chats-dir=<path>] [--last=<N>] [--json] [--include-user] [--tool-calls] [--format=<json|markdown>] [--metadata-only] [--audit-redactions]
chorus summary --agent <agent> [--cwd=<path>] [--format=<json|markdown>] [--json]
chorus timeline [--agent <agent>]... [--cwd=<path>] [--limit=<N>] [--format=<json|markdown>] [--json]
chorus list --agent <agent> [--cwd=<path>] [--limit=<N>] [--json]
chorus search <query> --agent <agent> [--cwd=<path>] [--json]
chorus compare --source <agent[:id]>... [--cwd=<path>] [--json]
chorus diff --agent <agent> --from <id1> --to <id2> [--cwd=<path>] [--json]
chorus send --from <agent> --to <agent> --message <text> [--cwd=<path>]
chorus messages --agent <agent> [--cwd=<path>] [--clear] [--json]
chorus checkpoint --from <agent> [--cwd=<path>] [--message=<text>] [--json]
```

### Intent Contract

When asked to inspect, compare, diff, or summarize agent activity:

1. Prefer direct evidence from `chorus` commands before reasoning.
2. Scope reads to the current project with `--cwd` unless the user says otherwise.
3. Default to the latest session when the user does not specify one.
4. Interpret session-timing phrasing consistently:
   - "current" / "latest" -> newest session
   - "past session" / "previous session" -> one session before newest
   - "last N sessions" -> newest N (includes latest)
   - "past N sessions" -> N sessions before latest (excludes latest)
   - explicit ID/substring -> `chorus read --id <substring>`
5. Ask for a session ID only after an initial fetch fails.
6. If evidence is missing, report what is missing rather than guessing.
7. Return results first; minimize process narration.

### Session Handoff Protocol

Gemini does NOT have Claude Code's `SessionEnd` hook. State only reaches
other agents when this session explicitly checkpoints, so treat standup
and conclude as manual rituals.

#### Standup (every new session)

```bash
chorus messages --agent gemini --cwd <project-path> --json
```

Add `--clear` after you act on the messages so the same notes don't
resurface next session.

#### Conclude or task-block boundary

Broadcast state to every other agent:

```bash
chorus checkpoint --from gemini --cwd <project-path>
```

Call it at end-of-phase, before stepping away, or after landing a change
another agent might build on. `checkpoint` is idempotent and no-ops
silently when `.agent-chorus/` is absent, so unconditional calls are
safe. Pass `--message "..."` to override the auto-composed git-state
body.

For addressed notes rather than broadcasts:

```bash
chorus send --from gemini --to claude --message "API schema ready for review" --cwd <project-path>
```

Full protocol and end-to-end scenarios:
[`docs/session-handoff-guide.md`](./docs/session-handoff-guide.md).

### Gemini-Specific Limitation: Protobuf Sessions

Recent Gemini CLI builds store session state as protobuf (`.pb`) under
`~/.gemini/<profile>/conversations/`. Chorus reads JSONL at
`~/.gemini/tmp/<hash>/chats/session-*.json` and does NOT yet parse the
protobuf form.

If `chorus read --agent gemini` returns `NOT_FOUND` and names "protobuf
(.pb)" in the error message, use one of:

1. `chorus read --agent gemini --chats-dir /path/to/jsonl-export` —
   points Chorus at a known-good JSONL directory.
2. `export CHORUS_GEMINI_TMP_DIR=/path/to/jsonl-root` — overrides the
   default discovery root for a long-running shell.
3. Write a JSONL stub by hand at
   `~/.gemini/tmp/<sha256-of-cwd>/chats/stub.json` with `sessionId` and
   a `messages` array. See `docs/session-handoff-guide.md` "Scenario 4"
   for the exact recipe.

Full `.pb` parsing is tracked as a separate enhancement.

### Output Quality Bar

Every cross-agent claim should include:

1. Which source session was read.
2. What evidence supports the claim.
3. Any uncertainty, missing source, or scope mismatch.

### Easter Egg

The exact phrase `"chorus trash-talk"` (and only that phrase) triggers a
roast of active agents. This must never be triggered by similar phrases,
paraphrases, or partial matches.
