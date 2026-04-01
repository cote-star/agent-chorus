# Development

Use this page for internals, test loops, local demo generation, and extension points.

## Local Setup

```bash
npm ci
cargo --version
```

## Context Pack Maintenance

The repo supports a context pack for agent onboarding:

```bash
# Build/update local context pack
chorus agent-context build

# Install pre-push hook that syncs pack on main pushes when needed
chorus agent-context install-hooks
```

The active pack (`.agent-context/current/`) is tracked in git. Recovery snapshots (`.agent-context/snapshots/`) and build history are git-ignored and stay local.

## Project Structure

```text
scripts/
  read_session.cjs        # Node.js CLI implementation
  adapters/               # Node.js agent adapters
    codex.cjs
    gemini.cjs
    claude.cjs
    cursor.cjs
    registry.cjs
    utils.cjs
  conformance.sh          # Cross-implementation parity tests
  test_edge_cases.sh      # Edge-case and error code tests
  validate_schemas.sh     # JSON schema validation
  check_readme_examples.sh

cli/
  src/
    main.rs               # Rust CLI entry point
    agents.rs             # Session parsing, redaction, error codes
    report.rs             # Compare and report logic
    adapters/             # Rust agent adapters
      mod.rs              # AgentAdapter trait + registry
      codex.rs
      gemini.rs
      claude.rs
      cursor.rs

schemas/
  handoff.schema.json     # Handoff packet schema
  read-output.schema.json # Read command output schema
  list-output.schema.json # List command output schema
  error.schema.json       # Structured error output schema

fixtures/
  session-store/          # Test session files per agent
  golden/                 # Canonical expected outputs for conformance
```

## Testing

```bash
# Full Node-side check bundle
npm run check

# Rust unit tests
cargo test --manifest-path cli/Cargo.toml
```

Equivalent granular checks:

```bash
# Cross-implementation conformance (Node vs Rust parity)
bash scripts/conformance.sh

# Edge-case tests
bash scripts/test_edge_cases.sh

# JSON schema validation
bash scripts/validate_schemas.sh

# README command verification
bash scripts/check_readme_examples.sh

# npm package content verification
bash scripts/check_package_contents.sh
```

## Regenerating Demo Assets

Requirements:

- `puppeteer` in `node_modules`
- `img2webp` on PATH (`brew install webp`)
- Recorder defaults are tuned for README clarity (`1080x640`, lossless WebP).

```bash
npm install --save-dev puppeteer
node scripts/record_demo.js --input fixtures/demo/player-status.html --output docs/demo-status.webp --duration-ms 21000
node scripts/record_demo.js --input fixtures/demo/player-handoff.html --output docs/demo-handoff.webp --duration-ms 20000
node scripts/record_demo.js --input fixtures/demo/player-setup.html --output docs/demo-setup.webp --duration-ms 20000
node scripts/record_demo.js --input fixtures/demo/player-context-pack.html --output docs/demo-context-pack.webp --duration-ms 20000
node scripts/record_demo.js --input fixtures/demo/player-trash-talk.html --output docs/demo-trash-talk.webp --duration-ms 17000
npm uninstall puppeteer
```

## Adding a New Agent

1. **Rust**: Create `cli/src/adapters/<agent>.rs` implementing `AgentAdapter`, register in `mod.rs`.
2. **Node**: Create `scripts/adapters/<agent>.cjs` exporting `resolve`, `read`, `list`, register in `registry.cjs`.
3. Add agent name to enums in `schemas/*.schema.json`.
4. Add fixtures in `fixtures/session-store/<agent>/` and golden files in `fixtures/golden/`.
5. Add conformance and edge-case tests.

## Contribution Docs

- Contribution process: [`CONTRIBUTING.md`](../CONTRIBUTING.md)
- Protocol contract: [`PROTOCOL.md`](../PROTOCOL.md)
- Context pack details: [`AGENT_CONTEXT.md`](../AGENT_CONTEXT.md)
