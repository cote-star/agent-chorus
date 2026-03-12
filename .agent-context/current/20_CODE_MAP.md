# Code Map

## High-Impact Paths
| Path | What | Why It Matters | Change Risk |
| --- | --- | --- | --- |
| `scripts/read_session.cjs` | Node CLI entry point | All commands route through here | High — must stay in parity with Rust |
| `cli/src/main.rs` | Rust CLI entry point | Clap command definitions, dispatch | High — must stay in parity with Node |
| `cli/src/agents.rs` | Rust session adapters + redaction | Core read/list/search logic | High — output contract changes here |
| `scripts/adapters/*.cjs` | Node session adapters | Per-agent JSONL parsing (codex, claude, gemini, cursor) | Medium — adapter-specific |
| `scripts/adapters/utils.cjs` | Shared Node utilities | Redaction, path normalization, JSON parsing | High — shared across all adapters |
| `cli/src/context_pack.rs` | Rust context-pack commands | Init, seal, verify, build, hooks | Medium — complex but self-contained |
| `scripts/context_pack/*.cjs` | Node context-pack commands | Mirror of Rust context-pack | Medium — must stay in parity |
| `schemas/*.json` | JSON Schema definitions | Output contract for all commands | High — breaking changes affect consumers |
| `fixtures/golden/*.json` | Golden output files | Conformance test baselines | Medium — must update when output changes |
| `PROTOCOL.md` | CLI contract specification | Canonical source of truth for behavior | High — governs both implementations |
| `cli/src/diff.rs` | Session diff logic | LCS-based line comparison | Low — self-contained module |
| `cli/src/messaging.rs` | Agent-to-agent messaging | JSONL message queue | Low — self-contained module |
| `cli/src/relevance.rs` | Relevance introspection | Pattern matching and suggestions | Low — self-contained module |
| `scripts/conformance.sh` | Conformance test runner | Validates Node/Rust parity | Medium — gates all merges |
| `scripts/validate_schemas.sh` | Schema validation runner | Validates output against JSON schemas | Medium — gates all merges |

## Extension Recipe
To add a new agent adapter:
1. Create `scripts/adapters/<agent>.cjs` exporting `readSession()`, `listSessions()`, `searchSessions()`.
2. Add the corresponding Rust adapter in `cli/src/agents.rs` (new match arm in `read_agent()`).
3. Add fixture data in `fixtures/session-store/<agent>/`.
4. Add golden output in `fixtures/golden/read-<agent>.json`.
5. Register the agent name in both CLI argument parsers (Node `SUPPORTED_AGENTS`, Rust `Agent` enum).
6. Update `scripts/conformance.sh` to include the new agent in parity checks.
7. Update `PROTOCOL.md` and `docs/CLI_REFERENCE.md` with the new agent.
