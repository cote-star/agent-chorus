# Evolution Log: Enforcement Fix (2026-03-27)

## Feedback
Colleague tested `feat/agent-context` branch on trust-stream-frontend.
Agent (Claude) read `00_START_HERE.md` but did NOT follow through to read
`30_BEHAVIORAL_INVARIANTS.md` or `20_CODE_MAP.md`. Started working with
incomplete context. When prompted "did you use the agent/context?", it went
back and read the full pack — producing significantly better output.

## Root cause
1. CLAUDE.md routing said "Follow the read order defined in that file" — too vague
2. START_HERE used "Use" for steps 4-5 instead of "Read" — agent treated as optional
3. No explicit "do not start work until" gate

## Fix applied
- CLAUDE.md: explicit 3-file list with **"BEFORE starting any task"**
- START_HERE: **"MANDATORY before starting work"**, **"Do NOT open repo source files until you have read steps 1-3"**
- Changed "Use" to "Read" everywhere
- Reordered: INVARIANTS before CODE_MAP (invariants prevent more mistakes)
- Updated both Node and Rust templates in agent-chorus

## Design principle
**P16 — Routing must be imperative, not suggestive.** Agents interpret "follow
the read order" as "read the first file, then decide if I need more." They
interpret "BEFORE starting any task, read these 3 files" as a hard gate. The
wording matters more than the content — an agent that skips the pack gets the
same quality as bare.

## Files changed
- `agent-chorus/cli/src/context_pack.rs` — Rust templates (2 locations)
- `agent-chorus/scripts/context_pack/init.cjs` — Node templates (2 locations)
- `trust-stream-frontend/CLAUDE.md` — routing block
- `trust-stream-frontend/.agent-context/current/00_START_HERE.md` — read order
