# Operations And Release

## Standard Validation
```bash
npm run check          # Full suite: conformance + readme + package + schemas + agent-context tests
npm run conformance    # Node/Rust parity only
npm run validate:schemas  # JSON schema validation only
cargo test --manifest-path cli/Cargo.toml  # Rust unit tests (29 tests)
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

## Branch Protection
- `main` has force-push denied and deletion denied (enabled alongside v0.12.1). All changes land through reviewed PRs.

## Release Flow
1. Ensure all checks pass: `npm run check && cargo clippy`
2. Bump version in `package.json` and `cli/Cargo.toml` (must match); verify locally via `bash scripts/release/verify_versions.sh v<version>`.
3. Commit Cargo.lock if changed.
4. Use trusted publish wrappers: `npm-play publish` then `cargo-play publish`
5. Tag release: `git tag v<version> && git push origin v<version>` — the tag push triggers `release.yml`, which handles npm, GitHub Packages, crates.io, and the GitHub Release + binary attachments automatically.

## Known Limitations
- **`NPM_TOKEN` rotation** — until the CI token is rotated, the automated npm publish step may fail; the manual workaround is `npm-play publish --confirm-publish` run from a worktree rooted under `~/sandbox/play`. Other publish surfaces (crates.io, GitHub Packages, GitHub Release) are unaffected.

## Context Pack Maintenance
1. Initialize scaffolding: `chorus agent-context init` (pre-push hook installed automatically)
2. Have your agent fill in the template sections (markdown + structured JSON).
3. Seal the pack: `chorus agent-context seal`
4. Verify the pack: `chorus agent-context verify` (interactive report) or `chorus agent-context verify --ci` (exit-code only, uses `templates/ci-agent-context.yml` for CI pipelines)
5. When freshness warnings appear on push, update content then run `chorus agent-context seal`

## Rollback/Recovery
- Restore latest snapshot: `chorus agent-context rollback`
- Restore named snapshot: `chorus agent-context rollback --snapshot <snapshot_id>`
