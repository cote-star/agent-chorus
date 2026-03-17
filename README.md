# Agent Chorus

![CI Status](https://github.com/cote-star/agent-chorus/actions/workflows/ci.yml/badge.svg)
![License](https://img.shields.io/badge/license-MIT-blue.svg)
![Version](https://img.shields.io/badge/version-0.8.0-green.svg)
[![Star History](https://img.shields.io/github/stars/cote-star/agent-chorus?style=social)](https://github.com/cote-star/agent-chorus)

**Let your AI agents talk about each other.**

Ask one agent what another is doing, and get an evidence-backed answer. No copy-pasting, no tab-switching, no guessing.

> If you use 2+ AI coding agents (Codex, Claude, Gemini, Cursor), Chorus gives them shared visibility — no orchestrator required.

![Before/after workflow](docs/silo-tax-before-after.webp)

```bash
chorus read --agent claude --json
```

**Two problems, one tool:**
- **Silo Tax** — multi-agent workflows break when agents cannot verify each other's work. Chorus gives every agent read access to every other agent's session evidence.
- **Cold-Start Tax** — every new session re-reads the same repo from zero. A [Context Pack](#context-pack) gives agents instant repo understanding in 5 ordered docs.

## See It In Action

### The Handoff

Switch from Gemini to Claude mid-task. Claude picks up where Gemini left off.

![Handoff Demo](docs/demo-handoff.webp)

### The Status Check

Three agents working on checkout. You ask Codex what the others are doing.

![Status Check Demo](docs/demo-status.webp)

### What You Get Back

Every response is structured, source-tracked, and redacted:

```bash
chorus read --agent codex --json
```

```json
{
  "agent": "codex",
  "source": "/home/user/.codex/sessions/2026/03/12/session-abc123.jsonl",
  "content": "The assistant's response with evidence...",
  "warnings": [],
  "session_id": "session-abc123",
  "cwd": "/workspace/project",
  "timestamp": "2026-03-12T10:30:00Z",
  "message_count": 12,
  "messages_returned": 1
}
```

Source file, session ID, and timestamp on every response. Secrets auto-redacted before output. Warnings surface scope mismatches.

## Quick Start

### 1. Install

```bash
npm install -g agent-chorus    # requires Node >= 18
# or
cargo install agent-chorus     # requires Rust >= 1.74
```

### 2. Setup

```bash
chorus setup
chorus doctor # Check session paths, provider wiring, and updates
```

From zero to a working skill query in under a minute:

![Setup Demo](docs/demo-setup.webp)

This wires skill triggers into your agent configs (`CLAUDE.md`, `GEMINI.md`, `AGENTS.md`) so agents know how to use chorus.

To cleanly reverse everything setup does (managed blocks, scaffolding, hooks):

```bash
chorus teardown           # reverse setup for this project
chorus teardown --global  # also remove ~/.cache/agent-chorus/
```

### 3. Ask

Tell any agent:

> "What is Claude doing?"
> "Compare Codex and Gemini outputs."
> "Pick up where Gemini left off."

The agent runs chorus commands behind the scenes and gives you an evidence-backed answer.

<details><summary>Session selection behavior</summary>

After `chorus setup`, provider instructions follow this behavior:

- If no session is specified, read the latest session in the current project.
- "past session" / "previous session" means one session before latest.
- "last N sessions" includes latest.
- "past N sessions" excludes latest (older N sessions).
- Ask for a session ID only if initial fetch fails or exact ID is explicitly requested.

</details>

## How It Works

1. **Ask naturally** - "What is Claude doing?" / "Did Gemini finish the API?"
2. **Agent runs chorus** - Your agent calls `chorus read`, `chorus list`, `chorus search`, `chorus compare`, `chorus diff`, `chorus send`, `chorus messages`, etc. behind the scenes.
3. **Evidence-backed answer** - Sources cited, divergences flagged, no hallucination.

**Tenets:**
- **Local-first** - reads directly from agent session logs on your machine. No data leaves.
- **Evidence-based** - every claim tracks to a specific source session file.
- **Privacy-focused** - automatically redacts API keys, tokens, and passwords.
- **Dual parity** - ships Node.js + Rust CLIs with identical output contracts.

## Real-World Recipes

### Handoff Recovery

Gemini crashed mid-task. Tell Claude to pick up where it left off.

```bash
chorus read --agent gemini --cwd . --json
```

Your agent reads Gemini's last output with full context — file paths, session ID, timestamps — and continues the work.

### Cross-Agent Verification

Codex says it fixed the payment bug. Verify against Claude's analysis before deploying.

```bash
chorus compare --source codex --source claude --cwd . --json
```

The response highlights agreements, contradictions, and divergences with evidence from both sessions.

### Security Audit

Before merging, check what secrets appeared in agent sessions and were redacted.

```bash
chorus read --agent claude --audit-redactions --json
```

Returns a `redactions` array showing each pattern matched (e.g., `openai_api_key`, `bearer_token`) and how many times.

### Agent Coordination

Tell Codex the auth module is ready for review — without switching tabs.

```bash
chorus send --from claude --to codex --message "auth module ready for review" --cwd .
chorus messages --agent codex --cwd . --json
```

Messages are stored locally in `.agent-chorus/messages/` and never leave your machine.

## Supported Agents

Full multi-agent coverage. No other tool matches this breadth across 4 agents and 9 capabilities.

| Feature              | Codex | Gemini | Claude | Cursor |
| :------------------- | :---: | :----: | :----: | :----: |
| **Read Content**     |  Yes  |  Yes   |  Yes   |  Yes   |
| **Auto-Discovery**   |  Yes  |  Yes   |  Yes   |  Yes   |
| **CWD Scoping**      |  Yes  |   No   |  Yes   |   No   |
| **List Sessions**    |  Yes  |  Yes   |  Yes   |  Yes   |
| **Search**           |  Yes  |  Yes   |  Yes   |  Yes   |
| **Comparisons**      |  Yes  |  Yes   |  Yes   |  Yes   |
| **Session Diff**     |  Yes  |  Yes   |  Yes   |  Yes   |
| **Redaction Audit**  |  Yes  |  Yes   |  Yes   |  Yes   |
| **Messaging**        |  Yes  |  Yes   |  Yes   |  Yes   |

Both Node.js and Rust implementations pass identical conformance tests against shared fixtures.

## Key Capabilities

### Session Diff

Compare two sessions from the same agent with line-level precision.

```bash
chorus diff --agent codex --from session-abc --to session-def --cwd . --json
```

### Redaction Audit Trail

See exactly what was redacted and why in any `chorus read` output.

```bash
chorus read --agent claude --audit-redactions --json
```

### Agent-to-Agent Messaging

Agents leave messages for each other through a local JSONL queue.

```bash
chorus send --from claude --to codex --message "auth module ready for review" --cwd .
chorus messages --agent codex --cwd . --json
```

### Relevance Introspection

Inspect and test the context-pack filtering patterns that decide which files matter.

```bash
chorus relevance --list --cwd .              # Show current include/exclude patterns
chorus relevance --test src/main.rs --cwd .  # Test if a file matches
chorus relevance --suggest --cwd .           # Suggest patterns for this project
```

## How It Compares

| | agent-chorus | CrewAI / AutoGen | ccswarm / claude-squad |
| :--- | :---: | :---: | :---: |
| **Approach** | Read-only evidence layer | Full orchestration framework | Parallel agent spawning |
| **Install** | `npm i -g agent-chorus` or `cargo install` | pip + ecosystem | git clone |
| **Agents** | Codex, Claude, Gemini, Cursor | Provider-specific | Usually Claude-only |
| **Dependencies** | Zero npm prod deps | Heavy Python/TS stack | Moderate |
| **Privacy** | Local-first, auto-redaction | Cloud-optional | Varies |
| **Cold-start solution** | Context Pack (5-doc briefing) | None | None |
| **Language** | Node.js + Rust (conformance-tested) | Python or TypeScript | Single language |
| **Agent messaging** | Built-in JSONL queue | Framework-specific | None |
| **Philosophy** | Visibility first, orchestration optional | Orchestration first | Task spawning |

## Architecture

Chorus sits between your agent and other agents' session logs. The workflow is evidence-first: one agent reads another agent's session evidence and continues with a local decision, without a central control plane.

![Claude to Codex handoff via read-only evidence](docs/orchestrator-handoff-flow.svg)

```mermaid
sequenceDiagram
    participant User
    participant Agent as Your Agent (Codex, Claude, etc.)
    participant Chorus as chorus CLI
    participant Sessions as Other Agent Sessions

    User->>Agent: "What is Claude doing?"
    Agent->>Chorus: chorus read --agent claude --json
    Chorus->>Sessions: Scan ~/.claude/projects/*.jsonl
    Sessions-->>Chorus: Raw session data
    Chorus->>Chorus: Redact secrets, format
    Chorus-->>Agent: Structured JSON
    Agent-->>User: Evidence-backed natural language answer
```

<details><summary>Diagram not rendering? View as image</summary>

![Architecture sequence diagram](docs/architecture.svg)

</details>

### Current Boundaries

- No orchestration control plane: no task router, scheduler, or work queues.
- No autonomous agent chaining by default; handoffs are human-directed.
- No live synchronization stream; reads are snapshot-based from local session logs.

## Context Pack

A context pack is an agent-first, token-efficient repo briefing for end-to-end understanding tasks.
Instead of re-reading the full repository on every request, agents start from `.agent-context/current/` and open project files only when needed.
This works the same for private repositories: the pack is local-first and does not require making your code public.

- `5` ordered docs + `manifest.json` (compact index, not a repo rewrite).
- Deterministic read order: `00` -> `10` -> `20` -> `30` -> `40`.
- Main-only smart sync: updates only when context-relevant files change.
- Local recovery snapshots with rollback support.

```bash
# Recommended workflow:
chorus context-pack init    # Creates .agent-context/current/ with templates
# ...agent fills in <!-- AGENT: ... --> sections...
chorus context-pack seal    # Validates content and locks the pack

# Manual rebuild (backward-compatible wrapper)
chorus context-pack build

# Install pre-push hook (advisory-only check on main push)
chorus context-pack install-hooks
```

Ask your agent explicitly:

> "Understand this repo end-to-end using the context pack first, then deep dive only where needed."

![Context Pack Read-Order](docs/cold-start-context-pack-hero.webp)

![Context Pack Demo](docs/demo-context-pack.webp)

Full context-pack internals and policy details: [`CONTEXT_PACK.md`](./CONTEXT_PACK.md)

<details><summary>Sync policy, usage boundaries, and layered model</summary>

### Main Push Sync Policy

- Pushes that do not target `main`: skipped.
- Pushes to `main` with no context-relevant changes: skipped.
- Pushes to `main` with context-relevant changes: advisory warning printed (no auto-build).

Optional pre-PR guard:

```bash
chorus context-pack check-freshness --base origin/main
```

### Usage Boundaries

- Do not treat context pack as a substitute for source-of-truth when changing behavior-critical code.
- Do not expect automatic updates from commits alone or non-`main` branch pushes.
- Do not put secrets in context-pack content; `.agent-context/current/` is tracked in git.

### Layered Model

- **Layer 0 (Evidence)**: cross-agent session reads with citations.
- **Layer 1 (Context)**: context-pack index for deterministic repo onboarding.
- **Layer 2 (Coordination, optional)**: explicit orchestration only when layers 0-1 are insufficient.

Recovery matrix:

- `.agent-context/current/` -> `git checkout <commit> -- .agent-context/current`
- `.agent-context/snapshots/` -> `chorus context-pack rollback`

</details>

## Easter Egg

`chorus trash-talk` roasts your agents based on their session content.

![Trash Talk Demo](docs/demo-trash-talk.webp)

## Roadmap

- **Context Pack customization** - user-defined doc structure, custom sections, team templates.
- **Windows installation** - native Windows support (currently macOS/Linux).
- **Cross-agent context sharing** - agents share context snippets (still read-only, still local).

<details><summary>Update notifications</summary>

Chorus checks for updates once per version.
- **Privacy**: Only contacts `registry.npmjs.org`.
- **Fail-silent**: If the check fails, it says nothing.
- **Opt-out**: Set `CHORUS_SKIP_UPDATE_CHECK=1`.

</details>

## Go Deeper

| If you need... | Go here |
| :--- | :--- |
| Full command syntax and JSON outputs | [`docs/CLI_REFERENCE.md`](./docs/CLI_REFERENCE.md) |
| Context-pack internals and policy details | [`CONTEXT_PACK.md`](./CONTEXT_PACK.md) |
| Protocol and schema contract details | [`PROTOCOL.md`](./PROTOCOL.md) |
| Contributing or extending the codebase | [`docs/DEVELOPMENT.md`](./docs/DEVELOPMENT.md) / [`CONTRIBUTING.md`](./CONTRIBUTING.md) |
| Release-level changes and upgrade notes | [`RELEASE_NOTES.md`](./RELEASE_NOTES.md) |

---

Every agent session is evidence. Chorus makes it readable.

Found a bug or have a feature idea? [Open an issue](https://github.com/cote-star/agent-chorus/issues). Ready to contribute? See [`CONTRIBUTING.md`](./CONTRIBUTING.md).

[![Star History Chart](https://api.star-history.com/svg?repos=cote-star/agent-chorus&type=Date)](https://star-history.com/#cote-star/agent-chorus&Date)
