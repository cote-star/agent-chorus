# Behavioral Invariants

## Core Invariants
1. **Node/Rust parity**: For every supported command, Node and Rust must produce identical JSON output given the same inputs. Verified by `scripts/conformance.sh`.
2. **Schema conformance**: All JSON output must validate against the corresponding schema in `schemas/`. Verified by `scripts/validate_schemas.sh`.
3. **Redaction completeness**: All sensitive patterns (API keys, tokens, PEM blocks, Bearer tokens) must be redacted in both implementations using the same pattern set.
4. **Output boundary markers**: Text output must be wrapped in `--- BEGIN CHORUS OUTPUT ---` / `--- END CHORUS OUTPUT ---`. JSON output must include `chorus_output_version: 1`.
5. **Backward-compatible env vars**: `CHORUS_*` env vars are canonical; `BRIDGE_*` fallbacks must continue to work.
6. **Backward-compatible sentinels**: Hook management must detect both `agent-chorus:` and legacy `agent-bridge:` sentinel markers.
7. **Read-only by default**: No command mutates agent session files. Only `send`, `messages --clear`, `checkpoint`, and context-pack writes modify local state.
8. **Fail-open hooks**: Pre-push hook context-pack errors must never block `git push`.
9. **Context-pack backward compatibility**: Markdown-only packs (no structured artifacts) must remain fully functional. Structured validation is opt-in based on `routes.json` presence.
10. **Checkpoint guard**: `chorus checkpoint` must exit 0 silently when `.agent-chorus/` does not exist in the target cwd. This is what makes `scripts/hooks/chorus-session-end.sh` safe to install globally — it no-ops on non-chorus projects.
11. **Fallback-hint specificity**: The Gemini and Cursor `NOT_FOUND` enrichments only fire when their specific fallback file types are detected (`.pb` under `<profile>/conversations/` for Gemini, `state.vscdb` under `User/workspaceStorage/` for Cursor). Absent those files, the generic `NOT_FOUND` message must be preserved unchanged — the probes are additive.
12. **Release gating**: Every `v*` tag push runs `scripts/release/verify_versions.sh`, which enforces `package.json.version === cli/Cargo.toml.version === tag[1:]` before any publish step executes. A mismatch fails the workflow before npm, crates.io, or GitHub Release jobs begin.
13. **Byte-identical cross-runtime JSON (v0.13.0)**: Node and Rust CLIs MUST emit byte-identical JSON for the same inputs (after deterministic scrubbing of volatile fields like absolute `source` paths and wall-clock timestamps). `scripts/conformance.sh` is the gating check; it runs in CI via `release.yml` → `verify` job and on every push/PR via `ci.yml`. Any output-shape change requires updating both runtimes, regenerating goldens, and passing conformance in the same PR.
14. **Golden fixture + conformance test required for new subcommands (v0.13.0)**: Any new subcommand or flag that produces structured output must ship a golden fixture under `fixtures/golden/` and a matching case in `scripts/conformance.sh` in the same change that adds the code. No golden = no merge. Rust `cargo test` covers the Rust side in isolation; conformance covers the cross-runtime contract.
15. **CI decoupling of registry publishes from GitHub Release (v0.13.0)**: In `.github/workflows/release.yml`, `package-node` and `package-rust` are siblings that both gate on `verify`; neither depends on the other, and `publish-crate` is not in either chain. `create-release` runs on `if: always() && needs.verify.result == 'success'` so a stale registry token or transient publisher failure cannot silently skip the GitHub Release + attached binaries. This invariant is what makes the `NPM_TOKEN` rotation gate a soft failure instead of a hard one.
16. **Session-start freshness gate (v0.14.0)**: Agents consuming a pack MUST compare `manifest.json` `head_sha_at_seal` against `git rev-parse HEAD` at session start, before any reasoning. When they diverge, the agent MUST warn the user explicitly. The `init` flow injects this instruction as the first line of the routing block in `CLAUDE.md` / `AGENTS.md` / `GEMINI.md`; removing or weakening that preamble is a contract break.
17. **Pack integrity check on every seal (v0.14.0)**: `seal` runs pack integrity validation (schema version, checksum recomputation, structural verify, alias-resolved file set) before committing the new snapshot. A seal that fails integrity must leave the previous snapshot untouched — P10's staging-dir + `rename` commit is what guarantees this atomicity.
18. **`--enforce-separate-commits` is off by default (v0.14.0)**: `chorus agent-context verify --ci` does NOT reject mixed `.agent-context/**` + code commits unless `--enforce-separate-commits` is explicitly passed. Teams opt in; the default behavior remains permissive so existing PR workflows don't break on upgrade.
19. **Seal is crash-safe (v0.14.0, P10)**: Seal writes through a staging directory and commits via `rename`. A stale lockfile from an interrupted seal MUST auto-recover on the next seal (not require manual cleanup). Concurrent `verify` runs MUST NOT race against a mid-flight seal — verify reads the committed snapshot, not the staging dir.

## Update Checklist Before Merging Behavior Changes

| Change type | Files that must change together |
| --- | --- |
| New CLI command | `cli/src/main.rs`, `scripts/read_session.cjs`, `schemas/<cmd>.json`, `fixtures/golden/<cmd>.json`, `scripts/conformance.sh`, `PROTOCOL.md`, `docs/CLI_REFERENCE.md` |
| New CLI flag on existing command | `cli/src/main.rs` (Clap struct), `scripts/read_session.cjs` (arg parse), `schemas/<cmd>.json` (if output changes), `fixtures/golden/<cmd>.json` (if output changes), `docs/CLI_REFERENCE.md` |
| New agent adapter | `cli/src/agents.rs` (Agent enum + match arm), `scripts/adapters/<agent>.cjs`, `fixtures/session-store/<agent>/`, `fixtures/golden/read-<agent>.json`, `scripts/conformance.sh`, `PROTOCOL.md` |
| Output format change | `cli/src/agents.rs` or relevant module, `scripts/read_session.cjs` or relevant adapter, `schemas/<cmd>.json`, ALL `fixtures/golden/*.json` that cover the changed output |
| New redaction pattern | `cli/src/agents.rs` (`redact_sensitive_text`), `scripts/adapters/utils.cjs` (`redactSecrets`). Silent failure if one is missed — secret passes through in output. |
| New context-pack artifact | `cli/src/agent_context.rs` (build function + init list + seal validation), `scripts/agent_context/init.cjs` (template function + outputs array), `scripts/agent_context/seal.cjs` (validation), `scripts/test_context_pack.sh` |
| Context-pack template change | `cli/src/agent_context.rs` (Rust template), `scripts/agent_context/init.cjs` (Node template). Must change both — parity tested by `test_context_pack.sh`. |
| New agent-context subcommand | `cli/src/main.rs` (Clap subcommand enum), `scripts/read_session.cjs` (command dispatch), `scripts/agent_context/<sub>.cjs` (Node implementation), `scripts/test_context_pack.sh` (integration tests), `docs/CLI_REFERENCE.md` |
| Skill definition change | `skills/agent-context/SKILL.md`. Update `wip/context-pack-skill/evolution/` log with rationale. |

## File Families
- `scripts/adapters/*.cjs` (5 files) — one per agent. Report as family when discussing adapter-layer changes. Inspect one representative.
- `scripts/agent_context/*.cjs` (11 files) — Node agent-context commands. Report as family. Must stay in parity with `cli/src/agent_context.rs`.
- `fixtures/golden/*.json` (14 files) — conformance baselines. Report as family. Derived — regenerated by running conformance with `--update`.
- `fixtures/session-store/` (multiple dirs) — test fixture data. Report as family per agent.
- `schemas/*.json` (6 files) — output schemas. Report individually when a specific schema changes, as family when discussing "all schemas".
- `.agent-context/current/*.json` (4 files) — structured artifacts. Report individually (routes, completeness, reporting, search_scope).

## Often Reviewed But Not Always Required
- `README.md` — update for new features but not for internal refactors.
- `docs/CLI_REFERENCE.md` — update for command/flag changes but not for implementation changes.
- `PROTOCOL.md` — update only when the CLI contract changes.
- `research/*.md` — research artifacts, not part of the product.

## Negative Guidance
- Do not enumerate `fixtures/golden/*.json` individually for impact analysis — they are derived baselines, reported as a family.
- Do not enumerate `scripts/adapters/*.cjs` individually unless the change is adapter-specific.
- Do not open `fixtures/session-store/` data files to understand code — they are test inputs, not documentation.
- Do not modify `fixtures/golden/*.json` by hand — run conformance to regenerate them.
- Do not assume a change to `cli/src/agent_context.rs` is complete without also checking `scripts/agent_context/init.cjs` and `seal.cjs`.
