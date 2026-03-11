# Context Pack

This repo includes a context-pack system for token-efficient agent onboarding.

## Goals
- Keep "understand the repo end-to-end" requests cheap in tokens.
- Give agents a dense, stable index before they open project files.
- Update context only when `main` changes are context-relevant.
- Keep context history recoverable while ensuring local-only data is not published in package artifacts.

## Layered Model
- **Layer 0 (Evidence)**: agents can inspect each other's session output with citations.
- **Layer 1 (Context)**: context-pack provides deterministic repo onboarding.
- **Layer 2 (Coordination, optional)**: only add orchestration once layers 0-1 are insufficient.

## Storage Model
- Active pack: `.agent-context/current/` — **tracked in git** so all contributors share the same context.
- Configuration: `.agent-context/relevance.json` — defines which files trigger updates.
- Guide: `.agent-context/GUIDE.md` — human-written high-level map (optional).
- Snapshots: `.agent-context/snapshots/<timestamp>_<sha>/` — git-ignored, local-only recovery.
- Build history: `.agent-context/history.jsonl` — git-ignored, local-only audit log.

Only `current/` and `relevance.json` are committed. Snapshots and history stay local.

## Naming Convention
Inside `.agent-context/current/`:
- `00_START_HERE.md`: compact index and read order
- `10_SYSTEM_OVERVIEW.md`: architecture and command surface
- `20_CODE_MAP.md`: high-impact files and extension paths
- `30_BEHAVIORAL_INVARIANTS.md`: contract-level constraints
- `40_OPERATIONS_AND_RELEASE.md`: tests, CI, release, maintenance
- `manifest.json`: machine-readable metadata, hashes, and checksums

Numeric prefixes keep deterministic read order for agents.

## Operational Guarantees
- Deterministic file order via numeric prefixes (`00`, `10`, `20`, `30`, `40`).
- Integrity metadata via `manifest.json` checksums and pack metadata.
- Local-only recovery via snapshots and rollback.
- Main-branch scoped auto-sync to avoid unnecessary churn.
- Pack content stays reviewable in git (`current/` tracked, recovery artifacts local).

## Commands
```bash
# 1. Initialize template scaffolding
bridge context-pack init

# 2. Agent fills in content...

# 3. Seal the pack (validate & snapshot)
bridge context-pack seal

# Manual build (backward-compatible wrapper around seal)
bridge context-pack build

# Install advisory-only pre-push hook
bridge context-pack install-hooks

# Sync context pack for a main push event (used by pre-push hook)
bridge context-pack sync-main --local-ref refs/heads/main --local-sha <local> --remote-ref refs/heads/main --remote-sha <remote>

# Restore latest snapshot
bridge context-pack rollback
```

## Update Policy
- For pushes that do not target `main`: no sync.
- For pushes to `main` with non-relevant file changes: no update.
- For pushes to `main` with relevant changes: **Advisory Warning**. The hook prints a warning if the pack is stale, but does not auto-build or block the push.

Relevant paths are configurable in `.agent-context/relevance.json`. Defaults include:
- command/runtime sources (`scripts/`, `cli/src/`)
- contracts (`schemas/`, `PROTOCOL.md`)
- docs that define behavior (`README.md`, `CONTRIBUTING.md`, `SKILL.md`)
- release/CI wiring (`.github/workflows/`, package metadata, Cargo metadata)
- fixture/golden data used by behavior tests

## Non-Goals
- Context pack is not a source-of-truth replacement for behavior-critical edits.
- Context pack does not write or mutate agent sessions.
- Context pack does not provide orchestration primitives (routing, queues, live sync).
