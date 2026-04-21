# System Overview

## Product Shape
- npm package: `agent-chorus` v0.13.0 (binaries: `chorus`, `chorus-node`)
- Rust crate: `agent-chorus` v0.13.0 (binary: `chorus`)
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
| `chorus read` | Read a single agent session. Supports `--tool-calls`, `--format {json\|md\|markdown}`, `--include-user` in **both** Node and Rust (Rust parity landed in v0.13.0). Node `--format json` has a known fall-through bug — use `--json`. | `agents.rs`, `read_session.cjs` |
| `chorus list` | List sessions for an agent | `agents.rs`, `read_session.cjs` |
| `chorus search` | Search session content | `agents.rs`, `read_session.cjs` |
| `chorus compare` | Compare sessions across agents | `agents.rs`, `read_session.cjs` |
| `chorus report` | Generate handoff coordinator report | `report.rs`, `read_session.cjs` |
| `chorus diff` | Line-level diff between sessions | `diff.rs`, `read_session.cjs` |
| `chorus summary` | Structured session digest (metadata-only, no LLM call). Node + Rust parity since v0.13.0. | `summary.rs`, `read_session.cjs` |
| `chorus timeline` | Cross-agent chronological interleave. Node + Rust parity since v0.13.0. | `timeline.rs`, `read_session.cjs` |
| `chorus relevance` | Inspect agent-context relevance patterns | `relevance.rs`, `relevance.cjs` |
| `chorus send` / `messages` | Agent-to-agent messaging | `messaging.rs`, `read_session.cjs` |
| `chorus checkpoint --from <agent>` | Broadcast git state to every other agent (v0.12.0) | `checkpoint.rs`, `read_session.cjs` |
| `chorus setup` | Wire chorus into a project (scaffolding, managed blocks, gitignore, Claude Code plugin). Node + Rust parity since v0.13.0. | `setup.rs`, `read_session.cjs` |
| `chorus doctor` | Diagnose installation, per-agent session discovery, pack state. Node + Rust parity since v0.13.0. | `doctor.rs`, `read_session.cjs` |
| `chorus teardown` | Cleanly reverse setup | `read_session.cjs` |
| `chorus agent-context init/seal/build` | Init, seal, build context packs | `agent_context.rs`, `agent_context/*.cjs` |
| `chorus agent-context verify` | Verify context pack completeness (interactive or `--ci` mode) | `agent_context.rs`, `scripts/agent_context/verify.cjs`, `templates/ci-agent-context.yml` |
| `chorus trash-talk` | Roast agents (easter egg) | `read_session.cjs` |

## Session Handoff (v0.12.0)
- `chorus checkpoint --from <agent>` broadcasts a lightweight git-state message (branch / uncommitted count / last commit) to every other agent's inbox in one call. Guards on `.agent-chorus/` presence so it is safe to call unconditionally.
- `scripts/hooks/chorus-session-end.sh` is a Claude Code `SessionEnd` hook wrapper. Installs via `~/.claude/settings.json`; hardened with `set -euo pipefail`, `realpath` canonicalization of `$CLAUDE_PROJECT_DIR`, and backgrounded+`disown` dispatch.
- Full protocol, standup/conclude rituals, and interruption recovery: `docs/session-handoff-guide.md` (linked from `CLAUDE.md`, `AGENTS.md`, and the rewritten `GEMINI.md`).

## Gemini / Cursor Fallback Detection (v0.12.0)
- `chorus read --agent gemini` probes `~/.gemini/<profile>/conversations/*.pb` when JSONL lookup misses. If `.pb` files exist, the `NOT_FOUND` error names the count, the directory, and points at `--chats-dir` + the handoff guide instead of returning the bare message.
- `chorus read --agent cursor` probes `User/workspaceStorage/<workspace-id>/state.vscdb` when file-based lookup misses. Mirror of the Gemini change. Full SQLite-backed reading is a follow-up; the probe alone turns opaque `NOT_FOUND` into actionable guidance.
- Both probes live in `cli/src/agents.rs` as `detect_gemini_pb_fallback_hint` / `detect_cursor_vscdb_fallback_hint`; the bare messages are composed by `gemini_not_found_message` / `cursor_not_found_message`.

## Full Rust Parity (v0.13.0)
- `cli/src/summary.rs`, `cli/src/timeline.rs`, `cli/src/doctor.rs`, `cli/src/setup.rs` ship the four previously-Node-only subcommands. Output shape matches the Node implementation byte-for-byte against shared golden fixtures in `fixtures/golden/`.
- `cli/src/agents.rs` carries a `ReadOptions` struct plus `_with_options` variants of the read functions that take `include_user`, `include_tool_calls`, and the rendering format. Rust treats `--format json` as an alias for `--json`; Node has a documented fall-through bug at `scripts/read_session.cjs:1759` where `--format json` produces plain text. Use `--json` on both runtimes for JSON.
- `--tool-calls` on Gemini and Cursor is a no-op in both runtimes — those adapters do not parse a tool-call schema from their stores yet. Flag succeeds silently; no `[TOOL: ...]` blocks appear in output. Tracked for a follow-up.
- Rust test suite: 52 tests (29 previously, 23 new for the v0.13.0 parity work).

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
