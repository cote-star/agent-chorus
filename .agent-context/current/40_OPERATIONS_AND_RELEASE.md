# Operations And Release

## Standard Validation
```bash
npm run check          # Full suite: conformance + readme + package + schemas
npm run conformance    # Node/Rust parity only
npm run validate:schemas  # JSON schema validation only
cargo check --manifest-path cli/Cargo.toml
cargo clippy --manifest-path cli/Cargo.toml
bash scripts/test_smoke.sh                    # CLI smoke tests
bash scripts/test_adversarial_redaction.sh    # Adversarial redaction tests
```

## CI Checks
- `.github/workflows/ci.yml`: runs on push/PR to main
  - Node conformance (`npm run check`)
  - Rust build + clippy (`cargo build`, `cargo clippy`)
  - Schema validation
- `.github/workflows/release.yml`: runs on version tag (`v*`)
  - Full validation suite
  - Cross-compile Rust binaries (Linux x64, macOS ARM64)
  - Publish to crates.io then npm

## Release Flow
1. Ensure all checks pass: `npm run check && cargo clippy`
2. Verify versions match: `bash scripts/release/verify_versions.sh`
3. Use trusted publish wrappers: `release-play inspect | jq` then `release-play verify | jq`
4. Publish (confirm-only, human-only): `release-play publish --target all --confirm-publish`
5. Tag release: `git tag v<version> && git push origin v<version>`
6. CI release workflow handles binary packaging and registry publish

## Context Pack Maintenance
1. Initialize scaffolding: `chorus context-pack init`
2. Have your agent fill in the template sections.
3. Seal the pack: `chorus context-pack seal`
4. Install pre-push hook: `chorus context-pack install-hooks`
5. When freshness warnings appear, update content then run `chorus context-pack seal`

## Rollback/Recovery
- Restore latest snapshot: `chorus context-pack rollback`
- Restore named snapshot: `chorus context-pack rollback --snapshot <snapshot_id>`
