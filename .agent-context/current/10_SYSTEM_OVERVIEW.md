# System Overview

## Product Shape
- npm package: `agent-chorus` v0.7.0 (binary: `chorus`, `chorus-node`)
- Rust crate: `agent-chorus` v0.7.0 (binary: `chorus`)
- 124 tracked files across Node scripts, Rust source, schemas, fixtures, and docs
- Ships as a global CLI tool (`npm install -g agent-chorus`)

## Runtime Architecture
1. User invokes `chorus <command>` (routed to Node or Rust binary).
2. CLI parses flags and resolves agent session directories via env vars or defaults.
3. Agent adapter (`scripts/adapters/*.cjs` or `cli/src/agents.rs`) scans JSONL session files, parsing turns and metadata.
4. Sensitive content is redacted (API keys, tokens, PEM blocks) with pattern-based filters.
5. Output is formatted as structured JSON (schema-validated) or human-readable text with boundary markers.

## Command/API Surface
| Command | Intent | Primary Source Files |
| --- | --- | --- |
| `chorus read` | Read a single agent session | `agents.rs`, `read_session.cjs` |
| `chorus list` | List sessions for an agent | `agents.rs`, `read_session.cjs` |
| `chorus search` | Search session content | `agents.rs`, `read_session.cjs` |
| `chorus compare` | Compare sessions across agents | `agents.rs`, `read_session.cjs` |
| `chorus report` | Generate handoff coordinator report | `report.rs`, `read_session.cjs` |
| `chorus diff` | Line-level diff between sessions | `diff.rs`, `read_session.cjs` |
| `chorus relevance` | Inspect context-pack relevance patterns | `relevance.rs`, `relevance.cjs` |
| `chorus send` / `messages` | Agent-to-agent messaging | `messaging.rs`, `read_session.cjs` |
| `chorus setup` / `doctor` | Bootstrap and diagnose installation | `main.rs`, `read_session.cjs` |
| `chorus context-pack *` | Init, seal, verify, build context packs | `context_pack.rs`, `context_pack/*.cjs` |

## Tracked Path Density
| Directory | Files | Content |
| --- | --- | --- |
| `scripts/` | 33 | Node implementation, adapters, context-pack, tests |
| `fixtures/` | 34 | Demo HTML, golden outputs, adversarial tests, session stores |
| `cli/` | 16 | Rust implementation (src, Cargo.toml, Cargo.lock) |
| `docs/` | 11 | CLI reference, development guide, SVGs, demo WebP assets |
| `schemas/` | 6 | JSON Schema definitions for all output types |
| `.agent-context/` | 7 | Context pack templates, guide, relevance config |
| Root | 17 | README, PROTOCOL, LICENSE, package.json, CI workflows |
