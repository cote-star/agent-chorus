# Behavioral Invariants

## Core Invariants
1. **Node/Rust parity**: For every supported command, Node and Rust must produce identical JSON output given the same inputs. Verified by `scripts/conformance.sh`.
2. **Schema conformance**: All JSON output must validate against the corresponding schema in `schemas/`. Verified by `scripts/validate_schemas.sh`.
3. **Redaction completeness**: All sensitive patterns (API keys, tokens, PEM blocks, Bearer tokens) must be redacted in both implementations using the same pattern set. Verified by `scripts/test_adversarial_redaction.sh`.
4. **Output boundary markers**: Text output must be wrapped in `--- BEGIN CHORUS OUTPUT ---` / `--- END CHORUS OUTPUT ---`. JSON output must include `chorus_output_version: 1`.
5. **Backward-compatible env vars**: `CHORUS_*` env vars are canonical; `BRIDGE_*` fallbacks must continue to work.
6. **Backward-compatible sentinels**: Hook management must detect both `agent-chorus:` and legacy `agent-bridge:` sentinel markers.
7. **Read-only by default**: No command mutates agent session files. Only `send`, `messages --clear`, and context-pack writes modify local state.
8. **Fail-open hooks**: Pre-push hook context-pack errors must never block `git push`.

## Update Checklist Before Merging Behavior Changes
- [ ] Both implementations updated (`scripts/read_session.cjs` + `cli/src/*.rs`)
- [ ] Golden fixtures updated (`fixtures/golden/*.json`)
- [ ] Schema updated if output shape changed (`schemas/*.json`)
- [ ] `PROTOCOL.md` updated if CLI contract changed
- [ ] `docs/CLI_REFERENCE.md` updated with new flags/commands
- [ ] `scripts/conformance.sh` passes
- [ ] `npm run check` passes (conformance + readme + package + schemas)
- [ ] `cargo clippy` clean
