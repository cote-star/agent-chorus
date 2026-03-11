# Release Notes

## v0.7.0 (2026-02-15)

### Highlights
- Context-pack v2: agent-driven content model with `init` → agent fill → `seal` workflow.
- Non-intrusive update notifications with `bridge doctor` integration.
- Generic relevance engine with configurable `.agent-context/relevance.json`.
- Advisory-only `sync-main` and fail-open pre-push hooks.
- Shared Node utilities (`cp_utils.cjs`) with symlink protection and stale lock recovery.
- Pure Rust timestamp generation and `globset`-based pattern matching.
- CLI smoke test suite (`scripts/test_smoke.sh`).

### Added
- `bridge context-pack init` — scaffolds template files, `GUIDE.md`, and `relevance.json`.
- `bridge context-pack seal` — validates canonical files, generates manifest, snapshot, and history.
- `scripts/update_check.cjs` + `cli/src/update_check.rs` — once-per-version update banner on stderr.
- `scripts/context_pack/relevance.cjs` + `cli/src/relevance.rs` — configurable include/exclude relevance matcher.
- `scripts/context_pack/cp_utils.cjs` — shared utilities (symlink checks, atomic writes, stale lock recovery).
- `scripts/test_smoke.sh` — CLI smoke tests for `doctor`, `init`, `seal`, `build`.
- `bridge doctor` now reports context pack state (`UNINITIALIZED`, `TEMPLATE`, `SEALED_VALID`, `SEALED_STALE`) and update status.
- `BRIDGE_SKIP_UPDATE_CHECK=1` environment variable to disable update checks.

### Changed
- `bridge context-pack build` is now a backward-compatible wrapper that routes to `init` or `seal` based on pack state.
- `bridge context-pack sync-main` is advisory-only — prints a warning, never auto-builds.
- Pre-push hook is fail-open — context-pack errors never block push.
- `bridge setup --context-pack` runs `init` + `install-hooks` instead of `build`.
- Relevance detection uses configurable `.agent-context/relevance.json` instead of hardcoded paths.
- Rust `now_stamp()` uses pure `SystemTime` calculation instead of shelling out to `date`.
- Rust pattern matching uses `globset` crate for proper `**` glob support.
- All documentation updated from "build generates content" to "agent authors + seal finalizes".

### Fixes
- Fixed `collectMatchingFiles.search` crash in `bridge doctor` and legacy `build`.
- Fixed `--pack-dir` flag extraction bug in `build.cjs`.
- Fixed `--cwd` passthrough in `build.cjs` subprocess calls.
- Added stale lockfile recovery (Node + Rust) for interrupted `seal` operations.
- Added symlink protection for all context-pack file writes.
- Gated unused Rust content-generator functions with `#[allow(dead_code)]` and doc comments.
- Reduced clippy warnings from 21 to ≤5.

### Upgrade Notes
- `bridge context-pack build` continues to work — no breaking changes for existing automation.
- New recommended workflow: `init` → agent fills content → `seal`.
- The `--changed-file` flag on `build` is deprecated (accepted with warning, will be removed in next major).

## v0.6.2 (2026-02-11)

### Highlights
- Adds launch-readiness README sections and metadata updates ahead of promotion.
- Aligns package metadata across npm and crates.io for consistent discoverability.
- Clarifies protocol-reference wording in setup intents.

### Changed
- README now includes the social star badge, "How It Compares" matrix, and an expanded roadmap section.
- README roadmap now includes planned non-intrusive update notifications with `bridge doctor` status visibility.
- README now includes a "Visibility Without Orchestration" section, a Claude->Codex handoff visual, and explicit current-boundary notes aligned with roadmap status.
- crates.io keywords in `cli/Cargo.toml` now align with launch messaging (`agent-bridge`, `multi-agent`, `cross-agent`, `context-engineering`).
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
- `documentation = "https://docs.rs/agent-bridge"` in `cli/Cargo.toml`.
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
- Adds Node and Rust parity for `bridge context-pack` commands.
- Finalizes docs and demo coverage so new users can adopt context-pack safely.

### Added
- `bridge context-pack build|sync-main|install-hooks|rollback|check-freshness`.
- `bridge setup --context-pack` bootstrap workflow.
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
bridge setup --context-pack
# or
bridge context-pack build
bridge context-pack install-hooks
```
