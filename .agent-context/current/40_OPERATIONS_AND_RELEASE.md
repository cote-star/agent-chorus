# Operations And Release

## Standard Validation
<!-- AGENT: Add local validation commands (tests, linters, etc.). -->

## CI Checks
<!-- AGENT: List CI workflows/steps that gate merges. -->

## Release Flow
<!-- AGENT: Describe how releases are triggered and what they produce. -->

## Context Pack Maintenance
1. Initialize scaffolding: `bridge context-pack init`
2. Have your agent fill in the template sections.
3. Seal the pack: `bridge context-pack seal`
4. Install pre-push hook: `bridge context-pack install-hooks`
5. When freshness warnings appear, update content then run `bridge context-pack seal`

## Rollback/Recovery
- Restore latest snapshot: `bridge context-pack rollback`
- Restore named snapshot: `bridge context-pack rollback --snapshot <snapshot_id>`
