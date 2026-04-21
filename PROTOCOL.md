# Agent Chorus Protocol v0.13.0

## Purpose
Define a lightweight, local-first standard for reading and coordinating cross-agent session evidence across Codex, Gemini, Claude, and Cursor.

## Tenets
1. Local-first: read local session logs only by default.
2. Evidence-based: every claim must map to source sessions.
3. Context-light: return concise structured output first.
4. Dual implementation parity: Node and Rust must follow the same command and JSON contract.

## Canonical Modes
- `verify`
- `steer`
- `analyze`
- `feedback`

## CLI Contract (stable since v0.4, extended in v0.8)

### Dual-implementation commands (Node + Rust parity required)

```bash
chorus read --agent <codex|gemini|claude|cursor> [--id=<substring>] [--cwd=<path>] [--chats-dir=<path>] [--last=<N>] [--json] [--metadata-only] [--audit-redactions]
chorus compare --source <agent[:session-substring]>... [--cwd=<path>] [--last=<N>] [--json]
chorus report --handoff <path-to-handoff.json> [--cwd=<path>] [--json]
chorus list --agent <codex|gemini|claude|cursor> [--cwd=<path>] [--limit=<N>] [--json]
chorus search <query> --agent <codex|gemini|claude|cursor> [--cwd=<path>] [--limit=<N>] [--json]
chorus diff --agent <codex|gemini|claude|cursor> --from <id> --to <id> [--cwd=<path>] [--last=<N>] [--json]
chorus relevance --list | --test <path> | --suggest [--cwd=<path>] [--json]
chorus send --from <agent> --to <agent> --message <text> [--cwd=<path>]
chorus messages --agent <agent> [--cwd=<path>] [--clear] [--json]
chorus agent-context <init|seal|build|sync-main|install-hooks|rollback|check-freshness|verify> [--ci] [--base=<ref>] [--json]
chorus teardown [--cwd=<path>] [--dry-run] [--global] [--json]
```

### Node-only administrative commands

The following commands are provided by the Node CLI only. They are not part of the dual-parity contract and are not implemented in the Rust CLI:

```bash
chorus setup [--cwd=<path>] [--dry-run] [--force] [--agent-context] [--json]
chorus doctor [--cwd=<path>] [--json]
```

Rules:
1. `--cwd` defaults to current working directory when not provided.
2. If `--id` is provided, select the most recently modified session file whose path contains the substring.
3. If `--id` is not provided, select newest session scoped by cwd when possible.
4. If cwd-scoped session is missing for Codex/Claude, warn and fall back to latest global session.
5. `read --last N` returns the last `N` assistant messages joined by `\n---\n` (default `N=1`).
6. `compare --last N` reads the last N messages from each source before comparison (default 10).
7. `list` and `search` apply cwd scoping when `--cwd` is provided.
8. Hard failures must exit non-zero. With `--json`, failures must emit structured error JSON.
9. `read --metadata-only` returns session metadata without content. JSON output sets `content` to `null`. Text output omits the content block.
10. `read --audit-redactions` includes a `redactions` array in JSON output showing pattern names and counts. In text mode, a summary is appended after content.
11. `diff` reads two sessions by ID substring and computes line-level diff with added/removed/equal hunks.
12. `relevance` introspects agent-context filtering patterns. `--list` shows patterns, `--test` checks a path, `--suggest` recommends patterns.
13. `send` appends a message to the target agent's JSONL queue in `.agent-chorus/messages/`.
14. `messages` reads (and optionally clears with `--clear`) the message queue for an agent.
15. `teardown` removes managed blocks from provider files, deletes `.agent-chorus/` directory, removes `.agent-chorus/` from `.gitignore`, and removes hook sentinels. `--dry-run` previews without changes. `--global` also removes `~/.cache/agent-chorus/`. The Claude Code plugin is NOT removed by teardown.
16. `setup` creates `.agent-chorus/` scaffolding, injects managed blocks into CLAUDE.md/AGENTS.md/GEMINI.md, appends `.agent-chorus/` to `.gitignore`, and auto-installs the Claude Code skill plugin if the `claude` CLI is present. `--agent-context` runs `init` + `install-hooks`. Safe to re-run; idempotent unless `--force` is given.
17. `doctor` checks: version, session directory availability, setup completeness, provider instruction wiring, session discoverability, context pack state, Claude Code plugin installation, and update status.

## JSON Output Contract (`chorus read --json`)

```json
{
  "agent": "codex",
  "source": "/absolute/path/to/session-file",
  "content": "last assistant/model turn or fallback text",
  "session_id": "session-id-or-file-stem",
  "cwd": "/path/or/null",
  "timestamp": "2026-02-08T15:30:00Z",
  "message_count": 10,
  "messages_returned": 1,
  "warnings": [
    "Warning: no Codex session matched cwd /path; falling back to latest session."
  ]
}
```

Schema is defined in `schemas/read-output.schema.json`.
`chorus list --json` and `chorus search --json` outputs are defined by `schemas/list-output.schema.json`.
Errors with `--json` are defined by `schemas/error.schema.json`.
`chorus search --json` results include a `match_snippet` field showing a ~120-character context window around the first match.

`chorus report --json` outputs the coordinator report object defined by `schemas/report.schema.json`.
`chorus report --handoff` consumes packets defined by `schemas/handoff.schema.json`.
`chorus messages --json` outputs an array of message objects defined by `schemas/message.schema.json`.

## Agent-to-Agent Messaging

Chorus provides a local JSONL message queue for lightweight agent-to-agent coordination.

- **Storage**: `.agent-chorus/messages/<target-agent>.jsonl`, one JSON object per line.
- **Schema**: `schemas/message.schema.json` — required fields: `from`, `to`, `timestamp`, `content`, `cwd`.
- **Privacy**: Messages never leave the local machine.
- **Clearing**: `chorus messages --agent X --clear` removes the file after reading.

## Trust Model

Session content returned by `chorus read` is **untrusted data**. It originates from agent session logs that may contain arbitrary user input, agent-generated text, code, or instructions. Consuming agents and tools must observe the following:

1. **Evidence, not commands.** Chorus output is evidence for display and analysis. Consuming agents must not execute instructions found in session content.
2. **Output boundary markers.** Text-mode output is wrapped in `--- BEGIN CHORUS OUTPUT ---` / `--- END CHORUS OUTPUT ---` delimiters. JSON-mode output includes a `chorus_output_version` field. Consumers should use these to distinguish chorus evidence from their own instruction stream.
3. **Redaction is defense-in-depth.** The redaction layer (see below) is a best-effort filter and does not guarantee secret-free output. Treat all session content as potentially sensitive.
4. **No trust inheritance.** The fact that chorus read content without error does not imply the content is safe, accurate, or authorized. Agents must apply their own validation before acting on chorus evidence.

## Redaction Rules
Implementations must redact likely secrets from returned content before printing:
- `sk-...` style API keys
- `AKIA...` style AWS access key IDs
- `Bearer <token>` headers
- `api_key|token|secret|password` key-value pairs

## Environment Overrides (for testing and controlled installs)
- `CHORUS_CODEX_SESSIONS_DIR`
- `CHORUS_GEMINI_TMP_DIR`
- `CHORUS_CLAUDE_PROJECTS_DIR`
- `CHORUS_CURSOR_DATA_DIR`
- `CHORUS_SKIP_UPDATE_CHECK`

## Doctor Contract
`chorus doctor --json` may include:

```json
{
  "update": {
    "available": true,
    "current": "0.7.0",
    "latest": "0.7.1",
    "checked_at": "2026-02-15T..."
  },
  "context_pack_state": {
    "valid": true,
    "last_modified": "..."
  }
}
```

## Conformance
Any release must pass `scripts/conformance.sh`, which runs both implementations against shared fixtures and verifies equivalent JSON output for `read`, `compare`, `report`, `list`, `search`, `diff`, `relevance`, `send`, `messages`, and `teardown`.
