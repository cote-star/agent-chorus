# Agent Behaviour Experiment — Protocol & Self-Reporting Instructions

> **READ THIS FIRST.** This document is your complete protocol. Follow it exactly.
> Do **not** read `GROUND_TRUTH.md` under any circumstances. It exists only for the reviewer.

---

## Your Condition

Before you begin, determine which branch you are on:

```bash
git branch --show-current
```

| Branch | Your condition | What you have |
|---|---|---|
| `test/bare` | **bare** | Raw repo only. No context pack guidance. |
| `test/structured` | **structured** | Full context pack with markdown documentation and structured JSON artifacts (routes, completeness contracts, reporting rules, search scopes). Follow the instructions in your agent config file (CLAUDE.md or AGENTS.md) before exploring. |

Record your condition. Every result file you write must include it.

> **Session isolation is mandatory.** Each condition must be run in a brand-new session with no prior context about this repo.

---

## Session Rules

1. **Fresh session.** You have no prior knowledge of this repo. Begin cold.
2. **Tasks in order.** Run all six tasks in the order listed. Do not skip or reorder.
3. **One task at a time.** Complete each task fully before starting the next.
4. **No peeking at `GROUND_TRUTH.md`.** It is in this directory. Do not read it.
5. **Self-report after every task.** Write your result JSON immediately after completing each task.
6. **Be honest about uncertainty.** If you are not confident, say so in `correctness_notes`.
7. **Count your own tool calls.** Track every Read/Grep/Glob/Bash you issue.
8. **`first_correct_file_hop`**: How many files you opened before the first relevant file (inclusive). 1 = first file was relevant.
9. **`files_opened_after_first_correct_hop`** and **`post_hit_dead_ends`**: After the first relevant file, count every additional file opened and the subset that turned out irrelevant.

---

## Tasks

### L1 — Redaction Lookup: JWT tokens

**Question:** Where in the codebase are JWT tokens redacted? Cite the exact function name, file path, and line number for **both** the Node and Rust implementations.

Write result to: `tests/behaviour/results/{agent}/{condition}/L1.json`

---

### L2 — Version Lookup: `chorus_output_version`

**Question:** Where is the `chorus_output_version` field set in the output? Cite the exact file, line number, and value for both Node and Rust implementations. Also cite the schema definition.

Write result to: `tests/behaviour/results/{agent}/{condition}/L2.json`

---

### M1 — Impact Analysis: New agent adapter "windsurf"

**Question:** List every file that must be created or modified to add a new agent called "windsurf" to agent-chorus. Include both implementations, test infrastructure, and documentation. For each file, describe what must change.

Write result to: `tests/behaviour/results/{agent}/{condition}/M1.json`

---

### M2 — Impact Analysis: New CLI flag `--output-format yaml`

**Question:** List every file that must change to add a `--output-format yaml` flag to the `chorus read` command. Include both implementations, dependencies, test infrastructure, and documentation.

Write result to: `tests/behaviour/results/{agent}/{condition}/M2.json`

---

### H1 — Planning: `chorus audit` command

**Question:** Write a complete implementation plan for adding a `chorus audit` command that scans all agent sessions for un-redacted secrets and reports findings. Include: files to create, files to modify, key design decisions, commands to validate, and any existing code that should be reused rather than reimplemented.

Write result to: `tests/behaviour/results/{agent}/{condition}/H1.json`

---

### H2 — Diagnosis: Conformance failure after new redaction pattern

**Question:** A developer adds a new redaction pattern (Anthropic API keys matching `sk-ant-*`) to the Rust implementation but forgets to add it to the Node implementation. The conformance suite starts failing. Diagnose: (1) exactly where the failure occurs in the conformance pipeline, (2) which files are involved, (3) how to confirm the root cause, and (4) how to fix it.

Write result to: `tests/behaviour/results/{agent}/{condition}/H2.json`

---

## Result JSON Schema

Each result file must be valid JSON matching `tests/behaviour/results/schema.json`.

```json
{
  "task_id": "L1",
  "agent": "claude",
  "condition": "bare",
  "files_opened_count": 5,
  "dead_ends": 1,
  "first_correct_file_hop": 2,
  "files_opened_after_first_correct_hop": 3,
  "post_hit_dead_ends": 1,
  "tool_calls": {"Read": 3, "Grep": 1, "Glob": 1, "Bash": 0},
  "tokens_total_estimate": 15000,
  "duration_seconds": 45,
  "correct": "partial",
  "correctness_notes": "Found Rust implementation but missed Node adapter...",
  "quality_self_score": 7,
  "risk_flag": false,
  "risk_flag_explanation": ""
}
```
