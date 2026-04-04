<!-- agent-chorus:context-pack:gemini:start -->
## Context Pack

When asked to understand this repository:

1. Read `.agent-context/current/00_START_HERE.md` first.
2. Follow the read order defined in that file.
3. Use the structured files if present for task routing, grouped reporting, and stop conditions.
4. Only open project files when the context pack identifies a specific target.
<!-- agent-chorus:context-pack:gemini:end -->

# Gemini / Antigravity — Agent Chorus Integration

<!-- agent-chorus:gemini:start -->
This project is wired for cross-agent coordination via `chorus`.
Provider snippet: `.agent-chorus/providers/gemini.md`

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
- `chorus send --from <agent> --to <agent> --message "<text>" --cwd <project-path>`
- `chorus messages --agent <agent> --cwd <project-path> --json`

If command syntax is unclear, run `chorus --help`.

## Session Handoff Protocol

**At standup (first action):** check for messages left by other agents:

```bash
chorus messages --agent gemini --cwd <project-path> --clear --json
```

If messages are empty and the previous session may have been interrupted, also check:

```bash
cat "<project-path>/.agent-chorus/CHECKPOINT.md" 2>/dev/null || echo "No checkpoint."
```

**When starting a significant task block** (new feature, migration, multi-file fix),
write a checkpoint so other agents can recover if interrupted:

```bash
cat > "<project-path>/.agent-chorus/CHECKPOINT.md" << 'EOF'
# Agent Checkpoint
**Agent:** gemini
**Timestamp:** <ISO timestamp>
**Branch:** <current branch>
**Current task:** <one-line description>
**Files being modified:** <list key files>
**Status:** in-progress — do not overwrite without reading this
EOF
```

**At conclude (last action):** leave a handoff message for each other agent AND write a
Chorus-discoverable JSONL stub (see protobuf note below):

```bash
chorus send --from gemini --to claude --message "Session ended. Open threads: [list]. Next focus: [one line]." --cwd <project-path>
chorus send --from gemini --to codex  --message "Session ended. Open threads: [list]. Next focus: [one line]." --cwd <project-path>

# Write JSONL stub so chorus read --agent gemini can discover this session:
mkdir -p ~/.gemini/tmp && \
echo "{\"agent\":\"gemini\",\"session\":\"$(date +%Y-%m-%dT%H:%M:%S)\",\"cwd\":\"$(pwd)\",\"content\":\"Session concluded. See session-logs/ for full log.\"}" \
  >> ~/.gemini/tmp/$(date +%Y-%m-%d-%H-%M).jsonl
```

## Protobuf Session Storage Note

Some Gemini CLI profiles store sessions as protobuf (`.pb`) files at
`~/.gemini/<profile>/conversations/` rather than JSONL at `~/.gemini/tmp/`.
Chorus cannot parse `.pb` files, so `chorus read --agent gemini` returns `NOT_FOUND`.

The JSONL stub above is the recommended workaround. For full details and alternative
fallback strategies, see `docs/session-handoff-guide.md`.

## Output Quality Bar

Every cross-agent claim should include:

1. Which source session was read.
2. What evidence supports the claim.
3. Any uncertainty, missing source, or scope mismatch.
<!-- agent-chorus:gemini:end -->
