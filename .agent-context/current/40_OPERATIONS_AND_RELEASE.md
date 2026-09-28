# Operations And Release

## Standard Validation
```bash
npm run check          # Full suite: conformance + readme + package + schemas + agent-context tests
npm run conformance    # Node/Rust parity only
npm run validate:schemas  # JSON schema validation only
cargo test --manifest-path cli/Cargo.toml  # Rust unit tests (139 tests as of v0.14.0)
cargo clippy --manifest-path cli/Cargo.toml
bash scripts/test_context_pack.sh  # Agent-context integration tests (9 tests)
```

## CI Checks
- `.github/workflows/ci.yml`: runs on push/PR to main
  - Node conformance (`npm run check`)
  - Rust build + clippy (`cargo build`, `cargo clippy`)
  - Schema validation
- `.github/workflows/release.yml`: runs on version tag (`v*`)
  - `verify` job runs `scripts/release/verify_versions.sh` — gates every downstream job on `package.json.version === cli/Cargo.toml.version === tag[1:]`
  - Full validation suite (conformance + README examples + schemas + agent-context tests)
  - Cross-compile Rust binaries (Linux x64, macOS ARM64) from `cli/target/release/chorus`
  - Publish to npm + GitHub Packages + crates.io
  - `create-release` job uses `softprops/action-gh-release@v2` to create the GitHub Release. Release body is the matching `## vX.Y.Z` section extracted from `RELEASE_NOTES.md`; Rust binaries from the package-rust job are attached automatically. Runs last so earlier failures skip it cleanly. (v0.12.1 fixed the binary upload path so attached artifacts are no longer empty.)
- `.github/workflows/yank.yml`: manual only (`workflow_dispatch`; inputs `version`, `reason`). Runs `cargo yank` with `CARGO_REGISTRY_TOKEN` and refuses the version currently in `cli/Cargo.toml`. Yank is reversible (`cargo yank --undo`) and only hides a version from dependency resolution; the `.crate` stays downloadable, so true removal is a crates.io support request.

### Decoupled `needs:` chain (commit 8582a70, v0.13.0)
The pre-v0.13.0 chain had `package-node.needs = [verify, publish-crate]` and `create-release.needs = [..., publish-crate, publish-github-package]`. Any crates.io failure silently skipped npm publish (this is how v0.12.1 shipped to GitHub + crates.io but not to npm); any registry publish failure cancelled the GitHub Release too. The 8582a70 change:

- Drops `publish-crate` from `package-node.needs`. npm and crates.io are independent registries — one's failure must not cascade into the other.
- Originally added `continue-on-error: true` on the `Publish to npm` step inside `package-node`. **Superseded 2026-09-28 (41632f2):** that made a failed publish look green — v0.16.1's npm publish failed with `E404` (token without publish scope) and went unnoticed for two weeks. npm publish is now its own `publish-npm` job (`needs: verify`, no `continue-on-error`), a sibling of `publish-crate`. `package-node` only builds and uploads the tarball.
- Drops `publish-crate` and `publish-github-package` from `create-release.needs` (they produce no artifacts for the Release). Adds `if: always() && needs.verify.result == 'success'` so the Release still ships even if a sibling publish job fails.

**Net topology**: `verify` → (`package-node` ‖ `package-rust`) both feed `create-release`; `publish-npm`, `publish-crate` and `publish-github-package` run as parallel siblings the Release does not wait on. A failing registry publisher is a red *job* on the run page — never a swallowed step — and does not block a successful release.

## Branch Protection
- `main` has force-push denied and deletion denied (enabled alongside v0.12.1). All changes land through reviewed PRs.

## Agent-Context Atomicity Guarantees (v0.14.0, P10)
- **Staging-dir + rename commit**: Seal writes the new snapshot to a staging directory inside `.agent-context/` and promotes it via `rename(2)` — an atomic filesystem op on POSIX. A crash mid-seal leaves the previous snapshot intact.
- **Stale lockfile auto-recovery**: If a prior seal was interrupted, the next seal detects the stale lock, recovers cleanly, and continues. Callers no longer need a manual `rm .agent-context/.lock` recipe.
- **Concurrent verify safety**: `verify` reads the committed snapshot only; a seal that is mid-flight in its staging dir cannot be observed by a racing verify run. This closes F29–F33 (concurrency) and F55 (recovery).

## Release Flow
1. Ensure all checks pass: `npm run check && cargo clippy`
2. Bump version in `package.json` and `cli/Cargo.toml` (must match); verify locally via `bash scripts/release/verify_versions.sh v<version>`.
3. Commit Cargo.lock if changed.
4. Use trusted publish wrappers: `npm-play publish` then `cargo-play publish`
5. Tag release: `git tag v<version> && git push origin v<version>` — the tag push triggers `release.yml`, which handles npm, GitHub Packages, crates.io, and the GitHub Release + binary attachments automatically.

## Known Limitations
- **Registry token rotation** — `publish-npm` (`AGENT_CHORUS_CI`) and `publish-crate` (`CARGO_REGISTRY_TOKEN`) fail red when their secret is stale; crates.io, GitHub Packages and the GitHub Release are independent of one another. Symptoms: crates.io `403 authentication failed`; npm `E404 … PUT registry.npmjs.org/agent-chorus` (npm reports a token without publish scope as 404, not 403). Recovery: rotate at https://github.com/cote-star/agent-chorus/settings/secrets/actions, then `gh run rerun <run-id> --failed` (or `--job <id>`) on the **original tag-push run** — a `workflow_dispatch` re-run skips every publish step because they are gated on `ref_type == 'tag'`. v0.16.1 hit both failures on 2026-09-15 and was recovered this way on 2026-09-28. Manual fallback (human-only): `npm-play publish --confirm-publish` / `cargo-play publish --confirm-publish` from `~/sandbox/play`.
- **Yanked crate versions** — `0.14.1` and `0.16.0` shipped source that must not be public and are yanked via `yank.yml` (superseded by `0.16.1`; check state with `cargo info agent-chorus`). `0.14.0` was tagged but never reached crates.io.

## Context Pack Maintenance
1. Initialize scaffolding: `chorus agent-context init` (pre-push hook installed automatically)
2. Have your agent fill in the template sections (markdown + structured JSON).
3. Seal the pack: `chorus agent-context seal`
4. Verify the pack: `chorus agent-context verify` (interactive report) or `chorus agent-context verify --ci` (exit-code only, uses `templates/ci-agent-context.yml` for CI pipelines)
5. When freshness warnings appear on push, update content then run `chorus agent-context seal`

## Rollback/Recovery
- Restore latest snapshot: `chorus agent-context rollback`
- Restore named snapshot: `chorus agent-context rollback --snapshot <snapshot_id>`
