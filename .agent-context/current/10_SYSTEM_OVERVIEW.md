# System Overview

## Product Shape
- npm package: `agent-chorus` v0.9.0 (binaries: `chorus`, `chorus-node`)
- Rust crate: `agent-chorus` v0.9.0 (binary: `chorus`)
- ~130 tracked files across Node scripts, Rust source, schemas, fixtures, and docs
- Ships as a global CLI tool (`npm install -g agent-chorus`)

## Runtime Architecture
1. User invokes `chorus <command>` (routed to Node or Rust binary).
2. CLI parses flags and resolves agent session directories via env vars or defaults.
3. Agent adapter (`scripts/adapters/*.cjs` or `cli/src/agents.rs`) scans JSONL session files, parsing turns and metadata.
4. Sensitive content is redacted (API keys, tokens, PEM blocks) with pattern-based filters.
5. Output is formatted as structured JSON (schema-validated) or human-readable text with boundary markers.

## Silent Failure Modes
- **Redaction miss**: If a new secret pattern is not in the redaction regex set, it passes through silently. No error, no warning — the secret appears in output. Both implementations must share the same pattern list.
- **Adapter fallback**: If a session file has unexpected schema, the adapter may return partial content without error. The `warnings` array in JSON output captures these, but text output does not surface them.
- **Agent-context stale shortcuts**: `verification_shortcuts` in `search_scope.json` reference line numbers. If the source file changes, the line numbers silently become wrong. Seal validates file existence but not line accuracy.
- **Golden fixture drift**: If output format changes but golden fixtures are not updated, `conformance.sh` catches it — but only if the test covers that specific command/flag combination.

## Command/API Surface
| Command | Intent | Primary Source Files |
| --- | --- | --- |
| `chorus read` | Read a single agent session | `agents.rs`, `read_session.cjs` |
| `chorus list` | List sessions for an agent | `agents.rs`, `read_session.cjs` |
| `chorus search` | Search session content | `agents.rs`, `read_session.cjs` |
| `chorus compare` | Compare sessions across agents | `agents.rs`, `read_session.cjs` |
| `chorus report` | Generate handoff coordinator report | `report.rs`, `read_session.cjs` |
| `chorus diff` | Line-level diff between sessions | `diff.rs`, `read_session.cjs` |
| `chorus relevance` | Inspect agent-context relevance patterns | `relevance.rs`, `relevance.cjs` |
| `chorus send` / `messages` | Agent-to-agent messaging | `messaging.rs`, `read_session.cjs` |
| `chorus setup` / `doctor` | Bootstrap and diagnose installation | `main.rs`, `read_session.cjs` |
| `chorus teardown` | Cleanly reverse setup | `read_session.cjs` |
| `chorus agent-context init/seal/build` | Init, seal, build context packs | `agent_context.rs`, `agent_context/*.cjs` |
| `chorus agent-context verify` | Verify context pack completeness (interactive or `--ci` mode) | `agent_context.rs`, `scripts/agent_context/verify.cjs`, `templates/ci-agent-context.yml` |
| `chorus trash-talk` | Roast agents (easter egg) | `read_session.cjs` |

## Tracked Path Density
| Directory | Files | Content |
| --- | --- | --- |
| `scripts/` | ~35 | Node implementation, adapters, agent-context, tests |
| `fixtures/` | ~34 | Demo HTML, golden outputs, adversarial tests, session stores |
| `cli/` | ~16 | Rust implementation (src, Cargo.toml, Cargo.lock) |
| `docs/` | ~11 | CLI reference, development guide, SVGs, demo WebP assets |
| `schemas/` | 6 | JSON Schema definitions for all output types |
| `.agent-context/` | ~12 | Context pack content, structured artifacts, guide, relevance config |
| Root | ~17 | README, PROTOCOL, LICENSE, package.json, CI workflows |
