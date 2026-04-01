# Rename: context-pack → agent-context

**Date:** 2026-04-01
**Status:** Planned
**Target version:** v0.10.0

## Why

- The output directory is already `.agent-context/` — the CLI subcommand should match
- "agent-context" is purpose-driven (context for agents), "context-pack" is artifact-driven (a pack of context)
- The team_skills skill is already named `agent-context`
- All work repos already use `.agent-context/` naming

## Scope

454 occurrences across 50 files. Three categories:

### Category A — Code (must be precise, tested)

| What | From | To | Files | Occurrences |
|---|---|---|---|---|
| CLI subcommand | `chorus context-pack` | `chorus agent-context` | main.rs, read_session.cjs | ~54 |
| Rust module | `context_pack.rs` / `mod context_pack` | `agent_context.rs` / `mod agent_context` | main.rs, context_pack.rs | ~116 |
| Node scripts dir | `scripts/context_pack/` | `scripts/agent_context/` | 9 files | ~72 |
| Node entry routing | `context-pack` command set | `agent-context` command set | read_session.cjs | ~42 |
| Teardown/relevance refs | `context_pack` imports | `agent_context` imports | teardown.rs, relevance.rs | ~7 |
| npm scripts | `context-pack:*` | `agent-context:*` | package.json | ~13 |
| CI workflow | `context-pack` test/build refs | `agent-context` refs | ci.yml | ~7 |
| Git hooks | `context-pack` freshness check | `agent-context` freshness check | .githooks/pre-push | ~4 |
| Test scripts | `test_context_pack.sh` | `test_agent_context.sh` | 1 file | ~9 |
| Smoke tests | `context-pack` command refs | `agent-context` refs | test_smoke.sh | ~5 |
| Package contents check | `context_pack` path refs | `agent_context` refs | check_package_contents.sh | ~2 |

### Category B — Documentation (straightforward find/replace)

| What | Files | Occurrences |
|---|---|---|
| README.md | 1 | ~13 |
| CONTEXT_PACK.md → AGENT_CONTEXT.md | 1 | ~8 |
| CLI_REFERENCE.md | 1 | ~18 |
| PROTOCOL.md | 1 | ~4 |
| RELEASE_NOTES.md | 1 | ~36 |
| CLAUDE.md / AGENTS.md / GEMINI.md | 3 | ~17 |
| CONTRIBUTING.md | 1 | ~1 |
| DEVELOPMENT.md | 1 | ~3 |
| Presentation + results docs | 2 | ~7 |

### Category C — Can leave as-is (historical / asset names)

| What | Why leave | Files |
|---|---|---|
| `research/context-pack-*.md` | Historical research docs — the term was correct at time of writing | 4 files |
| `wip/context-pack-skill/` | WIP artifacts, will be cleaned up separately | 3 files |
| `fixtures/demo/*context-pack*` | Demo recordings, would need re-recording | 3 files |
| `.agent-context/current/*` | Self-referential pack content, update via re-seal after rename | ~8 files |
| `docs/*context-pack*.webp` | Image assets, filenames don't affect functionality | 2 files |

## Execution Plan

### Phase 1 — Deprecation alias (non-breaking, ship in v0.9.2)

**Goal:** `chorus agent-context` works immediately, `chorus context-pack` still works with deprecation warning.

1. **Rust** — Add `agent-context` as primary command name, keep `context-pack` as hidden alias with stderr warning
2. **Node** — Add `agent-context` to command set, route both names to same handlers, warn on old name
3. **Tests** — Add tests for new command name, keep old tests passing
4. **Docs** — Update CLI_REFERENCE.md to show new name, note old name is deprecated

Conformance: both implementations must show same deprecation message.

### Phase 2 — Internal rename (ship in v0.10.0)

**Goal:** All internal code uses `agent_context` naming. Old CLI alias still works.

1. **Rust** — `mv cli/src/context_pack.rs cli/src/agent_context.rs`, update `mod` declaration, rename all internal `context_pack` → `agent_context` in function names and module paths
2. **Node** — `mv scripts/context_pack/ scripts/agent_context/`, update all `require()` paths in read_session.cjs and internal imports
3. **package.json** — Rename all `context-pack:*` scripts to `agent-context:*`, update `files` array
4. **CI** — Update ci.yml references
5. **Git hooks** — Update .githooks/pre-push
6. **Test scripts** — Rename and update
7. **Conformance tests** — Run full suite, verify parity

### Phase 3 — Documentation (same release as Phase 2)

1. `mv CONTEXT_PACK.md AGENT_CONTEXT.md`
2. Update README.md, PROTOCOL.md, CLAUDE.md, AGENTS.md, GEMINI.md, CONTRIBUTING.md
3. Update CLI_REFERENCE.md, DEVELOPMENT.md
4. Update RELEASE_NOTES.md (add rename entry, keep historical references)
5. Rename skill: `mv skills/context-pack/ skills/agent-context/`, update SKILL.md content

### Phase 4 — Re-seal agent-chorus's own context pack

1. Update `.agent-context/current/` files to use new naming
2. Validate all references resolve
3. Commit as separate commit

### Phase 5 — Remove old alias (v1.0.0, future)

1. Remove `context-pack` command alias from Rust and Node
2. Remove deprecation warning code
3. Clean break

### Phase 6 — Remaining assets and research docs

**Goal:** Update historical research docs, demo assets, and WIP artifacts so the entire repo is consistent.

1. **Research docs** — Update `research/context-pack-*.md` filenames and content headings
2. **Demo assets** — Rename `fixtures/demo/*context-pack*` and `docs/*context-pack*` files
3. **WIP** — Rename `wip/context-pack-skill/` directory
4. **Presentation/results docs** — Update `docs/context-pack-presentation.md` and `docs/context-pack-results.md` filenames and content

## Risk Mitigation

- **Conformance tests** catch Node/Rust parity drift — run after every phase
- **Phase 1 is non-breaking** — can ship immediately, gives users time to migrate
- **Phase 2+3 together** — one breaking release, not two
- **Category C files left alone** — avoids unnecessary churn in research/demo assets
- **Git hooks in target repos** reference `chorus context-pack check-freshness` — the deprecation alias in Phase 1 means existing hooks keep working

## Dependencies (required)

- team_skills PR #10 (agent-context skill) — establishes the name in the work ecosystem
- stream-models PR #392 — validates the approach in production
- Both must be merged or approved before publishing v0.10.0

## Not in scope

- Renaming the `.agent-context/` directory (already correct)
- Renaming the `agent-chorus` package itself
- Updating context packs in other repos (they reference `.agent-context/` which is unchanged)
