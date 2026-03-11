# Contributing to Agent Bridge

Thanks for helping improve Agent Bridge.

## Development Setup

1. Clone the repo.
2. Install Node dependencies: `npm ci`
3. Verify Rust toolchain is available: `cargo --version`
4. For architecture, structure, and demo generation details, see `docs/DEVELOPMENT.md`.

## Core Commands

1. Run parity and golden tests: `bash scripts/conformance.sh`
2. Run edge-case tests: `bash scripts/test_edge_cases.sh`
3. Validate README examples: `bash scripts/check_readme_examples.sh`
4. Validate npm publish contents: `bash scripts/check_package_contents.sh`
5. Validate JSON schemas: `bash scripts/validate_schemas.sh`
6. Run Rust tests: `cargo test --manifest-path cli/Cargo.toml`

You can also run the Node test bundle with `npm run check`.

## Pull Request Expectations

1. Keep Node and Rust outputs aligned with the protocol contract in `PROTOCOL.md`.
2. Add or update fixtures and golden files when behavior changes.
3. Update README and schemas when public CLI or JSON output changes.
4. Include tests for adapter changes and edge cases.
5. Keep changes scoped and explain tradeoffs in the PR description.
6. If core behavior/docs/contracts change, refresh the local context pack with `bridge context-pack seal` (or `build`).

## Release Safety

1. The published npm package must include `scripts/adapters/`.
2. The published npm package must not include local fixtures or CI-only files.
3. Run `bash scripts/check_package_contents.sh` before release.
