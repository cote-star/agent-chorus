# REPLACE: Repo Name — Agent Context

**MANDATORY before starting work.** Do NOT open repo source files until steps 1-3 are complete.

## Fast Facts

| Field | Value |
|---|---|
| Product | REPLACE: product/service/library name |
| Languages | REPLACE: primary languages and frameworks |
| Package manager | REPLACE: uv / npm / pnpm / cargo / etc. |
| Quality gate | REPLACE: primary validation commands |
| Core risk | REPLACE: most likely silent failure or production-risk area |
| Version | REPLACE: release/versioning mechanism |

## Scope Rule

This agent context covers REPLACE: the code and operational areas the pack is intended to guide. It does NOT cover REPLACE: adjacent systems or directories outside that scope.

## Read Order

1. This file (fast facts, scope, stop rules)
2. `10_SYSTEM_OVERVIEW.md`
3. `20_CODE_MAP.md` or `30_BEHAVIORAL_INVARIANTS.md` depending on task type
4. Then open source files as needed for your task

## Stop Rules

Before opening any source file, check whether your answer is already in the agent context:

- "Where is X configured?" → `20_CODE_MAP.md`
- "What files change for Y?" → `30_BEHAVIORAL_INVARIANTS.md`
- "How do I validate Z?" → `40_OPERATIONS_AND_RELEASE.md`
- "What is the runtime shape?" → `10_SYSTEM_OVERVIEW.md`

If the pack answers your question, do not open additional files. If it does not, use `search_scope.json` to bound your search.

## Not Covered in Detail

REPLACE: List any subsystems from the inventory that exist in the repo but are not covered in depth by this pack. Include directory path and a one-line description so agents know they exist even though the pack doesn't guide them. Remove this section if every subsystem is covered.
