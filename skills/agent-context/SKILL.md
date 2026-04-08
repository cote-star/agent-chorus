# Skill: Context Pack

Create, validate, and maintain a structured context pack (`.agent-context/`) for a repository so that AI agents navigate the codebase efficiently and make higher-quality decisions.

## When to use

- **Create**: First time setting up agent context for a repo. Best for repos with >50 files or >3 distinct subsystems. For small repos, agents can scan everything without a context pack.
- **Update (agent PR)**: When an agent opens a PR after completing work. The agent already knows what changed and should prep `.agent-context` updates as a separate commit in the PR.
- **Update (manual catchup)**: When significant human-authored work has been merged without agent involvement. The agent diffs since the last seal and proposes patches for human approval.

## Trigger phrases

- "create a context pack for this repo"
- "set up agent context"
- "update the context pack"
- "refresh the context pack"

## Prerequisites

- `chorus` CLI installed (`npm install -g agent-chorus` v0.9.0+)
- Git repository with at least one commit
- For update triggers: existing `.agent-context/` directory with a sealed pack

---

## Flow: Create

### Step 1 — Assess the repo

Check whether a context pack is warranted:

```bash
git ls-files | wc -l
```

- **>50 files or >3 top-level source directories**: proceed.
- **<50 files, simple structure**: warn the user that a context pack may add overhead without benefit. Proceed only if they confirm.

Check for an existing pack:

```bash
ls .agent-context/current/ 2>/dev/null
```

- If exists and sealed: ask whether to update (use the Update flow) or reinit from scratch.
- If exists but empty/scaffolded: proceed with filling.
- If missing: proceed with full init.

### Step 2 — Scaffold

```bash
chorus agent-context init --force
```

This creates:
- 5 markdown templates (00–40)
- 4 structured JSON artifacts (routes, completeness_contract, reporting_rules, search_scope)
- CLAUDE.md, AGENTS.md, GEMINI.md routing blocks (~100-200 tokens each)
- Pre-push hook for freshness warnings

### Step 3 — Fill the content layer (markdown)

Read the repo structure and fill each markdown file. Follow this order:

1. **00_START_HERE.md**: Fill Fast Facts (product, languages, quality gate, core risk, version). Fill Scope Rule. Fill Stop Rules based on repo patterns.

2. **10_SYSTEM_OVERVIEW.md**: Fill Product Shape, Runtime Architecture (3-5 steps), Silent Failure Modes (any code path where failure produces no error), Command/API Surface table, Tracked Path Density.

3. **20_CODE_MAP.md**: Identify 8-15 high-impact paths. For each: path, what it does, why it matters, risk level, authority (authoritative/derived/reference). Fill Quick Lookup Shortcuts (4-6 patterns). Fill Cross-Cutting Tracing Flows for changes that ripple through multiple files. Fill Extension Recipe.

4. **30_BEHAVIORAL_INVARIANTS.md**: Write 3-8 testable invariants. Fill Update Checklist with one row per common change type — explicit file paths, not descriptions. Identify File Families (glob pattern, member count, report-as-family or enumerate). Write Negative Guidance (what NOT to do — common over-exploration patterns).

5. **40_OPERATIONS_AND_RELEASE.md**: Fill validation commands, CI checks, release flow, context pack maintenance.

### Step 4 — Fill the authority layer (JSON contracts)

Fill the structured JSON artifacts with repo-specific content:

1. **routes.json**: Verify the default task routes make sense for this repo. Add `named_patterns` for repo-specific lookup and change patterns.

2. **completeness_contract.json**: For each common change type, fill `contractually_required_files` with explicit file paths and `required_file_families` with glob patterns. These are the files that MUST appear in an impact analysis answer.

3. **reporting_rules.json**: Fill `groupable_families` (homogeneous file sets to report as family, not individually). Fill `never_enumerate_individually` (derived/generated files). Fill `authoritative_vs_derived_paths` in global rules.

4. **search_scope.json**: For each task family, fill `search_directories` (where to look), `exclude_from_search` (where NOT to look), and `verification_shortcuts` (specific file + line range or function name for quick checks).

### Step 5 — Seal

```bash
chorus agent-context seal --force
```

Seal validates:
- All markdown files present and non-empty
- All structured artifact file references resolve on disk
- Grouped families don't point to generated files as authoritative edit targets
- Completeness contract patterns match real files

Fix any seal errors before proceeding.

### Step 6 — Self-test

Generate 3-4 test questions that span the task types:
- 1 lookup question (find a specific value or definition)
- 1 impact analysis question (list files that must change for a specific change type)
- 1 planning or diagnosis question

For each question:
1. Write the ground truth answer (you just read the repo — you know the correct answer)
2. Mentally evaluate: would an agent with no context pack find this? Would the context pack help?
3. If the pack wouldn't help on any question, the relevant section needs improvement

If the self-test reveals weak sections, improve them and re-seal.

### Step 7 — Commit

Stage and commit as a single commit:
```bash
git add .agent-context/ CLAUDE.md AGENTS.md GEMINI.md
git commit -m "feat: add agent context pack (.agent-context)"
```

---

## Flow: Update (Agent PR)

When you are an agent that has just completed work and is preparing a PR:

### Step 1 — Determine what changed

Review your own work: which files did you create, modify, or delete?

### Step 2 — Map changes to context pack sections

For each changed file, check:
- Is it in `20_CODE_MAP.md`? Does the entry need updating?
- Does it affect a change checklist row in `30_BEHAVIORAL_INVARIANTS.md`?
- Does it affect `search_scope.json` verification shortcuts (line numbers may have shifted)?
- Is it a new file that should be added to CODE_MAP or a completeness contract?
- Is it a deleted file that should be removed from contracts?

### Step 3 — Patch only the affected sections

Edit only the specific lines/entries that are affected. Do NOT rewrite entire files.

### Step 4 — Re-seal

```bash
chorus agent-context seal --force
```

### Step 5 — Commit as a separate commit

```bash
git add .agent-context/
git commit -m "chore: update agent context for <description of code change>"
```

This commit should be separate from the code change commits in the PR, so reviewers can assess them independently.

---

## Flow: Update (Manual Catchup)

When a human asks you to update the context pack after significant changes were merged without agent involvement:

### Step 1 — Find what changed since last seal

```bash
# Find the commit when the pack was last sealed
LAST_SEAL=$(jq -r '.generated_at' .agent-context/current/manifest.json)
echo "Last sealed: $LAST_SEAL"

# Show what changed since then
git log --oneline --since="$LAST_SEAL" -- . ':!.agent-context'
git diff $(git log -1 --before="$LAST_SEAL" --format=%H)..HEAD --stat -- . ':!.agent-context'
```

### Step 2 — Read the diff and propose patches

For each changed area:
1. State what changed in the code
2. State which context pack section is affected
3. Propose the specific edit (show the before/after)
4. Ask the user to approve or reject

Do NOT apply changes without approval. The user authored this code — you may misunderstand the intent.

### Step 3 — Apply approved patches and re-seal

After the user approves each section:

```bash
chorus agent-context seal --force
git add .agent-context/
git commit -m "chore: catchup agent context with recent changes"
```

---

## Quality bar

A context pack is ready when:
- `chorus agent-context seal` passes without errors
- Every markdown file has content (no unfilled template markers)
- Every JSON artifact has at least some repo-specific entries (no all-empty arrays)
- The self-test confirms the pack would help an agent on at least 2 of 3 test questions
- CLAUDE.md routing block is imperative: **"BEFORE starting any task, read these 3 files"** with explicit file list — not "follow the read order" (P16: agents interpret suggestive wording as optional)
- 00_START_HERE.md read order says **"MANDATORY before starting work"** and **"Do NOT open repo source files until steps 1-3"**
- AGENTS.md routing includes search_scope.json reference with "Search ONLY within scoped directories"
- Routing blocks are under 200 tokens each
- `chorus agent-context verify` passes (integrity check)
- CI gate recommended: `chorus agent-context verify --ci` in PR checks (see `templates/ci-agent-context.yml`)

## What NOT to do

- Do not create a context pack for repos with <50 files unless the user explicitly asks
- Do not touch actual repo source files — only `.agent-context/`, `CLAUDE.md`, `AGENTS.md`, `GEMINI.md`
- Do not auto-update the context pack on human-opened PRs — you lack the context of why changes were made
- Do not rewrite the entire pack on updates — patch only affected sections
- Do not include secrets, credentials, or sensitive configuration in the context pack
- Do not add the context pack to `.gitignore` — it is meant to be committed and shared
