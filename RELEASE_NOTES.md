# Release Notes

## v0.8.0 — 2026-03-20

### Added
- `chorus setup` now auto-appends `.agent-chorus/` to `.gitignore`
- `chorus setup` auto-installs the Agent Chorus Claude Code skill plugin if `claude` CLI is present
- `chorus doctor` now checks Claude Code plugin installation status
- `chorus teardown` now removes `.agent-chorus/` from `.gitignore`
- New Claude Code plugin system: `.claude-plugin/plugin.json`, `.claude-plugin/marketplace.json`, `skills/agent-chorus/SKILL.md`
- Package is now a self-describing marketplace — `claude plugin marketplace add <package-root>` works out of the box

### Changed
- PROTOCOL.md: split CLI contract into dual-parity and Node-only admin sections; rules 15–17 updated
- CLI_REFERENCE.md: added full Setup, Doctor, and Teardown operation tables
- Skill file updated from thin redirect to full trigger/intent/command reference

### Fixes
- Rust `teardown` now removes `.agent-chorus/` from `.gitignore` (parity with Node)
- Node `isSystemDirectory` now allows macOS temp dirs (`/var/folders/`) matching Rust parity

### Upgrade Notes
- Run `chorus setup --force` to pick up gitignore auto-management and plugin auto-install in existing projects
- To install the Claude Code plugin manually: `claude plugin marketplace add $(npm root -g)/agent-chorus && claude plugin install agent-chorus`

## v0.7.0 (2026-03-17)

### Highlights
- **Renamed**: `agent-bridge` / `bridge` → `agent-chorus` / `chorus`. All env vars, sentinels, output markers, and docs updated with backward-compatible fallbacks.
- **Full Node/Rust parity**: Conformance suite passes 14/14 tests. Both implementations produce identical JSON output for read, compare, report, list, search, and diff.
- **Jaccard-based comparison**: `chorus compare` now uses topic extraction + stop-word filtering + Jaccard similarity for pairwise agent comparison, replacing exact-match hashing. Tiered findings: >60% aligned (P3), >30% partial (P2), ≤30% divergent (P1).
- **Assistant-only search**: `chorus search` now indexes only assistant/model messages instead of raw content. Results include a `match_snippet` field with a ~120-character context window.
- **Teardown command** (new): `chorus teardown` removes managed blocks, scaffolding directory, and hook sentinels with `--dry-run` and `--global` support.
- **Relevance Introspection** (new): `chorus relevance --list | --test <path> | --suggest` — inspect and test context-pack filtering patterns.
- **Redaction Audit Trail** (new): `chorus read --audit-redactions` — shows what was redacted and why.
- **Session Diff** (new): `chorus diff --agent X --from id1 --to id2` — line-level diff between two sessions with unchanged-line collapsing.
- **Agent-to-Agent Messaging** (new): `chorus send` and `chorus messages` — simple JSONL message queue between agents with agent name validation.
- **Context-pack v2**: agent-driven content model with `init` → agent fill → `seal` workflow, manifest integrity via `verify`, and configurable relevance engine.
- **Security hardening**: trust model documentation, output boundary markers, `--metadata-only` flag, system directory guards, concurrent-read safety, and adversarial redaction test suite.

### Added
- `chorus teardown [--cwd] [--dry-run] [--global] [--json]` — clean removal of Agent Chorus integration from a project.
- `chorus compare --last N` — control how many messages to read from each source (default 10).
- `match_snippet` field in `chorus search --json` output — shows context around the first search hit.
- `detail` field in coordinator report findings — shows pairwise similarity breakdown.
- `equal_lines` field in `chorus diff --json` output — count of unchanged lines.
- Jaccard similarity with 62-word stop-word list for topic-based comparison.
- Hierarchical CWD matching — session CWD can be an ancestor or descendant of the expected path.
- Agent name validation in messaging commands (`send`, `messages`, `clear`) with flag-specific error context.
- Discriminated error messages in update check (404 vs HTTP errors vs timeout vs transport).
- Human-mode formatted tables for `chorus list` and `chorus search` (Rust) — column headers, CWD truncation, result counts, match snippet display.
- `chorus relevance --list` — show current include/exclude patterns and their source.
- `chorus relevance --test <path>` — test whether a file path is relevant and which pattern matched.
- `chorus relevance --suggest` — suggest patterns based on detected project conventions.
- `chorus read --audit-redactions` — include redaction audit trail (pattern names and counts) in output.
- `chorus diff --agent X --from id1 --to id2` — compare two sessions with line-level diff output.
- `chorus send --from X --to Y --message "..."` — send a message from one agent to another.
- `chorus messages --agent X [--clear] [--json]` — read (and optionally clear) messages for an agent.
- `schemas/message.schema.json` — JSON Schema for agent-to-agent messages.
- `cli/src/diff.rs` — LCS-based line diff module.
- `cli/src/messaging.rs` — JSONL message queue module with millisecond-precision timestamps.
- `cli/src/teardown.rs` — managed block removal, hook sentinel cleanup, `.agent-context/` preservation.
- `chorus context-pack init` — scaffolds template files, `GUIDE.md`, and `relevance.json`.
- `chorus context-pack seal` — validates canonical files, generates manifest, snapshot, and history.
- `chorus context-pack verify` — validates manifest checksums against actual file content.
- `chorus read --metadata-only` — returns session metadata without content (reduces injection surface).
- Output boundary markers: `--- BEGIN/END CHORUS OUTPUT ---` in text mode, `chorus_output_version: 1` in JSON.
- Trust Model section in `PROTOCOL.md` documenting trust boundaries and consuming-agent responsibilities.
- Cross-project session warnings for Cursor (no CWD scoping) and Gemini (multi-directory resolution).
- System directory guards on all adapters (Codex, Claude, Gemini, Cursor) — both Node and Rust.
- Adversarial redaction test suite (`scripts/test_adversarial_redaction.sh` + `fixtures/adversarial/`).
- `scripts/update_check.cjs` + `cli/src/update_check.rs` — once-per-version update banner on stderr.
- `scripts/context_pack/relevance.cjs` + `cli/src/relevance.rs` — configurable include/exclude relevance matcher with introspection API.
- `scripts/context_pack/cp_utils.cjs` — shared utilities (symlink checks, atomic writes, stale lock recovery).
- `scripts/test_smoke.sh` — CLI smoke tests for `doctor`, `init`, `seal`, `build`.
- `chorus doctor` now reports context pack state (`UNINITIALIZED`, `TEMPLATE`, `SEALED_VALID`, `SEALED_STALE`) and update status.
- `CHORUS_SKIP_UPDATE_CHECK=1` environment variable to disable update checks.

### Changed
- **Package renamed**: npm `agent-bridge` → `agent-chorus`, crate `agent-bridge` → `agent-chorus`, binary `bridge` → `chorus`.
- **Environment variables renamed**: `BRIDGE_*` → `CHORUS_*` with backward-compatible `BRIDGE_*` fallback.
- **Sentinel markers renamed**: `agent-bridge:` → `agent-chorus:` with legacy sentinel detection in hook management.
- **Output markers renamed**: `BEGIN/END BRIDGE OUTPUT` → `BEGIN/END CHORUS OUTPUT`, `bridge_output_version` → `chorus_output_version`.
- **Setup directory renamed**: `.agent-bridge/` → `.agent-chorus/`.
- `chorus compare` uses Jaccard similarity instead of exact content hashing for divergence detection.
- `chorus search` filters to assistant-only text before matching (Codex, Claude, Gemini, Cursor).
- `chorus diff` human output now shows `+N added, -N removed, N unchanged` and collapses long equal runs with context windows.
- Removed `--normalize` flag from `chorus compare` (superseded by Jaccard topic comparison).
- `schemas/report.schema.json` — added optional `detail` field to findings.
- `schemas/list-output.schema.json` — added optional `match_snippet` field.
- Removed dead `normalize` field from Rust `ReportRequest` struct and unused `normalize_content` function.
- Hook installation uses sentinel markers (`# --- agent-chorus:pre-push:start/end ---`) to append to existing hooks instead of clobbering.
- `repo_root` field in manifest.json now emits `"."` instead of the absolute path (prevents path leakage).
- SKILL.md consolidated into CLAUDE.md and AGENTS.md; SKILL.md is now a redirect.
- `chorus context-pack build` is now a backward-compatible wrapper that routes to `init` or `seal` based on pack state.
- `chorus context-pack sync-main` is advisory-only — prints a warning, never auto-builds.
- Pre-push hook is fail-open — context-pack errors never block push.
- `chorus setup --context-pack` runs `init` + `install-hooks` instead of `build`.
- Relevance detection uses configurable `.agent-context/relevance.json` instead of hardcoded paths.
- Rust `now_stamp()` uses pure `SystemTime` calculation instead of shelling out to `date`.
- Rust pattern matching uses `globset` crate for proper `**` glob support.
- JSONL reader drops truncated last line for concurrent-read safety (both Node and Rust).
- `read-output.schema.json` now includes `chorus_output_version`, allows nullable `content`, and optional `redactions` array.
- All documentation updated from "build generates content" to "agent authors + seal finalizes".
- All golden fixtures regenerated from Node reference implementation.

### Fixes
- Fixed duplicate `sha256()` and `readJson()` declarations in `seal.cjs` from merge.
- Fixed `collectMatchingFiles.search` crash in `chorus doctor` and legacy `build`.
- Fixed `--pack-dir` flag extraction bug in `build.cjs`.
- Fixed `--cwd` passthrough in `build.cjs` subprocess calls.
- Fixed `isSystemDirectory` rejecting macOS temp dirs under `/var/folders/` (both Node and Rust).
- Added stale lockfile recovery (Node + Rust) for interrupted `seal` operations.
- Added symlink protection for all context-pack file writes.
- Gated unused Rust content-generator functions with `#[allow(dead_code)]` and doc comments.
- Reduced clippy warnings to zero.

### Upgrade Notes
- Install via `npm install -g agent-chorus` (replaces `agent-bridge`).
- Old `BRIDGE_*` environment variables continue to work as fallbacks.
- Old `agent-bridge:` sentinel markers are auto-detected during hook management.
- `chorus context-pack build` continues to work — no breaking changes for existing automation.
- New recommended workflow: `init` → agent fills content → `seal`.
- The `--changed-file` flag on `build` is deprecated (accepted with warning, will be removed in next major).
- The `--normalize` flag on `chorus compare` is removed. Comparison now uses Jaccard similarity by default.
- `chorus teardown --dry-run` is recommended before running teardown to preview what will be removed.
- Consuming agents should treat `chorus read` output as untrusted data — see Trust Model in PROTOCOL.md.

## v0.6.2 (2026-02-11)

### Highlights
- Adds launch-readiness README sections and metadata updates ahead of promotion.
- Aligns package metadata across npm and crates.io for consistent discoverability.
- Clarifies protocol-reference wording in setup intents.

### Changed
- README now includes the social star badge, "How It Compares" matrix, and an expanded roadmap section.
- README roadmap now includes planned non-intrusive update notifications with `chorus doctor` status visibility.
- README now includes a "Visibility Without Orchestration" section, a Claude->Codex handoff visual, and explicit current-boundary notes aligned with roadmap status.
- crates.io keywords in `cli/Cargo.toml` now align with launch messaging (`agent-chorus`, `multi-agent`, `cross-agent`, `context-engineering`).
- Setup intent text now points to the canonical `PROTOCOL.md` URL.

### Upgrade Notes
- No CLI behavior, protocol schema, or command output contract changes.
- Safe patch upgrade focused on docs/metadata and release positioning.

## v0.6.1 (2026-02-11)

### Highlights
- Fixes README media rendering across GitHub, npm, and crates.io by switching demo/image links to absolute GitHub-hosted URLs.
- Hardens release workflow behavior for repeatable reruns and registry-safe publishing.
- Enforces crates.io publish before npm publish in release execution order.

### Changed
- README image references now use absolute `raw.githubusercontent.com` URLs for all demo and architecture assets.
- `Release` workflow now checks npm registry version availability and skips npm publish when that exact version already exists.
- `package-node` now depends on `publish-crate` so crates publish completes first on tag releases.

### Upgrade Notes
- No CLI behavior or schema contract changes.
- Recommended patch upgrade for improved package/readme rendering and release reliability.

## v0.6.0 (2026-02-11)

### Highlights
- Tightens repo positioning around evidence-first multi-agent workflows and cold-start reduction.
- Expands docs coverage for context-pack operations and practical CLI usage recipes.
- Adds GitHub Packages publish path in release workflow while preserving npm/crates publication.

### Changed
- README now includes a concise "Why It Exists" framing (`Silo Tax`, `Cold-Start Tax`, visibility-first layering).
- Context Pack section in README now includes a read-order hero image and an at-a-glance summary of operational behavior.
- `CONTEXT_PACK.md` now documents the layered model, operational guarantees, and explicit non-goals.
- `docs/CLI_REFERENCE.md` now includes common end-to-end recipes (handoff recovery, verification, cold-start onboarding).
- npm metadata keywords were expanded for discoverability (`cold-start`, `orchestration`, `evidence-based`, `context-engineering`).
- Release workflow now includes a GitHub Packages publish job and manual dispatch support.

## v0.5.4 (2026-02-10)

### Highlights
- Final documentation language and clarity pass across repository docs.
- Aligns phrasing around context-pack behavior for private repositories.

### Changed
- Polished wording in README, protocol, context-pack policy, and agent instruction docs for consistency and precision.
- Clarified that pack-first flows open project files as needed and do not require making code public.
- Minor grammar and heading consistency refinements across docs.

### Upgrade Notes
- No CLI behavior, schema, or runtime changes.
- Safe documentation-only patch release.

## v0.5.3 (2026-02-10)

### Highlights
- Clarifies context-pack wording for private-project users.
- Removes "open source files" phrasing that could be misread as requiring public code.

### Changed
- The README context-pack section now explicitly states that private repositories are fully supported without making code public.
- `CLAUDE.md` and `CONTEXT_PACK.md` now use "project files" wording for pack-first deep dives.
- Context-pack policy wording updated to clarify that local-only data is not published in package artifacts.

### Upgrade Notes
- No CLI behavior or output-contract changes.
- Safe documentation-only patch release.

## v0.5.2 (2026-02-10)

### Highlights
- Adds full metadata polish for npm and crates.io publication quality.
- Declares Rust MSRV explicitly so crates metadata shows a known `rust-version`.
- Improves demo maintainability by removing hardcoded package versions from demo text.

### Added
- `rust-version = "1.74"` in `cli/Cargo.toml`.
- `documentation = "https://docs.rs/agent-chorus"` in `cli/Cargo.toml`.
- npm metadata refinements: `preferGlobal`, Node `engines`, and expanded discoverability keywords.

### Changed
- Normalized npm `homepage` to `#readme`.
- Demo scripts and assets remain functionally unchanged, but visual labels no longer hardcode release version text.

### Upgrade Notes
- No runtime CLI behavior changes.
- Safe patch upgrade for both npm and crates users.

## v0.5.1 (2026-02-10)

### Highlights
- Improves demo readability in GitHub README with sharper text rendering.
- Adds a dedicated context-pack demo flow in the same terminal visual style.

### Changed
- Tuned demo recorder defaults for README display (`1080x640`) and explicit high-effort lossless WebP encoding.
- Increased terminal text weight in demo players to improve legibility after scaling.
- Updated context-pack demo layout to two panes for clearer text density.

### Upgrade Notes
- No CLI behavior changes.
- Rebuilt demo assets in `docs/demo-*.webp` and context-pack metadata snapshots.

## v0.5.0 (2026-02-10)

### Highlights
- Promotes context-pack to a first-class release feature for token-efficient, agent-first repo understanding.
- Adds Node and Rust parity for `chorus context-pack` commands.
- Finalizes docs and demo coverage so new users can adopt context-pack safely.

### Added
- `chorus context-pack build|sync-main|install-hooks|rollback|check-freshness`.
- `chorus setup --context-pack` bootstrap workflow.
- Agent instruction flow that prioritizes `.agent-context/current/` for end-to-end repo understanding tasks.

### Changed
- README now has a dedicated Context Pack section describing what it is, why to use it, recommended workflow, main-only sync policy, usage boundaries, and recovery model.
- Added context-pack demo steps and quick setup references in README.

### Fixes
- Reduced context-pack snapshot churn for unchanged builds.
- Improved hook install behavior with explicit `core.hooksPath` override warning.
- Improved freshness checks and CI alignment for context-pack update discipline.

### Upgrade Notes
- No breaking CLI changes for existing `read`, `list`, `search`, `compare`, `report`, `setup`, `doctor`, or `trash-talk` users.
- To enable context-pack automation in an existing repo:

```bash
chorus setup --context-pack
# or
chorus context-pack build
chorus context-pack install-hooks
```
