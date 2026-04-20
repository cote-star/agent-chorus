# Evolution Log: Frontend Test (2026-03-26)

## What we tested
Ran the context-pack creation skill on trust-stream-frontend (1,982 files,
React 18 + TypeScript + Vite SPA). This is the third repo type after ML pipeline
and CLI/library.

## Skill execution timing
- Full Create flow: ~15 minutes (slightly over 5-10 min target for a 2K file repo)
- Scaffold: 2s, Fill markdown: ~8 min, Fill JSON: ~5 min, Seal: 1s, Self-test: 2 min

## Template observations
- All v0.9.0 template sections mapped naturally to frontend concepts
- "Silent Failure Modes" was highly relevant: Auth0 tokens, MSW handler sync, Zustand persistence, feature flags, Apollo deprecation
- "File Families" mapped well: base components, UI primitives, API hooks, stores, E2E specs, page objects
- "Negative Guidance" was valuable: don't use Apollo (legacy), don't enumerate UI primitives individually, don't read E2E tests for blast radius
- Extension Recipe adapted cleanly to "New Feature Page" pattern
- **No template modifications needed** — same template, third repo type

## Experiment results (Run 6)
- Claude structured: 4/4 yes, 2.75 avg files, 22.5K tokens, 0 dead ends
- Codex structured: 3/4 yes, 10.25 avg files, 2 dead ends
- Both agents bare: 2/4 yes each
- All 5 pass criteria met

## Key finding: M1 (Zustand store) is the proof point
Both agents missed `src/__tests__/setup.tsx` store reset in bare (causes flaky tests
with no error). Both found it in structured because the invariants checklist explicitly
names it. This is the exact "silent failure" pattern context packs are designed to catch.

## What worked well
- The three-layer architecture generalizes: Claude used authority contracts (zero files
  opened on some tasks), Codex used search scopes (fewer dead ends)
- Negative guidance prevented Claude from proposing Apollo (deprecated) in the structured condition
- Self-test accurately predicted pack quality

## What could improve
- Skill execution time (15 min) is above target for large repos — consider parallelizing fill steps
- Codex protocol breach: rg search matched GROUND_TRUTH.md — need to add it to .gitignore or exclude from search in experiment repos
- search_scope verification_shortcuts could be more specific (function names vs directory-level)
