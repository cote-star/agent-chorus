# System Overview

## Product Shape
- npm package: `agent-chorus` v0.14.0 (binaries: `chorus`, `chorus-node`)
- Rust crate: `agent-chorus` v0.14.0 (binary: `chorus`)
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
- Rust test suite: 139 tests as of v0.14.0 (52 at v0.13.0 baseline, 87 added by the agent-context hardening pass).

## Agent-Context Hardening (v0.14.0)

### Manifest additions
- `aliases` — object mapping canonical filenames to on-disk names (e.g. `{"20_CODE_MAP.md": "20_architecture.md"}`). Verify retries with aliased filenames; seal carries the map forward across re-seals.
- `last_known_good_sha` — pointer to the sealed HEAD of the last fully-green `verify --ci` run. `rollback --latest-good` resolves this through `history.jsonl` (falling back to rotated archives).
- `schema_version` — now enforced by `verify`; unknown schema versions fail fast instead of silently degrading.
- Provenance block — head SHA at seal, seal timestamp, tool versions, tool hashes recorded so downstream consumers can verify pack origin.

### Tier concept (`init --tier 1|2|3`)
- **Tier 1**: `20_CODE_MAP.md` + `routes.json` only (minimal onboarding surface).
- **Tier 2**: Tier 1 + `30_BEHAVIORAL_INVARIANTS.md` + `completeness_contract.json`.
- **Tier 3**: full pack (default; identical to legacy behavior).
- Seal auto-detects which files are actually present, so a Tier-1/2 pack does not fail the required-files check.

### Session-start freshness gate
The routing blocks `init` upserts into `CLAUDE.md` / `AGENTS.md` / `GEMINI.md` now open with a mandatory first-line instruction: **agents must compare `head_sha_at_seal` against `git rev-parse HEAD` before any reasoning and warn the user when they diverge.** Rust and Node `init` flows emit the identical preamble. This is a behavior change for agents consuming existing packs — re-run `chorus agent-context init` (or re-seal) to pick up the gate.

### Separate-commit enforcement flag
`chorus agent-context verify --ci --enforce-separate-commits` inspects `base..HEAD` and fails if any commit mixes `.agent-context/**` with non-pack paths. **Off by default.** The gate is opt-in and intended for teams that have adopted the "pack edits land as their own commit" convention. See `docs/CLI_REFERENCE.md` for the JSON schema additions (`separate_commits`, `mixed_commits`).

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
