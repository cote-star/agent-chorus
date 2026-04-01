# CLI Reference

Use this page for full command syntax, examples, output contracts, and operational flags.

## Command Contract

```bash
chorus read --agent <codex|gemini|claude|cursor> [--id=<substring>] [--cwd=<path>] [--chats-dir=<path>] [--last=<N>] [--json] [--metadata-only] [--audit-redactions]
chorus compare --source <agent[:session-substring]>... [--cwd=<path>] [--last=<N>] [--json]
chorus report --handoff <handoff.json> [--cwd=<path>] [--json]
chorus list --agent <codex|gemini|claude|cursor> [--cwd=<path>] [--limit=<N>] [--json]
chorus search <query> --agent <codex|gemini|claude|cursor> [--cwd=<path>] [--limit=<N>] [--json]
chorus diff --agent <codex|gemini|claude|cursor> --from <id> --to <id> [--cwd=<path>] [--last=<N>] [--json]
chorus relevance --list | --test <path> | --suggest [--cwd=<path>] [--json]
chorus send --from <agent> --to <agent> --message <text> [--cwd=<path>]
chorus messages --agent <agent> [--cwd=<path>] [--clear] [--json]
chorus setup [--cwd=<path>] [--dry-run] [--force] [--agent-context] [--json]
chorus doctor [--cwd=<path>] [--json]
chorus agent-context <init|seal|build|sync-main|install-hooks|rollback|check-freshness|verify> [...]
chorus teardown [--cwd=<path>] [--dry-run] [--global] [--json]
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

Search results include a `match_snippet` field in JSON output showing ~120 characters of context around the first match. Only assistant/model messages are indexed.

## Comparing Agents

```bash
# Compare latest sessions across agents
chorus compare --source codex --source gemini --source claude --json

# Compare specific sessions
chorus compare --source codex:fix-bug --source claude:fix-bug --json

# Read last 3 messages from each source before comparing
chorus compare --source codex --source gemini --last 3 --json
```

The `--last N` flag controls how many recent assistant messages to read from each source (default 10). Comparison uses Jaccard topic similarity.

## Reporting

```bash
chorus report --handoff ./handoff_packet.json --json
```

## Context Pack

```bash
# Scaffold template files for agent-driven content
chorus agent-context init

# Validate and lock the pack after agents fill in content
chorus agent-context seal

# Verify manifest checksums against actual file content
chorus agent-context verify

# Build or refresh context pack files (backward-compatible wrapper)
chorus agent-context build

# Install pre-push hook to auto-sync context pack for main pushes
chorus agent-context install-hooks

# Restore latest local snapshot
chorus agent-context rollback

# Non-blocking warning check for stale pack updates
chorus agent-context check-freshness --base origin/main
```

You can also bootstrap agent-context from setup:

```bash
chorus setup --agent-context
```

## Common Recipes

```bash
# Handoff recovery: read latest work from another agent in this repo
chorus read --agent claude --cwd . --json

# Cross-agent verification: validate a claim with search + compare
chorus search "processPayment" --agent codex --cwd . --json
chorus compare --source codex --source claude --json

# Cold-start onboarding: build a compact index before deeper reads
chorus setup --agent-context
chorus agent-context build
chorus agent-context check-freshness --base origin/main

# Track session evolution: compare two sessions from the same agent
chorus diff --agent codex --from session-v1 --to session-v2 --json

# Security audit: check what secrets were redacted
chorus read --agent claude --audit-redactions --json

# Agent coordination: leave a message for another agent
chorus send --from claude --to codex --message "migration script is ready" --cwd .
chorus messages --agent codex --cwd . --json

# Context-pack filtering: check if a file is relevant
chorus relevance --test src/api/routes.ts --cwd .
```

## Session Diff

Compare two sessions from the same agent with line-level precision.

```bash
# Compare two Codex sessions
chorus diff --agent codex --from session-abc --to session-def --cwd . --json

# Compare with more message context
chorus diff --agent claude --from fix-auth --to fix-auth-v2 --last 5 --json
```

**JSON output:**

```json
{
  "agent": "codex",
  "session_a": "session-abc",
  "session_b": "session-def",
  "hunks": [
    { "tag": "removed", "lines": ["old line content"] },
    { "tag": "added", "lines": ["new line content"] },
    { "tag": "equal", "lines": ["unchanged content"] }
  ],
  "added_lines": 5,
  "removed_lines": 3,
  "equal_lines": 10,
  "summary": "+5 added, -3 removed, 10 unchanged"
}
```

## Relevance Introspection

Inspect and test the agent-context filtering patterns that control which files are considered relevant.

```bash
# Show current include/exclude patterns and their source
chorus relevance --list --cwd .

# Test if a specific file path matches the patterns
chorus relevance --test src/main.rs --cwd .

# Suggest patterns based on detected project conventions
chorus relevance --suggest --cwd .

# Machine-readable output
chorus relevance --list --cwd . --json
```

Patterns are configured in `.agent-context/relevance.json`. When no config exists, built-in defaults are used.

## Agent-to-Agent Messaging

A simple JSONL message queue for agents to leave messages for each other.

```bash
# Send a message from one agent to another
chorus send --from claude --to codex --message "auth module ready for review" --cwd .

# Read messages for an agent
chorus messages --agent codex --cwd . --json

# Read and clear messages after reading
chorus messages --agent codex --cwd . --clear
```

Messages are stored in `.agent-chorus/messages/<target-agent>.jsonl` and never leave your machine.

**JSON output for `chorus messages`:**

```json
[
  {
    "from": "claude",
    "to": "codex",
    "timestamp": "2026-03-11T10:30:00Z",
    "content": "auth module ready for review",
    "cwd": "/workspace/project"
  }
]
```

Message schema: `schemas/message.schema.json`.

## Setup

Wire Agent Chorus into a project. Creates provider scaffolding, injects managed blocks into agent instruction files, updates `.gitignore`, and auto-installs the Claude Code plugin if the `claude` CLI is present.

```bash
# Wire chorus into the current project
chorus setup

# Preview what would be created (no writes)
chorus setup --dry-run --json

# Replace existing managed blocks (idempotent refresh)
chorus setup --force

# Also initialize context pack and install pre-push hook
chorus setup --agent-context
```

Setup performs these operations:

| Operation | File / Target | Notes |
|---|---|---|
| `file` | `.agent-chorus/INTENTS.md` | Intent contract (skipped if exists unless --force) |
| `file` | `.agent-chorus/providers/{claude,codex,gemini}.md` | Per-agent trigger snippets |
| `integration` | `CLAUDE.md` / `AGENTS.md` / `GEMINI.md` | Managed blocks injected or created |
| `gitignore` | `.gitignore` | `.agent-chorus/` appended if not already present |
| `plugin` | `claude plugin` | Auto-installs Claude Code skill plugin if `claude` CLI is available |

**JSON output:**

```json
{
  "cwd": "/path/to/project",
  "dry_run": false,
  "force": false,
  "operations": [
    { "type": "file", "path": ".agent-chorus/INTENTS.md", "status": "created", "note": "Created intent contract" },
    { "type": "integration", "path": "CLAUDE.md", "status": "updated", "note": "Managed block written" },
    { "type": "gitignore", "path": ".gitignore", "status": "updated", "note": "Added .agent-chorus/ to .gitignore" },
    { "type": "plugin", "path": "claude plugin", "status": "created", "note": "Installed agent-chorus Claude Code plugin" }
  ],
  "warnings": [],
  "changed": 4
}
```

**Notes:**
- Safe to re-run — existing managed blocks and snippets are left unchanged unless `--force` is given
- The Claude Code plugin install is global (user scope). It is not reversed by `teardown`. To uninstall: `claude plugin uninstall agent-chorus`
- If `claude` CLI is not found, plugin installation is skipped with a `skipped` status and manual instructions

## Doctor

Check whether Agent Chorus is correctly wired for the current project.

```bash
chorus doctor
chorus doctor --cwd /path/to/project
chorus doctor --json
```

Doctor reports on: version, session directory availability, setup completeness (scaffolding + managed blocks), session discoverability for each agent, context pack state, Claude Code plugin installation, and update status.

**Example output:**

```
Agent Chorus doctor: PASS (/path/to/project)
- PASS version: agent-chorus v0.7.0
- PASS codex_sessions_dir: Found: ~/.codex/sessions
- PASS claude_projects_dir: Found: ~/.claude/projects
- PASS gemini_tmp_dir: Found: ~/.gemini/tmp
- PASS setup_intents: Found: .agent-chorus/INTENTS.md
- PASS snippet_claude: Found: .agent-chorus/providers/claude.md
- PASS integration_claude: Managed block present in CLAUDE.md
- PASS sessions_claude: At least one claude session discovered
- PASS context_pack_state: State: SEALED_VALID
- PASS update_status: Up to date (0.7.0)
- PASS claude_plugin: agent-chorus Claude Code plugin installed
```

**JSON output (`--json`):** array of `{ id, status, detail }` check objects, where `status` is `"pass"`, `"warn"`, or `"fail"`.

## Teardown

Remove Agent Chorus integration from a project.

```bash
# Preview what would be removed (recommended first step)
chorus teardown --cwd . --dry-run --json

# Actually remove integration
chorus teardown --cwd .

# Also remove global cache
chorus teardown --cwd . --global
```

Teardown performs these operations:
- Removes `<!-- agent-chorus:*:start/end -->` managed blocks from AGENTS.md, CLAUDE.md, GEMINI.md
- Deletes the `.agent-chorus/` scaffolding directory
- Removes `.agent-chorus/` from `.gitignore`
- Removes hook sentinels from pre-push hooks
- Preserves `.agent-context/` (contains project data; warns but does not delete)

**Note:** the Claude Code plugin is **not** removed by teardown — it is a global install. To uninstall: `claude plugin uninstall agent-chorus`

**JSON output:**

```json
{
  "cwd": "/path/to/project",
  "dry_run": false,
  "global": false,
  "operations": [
    { "type": "integration", "path": "CLAUDE.md", "status": "updated", "note": "Managed block removed" },
    { "type": "directory", "path": ".agent-chorus", "status": "deleted", "note": "Removed scaffolding directory" },
    { "type": "gitignore", "path": ".gitignore", "status": "updated", "note": "Removed .agent-chorus/ from .gitignore" },
    { "type": "hook", "path": ".git/hooks/pre-push", "status": "unchanged", "note": "No hook sentinel found" },
    { "type": "agent-context", "path": ".agent-context", "status": "preserved", "note": "Contains project data; not removed by teardown" }
  ],
  "warnings": [],
  "changed": 3
}
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

### Redaction Audit Trail

Use `--audit-redactions` with `chorus read` to see what was redacted and why:

```bash
chorus read --agent claude --audit-redactions --json
```

The JSON response includes a `redactions` array:

```json
{
  "agent": "claude",
  "content": "...",
  "redactions": [
    { "pattern": "openai_api_key", "count": 2 },
    { "pattern": "bearer_token", "count": 1 }
  ]
}
```

In text mode, a redaction summary is printed after the session content.
