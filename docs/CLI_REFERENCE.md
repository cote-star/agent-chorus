# CLI Reference

Use this page for full command syntax, examples, output contracts, and operational flags.

## Command Contract

```bash
chorus read --agent <codex|gemini|claude|cursor|hermes> [--id=<substring>] [--cwd=<path>] [--chats-dir=<path>] [--last=<N>] [--include-user] [--tool-calls] [--history=<on-demand|none|eager>] [--format=<json|markdown>] [--json] [--metadata-only] [--audit-redactions]
chorus summary --agent <codex|gemini|claude|cursor|hermes> [--cwd=<path>] [--format=<json|markdown>] [--json]
chorus timeline [--agent <agent>]... [--cwd=<path>] [--limit=<N>] [--format=<json|markdown>] [--json]
chorus compare --source <agent[:session-substring]>... [--cwd=<path>] [--last=<N>] [--json]
chorus report --handoff <handoff.json> [--cwd=<path>] [--json]
chorus list --agent <codex|gemini|claude|cursor|hermes> [--cwd=<path>] [--limit=<N>] [--json]
chorus search <query> --agent <codex|gemini|claude|cursor|hermes> [--cwd=<path>] [--limit=<N>] [--json]
chorus diff --agent <codex|gemini|claude|cursor|hermes> --from <id> --to <id> [--cwd=<path>] [--last=<N>] [--json]
chorus relevance --list | --test <path> | --suggest [--cwd=<path>] [--json]
chorus send --from <agent> --to <agent> --message <text> [--cwd=<path>]
chorus messages --agent <agent> [--cwd=<path>] [--clear] [--json]
chorus checkpoint --from <agent> [--cwd=<path>] [--message=<text>] [--json]
chorus setup [--cwd=<path>] [--dry-run] [--force] [--agent-context] [--json]
chorus doctor [--cwd=<path>] [--json]
chorus agent-context <init|seal|build|sync-main|install-hooks|rollback|check-freshness|verify|check-tool-integrity> [...]
chorus teardown [--cwd=<path>] [--dry-run] [--global] [--json]
```

## Reading a Session

```bash
# Read from Codex (defaults to latest session, last message)
chorus read --agent codex

# Read from Claude, scoped to current working directory
chorus read --agent claude --cwd /path/to/project

# Read live status with the latest user prompt included
chorus read --agent claude --cwd /path/to/project --include-user --json

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
When `--include-user` is present, Chorus includes the user prompt(s) that anchor the returned assistant message(s). This is intended for live status checks; assistant-only remains the default for narrower handoff reads.

**JSON output includes metadata:**

```json
{
  "agent": "codex",
  "source": "/path/to/session.jsonl",
  "content": "USER:\nInvestigate the auth regression.\n---\nASSISTANT:\nI am tracing the middleware and session issuance flow now...",
  "warnings": [],
  "session_id": "session-abc123",
  "cwd": "/workspace/project",
  "timestamp": "2026-01-15T10:30:00Z",
  "message_count": 12,
  "messages_returned": 2,
  "included_roles": ["user", "assistant"]
}
```

### Tool Calls

Use `--tool-calls` to include tool call content (Read, Edit, Bash, Write, etc.) that is normally stripped during extraction. When active, assistant messages include `[TOOL: <name>]...[/TOOL]` blocks alongside text content.

```bash
# See which files an agent read and edited
chorus read --agent codex --tool-calls --json

# Combine with --include-user for full forensics
chorus read --agent claude --tool-calls --include-user --json
```

The JSON response includes `"included_tool_calls": true` in metadata when active. Without the flag, behavior is unchanged.

**Behaviour note — Gemini and Hermes (uniform NOT_AVAILABLE warning, v0.16.0):**
The Gemini JSONL transcript format and the (provisional) Hermes session
format do not carry tool-call structure that the adapters can surface.
When `--tool-calls` is passed for these agents, the command runs without
error, `included_tool_calls: true` is still emitted (the flag was
honored), and a uniform warning is pushed into `result.warnings`:

```
--tool-calls has no effect for <agent> sessions: this agent's transcript format does not carry tool calls.
```

The exact phrasing is byte-identical between Node and Rust dispatch, so
consumers can match on it deterministically. This warning is what
distinguishes "agent format genuinely has no tool calls" from the prior
silent no-op (which looked indistinguishable from "the session had no
tool calls"). Mirrors `AGENTS_WITHOUT_TOOL_CALLS` in
`scripts/read_session.cjs` and `agent_has_no_tool_calls` in
`cli/src/main.rs`.

Cursor (both CLI JSONL and IDE `store.db` surfaces) runs `--tool-calls`
without error but does not currently emit `[TOOL: ...]` blocks; this
behavior is tracked for a follow-up rather than escalated to the uniform
warning because the cursor surfaces *do* carry tool-call data — the
adapters just don't surface it yet.

### Read Flag Reference

| Flag | Description | Default |
|---|---|---|
| `--agent` | Agent to read from (`codex`, `gemini`, `claude`, `cursor`) | required |
| `--id` | Session-id substring match; omit to pick the latest session | latest |
| `--cwd` | Working directory to scope sessions | current directory |
| `--chats-dir` | Override session-discovery root (Gemini `.pb` fallback, etc.) | agent default |
| `--last` | Number of trailing assistant messages to include | 1 |
| `--include-user` | Include the paired user prompt(s) with each assistant message | off |
| `--tool-calls` | Surface `[TOOL: <name>]...[/TOOL]` blocks in `content` | off |
| `--history` | History scope: `on-demand` (default, latest session only), `none` (metadata only), `eager` (reserved — emits warning) | `on-demand` |
| `--format` | Output format (`json`, `md` / `markdown`) | text unless `--json` |
| `--json` | Machine-readable JSON output | off |
| `--metadata-only` | Return metadata without `content` | off |
| `--audit-redactions` | Include a `redactions` summary in output | off |

**`--format` vs `--json`:** Rust treats `--format json` as an alias for `--json`. **Node has a bug here** — `--format json` falls through to plain-text output instead of JSON (see `scripts/read_session.cjs:1759`). The bug is documented and left in place because fixing it is a user-visible output-contract change; use `--json` for JSON output on both runtimes.

### History Contract (`--history`, v0.16.0)

`chorus read` is single-session by design. The `--history` flag makes that
contract explicit; the default (`on-demand`) is what consumers should
nearly always use.

| Value | Semantics |
|---|---|
| `on-demand` (default) | Return ONLY the latest session for the cwd. Chorus does NOT auto-pull prior sessions into the returned content. When historical context is needed, the consumer calls `chorus list`, `chorus timeline`, or `chorus search` EXPLICITLY. This is the "on-demand recall" pattern — field measurements found a 2.5x token inflation when agents eagerly read multiple prior sessions, so the default is deliberately narrow. |
| `none` | Equivalent to `--metadata-only`. The JSON `content` field is `null`; text output omits the content block. Useful for cheap session-existence probes and routing decisions. |
| `eager` | RESERVED for a future multi-session merge. Today it behaves identically to `on-demand` AND pushes a warning into `result.warnings` so consumers cannot silently come to depend on it: `--history=eager is reserved for a future multi-session merge and currently behaves identically to --history=on-demand. Use \`chorus list / timeline / search\` to pull additional sessions explicitly.` |

Invalid values are rejected at parse time on both runtimes (e.g. `--history=full` exits non-zero with `Invalid --history value: full. Allowed: on-demand | none | eager.`).

```bash
# Default — single latest session for the cwd
chorus read --agent claude --cwd . --json

# Metadata-only probe ("does claude have any session for this cwd?")
chorus read --agent claude --cwd . --history none --json

# Reserved value — works, but pushes a warning into the JSON
chorus read --agent claude --cwd . --history eager --json
```

The history contract is also written into provider snippets and the
`CLAUDE.md` / `AGENTS.md` / `GEMINI.md` managed blocks by `chorus setup`
(v0.16.0+), so consuming agents are reminded of the on-demand rule
inside their own instruction files. See "Setup" below and the
stale-snippet checks under "Doctor".

### `cwd_mismatch` (v0.16.0)

When `--cwd <PATH>` is passed but no session matches and the adapter
falls back to the latest session anyway (the long-standing Codex /
Claude / Cursor behavior — see Rule 4 in `PROTOCOL.md`), the JSON output
now carries an explicit boolean:

```json
{
  "agent": "codex",
  "cwd": "/workspace/missing-project",
  "warnings": [
    "Warning: no Codex session matched cwd /workspace/missing-project; falling back to latest session."
  ],
  "cwd_mismatch": true
}
```

The field is **only emitted when the fallback fires**. When `--cwd`
resolves cleanly, `cwd_mismatch` is absent from the output (it is NOT
emitted as `false`). This keeps JSON consumers honest: any code that
checks `result.cwd_mismatch === true` will detect the silent-fallback
case without scanning the warnings array.

In addition, the same warning string is mirrored to **stderr** prefixed
with `chorus:`:

```
chorus: Warning: no Codex session matched cwd /workspace/missing-project; falling back to latest session.
```

Stderr-watching humans see it immediately even when stdout is
JSON-piped. Schema: see `cwd_mismatch` in
[`schemas/read-output.schema.json`](../schemas/read-output.schema.json).

## Session Summary

Structured session digest without reading full content. Extracts metadata locally — no LLM calls. Node and Rust emit byte-identical JSON for the same inputs (Rust parity landed in v0.13.0).

**Synopsis**

```
chorus summary --agent <codex|gemini|claude|cursor> [--id <substring>] [--cwd PATH] [--format {json|md|markdown}] [--json]
```

**Examples**

```bash
# Quick status check
chorus summary --agent claude --cwd . --json

# Human-readable markdown output
chorus summary --agent claude --format markdown
```

**Flags**

| Flag | Description | Default |
|---|---|---|
| `--agent` | Agent to summarize (`claude`, `codex`, `gemini`, `cursor`) | required |
| `--id` | Session-id substring match; omit to pick the latest session | latest |
| `--cwd` | Working directory to scope sessions | current directory |
| `--format` | Output format (`json`, `md` / `markdown`) | text unless `--json` |
| `--json` | Machine-readable JSON output | off |

**JSON output:**

```json
{
  "agent": "claude",
  "session_id": "...",
  "message_count": 47,
  "duration_estimate": "~25 min",
  "user_requests": ["Fix the auth bug"],
  "files_referenced": ["src/auth.ts"],
  "tool_calls_by_type": {"Read": 12, "Edit": 8, "Bash": 5},
  "last_response_snippet": "Auth bug was in token refresh logic..."
}
```

**Fields:**

| Field | Type | Description |
|---|---|---|
| `agent` | `string` | Agent name |
| `session_id` | `string` | Session identifier |
| `message_count` | `number` | Total messages in session |
| `duration_estimate` | `string` | First-to-last message timestamp delta |
| `user_requests` | `string[]` | First 5 user messages, truncated to 150 chars each |
| `files_referenced` | `string[]` | Extracted from `tool_use` inputs (`file_path`, `path` fields) |
| `tool_calls_by_type` | `object` | Count of tool calls by tool name |
| `last_response_snippet` | `string` | Last assistant message excerpt (300 chars, not an LLM summary) |

**Exit codes**

| Code | Condition |
|---|---|
| `0` | Success |
| non-zero | `NOT_FOUND` (no matching session), `PARSE_FAILED`, `EMPTY_SESSION`, `UNSUPPORTED_AGENT`, `IO_ERROR` — see [Error Codes](#error-codes) |

## Timeline

Cross-agent chronological view interleaving sessions from multiple agents for a given working directory. Node and Rust emit byte-identical JSON for the same inputs (Rust parity landed in v0.13.0).

**Synopsis**

```
chorus timeline [--agent <agent>]... [--cwd PATH] [--limit N] [--format {json|md|markdown}] [--json]
```

**Examples**

```bash
# All agents, default limit
chorus timeline --cwd . --json

# Specific agents with custom limit
chorus timeline --cwd ~/project --agent claude --agent codex --limit 5 --json

# Human-readable markdown
chorus timeline --cwd . --format markdown
```

**Flags:**

| Flag | Description | Default |
|---|---|---|
| `--agent` | Agent to include (repeatable) | All four agents |
| `--cwd` | Working directory to scope sessions | Current directory |
| `--limit` | Maximum sessions per agent | `5` |
| `--format` | Output format: `json`, `markdown` / `md` | `json` (with `--json`) |

**JSON output:**

```json
[
  {
    "agent": "claude",
    "session_id": "session-abc",
    "timestamp": "2026-04-13T14:30:00Z",
    "snippet": "Last assistant message excerpt (200 chars)..."
  },
  {
    "agent": "codex",
    "session_id": "session-def",
    "timestamp": "2026-04-13T14:15:00Z",
    "snippet": "Another session excerpt..."
  }
]
```

Sessions are sorted by timestamp descending. Each entry includes a snippet from the last assistant message (200 chars).

**Exit codes**

| Code | Condition |
|---|---|
| `0` | Success (including empty result when no sessions are discovered) |
| non-zero | `IO_ERROR`, `UNSUPPORTED_AGENT` — see [Error Codes](#error-codes) |

## Output Formats

The `--format` flag controls output rendering. Supported on `chorus read`, `chorus summary`, and `chorus timeline`.

| Format | Flag | Description |
|---|---|---|
| JSON | `--json` or `--format json` | Machine-readable structured output (default with `--json`) |
| Markdown | `--format markdown` or `--format md` | Formatted markdown for human review, demos, and documentation |
| Text | *(default)* | Plain text output |

```bash
# JSON (machine-readable)
chorus summary --agent claude --json

# Markdown (human-friendly)
chorus summary --agent claude --format markdown

# Works on read and timeline too
chorus read --agent codex --format md
chorus timeline --cwd . --format markdown
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

### Cursor-only `source` field (v0.16.0)

`chorus list --agent cursor` and `chorus search --agent cursor` entries
carry an extra string field — `"source": "cli" | "app"` — distinguishing
the two on-disk Cursor surfaces:

| Value | Surface | Backing store |
|---|---|---|
| `"cli"` | cursor-agent CLI transcripts | `~/.cursor/projects/<project>/agent-transcripts/<session>/*.jsonl` |
| `"app"` | Cursor IDE workspace chats | `~/.cursor/chats/<hash>/<uuid>/store.db` (SQLite) |

Example:

```json
[
  {
    "session_id": "store",
    "agent": "cursor",
    "source": "app",
    "cwd": "/Users/me/code/app",
    "modified_at": "2026-05-22T18:11:00Z",
    "file_path": "/Users/me/.cursor/chats/abc.../uuid.../store.db"
  },
  {
    "session_id": "abcd1234-...",
    "agent": "cursor",
    "source": "cli",
    "cwd": "/Users/me/code/app",
    "modified_at": "2026-05-22T17:42:00Z",
    "file_path": "/Users/me/.cursor/projects/-Users-me-code-app/agent-transcripts/abcd.../abcd....jsonl"
  }
]
```

The `source` field is **cursor-only** — it is not emitted for codex,
claude, gemini, or hermes. List/search results from those agents
retain the existing schema unchanged.

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

Build a structured cross-agent report from a handoff packet (a JSON file
that names the task, success criteria, and source sessions to compare).

```bash
chorus report --handoff ./handoff_packet.json --json
```

### Handoff Schema (v0.16.0 — surfaced in `--help`)

The full schema is now embedded in `chorus report --help` (Rust CLI) so
operators don't need to leave the terminal to see it. Reproduced here
for searchability; the canonical source is
[`schemas/handoff.schema.json`](../schemas/handoff.schema.json).

```json
{
  "mode": "analyze",
  "task": "<short description>",
  "success_criteria": ["<criterion>", ...],
  "sources": [
    {
      "agent": "claude",
      "session_id": "<id>",
      "current_session": true,
      "cwd": "<path>",
      "last_n": 10
    }
  ],
  "constraints": ["<constraint>", ...]
}
```

**Required fields:** `mode`, `task`, `success_criteria` (non-empty),
`sources` (each entry requires `agent`, plus either `session_id` OR
`current_session: true`).

**Optional fields:** `cwd` and `last_n` per-source, top-level
`constraints`.

**Strictness:** unknown fields produce `INVALID_HANDOFF`. `mode` must be
one of the canonical modes (`verify`, `steer`, `analyze`, `feedback`).

Minimal copy-pasteable example (write to `handoff.json`):

```json
{
  "mode": "analyze",
  "task": "Compare claude and codex outputs",
  "success_criteria": ["Identify agreements and contradictions"],
  "sources": [
    {"agent": "claude", "current_session": true},
    {"agent": "codex",  "current_session": true}
  ]
}
```

```bash
chorus report --handoff handoff.json --json
```

## Context Pack

```bash
# Scaffold template files for agent-driven content
chorus agent-context init

# Validate and lock the pack after agents fill in content
chorus agent-context seal

# Verify manifest checksums against actual file content
chorus agent-context verify

# CI mode: combined integrity + freshness check (for PR gates)
chorus agent-context verify --ci

# CI mode with a custom diff base
chorus agent-context verify --ci --base origin/develop

# Build or refresh context pack files (backward-compatible wrapper)
chorus agent-context build

# Install pre-push hook to auto-sync context pack for main pushes
chorus agent-context install-hooks

# Restore latest local snapshot
chorus agent-context rollback

# P13/F58: restore the snapshot the manifest's last_known_good_sha points to
chorus agent-context rollback --latest-good

# Non-blocking warning check for stale pack updates
chorus agent-context check-freshness --base origin/main

# P7: zone-grouped diff from seal-time baseline → current HEAD
#     (subagent reconciliation protocol)
chorus agent-context diff --since-seal
chorus agent-context diff --since-seal --format text

# P13/F46: tiered adoption — scaffold a narrower starting pack
chorus agent-context init --tier 1   # CODE_MAP + routes.json only
chorus agent-context init --tier 2   # + BEHAVIORAL_INVARIANTS + completeness_contract
chorus agent-context init --tier 3   # full pack (default; existing behavior)
```

### P13 — Authoring ergonomics (tiers, aliases, last-known-good)

The init flow supports three adoption tiers. Tier 3 preserves legacy
behavior; tiers 1 and 2 scaffold a narrower core so teams can adopt the
skill without committing to the full pack upfront.

| Tier | Files scaffolded |
|---|---|
| 1 | `20_CODE_MAP.md`, `routes.json` |
| 2 | Tier 1 + `30_BEHAVIORAL_INVARIANTS.md`, `completeness_contract.json` |
| 3 | Full pack (default — all nine files) |

`manifest.json` gains two P13 fields that `seal` and `verify` carry
forward between runs:

- `aliases` — object mapping canonical filenames to the on-disk names the
  team prefers. Example: `{"20_CODE_MAP.md": "20_architecture.md"}`.
  `verify` accepts the aliased filename when the canonical one is missing.
- `last_known_good_sha` — SHA promoted by `verify --ci` on a green run.
  `rollback --latest-good` resolves this pointer to the matching snapshot
  in `history.jsonl` and restores it, giving teams a one-command
  "undo to last green".

The routing blocks written to `CLAUDE.md` / `AGENTS.md` / `GEMINI.md`
at `init` time now start with a mandatory session-start freshness gate
(F47): "Before any reasoning, check
`.agent-context/current/manifest.json`'s `head_sha_at_seal` vs
`git rev-parse HEAD`. If they diverge, warn the user."

You can also bootstrap agent-context from setup:

```bash
chorus setup --agent-context
```

## Context Pack Verification

Verify the integrity and freshness of a context pack.

```bash
# Human-readable integrity check
chorus agent-context verify

# CI mode: combined integrity + freshness (exits non-zero if stale or corrupt)
chorus agent-context verify --ci

# Specify a custom diff base (default: origin/main)
chorus agent-context verify --ci --base origin/develop
```

**Flags:**

| Flag | Description | Default |
|---|---|---|
| `--ci` | Combined integrity + freshness check with JSON output | off |
| `--base` | Git ref to diff against for freshness detection | `origin/main` |
| `--json` | Force JSON output (implied by `--ci`) | off |
| `--enforce-separate-commits` | (P6) Under `--ci`, fail when any commit in `base..HEAD` touches both `.agent-context/**` and non-pack paths | off |

**JSON output (`--ci`):**

```json
{
  "integrity": "pass",
  "freshness": "stale",
  "changed_files": ["src/main.rs", "PROTOCOL.md"],
  "pack_updated": false,
  "exit_code": 1
}
```

| Field | Type | Meaning |
|---|---|---|
| `integrity` | `"pass"` / `"fail"` | Whether manifest checksums match file content |
| `freshness` | `"fresh"` / `"stale"` | Whether context-relevant files changed since last seal |
| `changed_files` | `string[]` | Context-relevant files modified relative to `--base` |
| `pack_updated` | `boolean` | Whether `.agent-context/current/` was also modified |
| `exit_code` | `number` | `0` if both checks pass, non-zero otherwise |

When `--enforce-separate-commits` is set, the JSON output adds:

| Field | Type | Meaning |
|---|---|---|
| `separate_commits` | `"pass"` / `"fail"` | Whether every commit keeps pack and non-pack changes separate |
| `mixed_commits` | `string[]` | Human-readable lines (`commit <sha> mixes pack + non-pack changes`) for each offender |

A mixed commit causes `exit_code: 1` even when integrity and freshness pass.
The gate is off by default because many teams land pack updates in the same
commit as the code change that motivated them; only enable it when your team
has agreed on the "pack edits land as their own commit" convention (see the
"Known limitations" section in `RELEASE_NOTES.md`). The Node entrypoint
(`scripts/agent_context/verify.cjs`) accepts the flag for parity but exits 1
with a message pointing to the Rust binary until the Node port lands.

A CI workflow template is available at `templates/ci-agent-context.yml`.

The `--ci` JSON payload also carries the P7 subagent-reconciliation shape under
`diff_since_seal` and the flat `acceptance_tests_invalidated` list. CI fails
(`exit_code = 1`) when `acceptance_tests_invalidated` is non-empty AND the pack
wasn't updated, so stale ground truth cannot pass the gate.

### Trust boundary & pack integrity (P12)

**Semantic `look_for` (F40):** `search_scope.json` verification_shortcuts now
strip comments from the referenced file before matching the `look_for`
substring. Supported extensions: `.py` (line `#` + `"""..."""` docstrings),
`.rs` / `.ts` / `.tsx` / `.js` / `.jsx` / `.cjs` / `.mjs`
(`//` line + `/* */` block). Other extensions fall back to the existing raw
substring contract. A match that only appears inside comments surfaces as:

```
LOOK_FOR_MISSING: search_scope lookup: look_for matches only comments in calc.py: MIN_CELL_SIZE = 30
```

When authors want regex semantics, add `look_for_regex` alongside `look_for`:

```json
{
  "file": "calc.py",
  "look_for": "MIN_CELL_SIZE",
  "look_for_regex": "MIN_CELL_SIZE\\s*=\\s*\\d+"
}
```

`look_for_regex` takes precedence over `look_for` when both are present.

**Verified acceptance tests (F41):** `acceptance_tests.md` tests may declare
`verified: true` with a list of `anchors` pinning `{file, line, line_contains}`
pointers into real code. On verify, each anchor's `line_contains` must appear
at the named line (±3 lines tolerance); a miss emits
`VERIFIED_ANCHOR_MISS` in `structural_warnings[]`. The pack is considered
"ship-quality" when at least 2 of N tests are verified; fewer emits the
non-fatal `VERIFIED_COUNT_LOW`.

**Audit trail (F42):** `history.jsonl` entries now carry:

| Field | Meaning |
|---|---|
| `sealed_by` | `"name <email>"` from `git config user.{name,email}`. |
| `prose_diff_sections` | H2 sections whose body changed vs the previous snapshot, keyed `<file>#<heading>` (e.g. `20_CODE_MAP.md#Contexts`). Empty on first seal. |
| `seal_reason` | Mirror of `reason` for explicit audit reads. |

**HIGH_TRUST_DIFF labeling (F39):** the shipped CI workflow
(`templates/ci-agent-context.yml`) applies label `HIGH_TRUST_DIFF` when a PR
diff touches prose in `.agent-context/current/30_BEHAVIORAL_INVARIANTS.md`,
`.agent-context/current/00_START_HERE.md`, or any of `CLAUDE.md`/`AGENTS.md`/
`GEMINI.md`. Branch protection should require CODEOWNERS approval on the label.

**Known limitation — `[skip ci]` bypass (F43):** the PR gate runs on
`pull_request` events, so a merge with `[skip ci]` will skip the PR check
entirely. Solutions in order of strength:

1. Configure branch-protection rules to disallow `[skip ci]` on protected
   branches (the primary defense).
2. The shipped CI template also runs `chorus agent-context verify --ci` on
   push to `main`, so drift lands as a red check on the merged commit even
   when the PR gate was skipped.
3. Teams that want a stricter gate can require the post-merge verify
   workflow to pass via branch protection.

## Context Pack Diff (Subagent Reconciliation)

P7. Zone-grouped diff from the seal-time baseline to current HEAD. Intended for
the orchestrator of a parallel-subagent session: after subagents modify code,
run this to learn which pack sections are impacted, then dispatch a single
reconciler subagent to patch and re-seal.

```bash
# Machine-readable JSON (default)
chorus agent-context diff --since-seal

# Human-readable summary of zones + actions
chorus agent-context diff --since-seal --format text

# Explicit pack dir / cwd
chorus agent-context diff --since-seal --pack-dir .agent-context --cwd /repo
```

**Flags:**

| Flag | Description | Default |
|---|---|---|
| `--since-seal` | Required. Diff from `manifest.post_commit_sha` (preferred) or `head_sha_at_seal`. | — |
| `--format` | `json` (default) or `text`. | `json` |
| `--pack-dir` | Override pack directory. | `.agent-context` |
| `--cwd` | Working directory. | current directory |

**JSON output:**

```json
{
  "baseline_sha": "abc1234…",
  "pack_updated": false,
  "zones": [
    {
      "paths": ["src/**"],
      "affects": ["20_CODE_MAP.md"],
      "changed_files": ["src/lib.rs", "src/new_module.rs"],
      "signature_drifts": [],
      "count_deltas": [],
      "deleted_files": []
    }
  ],
  "acceptance_tests_invalidated": [],
  "recommended_reconciliation_actions": [
    "Review 20_CODE_MAP.md: 2 file(s) changed in zone",
    "Re-seal the pack (`chorus agent-context seal --force`) after patching sections"
  ]
}
```

| Field | Type | Meaning |
|---|---|---|
| `baseline_sha` | `string` or `null` | The seal-time commit (`post_commit_sha` if present, else `head_sha_at_seal`). |
| `pack_updated` | `boolean` | Whether `.agent-context/current/` was touched since the baseline. |
| `zones[]` | array | One entry per authored zone in `relevance.json` that had a matching changed file. |
| `zones[].signature_drifts` | array | Reserved for P2 baseline-drift integration — empty today. |
| `zones[].count_deltas` | array | Reserved for P2 — empty today. |
| `zones[].deleted_files` | array | Reserved for P2 — empty today. |
| `acceptance_tests_invalidated[]` | array | Acceptance tests whose `invalidated_by` functions drifted (requires P4 schema in `acceptance_tests.md`). |
| `recommended_reconciliation_actions[]` | `string[]` | Natural-language bullets the reconciler subagent can follow. |

See the "Parallel subagent pattern" section in `skills/agent-context/SKILL.md`
for the orchestrator workflow.

## Common Recipes

```bash
# Quick status check: structured digest without full read
chorus summary --agent claude --cwd . --json

# Cross-agent timeline: what happened across all agents
chorus timeline --cwd . --format markdown

# Tool call forensics: what files did the agent actually touch?
chorus read --agent codex --tool-calls --json

# Handoff recovery: read latest work from another agent in this repo
chorus read --agent claude --cwd . --include-user --json

# Live status check: include the current prompt that defines the work
chorus read --agent claude --cwd . --include-user --json

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

# Human-friendly output for any command
chorus summary --agent claude --format markdown
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

## Session Checkpoint

Emit a lightweight state-broadcast message to every other agent's inbox.
Safe to call unconditionally — no-ops silently when `.agent-chorus/` is
not present in the working directory. Designed for Claude Code's
`SessionEnd` hook; works equally well for any agent as a manual
break-point checkpoint.

**Synopsis**

```
chorus checkpoint --from <agent> [--cwd PATH] [--message TEXT] [--json]
```

**Examples**

```bash
# Auto-composed message — captures branch, uncommitted-file count, last commit
chorus checkpoint --from claude --cwd .

# Custom message overrides the auto-compose
chorus checkpoint --from codex --message "auth refactor half-done; types still broken" --cwd .

# JSON output for scripting
chorus checkpoint --from gemini --cwd . --json
```

**Flags**

| Flag | Description | Default |
|---|---|---|
| `--from` | Agent issuing the checkpoint (`claude`, `codex`, `gemini`, `cursor`) | required |
| `--cwd` | Working directory containing `.agent-chorus/` | current directory |
| `--message` | Override text; skips git-state auto-compose | auto-composed |
| `--json` | Machine-readable output | off |

**JSON output:**

```json
{
  "ok": true,
  "from": "claude",
  "recipients": ["codex", "gemini", "cursor"],
  "message": "claude session ended. Branch: main | Uncommitted: 3 | Last commit: abc123 fix auth bug"
}
```

Each recipient receives an identical JSONL line in
`.agent-chorus/messages/<recipient>.jsonl`. The message conforms to the
same schema as `chorus send`.

**Behaviour notes**

- **Guard**: absence of `.agent-chorus/` in the resolved cwd is not an
  error. The command exits 0 silently so it is safe to install as a
  global hook.
- **Git soft-failures**: missing git, no commits, detached HEAD — all
  degrade to the string `unknown` in the composed message rather than
  raising.
- **Idempotency**: re-running the same checkpoint appends another JSONL
  line. Recipients decide how to deduplicate.

**Exit codes**

| Code | Condition |
|---|---|
| `0` | Success, or silent no-op because `.agent-chorus/` is absent |
| non-zero | `--from` missing or invalid agent name; `.agent-chorus/messages/` exists but is not writable |

See also: `docs/session-handoff-guide.md` for end-to-end scenarios and
the Claude Code `SessionEnd` hook wiring.

## Setup

Wire Agent Chorus into a project. Creates provider scaffolding, injects managed blocks into agent instruction files, updates `.gitignore`, and auto-installs the Claude Code plugin if the `claude` CLI is present. Node and Rust emit byte-identical JSON for the same inputs (Rust parity landed in v0.13.0).

**Synopsis**

```
chorus setup [--cwd PATH] [--dry-run] [--force] [--agent-context] [--json]
```

**Examples**

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

**Flags**

| Flag | Description | Default |
|---|---|---|
| `--cwd` | Target project directory | current directory |
| `--dry-run` | Preview operations without writing | off |
| `--force` | Replace existing managed blocks / scaffolding | off |
| `--agent-context` | Also run `chorus agent-context init` and install pre-push hook | off |
| `--json` | Machine-readable JSON output | off |

Setup performs these operations:

| Operation | File / Target | Notes |
|---|---|---|
| `file` | `.agent-chorus/INTENTS.md` | Intent contract (skipped if exists unless --force) |
| `file` | `.agent-chorus/providers/{claude,codex,gemini}.md` | Per-agent trigger snippets. v0.16.0+ snippets carry a top-of-block "History contract" section that documents the on-demand history rule. |
| `integration` | `CLAUDE.md` / `AGENTS.md` / `GEMINI.md` | Managed blocks injected or created. v0.16.0+ blocks open with **History contract (READ FIRST — violating this costs 2.5x tokens)** and list `chorus list / timeline / search` as the on-demand recall path. The block's support-commands list also enumerates `diff`, `audit-redactions`, `relevance`, `send`, and `messages`. |
| `gitignore` | `.gitignore` | `.agent-chorus/` appended if not already present |
| `plugin` | `claude plugin` | Auto-installs Claude Code skill plugin if `claude` CLI is available |

**Stale-snippet detection (v0.16.0):** `chorus doctor` emits
`snippet_<agent>_stale: warn` and `integration_<agent>_stale: warn`
when these files exist but were generated before the v0.16.0 history
contract was added. The remediation is `chorus setup --force`, which
refreshes the snippet and managed block in place. See "Doctor — Check
Catalogue" above.

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

**Exit codes**

| Code | Condition |
|---|---|
| `0` | Success (including `--dry-run` previews and idempotent re-runs) |
| non-zero | `IO_ERROR` (unwritable target), invalid flag combination |

## Doctor

Check whether Agent Chorus is correctly wired for the current project. Node and Rust emit byte-identical JSON for the same inputs (Rust parity landed in v0.13.0).

**Synopsis**

```
chorus doctor [--cwd PATH] [--json]
```

**Examples**

```bash
chorus doctor
chorus doctor --cwd /path/to/project
chorus doctor --json
```

Doctor reports on: version, session directory availability, setup completeness (scaffolding + managed blocks), session discoverability for each agent, context pack state, Claude Code plugin installation, and update status.

**Flags**

| Flag | Description | Default |
|---|---|---|
| `--cwd` | Target project directory | current directory |
| `--json` | Machine-readable JSON output | off |

**Example output:**

```
Agent Chorus doctor: PASS (/path/to/project)
- PASS version: agent-chorus v0.16.0
- PASS codex_sessions_dir: Found: ~/.codex/sessions
- PASS claude_projects_dir: Found: ~/.claude/projects
- PASS gemini_tmp_dir: Found: ~/.gemini/tmp
- PASS setup_intents: Found: .agent-chorus/INTENTS.md
- PASS snippet_claude: Found: .agent-chorus/providers/claude.md
- PASS integration_claude: Managed block present in CLAUDE.md
- PASS sessions_claude: At least one claude session discovered
- PASS sessions_cursor_cli: At least one cursor-agent CLI transcript discovered
- INFO sessions_cursor_app: Cursor IDE not configured (data directory absent: ~/.cursor/chats)
- INFO sessions_hermes: Hermes not configured (data directory absent: ~/.hermes/sessions)
- PASS context_pack_state: State: SEALED_VALID
- INFO context_pack_hooks_path: Effective git hooks path: .git/hooks (default)
- PASS context_pack_pre_push: Found: .git/hooks/pre-push
- PASS update_status: Up to date (0.16.0)
- PASS claude_plugin: agent-chorus Claude Code plugin installed
```

**JSON output (`--json`):** object `{ cwd, overall, checks: [...] }`
where each check is `{ id, status, detail }`. `status` is one of
`"pass"`, `"info"`, `"warn"`, or `"fail"`. The top-level `overall`
collapses the checks (see severity model below).

### Doctor — Severity Model (v0.16.0)

Doctor returns four severity levels per check:

| Severity | Meaning | Elevates `overall`? |
|---|---|---|
| `pass` | Check passed. | no |
| `info` | Informational state — typically "this feature is intentionally not configured" (e.g. Hermes not installed, cwd not a git repo). Distinguishable from `pass` for tooling that wants to surface configuration absence, but is NOT a problem. | **no** |
| `warn` | Soft failure — something is misconfigured but the install still works. Includes stale snippets, dangling env overrides, missing managed blocks on an initialized install. | yes (sets `overall: warn`) |
| `fail` | Hard failure — the install is broken or an adapter errored. | yes (sets `overall: fail`) |

`overall` is computed as:
1. `fail` if any check is `fail`.
2. else `warn` if any check is `warn`.
3. else `pass`. (`info` never elevates `overall`.)

This matters for CI: `chorus doctor --json | jq -e '.overall == "pass"'`
will succeed on an install that has `info`-tagged checks (e.g. "Hermes
not installed") and fail on `warn` or `fail`.

### Doctor — Check Catalogue (v0.16.0)

The set of check IDs and their possible severities. New or changed
entries in v0.16.0 are marked `[v0.16.0]`.

| Check ID | Possible severities | What it reports |
|---|---|---|
| `version` | `pass` | The running `chorus` version. |
| `codex_sessions_dir` / `claude_projects_dir` / `gemini_tmp_dir` | `pass` / `warn` | Whether the agent's base directory exists. |
| `setup_intents` | `pass` / `warn` / `info` | Whether `.agent-chorus/INTENTS.md` exists. `info` when the repo is uninitialized; `warn` when the repo has been initialized but the intents file is missing. |
| `snippet_<agent>` | `pass` / `warn` / `info` | Whether the per-agent provider snippet (`.agent-chorus/providers/<agent>.md`) exists. Severity follows the same `info`-vs-`warn` rule as `setup_intents`. |
| `integration_<agent>` | `pass` / `warn` / `info` | Whether the managed block is injected in `AGENTS.md` / `CLAUDE.md` / `GEMINI.md`. |
| `sessions_codex` / `sessions_claude` / `sessions_gemini` | `pass` / `warn` / `fail` | Whether at least one session for the cwd was discovered. `fail` if the adapter errored. |
| `sessions_cursor_cli` `[v0.16.0]` | `pass` / `info` / `warn` | Cursor CLI (cursor-agent) transcript surface. `info` when the data directory is absent (tool not installed — intentional). `warn` when the directory exists but has zero sessions. Replaces the previous single `sessions_cursor` check. |
| `sessions_cursor_app` `[v0.16.0]` | `pass` / `info` / `warn` | Cursor IDE (desktop app) `store.db` surface. Same `info`-vs-`warn` rule. Replaces the previous single `sessions_cursor` check. |
| `sessions_hermes` `[v0.16.0]` | `pass` / `info` / `warn` | Hermes (provisional) sessions. Now downgrades to `info` when the data directory is absent (was `warn` in v0.15.0 — a noisy false positive for anyone who doesn't run Hermes). |
| `env_override_dangling` `[v0.16.0]` | `warn` | Emitted (potentially multiple times) for each `CHORUS_*` / `BRIDGE_*` env var that points at a non-existent directory. Sessions from that adapter would be invisible until the var is cleared or the directory exists; doctor surfaces it instead of silently hiding adapter output. |
| `snippet_<agent>_stale` `[v0.16.0]` | `warn` | Emitted when `.agent-chorus/providers/<agent>.md` exists but lacks the load-bearing "History contract" section introduced in v0.16.0. Remediation: `chorus setup --force`. |
| `integration_<agent>_stale` `[v0.16.0]` | `warn` | Emitted when the managed block inside `AGENTS.md` / `CLAUDE.md` / `GEMINI.md` exists but predates the v0.16.0 history contract. Remediation: `chorus setup --force`. |
| `context_pack_state` | `pass` / `warn` | `SEALED_VALID` / `TEMPLATE` / `UNINITIALIZED`. |
| `context_pack_guidance` | `warn` | Present only when the pack state is `UNINITIALIZED` or `TEMPLATE`. |
| `context_pack_hooks_path` `[v0.16.0]` | `info` | Reports the effective git hooks path (`configured` via `git config core.hooksPath`, else `default` = `.git/hooks`). When the cwd is **not a git repo**, this check reports `info` ("cwd is not a git repository; git hooks checks skipped") rather than falsely reporting a global hooks path as if it applied to this cwd. |
| `context_pack_pre_push` `[v0.16.0]` | `pass` / `warn` / `info` | Whether a pre-push hook exists at the effective hooks path. `info` when the cwd is not a git repo. |
| `update_status` | `pass` / `warn` | Update check result. `warn` if the update check itself errored. |
| `claude_plugin` | `pass` / `warn` | Claude Code plugin install state. `warn` if the `claude` CLI is missing or the plugin isn't installed. |

**Why the `info` tier exists:** before v0.16.0, "Hermes not installed"
and "cwd is not a git repo" both rendered as `warn`, which polluted
`overall: warn` for installs that were fully healthy for their actual
use case. The `info` tier separates *intentional absence* from
*misconfiguration*. Tooling that only cares about real problems should
check `overall != "pass"`; tooling that wants to render full
configuration state should iterate the `checks` array and surface
`info` rows distinctly.

**Exit codes**

| Code | Condition |
|---|---|
| `0` | `overall` is `pass` or `warn` (and the doctor run completed). `info`-only installs exit `0` with `overall: pass`. |
| non-zero | At least one check returned `fail`, or the doctor run itself errored before reporting. |

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
| `CHORUS_CURSOR_DATA_DIR`     | cursor-agent projects root (CLI surface) | `~/.cursor/projects`                   |
| `CHORUS_CURSOR_APP_DATA_DIR` | Cursor IDE chat store root (app surface, v0.16.0) | `~/.cursor/chats`                      |
| `CHORUS_HERMES_DATA_DIR`     | Hermes sessions (provisional) | `~/.hermes/sessions`                |

Every `CHORUS_*` variable has a backward-compatible `BRIDGE_*` alias
(e.g. `BRIDGE_CURSOR_APP_DATA_DIR`). When both are set, `CHORUS_*` wins.
`chorus doctor` emits `env_override_dangling: warn` when any of these
points at a non-existent directory (see "Doctor — Severity Model"
below).

## Agent-Specific Notes

### Gemini: protobuf (`.pb`) fallback

Recent Gemini CLI builds store session state as protobuf at
`~/.gemini/<profile>/conversations/*.pb`. Chorus reads JSONL at
`~/.gemini/tmp/<hash>/chats/session-*.json` and does NOT yet parse the
protobuf form.

When `chorus read --agent gemini` returns `NOT_FOUND` and the error
message mentions "protobuf (.pb)", use one of these workarounds:

```bash
# Option 1: point Chorus at a known-good JSONL directory
chorus read --agent gemini --chats-dir /path/to/jsonl-export --cwd .

# Option 2: override discovery root for a long-running shell
export CHORUS_GEMINI_TMP_DIR=/path/to/jsonl-root
chorus read --agent gemini --cwd .
```

For the full workaround including a JSONL-stub recipe, see
[`docs/session-handoff-guide.md`](./session-handoff-guide.md) "Scenario
4 — Gemini protobuf fallback".

### Cursor: two on-disk surfaces (CLI transcripts + IDE app store, v0.16.0)

As of v0.16.0, Chorus reads Cursor sessions from **both** the cursor-agent CLI
transcript tree and the Cursor IDE (desktop app) chat store. The two surfaces
are independent; either can be empty without breaking the other.

| Surface | Path | Format | Override env |
|---|---|---|---|
| `cli` | `~/.cursor/projects/<project>/agent-transcripts/<session>/<session>.jsonl` | JSONL (one event per line) | `CHORUS_CURSOR_DATA_DIR` (legacy: `BRIDGE_CURSOR_DATA_DIR`) |
| `app` | `~/.cursor/chats/<hash>/<uuid>/store.db` | SQLite | `CHORUS_CURSOR_APP_DATA_DIR` (legacy: `BRIDGE_CURSOR_APP_DATA_DIR`) |

Both surfaces appear in the same `chorus list --agent cursor` /
`chorus search --agent cursor` results, distinguished by the cursor-only
`source: "cli" | "app"` field (documented under "Listing Sessions"
above). `chorus read --agent cursor` selects between them via `--id`
substring match like every other adapter.

Per-session `--cwd` scoping for the CLI surface is derived from
`<project>/.workspace-trusted` (`workspacePath`, when present) or from a
filesystem-validated demangle of the project directory name. The IDE
app surface scopes via the workspace path persisted in `store.db`.
`--include-user` is supported on both surfaces; `--tool-calls` runs
without error but does not currently surface `[TOOL: ...]` blocks (see
the tool-calls behaviour note in the read section).

**Node runtime requirement (app surface only):** the IDE app surface
requires **Node >= 22.5** for the built-in `node:sqlite` module.
On older Node versions the Rust CLI still exposes the IDE app surface
(it links `rusqlite`), but the Node CLI gracefully falls back to
showing only the CLI/JSONL surface — `chorus list --agent cursor`
will simply omit `source: "app"` rows, and `chorus doctor` reports
"Cursor IDE SQLite reader unavailable (requires Node >= 22.5 with
node:sqlite)" rather than failing. This is intentional: degraded
visibility, not a hard error.

See `docs/adapters/CURSOR_HERMES_NATIVE_ADAPTER.md` for the full adapter
architecture.

### Hermes (provisional scaffold)

Hermes is wired as a recognized agent but its on-disk format is unconfirmed.
The adapter assumes claude-like JSONL under `~/.hermes/sessions` (override via
`CHORUS_HERMES_DATA_DIR`). It returns cleanly when no data exists. Behavior
has not been validated against a real Hermes install.

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

## Update Notifications

Chorus checks for updates once per version.

- **Privacy**: Only contacts `registry.npmjs.org`.
- **Fail-silent**: If the check fails, it says nothing.
- **Opt-out**: Set `CHORUS_SKIP_UPDATE_CHECK=1`.

## Unknown Flag Handling (F11, v0.16.0)

Both runtimes now **fail closed on unknown flags**. The Rust CLI inherits
this behavior from clap. The Node CLI previously had a hand-rolled
parser that silently ignored unknown flags — typos like `--Json` (wrong
case) or `--limt` (transposed letters) used to fall through to default
behavior, producing surprising output. As of v0.16.0 the Node CLI
mirrors clap and rejects unknown flags by name:

```
$ chorus list --agent codex --limt 3
Unknown flag for 'list': --limt. Run `chorus list --help` to see allowed flags.
```

The validator runs at dispatch time, before the command's own parser, so
the error names the offending flag and the subcommand explicitly.
`agent-context` and `trash-talk` have their own nested parsers; the
top-level validator passes through to them.

The full per-command allowlist lives in
`scripts/read_session.cjs:ALLOWED_FLAGS`. A flag not in the allowlist is
rejected even if the underlying handler would have accepted it — the
allowlist is the contract.

## Parity Notes

As of v0.16.0, Node and Rust have full parity across every supported subcommand: `read` (including `--include-user`, `--tool-calls`, `--history`, and `--format {json|md|markdown}`), `list`, `search`, `compare`, `diff`, `summary`, `timeline`, `send`, `messages`, `checkpoint`, `setup`, `doctor`, `teardown`, `agent-context`, and `relevance`. All shared outputs are conformance-tested via `scripts/conformance.sh` against golden fixtures in `fixtures/golden/`.

### Search Invariant (`read(text) ⊆ search(text-tokens)`, CI-enforced in v0.16.0)

Every adapter must satisfy this invariant: if `chorus read --agent <X>` returns
content for a session, then `chorus search --agent <X> <tokens-from-that-content>`
must return that session in its results. Conformance now enforces this for
every supported adapter — claude, codex, gemini, cursor (both CLI and IDE
app surfaces), and hermes — in `scripts/conformance.sh` (see the
`search-read-parity` block).

The codex extractor was the original motivating bug: prior to v0.16.0 it
walked a top-level `role`/`content` schema that never existed in real
codex sessions, so the read path returned content from one envelope and
the search path indexed nothing — silently returning empty results. The
fix walks the real `response_item.payload.message` and
`event_msg.payload.message` envelopes that codex actually emits, so
read and search now operate on the same content.

This invariant is what makes "evidence-based" claims auditable: a
consumer that quotes content from `chorus read` can verify the source
session is discoverable via `chorus search` without trusting the read
adapter blindly.

Two documented wrinkles:

- **`--format json` on `chorus read`:** Rust treats `--format json` as an alias for `--json`. Node has a bug where `--format json` falls through to plain-text output (see `scripts/read_session.cjs:1759`); `--json` continues to produce JSON on Node as expected. The Node bug is documented and left in place; use `--json` for JSON output on both runtimes.
- **`--tool-calls` on Gemini and Cursor:** runs without error but returns no `[TOOL: ...]` blocks in either runtime. The Gemini JSONL and Cursor session stores do not carry a tool-call schema that the adapters can surface. Tracked for a follow-up.
