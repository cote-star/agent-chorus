# Release Notes

## v0.14.0 — 2026-04-21

**Agent-context hardening pass: P1–P13 addressing failure modes F19–F58 across integrity, hostile input, concurrency, schema lifecycle, and authoring ergonomics.**

v0.14.0 ships the thirteen-pass hardening effort planned in `research/agent-context-gaps-plan.md` on top of v0.13.0's full Rust parity. Every pass closes a named set of failure modes from the gap analysis. The Rust CLI and Node adapter receive matching changes; both implementations continue to emit byte-identical outputs where they share contract. The headline behavior change is the **session-start freshness gate**: routing blocks in `CLAUDE.md` / `AGENTS.md` / `GEMINI.md` now carry a mandatory first-line instruction directing agents to compare `head_sha_at_seal` against `git rev-parse HEAD` before any reasoning and warn the user when they diverge.

### Integrity & provenance (P1, P2, P11, P12)

- **P1 — rich manifest + provenance.** `manifest.json` now carries provenance fields (head SHA at seal, seal timestamp, tool versions, tool hashes) and records the authoring/sealing chain so downstream consumers can verify pack origin.
- **P2 — structural verifier.** `chorus agent-context verify` gains a structural pass that validates required sections, cross-file references, and JSON-schema-bound authority files beyond the earlier checksum-only integrity check.
- **P11 — schema version enforcement + install integrity (F34, F36, F37, F38).** Manifest now pins a schema version; `verify` rejects packs whose schema version is unknown to the installed CLI, and `install` performs integrity checks so tampered or partially-installed packs fail fast instead of silently degrading.
- **P12 — trust boundary & pack integrity.** Pack integrity validation runs on every seal. The trust boundary between pack content and agent reasoning is documented and enforced end-to-end.

### Hostile input safety (P8, P9)

- **P8 — hostile input & platform safety (F19–F23).** Seal and verify now harden against hostile pack content: path traversal, symlink escape, oversized files, non-UTF-8 sequences, and platform-specific name collisions are all rejected with clear diagnostics rather than producing corrupt packs.
- **P9 — git edge cases (F24–F28).** Detached HEAD, submodules, worktrees, shallow clones, and grafted histories are all handled explicitly. `build_manifest` records a `detached` flag and `SealOptions` carries `follow_symlinks: false` by default.

### Concurrency & atomicity (P10)

- **P10 — concurrency, atomic writes & recovery (F29–F33, F55).** Seal is now crash-safe: writes go through a staging directory with `rename` commit, stale lockfiles from interrupted seals are detected and recovered, and concurrent `verify` runs no longer race against a mid-flight seal.

### Authoring ergonomics & lifecycle (P13)

- **F46 — Tiered adoption.** `agent-context init --tier <1|2|3>` lets teams scaffold a narrower starting pack. Tier 1 ships `20_CODE_MAP.md` + `routes.json` only; Tier 2 adds `30_BEHAVIORAL_INVARIANTS.md` + `completeness_contract.json`; Tier 3 is the full pack (default, identical to legacy behavior). Seal auto-detects which files are actually present, so a Tier-1/2 pack does not fail the required-files check. Node parity added to `scripts/agent_context/init.cjs`.
- **F50 — Pack-file alias support.** `manifest.json` gains an `aliases` object mapping canonical filenames to on-disk names (e.g. `{"20_CODE_MAP.md": "20_architecture.md"}`). Both `verify` and the Node verifier retry with the aliased filename when the canonical one is missing and surface a `NOTE` in human output so an author can see the alias was consulted. `seal` carries the `aliases` map forward across re-seals.
- **F58 — Last-known-good pointer.** `manifest.json` gains `last_known_good_sha`. `verify --ci` promotes the sealed HEAD into this field on a fully green run. `agent-context rollback --latest-good` resolves the pointer through `history.jsonl` (falling back to rotated archives) and restores the matching snapshot. `--latest-good` and `--snapshot` are mutually exclusive. Node parity added in `scripts/agent_context/rollback.cjs` and `scripts/agent_context/verify.cjs`.
- **F47 — Session-start freshness gate.** The routing blocks `init` upserts into `CLAUDE.md` / `AGENTS.md` / `GEMINI.md` now open with a mandatory first-line instruction: agents must compare `head_sha_at_seal` against `git rev-parse HEAD` before any reasoning and warn the user when they diverge. The Rust and Node `init` flows emit the identical preamble. **This is a behavior change for agents consuming existing packs** — re-run `chorus agent-context init` (or re-seal) after upgrade to pick up the gate.

### Zone-aware freshness & pre-edit awareness (P3, P4, P5)

- **P3 — zone-aware freshness + suggest-patches.** Freshness detection now operates per pack zone (code map, invariants, operations, etc.) instead of a single global-stale signal. `check-freshness` emits targeted suggestion patches naming which sections the diff affects, which P6 hook intelligence consumes.
- **P4 — pre-edit awareness.** Authoring flows now read the pack before editing so that the agent is aware of the invariants it is about to mutate.
- **P5 — count SSOT via handlebars.** Counts quoted in narrative markdown (file counts, invariant counts, etc.) are expanded from the manifest through a single handlebars-style source of truth, eliminating drift between prose and data.

### Subagent reconciliation (P7)

- **P7 — subagent reconciliation diff --since-seal.** `chorus agent-context diff --since-seal` compares a subagent's working tree against the last sealed state so a parent agent can reconcile parallel subagent work without re-reading full sessions.

### Hook intelligence + separate-commit enforcement (P6)

- Pre-push hook now detects pack-only pushes (every path in the push range starts with `.agent-context/`) and skips the freshness cycle with a `pack-only push, skipping freshness check` message. Closes the noise loop where code pushes warn "pack is stale", the agent updates the pack, and the follow-up push re-warns about its own commit.
- Each `chorus agent-context verify` / `check-freshness` warning now writes `.agent-context/current/.last_freshness.json` with `{changed_files, affected_sections, timestamp}`. On a subsequent pack-only push the hook reads this state, checks whether the push touches the section files the prior warning named, and prints `warning appears addressed: sections [X, Y] updated`.
- New opt-in flag `chorus agent-context verify --ci --enforce-separate-commits`. When set, verify inspects `base..HEAD` and fails if any commit mixes `.agent-context/**` with non-pack paths. **Off by default;** the gate is intended for teams that have adopted the "pack edits land as their own commit" convention. See `docs/CLI_REFERENCE.md` for the JSON schema additions (`separate_commits`, `mixed_commits`).

### Fixed

- **Gemini adapter: `.jsonl` files now indexed and readable.** Pre-existing bug where the
  list/scope discovery only picked up `.json`; newer Gemini CLI writes `.jsonl`. Listings now
  include both, and `chorus read --agent gemini --id <session>` now parses `.jsonl`
  line-delimited sessions (header + message lines + `$set` metadata), dedupes streaming-
  duplicate assistant turns on message `id`, and funnels through the same `Session` shape as
  the legacy single-document `.json` path. Rust and Node adapters dispatch on file extension
  so downstream callers remain format-agnostic.
- **Gemini adapter: cwd inference from scope directory.** Listings used to emit `cwd: null`
  for every Gemini session. The scope directory name (e.g. `play`) is now returned as the cwd
  hint, so `chorus read --agent gemini --cwd <X>` filtering works for named scopes.
  Hex-hash scopes still return the hash (lossy; users can set `--chats-dir` to pin).

### Known Limitations

- **Markdown merge conflicts (#11):** Parallel PRs that both edit the same pack markdown file (e.g. `20_CODE_MAP.md`) can conflict on merge. The tooling cannot auto-resolve these. Mitigation: keep pack files organized around stable H2 section headings so edits cluster inside bounded sections and conflicts stay localized. Re-seal after the human conflict resolution.
- **Squash-merge collapses pack commits (#12):** When a PR uses squash merge, the separate pack commit is folded into the squash parent. This is a git workflow decision outside the tooling's authority. Mitigation: teams that squash should land pack updates as their own PR (the team convention documented in `skills/agent-context/SKILL.md`). Teams that merge-commit or rebase-and-merge can keep pack updates in the same PR; `--enforce-separate-commits` is available for those teams to hard-require separate commits.

### Deferred (TODO(P13-continuation))

These items from the P13 plan are **intentionally deferred** and carry a `TODO(P13-continuation)` marker in the plan/code. Tracked for a follow-up P13-continuation package; nothing in this release blocks on them:

- **F48** — `explain-diff` subcommand (new command surface).
- **F49** — Monorepo multi-team mode (structural change).
- **F51** — Canonical routing template (better coordinated via the `team_skills` track).
- **F52** — Scheduled job to re-run acceptance tests.
- **F53** — Cross-file integrity check.
- **F54** — Difficulty floor for acceptance tests.
- **F59** — Cryptographic history chain.
- **F45** — `AUTHORING_TODO.md`.

### Upgrade Notes

- Re-run `chorus agent-context init` (or re-seal) after upgrade so existing packs pick up the session-start freshness gate preamble in `CLAUDE.md` / `AGENTS.md` / `GEMINI.md`.
- `--enforce-separate-commits` is off by default; enable explicitly in CI only if your team has adopted the separate-pack-commit convention.
- Existing packs sealed before v0.14.0 will continue to verify, but new fields (`aliases`, `last_known_good_sha`) are only populated on a re-seal under v0.14.0.

## v0.13.0 — 2026-04-21

**Full Rust parity for the v0.11.0 Node-only surface, plus CI decoupling so a stale registry token no longer silently drops a GitHub Release.**

Closes the parity gap that has lingered since v0.11.0. The Rust CLI now implements `summary`, `timeline`, `doctor`, and `setup` end-to-end, and the existing `read` subcommand gains `--include-user`, `--tool-calls`, and `--format {json|md|markdown}`. Both implementations continue to emit byte-identical JSON for the same inputs; conformance against shared fixtures gates merges.

### Added — Rust parity for v0.11.0 features

- `chorus summary --agent <agent> [--cwd] [--format] [--json]` in Rust — structured session digest (message count, duration estimate, user requests, files referenced, tool-call counts, last-response snippet). Metadata-only, no LLM calls. Matches the Node output shape byte-for-byte.
- `chorus timeline [--agent]... [--cwd] [--limit] [--format] [--json]` in Rust — cross-agent chronological view interleaving sessions. Defaults to all four agents with limit 5 per agent. Sorted by timestamp descending.
- `chorus doctor [--cwd] [--json]` in Rust — environment + setup diagnostic. Reports on version, session directories, scaffolding, managed blocks, session discoverability per agent, context pack state, Claude Code plugin installation, and update status. Each check returns `pass | warn | fail`.
- `chorus setup [--cwd] [--dry-run] [--force] [--agent-context] [--json]` in Rust — project scaffolding, managed-block injection into `CLAUDE.md`/`AGENTS.md`/`GEMINI.md`, `.gitignore` append, and optional Claude Code plugin install. `--dry-run` preview matches Node operation-by-operation.

### Added — `read` flag parity in Rust

- `--include-user` pairs the user prompt(s) that anchor the returned assistant message(s). Designed for live status checks; assistant-only remains the default for narrower handoff reads.
- `--tool-calls` surfaces `[TOOL: <name>]...[/TOOL]` blocks alongside text content (normally stripped during extraction). Metadata includes `"included_tool_calls": true` when active.
- `--format {json|md|markdown}` renders output as JSON or formatted markdown. Rust treats `--format json` as an alias for `--json`. Node has a known bug here where `--format json` falls through to plain-text output (see `scripts/read_session.cjs:1759`); the Node bug is documented and left in place because fixing it is an output-contract change that should roll with a dedicated deprecation.

### Added — golden fixtures + conformance harness for parity regression

- `cargo test --manifest-path cli/Cargo.toml` now runs **52 tests** (29 pre-existing plus 23 new parity tests covering the four subcommands and the three `read` flags).
- New golden fixtures under `fixtures/golden/` seal the v0.13.0 output shapes; `scripts/conformance.sh` diffs them against both Node and Rust on every CI run.
- `scripts/release/generate_goldens.sh` rebuilds the fixture set when output shapes are intentionally bumped.

### Changed — CI decoupling (release.yml)

- `package-node.needs` drops `publish-crate`. npm and crates.io are independent registries; one registry's failure must not cascade into skipping the other. Prior to v0.13.0, a transient crates.io hiccup silently skipped npm publish (this is how v0.12.1 shipped to GitHub + crates.io but not to npm).
- `Publish to npm` step gets `continue-on-error: true`. A stale `NPM_TOKEN` (see Known Limitations below) no longer fails the whole `package-node` job — the tarball is already built and uploaded as a workflow artifact before the npm publish step runs.
- `create-release.needs` drops `publish-crate` and `publish-github-package`. Those jobs don't produce downloadable artifacts for the GitHub Release; only `package-node` (the `.tgz`) and `package-rust` (the two binaries) do. Added `if: always() && needs.verify.result == 'success'` so the Release still ships even if a sibling publish job fails.
- Net effect: from v0.13.0 onwards, a stale registry token or a transient registry hiccup leaves the GitHub Release + attached binaries intact. The failing publisher is still visible as a red job on the workflow run page.

### Known Limitations

- **Gemini + Cursor `--tool-calls` is a no-op.** Those adapters do not parse a tool-call schema from their underlying stores, so the flag runs without error but returns no `[TOOL: ...]` blocks. This matches the Node behavior — it is a missing-capability in the adapter layer, not a Rust-specific gap. Tracked for a later release.
- **`NPM_TOKEN` rotation.** The automated npm publish step degrades gracefully (see CI decoupling above), but the token still needs to be rotated at https://github.com/cote-star/agent-chorus/settings/secrets/actions before the npm publish step can succeed on its own. Until rotation, the manual workaround is `npm-play publish --confirm-publish` from a worktree rooted under `~/sandbox/play`.

## v0.12.2 — 2026-04-20

**Docs + pack-freshness release. Zero code-behavior changes.**

A holistic cleanup on top of v0.12.1 to match every surface of the repo to the actual v0.12.x feature set. The rapid v0.12.0 and v0.12.1 releases landed the new code but left stale traces in secondary surfaces; this release retires them.

### Refreshed — the repo's own `.agent-context/` pack

Was sealed 2026-04-08 at commit `1487f29`; 15 commits / 12 days behind `main`. Its `00_START_HERE.md` still claimed "Version: 0.9.1" and `10_SYSTEM_OVERVIEW.md` predated `chorus checkpoint`. Now re-sealed at the v0.12.2 HEAD with:

- Product version updated in `00_START_HERE.md`
- `chorus checkpoint`, `chorus summary`, `chorus timeline`, `--tool-calls`, `--format markdown`, `--include-user` documented in `10_SYSTEM_OVERVIEW.md`
- `cli/src/checkpoint.rs`, `scripts/hooks/chorus-session-end.sh`, and the new Gemini/Cursor fallback helpers (`detect_gemini_pb_fallback_hint`, `detect_cursor_vscdb_fallback_hint`, etc.) added to `20_CODE_MAP.md`
- Three new invariants in `30_BEHAVIORAL_INVARIANTS.md` covering the `chorus checkpoint` `.agent-chorus/` guard, Gemini/Cursor fallback-hint specificity, and the `verify_versions.sh` release gate
- Automated GitHub Release flow + `NPM_TOKEN` rotation caveat documented in `40_OPERATIONS_AND_RELEASE.md`
- `search_scope.json` extended with new probe helpers

### Fixed — stale roadmap + protocol version claims

- `README.md` lines 253 + 446: "Rust parity planned for v0.12.0" → "still pending". v0.12.0 shipped session handoff, not Rust parity for the v0.11.0 Node features; the old claim was literally false.
- `docs/CLI_REFERENCE.md` line 788: same fix.
- `PROTOCOL.md` header: `v0.8.1` → `v0.12.2` (four minor versions behind).

### Archived — research + WIP artifacts for shipped work

- `research/handoff-2026-03-{25,26,26-evening}.md` → `research/archive/2026-Q1/` (historical session handoff notes).
- `wip/agent-context-skill/` → `research/archive/skill-development-log/` (skill shipped in v0.12.0 as `skills/agent-context/`; the WIP tree was now mis-labeled). New `wip/README.md` makes the convention explicit.
- New `research/archive/README.md` explains the archive convention and the split between active research docs and historical ones.

### Normalized — script help wording

- `scripts/agent_context/check_freshness.sh`: comment now names `agent-context` (primary) instead of `context-pack` (deprecated alias, still shipped for back-compat until v1.0.0).
- `scripts/test_smoke.sh`: added a clarifying comment so the intentional `context-pack` alias exercise is not mistaken for a drift-we-should-fix.
- Node-facing `--context-pack` flag on `chorus setup` and the hook-sentinel marker names remain untouched (removing them is breaking; deferred to v1.0.0 per `research/rename-progress.md`).

### Also landed

- GitHub Actions `package-rust` step uploads binaries from the correct path (fix shipped in v0.12.1). v0.12.2 will be the second release to surface those binaries on the GitHub Releases page automatically.

## v0.12.1 — 2026-04-20

Infrastructure fixes and supply-chain hygiene on top of v0.12.0. No user-facing behavior changes.

### Fixed — `release.yml` binary upload path

`package-rust` was uploading from `target/release/chorus` but the build step uses `cargo build --manifest-path cli/Cargo.toml --release`, which produces binaries at `cli/target/release/chorus`. Every release since v0.9.1 silently uploaded empty artifacts and the `create-release` job (landed in v0.11.0-era release-automation work) therefore had nothing to attach. v0.12.1 is the first release where the Rust binaries ship on the GitHub Release page automatically for both linux-x64 and macos-arm64.

### Fixed — `basic-ftp` advisory (GHSA-6v7q-wjvx-w8wg / -chqc-8p9q-pq6q / -rp42-5vxx-qpwr)

`npm audit` flagged three high-severity CVEs on `basic-ftp <=5.2.2`, a transitive devDependency via `puppeteer`. The vulnerability affects demo-recording tooling only — it is NOT shipped in the published npm tarball — but refreshing `package-lock.json` via `npm audit fix` removes it from contributor installs. Audit is clean on v0.12.1: `found 0 vulnerabilities`.

### Also in this release

- Branch protection on `main` is now enabled with force-push denied. No direct pushes to the released branch; every change must go through a reviewed PR.

## v0.12.0 — 2026-04-20

Closes [#8](https://github.com/cote-star/agent-chorus/issues/8). Ships the session handoff protocol, interruption-resilience hook, and better error messages for two opaque-storage cases that previously returned bare `NOT_FOUND`.

### Added — `chorus checkpoint` subcommand

A first-class state-broadcast command that sends current git state (branch, uncommitted file count, last commit hash + subject) to every other agent's inbox in one call. Works on any OS, fully tested, idempotent, guards on `.agent-chorus/` presence so it is safe to call unconditionally in hooks.

```bash
chorus checkpoint --from claude
chorus checkpoint --from codex --message "Payment refactor half-done; types still broken" --json
```

Replaces the pattern of calling `chorus send` three times when you just want everyone to know where you left off. `chorus send` is still the right tool for targeted messages; `checkpoint` is a loud hello/goodbye for the whole room.

### Added — `scripts/hooks/chorus-session-end.sh`

Thin shell wrapper around `chorus checkpoint` designed for Claude Code's `SessionEnd` hook so agents broadcast their state on any exit — clean, crash, or closed window. Install via `~/.claude/settings.json`:

```json
{
  "hooks": {
    "SessionEnd": [{
      "hooks": [{ "type": "command", "command": "bash /path/to/scripts/hooks/chorus-session-end.sh", "timeout": 10 }]
    }]
  }
}
```

Hardened with `set -euo pipefail`, `realpath` canonicalization of `$CLAUDE_PROJECT_DIR` (defeats env-var path traversal), and a backgrounded+`disown`ed dispatch so a hanging `chorus` binary cannot pin the CLI exit past the settings-json timeout. Gracefully no-ops when chorus is missing from PATH or when `.agent-chorus/` is not present.

### Added — Session Handoff Protocol in all three provider files

`CLAUDE.md`, `AGENTS.md`, and a fully rewritten `GEMINI.md` (10 → 137 lines, previously a context-pack stub) now carry an explicit **Session Handoff Protocol** section with standup / conclude / checkpoint rituals. The WHEN is spelled out, not just the WHAT. Codex and Gemini do not have Claude Code's `SessionEnd` hook — those rituals instruct the agent to call `chorus checkpoint` manually at task-block boundaries.

### Added — `docs/session-handoff-guide.md`

New standalone guide that walks through five scenarios end-to-end: clean handoff, interrupted handoff (Claude Code), mid-task checkpoint for agents without a hook system, Gemini protobuf fallback, and Cursor SQLite fallback. Linked from the three provider files and the CLI reference.

### Improved — Gemini `NOT_FOUND` detects `.pb` files (F40)

`chorus read --agent gemini` now probes `~/.gemini/<profile>/conversations/*.pb` when the JSONL search comes up empty. If protobuf files are present, the error message names the count, names the exact path, explains that Chorus does not parse the format yet, and points at `--chats-dir` plus the new guide. Verified against a live install with 4+ `.pb` files at `~/.gemini/antigravity/conversations/`.

### Improved — Cursor `NOT_FOUND` detects `state.vscdb` files

Mirror of the Gemini change for Cursor. Modern Cursor persists chat and composer data in SQLite `state.vscdb` files under `User/workspaceStorage/<workspace-id>/`; chorus's cursor reader currently only scans JSON/JSONL by filename. When the SQLite backend is in use the error now names the count, the `workspaceStorage/` path, and points at the guide. Verified against a live install with 8 `state.vscdb` files. Full `rusqlite`-backed reading is tracked as a follow-up.

### Also in this release

- **GitHub Releases are now automated.** Tag pushes on `v*` trigger `softprops/action-gh-release@v2` to create the GitHub Release with the matching `RELEASE_NOTES.md` section as the body and the built Rust binaries attached. Closed a gap where tags v0.8.0–v0.10.0 existed on the remote and had been published to npm / crates.io / GitHub Packages, but the GitHub Releases page still showed v0.7.0 because the workflow never created releases explicitly.
- **`RELEASE_NOTES.md`** backfilled with entries for v0.9.0 (three-layer context pack + `search_scope.json`) and v0.9.1 (P16 imperative routing enforcement). Both versions existed as tags and npm/crates releases; their notes were previously missing.

### Thanks

Thanks to [@oloflun](https://github.com/oloflun) for the detailed report in issue #8 — the writeup identified four distinct gaps cleanly and made the shape of the fix obvious.

---

## v0.11.0 — 2026-04-13

### Added — `--tool-calls` flag on `chorus read`

Surfaces tool call content (Read, Edit, Bash, Write, etc.) that was previously stripped during extraction. When `--tool-calls` is passed, assistant messages include `[TOOL: <name>]...[/TOOL]` blocks alongside text content.

- New extraction functions in `utils.cjs`: `extractClaudeContentWithToolCalls()`, `extractContentWithToolCalls()`, `extractToolCallSummary()`, `extractFilePaths()`
- Claude and Codex adapters switch extraction based on the flag
- Result includes `included_tool_calls: true` metadata when active
- Without the flag, behavior is unchanged (backward compatible)

### Added — `chorus summary` command

Structured session digest without reading full content. Extracts metadata locally — no LLM calls.

```json
{
  "agent": "claude",
  "session_id": "...",
  "message_count": 47,
  "duration_estimate": "~25 min",
  "user_requests": ["Fix the auth bug"],
  "files_referenced": ["src/auth.ts"],
  "tool_calls_by_type": {"Read": 12, "Edit": 8, "Bash": 5},
  "last_response_snippet": "Auth bug was in token refresh logic..."
}
```

- `files_referenced`: extracted from `tool_use` inputs (`file_path`, `path` fields)
- `tool_calls_by_type`: count of tool calls by tool name
- `duration_estimate`: first-to-last message timestamp delta
- `user_requests`: first 5 user messages, truncated to 150 chars each
- `last_response_snippet`: last assistant message excerpt (300 chars, not an LLM summary)

### Added — `chorus timeline` command

Cross-agent chronological view interleaving sessions from multiple agents for a given cwd.

```bash
chorus timeline --cwd ~/project --agent claude --agent codex --limit 5 --json
```

- Lists sessions from all requested agents (default: all four), sorted by timestamp descending
- Each entry includes a snippet (last assistant message, 200 chars)
- `--agent` is repeatable; `--limit` controls per-agent session count (default 5)

### Added — `--format markdown` output mode

Renders `chorus read`, `chorus summary`, and `chorus timeline` output as formatted markdown instead of JSON or raw text. Useful for human-facing demos and documentation.

```bash
chorus summary --agent claude --format markdown
chorus timeline --cwd . --format md
chorus read --agent codex --format markdown
```

### Added — `--include-user` flag on `chorus read` (from v0.10.1)

Pairs each returned assistant message with the preceding user prompt. Useful for "what is this agent doing?" status checks where the task-defining prompt matters.

- Intent router updated: "What is Claude doing?" now routes to `--include-user`
- All four adapters (Claude, Codex, Gemini, Cursor) support the flag
- Result includes `included_roles: ["user", "assistant"]` when active

### Changed — `verify` subcommand wired into dispatch

`chorus agent-context verify` was implemented in v0.10.0 but not registered in the CLI dispatch map. Now works correctly from the command line.

### Changed — Skill v0.11.0 with clean scope boundary

- SKILL.md updated with all new commands in synopsis, intent contract, and intent router
- "Context Pack Usage" section replaced with "Scope Boundary" — chorus is for session visibility and coordination only; pack creation/management is handled by repo-local tooling (e.g., team skills)
- Presentation docs cleaned of cross-tool terminology leakage

### Changed — Conformance comparator

`compare_read_output.cjs` now skips `included_roles` and `included_tool_calls` fields during Node vs Rust parity comparison, since these are Node-only additions pending Rust implementation.

### Testing

- Conformance: 14/14 passing
- Full suite: 34/34 passing, 0 failures
- Schema updated: `read-output.schema.json` extended for `included_roles`, `included_tool_calls`

### Upgrade Notes

- The global binary must be rebuilt: `npm install -g .` (or via npm registry after publish)
- Skill auto-updates via symlink for Codex (`~/.codex/skills/`) and Gemini (`~/.gemini/skills/`)
- Claude Code plugin updates via marketplace path after `npm install -g`
- Rust implementation does not yet include v0.11.0 features — parity deferred to v0.12.0

---

## v0.10.0 — 2026-03-27

### Changed — CLI subcommand renamed: `context-pack` to `agent-context`

The `chorus context-pack` subcommand has been renamed to `chorus agent-context` to better reflect the feature's identity and align with the `.agent-context/` directory it manages.

- `chorus context-pack <subcommand>` is now `chorus agent-context <subcommand>`
- `chorus setup --context-pack` is now `chorus setup --agent-context`
- npm script prefix `context-pack:*` is now `agent-context:*`
- `CONTEXT_PACK.md` renamed to `AGENT_CONTEXT.md`
- `skills/context-pack/` renamed to `skills/agent-context/`
- All documentation updated to use the new command name
- The `.agent-context/` directory name is unchanged (it was already correct)
- "Context pack" as a concept noun is unchanged in prose

### Added — `chorus agent-context verify` with CI mode

- `chorus agent-context verify` validates manifest checksums against actual file content (integrity check).
- `chorus agent-context verify --ci` combines integrity and freshness checking for use in PR gates. Exits non-zero if the pack is stale or corrupt.
- `--base` flag specifies the diff base for freshness detection (default: `origin/main`).
- CI mode produces structured JSON output: `{ integrity, freshness, changed_files, pack_updated, exit_code }`.
- CI workflow template available at `templates/ci-agent-context.yml`.
- `manifest.json` now records provenance metadata (commit SHA and timestamp) used by the freshness check.

### Upgrade Notes
- The old `chorus context-pack` subcommand will continue to work as an alias in a future compatibility release. For now, update scripts and automation to use `chorus agent-context`.

---

## v0.9.1 — 2026-03-27

### Fixed — P16 enforcement sweep: imperative routing in all locations

Backfilled from git history; see commits `729dfeb`, `eaf897e`, `9e94c46`, `748d028`.

- Routing blocks (`CLAUDE.md`, `AGENTS.md`, `GEMINI.md`) switched from suggestive ("you may want to read...") to imperative ("you MUST read... BEFORE opening source files"). Matches findings from real-world feedback that suggestive language was routinely ignored by agents.
- `context-pack` read-order made imperative in the template preambles.
- Full audit sweep: P16 added to design principles, all existing packs re-sealed, stale docs updated.

### Shipped concurrently
- Interactive HTML visualization of context-pack results.
- Run 6 experimental validation (frontend, React/TS stress-test) — all success criteria passed.

---

## v0.9.0 — 2026-03-26

### Added — three-layer context pack with `search_scope.json`

Backfilled from git history; see commits `485ff0c`, `f59d03e`, `a6e9f3f`, `3055645`, `a1d2580`.

- **Three-layer context pack architecture**: narrative prose (00–40 markdown) + authority JSON (routes, completeness_contract, reporting_rules) + new navigation layer (`search_scope.json`).
- **`search_scope.json`**: structured constraint layer for search-first agents. Defines task families with bounded `search_directories`, `exclude_from_search`, and `verification_shortcuts` (file + look_for) to cut exploration cost on unfamiliar repos.
- **Template additions**: Quick Lookup + Cross-Cutting Tracing sections added to pack templates (findings from Run 3).
- **Fixes**: seal metadata drift resolved; `init` now wires agent config files (CLAUDE.md / AGENTS.md / GEMINI.md) on creation.
- **Tests**: focused coverage for upsert, snapshot, init wiring, and seal sync.

---

## v0.8.3 — 2026-03-23

### Changed — context-pack templates

**`00_START_HERE.md` template:** Task-Type Routing section added. Agents now get explicit routing at orientation time:
- Impact analysis → read `30_BEHAVIORAL_INVARIANTS.md` first, CODE_MAP second
- Navigation → CODE_MAP Scope Rule
- Diagnosis → SYSTEM_OVERVIEW Silent Failure Modes first

**`10_SYSTEM_OVERVIEW.md` template:** Silent Failure Modes subsection added. Any code path where a failure produces no error (null return, silent drop, unchecked default) must be documented here.

**`20_CODE_MAP.md` template:** Three changes:
- Incompleteness note added before the table: agents must not treat CODE_MAP as a complete blast-radius list
- `Risk` column required on every row — must name the failure mode ("Silent failure if missed", "KeyError at runtime"), not just "High/Medium/Low"
- `Approach` column added for repos with coexisting architectural patterns

**`30_BEHAVIORAL_INVARIANTS.md` template:** Checklist rows must name explicit file paths, not descriptions. Added examples showing good vs bad row content.

### Changed — `chorus context-pack seal` content quality warnings

Seal now emits advisory warnings (never blocks) for:
- `20_CODE_MAP.md` missing a Risk column or having empty Risk values
- `30_BEHAVIORAL_INVARIANTS.md` Update Checklist with no rows, or rows without explicit file paths
- `10_SYSTEM_OVERVIEW.md` missing a Runtime Architecture or Silent Failure Modes section

### Changed — skill

`skills/agent-chorus/SKILL.md`: Context Pack Usage section added. Agents with the chorus plugin now have explicit instructions: read BEHAVIORAL_INVARIANTS before CODE_MAP for impact analysis; CODE_MAP is a navigation index not an exhaustive list.

### Why

Run 2 of the stream-models context pack experiment identified these as the highest-leverage template interventions. The BEHAVIORAL_INVARIANTS blast-radius requirement was the single change that prevented a systematic file exclusion error across all agents and conditions.

---

## v0.8.2 — 2026-03-23

### Changed
- `chorus context-pack init` now auto-installs the pre-push hook after scaffolding templates — no manual `install-hooks` step required
- `chorus context-pack seal` now warns if the pre-push hook is not installed, so the gap is visible on every seal run
- `40_OPERATIONS_AND_RELEASE.md` template updated: "Install pre-push hook" removed as a manual step (it is now automatic)

### Why
Context packs were going stale silently. The hook existed but was never wired into the init flow, so repos got a context pack with no freshness detection. This closes the installation gap: every `init` now leaves the repo with staleness detection active from the first push.

---

## v0.8.1 — 2026-03-20

### Fixes
- `marketplace.json`: removed unrecognized root keys (`$schema`, `description`) and changed `source` from `"."` to `"./"` — `claude plugin marketplace add` now works correctly for all users
- Node `isSystemDirectory` now allows macOS temp dirs (`/var/folders/`) matching Rust parity — fixes `setup`/`teardown` `--dry-run` in temp directories

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
