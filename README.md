# Agent Chorus

![CI Status](https://github.com/cote-star/agent-chorus/actions/workflows/ci.yml/badge.svg)
![License](https://img.shields.io/badge/license-MIT-blue.svg)
![Version](https://img.shields.io/badge/version-0.16.0-green.svg)
[![Star History](https://img.shields.io/github/stars/cote-star/agent-chorus?style=social)](https://github.com/cote-star/agent-chorus)

**Let your AI agents talk about each other.**

Ask one agent what another is doing, and get an evidence-backed answer. No copy-pasting, no tab-switching, no guessing.

> If you use 2+ AI coding agents (Codex, Claude, Gemini, Cursor CLI, Cursor IDE), Chorus gives them shared visibility — no orchestrator required.

![Before/after workflow](docs/silo-tax-before-after.webp)

```bash
chorus read --agent claude --include-user --json
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

### From Zero to a Working Query

`chorus setup` wires every agent on the box in under a minute.

![Setup Demo](docs/demo-setup.webp)

## Quick Start

```bash
# 1. Install
npm install -g agent-chorus      # requires Node >= 18
# or
cargo install agent-chorus       # requires Rust >= 1.74

# 2. Wire your agents
chorus setup                     # patches CLAUDE.md / GEMINI.md / AGENTS.md, adds .gitignore entries
chorus doctor                    # verify session paths, provider wiring, updates

# 3. Ask any agent in natural language
#    "What is Claude doing?"  /  "Compare Codex and Gemini outputs."  /  "Pick up where Gemini left off."
```

Or call chorus directly:

```bash
chorus read --agent codex --include-user --json
```

Every response is structured, source-tracked, and redacted:

```json
{
  "agent": "codex",
  "session_id": "session-abc123",
  "content": "USER:\nInvestigate the auth regression...\n---\nASSISTANT:\nI am tracing the auth middleware...",
  "timestamp": "2026-06-02T10:30:00Z",
  "message_count": 12,
  "source": "/home/user/.codex/sessions/2026/06/02/session-abc123.jsonl"
}
```

Source file, session ID, and timestamp on every response. Secrets auto-redacted before output. Prefer `--format markdown` for human review.

To reverse everything `setup` did: `chorus teardown` (add `--global` to also drop `~/.cache/agent-chorus/`).

## What's New in v0.16.0

- **Cursor IDE adapter.** Chorus now reads both the `cursor-agent` CLI transcripts *and* Cursor IDE app sessions through one adapter. If you use the Cursor app, your sessions are now first-class.
- **`--history=on-demand` default.** `chorus read` now returns just the latest session for the current `cwd`. Closes the 2.5x token-inflation issue measured in the v0.15 field study. Provider snippets carry the contract so consumer agents inherit it automatically.
- **`cwd_mismatch` is now explicit.** When `--cwd` matches no session, the output says so. No more silent fallbacks that read like real data.
- **Doctor honesty pass.** New `info` severity, env-var dangling-path detection, git-aware hooks checks, stale-snippet detection. Doctor tells the truth or stays quiet.
- **Codex search parity fix.** `chorus search --agent codex` no longer silently returns empty. The `read ⊆ search` invariant is now enforced for every adapter.
- **`--help` overhaul.** Per-subcommand help leads with that subcommand. `chorus report --help` ships a copy-pasteable handoff JSON schema.

Full changelog and upgrade notes: [`RELEASE_NOTES.md`](./RELEASE_NOTES.md).

## How It Works

1. **Ask naturally** — "What is Claude doing?" / "Did Gemini finish the API?"
2. **Your agent runs chorus** — `chorus summary`, `read`, `timeline`, `compare`, `search`, `diff`, `send`, `messages`, `checkpoint`, etc.
3. **Evidence-backed answer** — sources cited, divergences flagged, no hallucination.

**Tenets:**
- **Local-first** — reads agent session logs directly on your machine. No data leaves.
- **Evidence-based** — every claim tracks to a specific source session file.
- **Privacy-focused** — auto-redacts API keys, tokens, and passwords.
- **Dual parity** — Node.js + Rust CLIs ship identical output contracts, conformance-tested against shared fixtures.

## How It Compares

| | agent-chorus | CrewAI / AutoGen / LangGraph | ccswarm / claude-squad |
| :--- | :---: | :---: | :---: |
| **Approach** | Read-only evidence layer | Full orchestration framework | Parallel agent spawning |
| **Install** | `npm i -g agent-chorus` or `cargo install` | pip + ecosystem | git clone |
| **Agents covered** | Codex, Claude, Gemini, Cursor CLI + IDE | Provider-specific | Usually Claude-only |
| **Runtime deps shipped** | Zero | Heavy Python/TS stack | Moderate |
| **Privacy** | Local-first, auto-redaction | Cloud-optional | Varies |
| **Built-ins unique to chorus** | session summary (no LLM), cross-agent timeline, session diff, handoff protocol, agent-to-agent messages, context pack | none | none |
| **Implementation** | Node.js + Rust, conformance-tested | Single language | Single language |
| **Philosophy** | Visibility first, orchestration optional | Orchestration first | Task spawning |

Chorus is the evidence layer underneath whatever orchestrator you use — not a replacement. If you want a router/scheduler/work-queue, pair chorus with one of the frameworks above. If you just want agents to see each other's work, chorus alone is enough.

## Key Capabilities

A taste — see [`docs/CLI_REFERENCE.md`](./docs/CLI_REFERENCE.md) for the full surface.

```bash
# Structured digest — files, tools, duration. No LLM calls.
chorus summary --agent claude --cwd . --json

# Chronological view across every agent on the project
chorus timeline --cwd . --format markdown

# What an agent actually touched (Read/Edit/Bash/Write)
chorus read --agent codex --tool-calls --json

# Verify one agent's claim against another
chorus compare --source codex --source claude --cwd . --json

# Audit what got redacted, and why
chorus read --agent claude --audit-redactions --json

# Coordinate without switching tabs
chorus send --from claude --to codex --message "auth module ready" --cwd .
chorus messages --agent codex --cwd . --json

# Broadcast where you left off before ending a session
chorus checkpoint --from claude --cwd .
```

## Supported Agents

Every adapter supports `read / list / search / summary / timeline / send / messages / checkpoint`. The differences are where each agent stores sessions and which adapters render tool-call content vs. emit a uniform NOT_AVAILABLE warning.

| Agent | Session source | `--tool-calls` |
| --- | --- | --- |
| **Codex** | `~/.codex/sessions/**/*.jsonl` | renders |
| **Claude** | `~/.claude/projects/**/*.jsonl` | renders |
| **Cursor CLI** | `~/.cursor/projects/*/agent-transcripts/**/*.jsonl` | renders |
| **Cursor IDE (app)** *(new in v0.16.0)* | `~/.cursor/chats/*/*/store.db` (SQLite; requires Node ≥ 22.5 for the IDE surface) | renders |
| **Gemini** | `~/.gemini/tmp/*/chats/*.json[l]` | uniform `NOT_AVAILABLE` warning (gemini transcripts don't carry tool calls) |
| **Hermes** *(provisional scaffold)* | `~/.hermes/sessions/**/*.jsonl` | uniform `NOT_AVAILABLE` warning |

Node and Rust runtimes ship identical output for every agent above; the `read(text) ⊆ search(text-tokens)` invariant is CI-enforced for each adapter. Full per-flag reference: [`docs/CLI_REFERENCE.md`](./docs/CLI_REFERENCE.md).

## Context Pack

A context pack is an agent-first, token-efficient repo briefing for end-to-end understanding tasks. Instead of re-reading the full repository on every request, agents start from `.agent-context/current/` and open project files only when needed. Local-first, no need to make your repo public.

```bash
chorus agent-context init    # creates .agent-context/current/ with templates
# ...agent fills in the <!-- AGENT: ... --> sections...
chorus agent-context seal    # validates and locks the pack
```

Ask your agent: *"Understand this repo end-to-end using the context pack first, then deep dive only where needed."*

![Context Pack Read-Order](docs/cold-start-agent-context-hero.webp)

CI gate: `chorus agent-context verify --ci` exits non-zero if the pack is stale or corrupt. Internals, sync policy, enforcement: [`AGENT_CONTEXT.md`](./AGENT_CONTEXT.md).

## Architecture, in one diagram

Chorus sits between your agent and other agents' session logs. Read-only, evidence-first, no central control plane.

![Claude to Codex handoff via read-only evidence](docs/orchestrator-handoff-flow.svg)

Boundaries: no task router, no scheduler, no autonomous chaining, no live sync stream. Snapshot-based reads from local logs, by design.

## Easter Egg

`chorus trash-talk` roasts your agents based on their session content.

![Trash Talk Demo](docs/demo-trash-talk.webp)

## Go Deeper

| If you need... | Go here |
| :--- | :--- |
| Full command syntax and JSON outputs | [`docs/CLI_REFERENCE.md`](./docs/CLI_REFERENCE.md) |
| Adapter formats, schema contracts, redaction rules | [`PROTOCOL.md`](./PROTOCOL.md) |
| Session handoff protocol, hooks, Gemini `.pb` fallback | [`docs/session-handoff-guide.md`](./docs/session-handoff-guide.md) |
| Agent-context internals and policy | [`AGENT_CONTEXT.md`](./AGENT_CONTEXT.md) |
| Release-level changes and upgrade notes | [`RELEASE_NOTES.md`](./RELEASE_NOTES.md) |
| Contributing or extending the codebase | [`docs/DEVELOPMENT.md`](./docs/DEVELOPMENT.md) / [`CONTRIBUTING.md`](./CONTRIBUTING.md) |

---

Every agent session is evidence. Chorus makes it readable.

Found a bug or have a feature idea? [Open an issue](https://github.com/cote-star/agent-chorus/issues). Ready to contribute? See [`CONTRIBUTING.md`](./CONTRIBUTING.md).

[![Star History Chart](https://api.star-history.com/svg?repos=cote-star/agent-chorus&type=Date)](https://star-history.com/#cote-star/agent-chorus&Date)
