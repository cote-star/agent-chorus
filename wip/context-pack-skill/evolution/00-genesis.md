# Evolution Log: Context-Pack Skill

## 2026-03-26 — Genesis

### Where we are
- agent-chorus v0.9.0 published with three-layer context pack CLI
- Validated on 2 repo types (ML pipeline + CLI/library) across 5 experiment runs
- 15 design principles (P1–P15) documented from cross-agent experiments
- Three-layer architecture proven: content (markdown), authority (JSON contracts), navigation (search scopes)

### What we're building
A single skill that creates, validates, and maintains `.agent-context` for any repo.

### Three triggers
1. **Create**: "create context pack" → full init + fill + self-test
2. **PR update**: agent-opened PR → auto-prep .agent-context patch (separate commit)
3. **Manual catchup**: "update context pack" → diff-based patch proposals

### Key insight from experiments
The skill doesn't just scaffold templates — it must fill them with real repo content.
Empty arrays in JSON artifacts are useless. The skill must:
- Read the repo structure and identify high-impact paths
- Identify file families and derived files
- Write concrete verification shortcuts with line ranges
- Generate change-type checklists with explicit file lists
- Then self-test to validate the pack actually helps

### Open questions
- How long should the full create flow take? (target: 5-10 min for ~200 file repo)
- Should self-test use sub-agents or the same agent in a sandboxed context?
- How granular should the update detection be? (file-level? function-level?)
- Should the skill output a PR or just stage changes?
