# Code Map

## High-Impact Paths

> **This table is a navigation index, not a complete blast-radius list.** For impact analysis tasks,
> read `30_BEHAVIORAL_INVARIANTS.md` Update Checklist first — it has the full file set per change type.

| Path | What | Why It Matters | Risk | Authority |
| --- | --- | --- | --- | --- |
| `scripts/read_session.cjs` | Node CLI entry point | All commands route through here | Parity break if Rust not updated | authoritative |
| `cli/src/main.rs` | Rust CLI entry point | Clap command definitions, dispatch | Parity break if Node not updated | authoritative |
| `cli/src/agents.rs` | Rust session adapters + redaction | Core read/list/search logic | Silent redaction miss if pattern missing | authoritative |
| `scripts/adapters/*.cjs` | Node session adapters | Per-agent JSONL parsing | Adapter-specific | authoritative |
| `scripts/adapters/utils.cjs` | Shared Node utilities | Redaction, path normalization, JSON parsing | Silent redaction miss | authoritative |
| `cli/src/agent_context.rs` | Rust agent-context commands | Init, seal, verify, build, hooks | Complex but self-contained | authoritative |
| `scripts/agent_context/*.cjs` | Node agent-context commands | Mirror of Rust agent-context | Parity break if Rust not updated | authoritative |
| `scripts/agent_context/verify.cjs` | Node verify subcommand | Context pack verification + CI mode | Must stay in parity with Rust | authoritative |
| `templates/ci-agent-context.yml` | CI template for verify --ci | Defines CI pipeline step for verification | Referenced by verify subcommand | authoritative |
| `schemas/*.json` | JSON Schema definitions | Output contract for all commands | Breaking change for consumers | authoritative |
| `fixtures/golden/*.json` | Golden output files | Conformance test baselines | Must update when output changes | derived |
| `skills/agent-context/SKILL.md` | Agent-context creation skill | Three-flow skill definition (create/update/catchup) | Governs how agents create and maintain packs | authoritative |
| `tests/behaviour/` | Agent behaviour experiments | Experiment protocol, ground truth, result schema | Validates context pack effectiveness | reference |
| `PROTOCOL.md` | CLI contract specification | Canonical source of truth for behavior | Governs both implementations | authoritative |
| `cli/src/diff.rs` | Session diff logic | LCS-based line comparison | Self-contained | authoritative |
| `cli/src/messaging.rs` | Agent-to-agent messaging | JSONL message queue (`send_message`) — reused by checkpoint | Self-contained | authoritative |
| `cli/src/checkpoint.rs` | `chorus checkpoint` broadcast (v0.12.0) | Composes git state + fans out via `send_message`; guards on `.agent-chorus/` | Self-contained; parity lives in `read_session.cjs` | authoritative |
| `cli/src/relevance.rs` | Relevance introspection | Pattern matching and suggestions | Self-contained | authoritative |
| `scripts/hooks/chorus-session-end.sh` | Claude Code `SessionEnd` hook wrapper (v0.12.0) | Thin shell around `chorus checkpoint --from claude`; backgrounded + `disown` | Hardening of env and timeouts matters — do not inline the message composition | authoritative |
| `scripts/hooks/README.md` | Hook directory docs | Install snippet and security notes | reference |
| `scripts/conformance.sh` | Conformance test runner | Validates Node/Rust parity | Gates all merges | reference |
| `scripts/test_context_pack.sh` | Context-pack test runner | Validates init/seal/parity | Gates all merges | reference |

## Quick Lookup Shortcuts
| I need to find... | Open this file | Look for |
| --- | --- | --- |
| CLI command definition | `cli/src/main.rs` | `#[derive(Subcommand)]` enum |
| Node command handler | `scripts/read_session.cjs` | `case '<command>':` in the switch |
| Output schema for a command | `schemas/<command>.json` | JSON Schema root |
| Redaction patterns | `cli/src/agents.rs` | `fn redact_sensitive_text` |
| Gemini `.pb` / Cursor `state.vscdb` probes | `cli/src/agents.rs` | `detect_gemini_pb_fallback_hint`, `detect_cursor_vscdb_fallback_hint`, `gemini_not_found_message`, `cursor_not_found_message`, `gemini_base_dir`, `cursor_base_dir` |
| Checkpoint broadcast logic | `cli/src/checkpoint.rs` | `fn run`, `compose_state_message` |
| Context-pack template content | `cli/src/agent_context.rs` | `fn build_template_*` functions |
| Conformance test for a command | `scripts/conformance.sh` | `expect_success "<label>"` calls |

## Cross-Cutting Tracing Flows
- **New CLI command**: `main.rs` Clap enum → `main.rs` dispatch → `agents.rs` or new module → `read_session.cjs` handler → `schemas/<cmd>.json` → `fixtures/golden/<cmd>.json` → `conformance.sh` → `PROTOCOL.md` → `docs/CLI_REFERENCE.md`
- **New agent adapter**: `agents.rs` Agent enum + match arm → `scripts/adapters/<agent>.cjs` → `fixtures/session-store/<agent>/` → `fixtures/golden/read-<agent>.json` → `conformance.sh` → `PROTOCOL.md`
- **New context-pack artifact**: `agent_context.rs` build function + init list → `scripts/agent_context/init.cjs` template function + outputs array → `scripts/agent_context/seal.cjs` validation → `scripts/test_context_pack.sh` test

## Minimum Sufficient Evidence
- **Lookup**: authoritative source file + exact value.
- **Impact analysis**: update checklist from BEHAVIORAL_INVARIANTS + confirmation both implementations covered.
- **Planning**: files to create/modify + commands + validation criteria + parity check.
- **Diagnosis**: runtime path in SYSTEM_OVERVIEW + code location + confirmation method.

## Extension Recipe
To add a new agent adapter:
1. Create `scripts/adapters/<agent>.cjs` exporting `readSession()`, `listSessions()`, `searchSessions()`.
2. Add the corresponding Rust adapter in `cli/src/agents.rs` (new match arm in `read_agent()`).
3. Add fixture data in `fixtures/session-store/<agent>/`.
4. Add golden output in `fixtures/golden/read-<agent>.json`.
5. Register the agent name in both CLI argument parsers (Node `SUPPORTED_AGENTS`, Rust `Agent` enum).
6. Update `scripts/conformance.sh` to include the new agent in parity checks.
7. Update `PROTOCOL.md` and `docs/CLI_REFERENCE.md` with the new agent.
