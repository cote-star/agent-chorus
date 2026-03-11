# Operations And Release

## Standard Validation
<!-- AGENT: Add local validation commands (tests, linters, etc.). -->

## CI Checks
<!-- AGENT: List CI workflows/steps that gate merges. -->

## Release Flow
<!-- AGENT: Describe how releases are triggered and what they produce. -->

## Context Pack Maintenance
1. Initialize scaffolding: `chorus context-pack init`
2. Have your agent fill in the template sections.
3. Seal the pack: `chorus context-pack seal`
4. Install pre-push hook: `chorus context-pack install-hooks`
5. When freshness warnings appear, update content then run `chorus context-pack seal`

## Rollback/Recovery
- Restore latest snapshot: `chorus context-pack rollback`
- Restore named snapshot: `chorus context-pack rollback --snapshot <snapshot_id>`
