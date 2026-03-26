# Context-Pack Skill — WIP

Working directory for the context-pack creation and maintenance skill.
Everything here tracks the evolution from research to shipped skill.

## Structure

```
wip/context-pack-skill/
  README.md              ← this file
  notes/                 ← raw notes, Slack excerpts, design decisions
  research/              ← experiment findings that inform the skill
  plans/                 ← skill definitions, schemas, architecture
  evolution/             ← chronological log of changes and rationale
```

## Quick Links

- Research program: `research/action-plan.md` (Phases 1–10)
- Design principles: `research/context-pack-design-principles.md` (P1–P15)
- v2 architecture: `research/context-pack-v2-design.md`
- Published CLI: `chorus context-pack init|seal|verify|build` (v0.9.0)

## Skill Surface (planned)

| Trigger | What happens | Guard |
|---|---|---|
| "create context pack" | Full init + fill + self-test | Human confirms before commit |
| Agent opens PR | Auto-prep .agent-context patch in PR | Human reviews as part of PR |
| "update context pack" | Diff since last seal → propose patches | Human approves each section |
