# Code Map

## High-Impact Paths

> **This table is a navigation index, not a complete blast-radius list.** For impact analysis tasks,
> read `30_BEHAVIORAL_INVARIANTS.md` Update Checklist first — it has the full file set per change type.

| Path | What | Why It Matters | Risk | Authority |
| --- | --- | --- | --- | --- |
| `scripts/read_session.cjs` | Node CLI entry point | All commands route through here | Parity break if Rust not updated | authoritative |
| `cli/src/main.rs` | Rust CLI entry point | Clap command definitions, dispatch | Parity break if Node not updated | authoritative |
| `cli/src/agents.rs` | Rust session adapters + redaction. Carries `ReadOptions` struct and `_with_options` variants (v0.13.0) for `--include-user`, `--tool-calls`, `--format` plumbing. | Core read/list/search logic | Silent redaction miss if pattern missing | authoritative |
| `cli/src/summary.rs` | Rust `chorus summary` subcommand (v0.13.0) | Structured session digest; must match Node `read_session.cjs` summary handler byte-for-byte | Parity break regresses conformance | authoritative |
| `cli/src/timeline.rs` | Rust `chorus timeline` subcommand (v0.13.0) | Cross-agent chronological interleave; must match Node implementation | Parity break regresses conformance | authoritative |
| `cli/src/doctor.rs` | Rust `chorus doctor` subcommand (v0.13.0) | Environment + setup diagnostics; must match Node check ordering and JSON shape | Parity break regresses conformance | authoritative |
| `cli/src/setup.rs` | Rust `chorus setup` subcommand (v0.13.0) | Project scaffolding + managed-block injection + Claude Code plugin install | Parity break regresses conformance; file-writing code, touch with care | authoritative |
| `scripts/adapters/*.cjs` | Node session adapters | Per-agent JSONL parsing | Adapter-specific | authoritative |
| `scripts/adapters/utils.cjs` | Shared Node utilities | Redaction, path normalization, JSON parsing | Silent redaction miss | authoritative |
| `cli/src/cursor_cwd.rs` / `scripts/adapters/cursor_cwd.cjs` | Cursor per-session cwd resolution | `.workspace-trusted` → `workspacePath`, else fs-validated demangle (`walk_existing`) | Wrong cwd ⇒ wrong `--cwd` scoping; keep Rust/Node behavior identical | authoritative |
| `cli/src/cursor_parse.rs` / `scripts/adapters/cursor_parse.cjs` | Cursor transcript flatten | `{role,message:{content:[…]}}` → text turns (text segments only) | Parser drift breaks reads; keep parity | authoritative |
| `cli/src/cursor_app.rs` / `scripts/adapters/cursor_app.cjs` (v0.16.0) | Cursor IDE (app) SQLite adapter — reads `~/.cursor/chats/<hash>/<uuid>/store.db`. Rust uses `rusqlite` (bundled); Node uses built-in `node:sqlite` (requires ≥ 22.5; graceful invisible-fallback on older Node). Parses meta hex-JSON, walks protobuf-style root blob chain of child SHAs, decodes claude-shaped message JSON, recovers workspace cwd from "Workspace Path:" header. | Schema diff: see invariant 20 in 30_BEHAVIORAL_INVARIANTS.md. Format may evolve with Cursor IDE updates — keep parity between Rust + Node. | authoritative |
| `cli/src/adapters/hermes.rs` / `scripts/adapters/hermes.cjs` | Hermes adapter (provisional). Claude-like JSONL under `~/.hermes/sessions`. v0.16.0 added live fixture + uniform `--tool-calls` NOT_AVAILABLE warning path; full hermes integration still pending OSS agent-lab. | Format unconfirmed for real hermes; fixture-validated only. Revisit when real hermes available. | provisional |
| `cli/src/agent_context.rs` | Rust agent-context commands | Init, seal, verify, build, hooks | Complex but self-contained | authoritative |
| `scripts/agent_context/*.cjs` | Node agent-context commands (v0.14.0 hardening: `init.cjs`, `seal.cjs`, `verify.cjs`, `rollback.cjs`, `check_freshness.cjs`, `relevance.cjs`, `install_hooks.cjs`, `cp_utils.cjs` all touched by P1–P13) | Mirror of Rust agent-context | Parity break if Rust not updated | authoritative |
| `scripts/agent_context/verify.cjs` | Node verify subcommand | Context pack verification + CI mode; P6 `--enforce-separate-commits`; P13 `last_known_good_sha` promotion | Must stay in parity with Rust | authoritative |
| `scripts/agent_context/rollback.cjs` | Node rollback subcommand | Snapshot restore; P13 `--latest-good` resolves through `history.jsonl` + rotated archives | Must stay in parity with Rust | authoritative |
| `templates/ci-agent-context.yml` | CI template for verify --ci | Defines CI pipeline step for verification | Referenced by verify subcommand | authoritative |
| `templates/relevance.json` | Relevance default patterns (v0.14.0) | Seeded into new packs by `init`; overrideable per repo | Referenced by relevance subsystem | authoritative |
| `templates/settings.agent-context.json` | Default settings template (v0.14.0) | Seeded into new packs by `init`; covers tier, aliases, enforcement toggles | Referenced by init + seal | authoritative |
| `schemas/*.json` | JSON Schema definitions | Output contract for all commands. v0.16.0 adds optional `cwd_mismatch` boolean on read-output and optional `source: "cli"\|"app"` on list-output (cursor only). | Breaking change for consumers | authoritative |
| `fixtures/golden/*.json` | Golden output files | Conformance test baselines. v0.16.0 added: `read-cursor-app.json`, `read-cursor-app-tool-calls.json`, `read-cursor-app-redaction.json`, `read-{claude,codex}-tool-fixture.json`. Cursor app and redaction fixtures sit under `fixtures/session-store/cursor/chats/<hash>/<uuid>/store.db`. | Must update when output changes | derived |
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
| Redaction patterns | `cli/src/agents.rs`, `scripts/adapters/utils.cjs` | Rust: `redact_sensitive_text`, `redact_sensitive_text_with_audit`, helper matcher, tests. Node: `redactSensitiveText`, `redactSensitiveTextWithAudit`. |
| Cursor native read/list/search + cwd scoping (v0.15.0 CLI surface + v0.16.0 app surface merge) | `cli/src/agents.rs`, `scripts/adapters/cursor.cjs`, `cli/src/cursor_app.rs`, `scripts/adapters/cursor_app.cjs` | `read/list/search_cursor_session(s)` merges both surfaces; `Candidate::Cli` / `Candidate::App` enum dispatch; `source: "cli"\|"app"` field on list/search entries; v0.16.0 `cursor_app_base_dir()` → `~/.cursor/chats` |
| Cursor IDE SQLite parsing (v0.16.0) | `cli/src/cursor_app.rs`, `scripts/adapters/cursor_app.cjs` | `parse_root_blob_chain` (protobuf-style varint reader), `read_cursor_app_turns` (claude-shaped message JSON), `cursor_app_session_workspace` (workspace cwd from header), `node:sqlite` graceful require |
| README structure rules | `.agent-context/current/30_BEHAVIORAL_INVARIANTS.md` | "README structure discipline (v0.16.0)" section — progressive disclosure, ~250 line cap, no per-feature deep dives, marketing-shaped tables only |
| v0.16.0 invariants (cursor IDE, --history, cwd_mismatch, doctor severity, read⊆search, --help, checkpoint silence) | `.agent-context/current/30_BEHAVIORAL_INVARIANTS.md` | invariants 20-26 |
| Gemini `.pb` probe + legacy Cursor `state.vscdb` diagnostic | `cli/src/agents.rs` | `detect_gemini_pb_fallback_hint`, `gemini_not_found_message`, `detect_cursor_vscdb_fallback_hint` (secondary only), `cursor_not_found_message` |
| Checkpoint broadcast logic | `cli/src/checkpoint.rs` | `fn run`, `compose_state_message` |
| Read options plumbing (v0.13.0) | `cli/src/agents.rs` | `struct ReadOptions`, `*_with_options` read functions |
| Summary / timeline / doctor / setup parity (v0.13.0) | `cli/src/{summary,timeline,doctor,setup}.rs` | one module per subcommand, dispatched from `main.rs`; new structured Rust subcommands should mention the module-vs-inline decision |
| Context-pack template content | `cli/src/agent_context.rs` | `fn build_template_*` functions |
| Conformance test for a command | `scripts/conformance.sh` | `expect_success "<label>"` calls |

## Cross-Cutting Tracing Flows
- **New CLI command**: `main.rs` Clap enum → `main.rs` dispatch → Rust implementation location (`cli/src/<cmd>.rs` if following the one-module-per-structured-subcommand pattern, or explicit inline-dispatch rationale) → `read_session.cjs` handler → `schemas/<cmd>.json` → `fixtures/golden/<cmd>.json` → `conformance.sh` → `PROTOCOL.md` → `docs/CLI_REFERENCE.md`
- **New redaction pattern**: Rust plain path (`cli/src/agents.rs::redact_sensitive_text`) → Rust audit path (`redact_sensitive_text_with_audit`) → Rust helper/test → Node plain path (`scripts/adapters/utils.cjs::redactSensitiveText`) → Node audit path (`redactSensitiveTextWithAudit`) → adversarial fixture/test → `PROTOCOL.md` if contract text changes.
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
