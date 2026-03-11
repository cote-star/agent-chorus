# Claude Code Instructions

> **Naming convention**: Use `bridge context-pack ...` commands. Legacy npm
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
bridge context-pack init
# ...fill in template sections...
bridge context-pack seal
```

If the context pack is stale (already initialized), run:

```bash
bridge context-pack seal
```

## Context Pack Maintenance

After making changes to source files, check whether `.agent-context/current/`
needs updating. If the changes affect architecture, commands, behavioral
invariants, or the code map, run:

```bash
bridge context-pack seal
```

Skip for typo-only, comment-only, or test-only changes.

<!-- agent-bridge:claude:start -->
## Agent Bridge Integration

This project is wired for cross-agent coordination via `bridge`.
Provider snippet: `.agent-bridge/providers/claude.md`

When a user asks for another agent status (for example "What is Claude doing?"),
run Agent Bridge commands first and answer with evidence from session output.

Session routing and defaults:
1. Start with `bridge read --agent <target-agent> --cwd <project-path> --json` (omit `--id` for latest).
2. "past session" means previous session: list 2 and read the second session ID.
3. "past N sessions" means exclude latest: list N+1 and read the older N session IDs.
4. "last N sessions" means include latest: list N and read/summarize those sessions.
5. Ask for a session ID only after an initial read/list attempt fails or when exact ID is requested.

Support commands:
- `bridge list --agent <agent> --cwd <project-path> --json`
- `bridge search "<query>" --agent <agent> --cwd <project-path> --json`
- `bridge compare --source codex --source gemini --source claude --cwd <project-path> --json`

If command syntax is unclear, run `bridge --help`.
<!-- agent-bridge:claude:end -->
