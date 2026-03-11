# Agent Bridge Protocol v0.7.0

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

## CLI Contract (stable since v0.4)
Both implementations must support:

```bash
bridge read --agent <codex|gemini|claude|cursor> [--id=<substring>] [--cwd=<path>] [--chats-dir=<path>] [--last=<N>] [--json]
bridge compare --source <agent[:session-substring]>... [--cwd=<path>] [--normalize] [--json]
bridge report --handoff <path-to-handoff.json> [--cwd=<path>] [--json]
bridge list --agent <codex|gemini|claude|cursor> [--cwd=<path>] [--limit=<N>] [--json]
bridge search <query> --agent <codex|gemini|claude|cursor> [--cwd=<path>] [--limit=<N>] [--json]
bridge context-pack <init|seal|build|sync-main|install-hooks|rollback|check-freshness>
```

Rules:
1. `--cwd` defaults to current working directory when not provided.
2. If `--id` is provided, select the most recently modified session file whose path contains the substring.
3. If `--id` is not provided, select newest session scoped by cwd when possible.
4. If cwd-scoped session is missing for Codex/Claude, warn and fall back to latest global session.
5. `read --last N` returns the last `N` assistant messages joined by `\n---\n` (default `N=1`).
6. `compare --normalize` collapses whitespace before divergence checks.
7. `list` and `search` apply cwd scoping when `--cwd` is provided.
8. Hard failures must exit non-zero. With `--json`, failures must emit structured error JSON.

## JSON Output Contract (`bridge read --json`)

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
`bridge list --json` and `bridge search --json` outputs are defined by `schemas/list-output.schema.json`.
Errors with `--json` are defined by `schemas/error.schema.json`.

`bridge report --json` outputs the coordinator report object defined by `schemas/report.schema.json`.
`bridge report --handoff` consumes packets defined by `schemas/handoff.schema.json`.

## Redaction Rules
Implementations must redact likely secrets from returned content before printing:
- `sk-...` style API keys
- `AKIA...` style AWS access key IDs
- `Bearer <token>` headers
- `api_key|token|secret|password` key-value pairs

## Environment Overrides (for testing and controlled installs)
- `BRIDGE_CODEX_SESSIONS_DIR`
- `BRIDGE_GEMINI_TMP_DIR`
- `BRIDGE_CLAUDE_PROJECTS_DIR`
- `BRIDGE_CURSOR_DATA_DIR`
- `BRIDGE_SKIP_UPDATE_CHECK`

## Doctor Contract
`bridge doctor --json` may include:

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
Any release must pass `scripts/conformance.sh`, which runs both implementations against shared fixtures and verifies equivalent JSON output for `read`, `compare`, `report`, `list`, and `search`.
