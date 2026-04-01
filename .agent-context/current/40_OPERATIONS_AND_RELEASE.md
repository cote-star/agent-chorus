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
  - Full validation suite
  - Cross-compile Rust binaries (Linux x64, macOS ARM64)
  - Publish to crates.io then npm

## Release Flow
1. Ensure all checks pass: `npm run check && cargo clippy`
2. Bump version in `package.json` and `cli/Cargo.toml` (must match).
3. Commit Cargo.lock if changed.
4. Use trusted publish wrappers: `npm-play publish` then `cargo-play publish`
5. Tag release: `git tag v<version> && git push origin v<version>`

## Context Pack Maintenance
1. Initialize scaffolding: `chorus agent-context init` (pre-push hook installed automatically)
2. Have your agent fill in the template sections (markdown + structured JSON).
3. Seal the pack: `chorus agent-context seal`
4. When freshness warnings appear on push, update content then run `chorus agent-context seal`

## Rollback/Recovery
- Restore latest snapshot: `chorus agent-context rollback`
- Restore named snapshot: `chorus agent-context rollback --snapshot <snapshot_id>`
