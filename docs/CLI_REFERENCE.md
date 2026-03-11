# CLI Reference

Use this page for full command syntax, examples, output contracts, and operational flags.

## Command Contract

```bash
chorus read --agent <codex|gemini|claude|cursor> [--id=<substring>] [--cwd=<path>] [--chats-dir=<path>] [--last=<N>] [--json]
chorus compare --source <agent[:session-substring]>... [--cwd=<path>] [--normalize] [--json]
chorus report --handoff <handoff.json> [--cwd=<path>] [--json]
chorus list --agent <codex|gemini|claude|cursor> [--cwd=<path>] [--limit=<N>] [--json]
chorus search <query> --agent <codex|gemini|claude|cursor> [--cwd=<path>] [--limit=<N>] [--json]
chorus setup [--cwd=<path>] [--dry-run] [--force] [--context-pack] [--json]
chorus doctor [--cwd=<path>] [--json]
chorus context-pack <build|sync-main|install-hooks|rollback|check-freshness> [...]
```

## Reading a Session

```bash
# Read from Codex (defaults to latest session, last message)
chorus read --agent codex

# Read from Claude, scoped to current working directory
chorus read --agent claude --cwd /path/to/project

# Read the previous (past) Claude session
chorus list --agent claude --cwd /path/to/project --limit 2 --json
chorus read --agent claude --id "<second-session-id>" --cwd /path/to/project

# Read the last 5 assistant messages from a session
chorus read --agent codex --id "fix-bug" --last 5

# Read from Cursor
chorus read --agent cursor --json

# Get machine-readable JSON output
chorus read --agent gemini --json
```

When `--last N` is greater than 1, multiple messages are separated by `\n---\n` in the `content` field.

**JSON output includes metadata:**

```json
{
  "agent": "codex",
  "source": "/path/to/session.jsonl",
  "content": "The assistant's response...",
  "warnings": [],
  "session_id": "session-abc123",
  "cwd": "/workspace/project",
  "timestamp": "2026-01-15T10:30:00Z",
  "message_count": 12,
  "messages_returned": 1
}
```

## Listing Sessions

```bash
# List the 10 most recent Codex sessions
chorus list --agent codex --json

# Limit results
chorus list --agent claude --limit 5 --json

# Scope to a working directory
chorus list --agent codex --cwd /path/to/project --json
```

**JSON output:**

```json
[
  {
    "session_id": "session-abc123",
    "agent": "codex",
    "cwd": "/workspace/project",
    "modified_at": "2026-01-15T10:30:00Z",
    "file_path": "/home/user/.codex/sessions/2026/01/15/session-abc123.jsonl"
  }
]
```

## Searching Sessions

```bash
# Find sessions mentioning "authentication"
chorus search "authentication" --agent claude --json

# Limit results
chorus search "bug fix" --agent codex --limit 3 --json
```

## Comparing Agents

```bash
# Compare latest sessions across agents
chorus compare --source codex --source gemini --source claude --json

# Compare specific sessions
chorus compare --source codex:fix-bug --source claude:fix-bug --json

# Ignore whitespace differences
chorus compare --source codex --source gemini --normalize --json
```

The `--normalize` flag collapses all whitespace before comparison.

## Reporting

```bash
chorus report --handoff ./handoff_packet.json --json
```

## Context Pack

```bash
# Build or refresh context pack files
chorus context-pack build

# Install pre-push hook to auto-sync context pack for main pushes
chorus context-pack install-hooks

# Restore latest local snapshot
chorus context-pack rollback

# Non-blocking warning check for stale pack updates
chorus context-pack check-freshness --base origin/main
```

You can also bootstrap context-pack from setup:

```bash
chorus setup --context-pack
```

## Common Recipes

```bash
# Handoff recovery: read latest work from another agent in this repo
chorus read --agent claude --cwd . --json

# Cross-agent verification: validate a claim with search + compare
chorus search "processPayment" --agent codex --cwd . --json
chorus compare --source codex --source claude --json

# Cold-start onboarding: build a compact index before deeper reads
chorus setup --context-pack
chorus context-pack build
chorus context-pack check-freshness --base origin/main
```

## Error Codes

When `--json` is active, errors are returned as structured JSON:

```json
{
  "error_code": "NOT_FOUND",
  "message": "No Codex session found."
}
```

| Error Code          | Meaning                            |
| :------------------ | :--------------------------------- |
| `NOT_FOUND`         | No matching session found          |
| `PARSE_FAILED`      | Session file could not be parsed   |
| `INVALID_HANDOFF`   | Malformed handoff packet           |
| `UNSUPPORTED_AGENT` | Unknown agent type                 |
| `UNSUPPORTED_MODE`  | Invalid mode in handoff            |
| `EMPTY_SESSION`     | Session exists but has no messages |
| `IO_ERROR`          | General I/O error                  |

## Configuration

Override default paths using environment variables.

| Variable                     | Description               | Default                                |
| :--------------------------- | :------------------------ | :------------------------------------- |
| `CHORUS_CODEX_SESSIONS_DIR`  | Path to Codex sessions    | `~/.codex/sessions`                    |
| `CHORUS_GEMINI_TMP_DIR`      | Path to Gemini temp chats | `~/.gemini/tmp`                        |
| `CHORUS_CLAUDE_PROJECTS_DIR` | Path to Claude projects   | `~/.claude/projects`                   |
| `CHORUS_CURSOR_DATA_DIR`     | Path to Cursor data       | `~/Library/Application Support/Cursor` |

## Redaction

Chorus automatically redacts sensitive data before output:

| Pattern               | Example Input            | Redacted Output      |
| :-------------------- | :----------------------- | :------------------- |
| OpenAI-style API keys | `sk-abc123...`           | `sk-[REDACTED]`      |
| AWS access key IDs    | `AKIA1234567890ABCDEF`   | `AKIA[REDACTED]`     |
| Bearer tokens         | `Bearer eyJhbG...`       | `Bearer [REDACTED]`  |
| Secret assignments    | `api_key="super-secret"` | `api_key=[REDACTED]` |

Redaction is applied to `api_key`, `apikey`, `token`, `secret`, and `password` assignments with `=` or `:` separators.
