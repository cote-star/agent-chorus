#!/usr/bin/env node
'use strict';

const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

function parseArgs(argv) {
  const opts = {
    packDir: process.env.CHORUS_CONTEXT_PACK_DIR || process.env.BRIDGE_CONTEXT_PACK_DIR || '.agent-context',
    cwd: process.cwd(),
    force: false,
    // P13/F46: adoption tier. 3 = full pack (legacy default), 2 = CODE_MAP +
    // BEHAVIORAL_INVARIANTS + routes + completeness_contract, 1 = CODE_MAP +
    // routes only. Node parity with cli/src/agent_context.rs::InitTier.
    tier: 3,
  };

  for (let i = 2; i < argv.length; i += 1) {
    const token = argv[i];
    const [name, inline] = token.startsWith('--') ? token.split('=', 2) : [token, null];
    const next = inline != null ? inline : argv[i + 1];
    switch (name) {
      case '--pack-dir':
        opts.packDir = next || opts.packDir;
        if (inline == null) i += 1;
        break;
      case '--cwd':
        opts.cwd = next ? path.resolve(next) : opts.cwd;
        if (inline == null) i += 1;
        break;
      case '--force':
        opts.force = true;
        break;
      case '--tier': {
        const parsed = Number.parseInt(next, 10);
        if (![1, 2, 3].includes(parsed)) {
          console.error('[context-pack] init --tier accepts 1, 2, or 3');
          process.exit(1);
        }
        opts.tier = parsed;
        if (inline == null) i += 1;
        break;
      }
      default:
        break;
    }
  }

  return opts;
}

const { runGit, ensureDir, safeWriteText, upsertContextPackBlock } = require('./cp_utils.cjs');

function isNonEmptyDir(dirPath) {
  if (!fs.existsSync(dirPath)) return false;
  const entries = fs.readdirSync(dirPath);
  return entries.length > 0;
}

function nowStamp() {
  return new Date().toISOString();
}

function relPath(target, base) {
  return path.relative(base, target) || target;
}

/**
 * P3: produce the default relevance.json shipped by init, including a
 * `zones[]` array so freshness can map changed files to pack sections.
 * When `hasStudy` is true, include a `study/**` zone; otherwise fall back to
 * a placeholder `docs/**` zone so the default file is always zone-map-valid.
 */
function defaultRelevanceJson(hasStudy = false) {
  const studyZone = hasStudy
    ? '    {"paths": ["study/**", "docs/methodology/**"], "affects": ["10_SYSTEM_OVERVIEW.md", "30_BEHAVIORAL_INVARIANTS.md"]},\n'
    : '    {"paths": ["docs/**"], "affects": ["10_SYSTEM_OVERVIEW.md", "30_BEHAVIORAL_INVARIANTS.md"]},\n';
  return `{
  "include": ["**"],
  "exclude": [
    ".agent-context/**",
    ".git/**",
    "node_modules/**",
    "target/**",
    "dist/**",
    "build/**",
    "vendor/**",
    "tmp/**"
  ],
  "zones": [
${studyZone}    {"paths": ["src/**", "cli/src/**"], "affects": ["20_CODE_MAP.md", "30_BEHAVIORAL_INVARIANTS.md"]},
    {"paths": ["scripts/run_*.py", "scripts/**"], "affects": ["20_CODE_MAP.md", "40_OPERATIONS_AND_RELEASE.md"]},
    {"paths": ["pyproject.toml", "Cargo.toml", "package.json", "cli/Cargo.toml"], "affects": ["40_OPERATIONS_AND_RELEASE.md"]}
  ]
}
`;
}

function guideContent() {
  return `# Context Pack Generation Guide

This guide tells AI agents how to fill in the context pack templates.

## Process
1. Read each file in \`.agent-context/current/\` in numeric order.
2. Fill the markdown templates with repository-derived content.
3. Update the structured files (\`routes.json\`, \`completeness_contract.json\`, \`reporting_rules.json\`) so they describe routing, completeness, and reporting rules.
4. After filling all sections, run \`chorus context-pack seal\` to finalize (manifest + snapshot).

## Quality Criteria
- Content must be factual and verifiable from the repository.
- Prefer concise bullets over long prose.
- Keep total word count under ~2000 words across all files.
- Do not include secrets or credentials.
- If unsure, note \`TBD\` rather than inventing details.
- Structured artifacts should stay deterministic and explicit. Do not auto-generate them from prose in v1.

## When to Update
- After significant architectural or contract changes.
- After adding new commands/APIs/features.
- When \`chorus context-pack check-freshness\` reports stale content.
`;
}

function templateStartHere(repoName, branch, headSha, generatedAt) {
  return `# Context Pack: Start Here

## Snapshot
- Repo: \`${repoName}\`
- Branch at generation: \`${branch}\`
- HEAD commit: \`${headSha}\`
- Generated at: \`${generatedAt}\`

## Read Order — MANDATORY before starting work
1. Read this file completely.
2. Read \`30_BEHAVIORAL_INVARIANTS.md\` — change checklists, file families, negative guidance.
3. Read \`20_CODE_MAP.md\` — navigation index, tracing flows, extension recipe.

Do NOT open repo source files until you have read steps 1-3. These three files give you enough context to avoid common mistakes (wrong patterns, missing files, deprecated approaches).

Read on demand:
- \`10_SYSTEM_OVERVIEW.md\` — for architecture or diagnosis tasks.
- \`40_OPERATIONS_AND_RELEASE.md\` — for test, CI, or deploy tasks.

## Task-Type Routing
**Impact analysis** (list every file that must change): read \`30_BEHAVIORAL_INVARIANTS.md\` Update Checklist *before* \`20_CODE_MAP.md\` — the checklist has the full blast radius per change type. CODE_MAP alone is not exhaustive.
**Navigation / lookup** (find a file, find a value): start with \`20_CODE_MAP.md\` Scope Rule.
**Planning** (add a new feature/module): follow the Extension Recipe in \`20_CODE_MAP.md\`, then cross-check the BEHAVIORAL_INVARIANTS checklist for that change type.
**Diagnosis** (silent failures, unexpected output): start with \`10_SYSTEM_OVERVIEW.md\` Silent Failure Modes, then the relevant diagnostic row in \`30_BEHAVIORAL_INVARIANTS.md\`.

## Structured Routing
- If \`routes.json\` exists, use it as the authoritative task router before opening repo files.
- Use \`completeness_contract.json\` for "what must be included" and \`reporting_rules.json\` for "how to report it".
- Use \`search_scope.json\` for "where to search" — it bounds search directories and lists verification shortcuts.
- If the structured layer and markdown disagree, continue exploring and report the mismatch explicitly.

## Fast Facts
<!-- AGENT: Replace with 3-5 bullets covering product, languages/entry points, quality gate, core risk. -->

## Scope Rule
<!-- AGENT: Provide navigation rules — what to open first for each area of the codebase, what to skip. -->

## Stop Rules
<!-- AGENT: Describe when a task has enough evidence to stop. Keep this short and operational.
Call out grouped reporting defaults, generated-file avoidance, and any cases where agents may keep exploring because the pack and repo diverge. -->
`;
}

function templateSystemOverview() {
  return `# System Overview

<!-- AGENT: Fill by introspecting the repository. -->

## Product Shape
<!-- AGENT: Add package version(s), tracked file count, delivery mechanism(s). -->

## Runtime Architecture
<!-- AGENT: Describe primary execution flow in 3-5 numbered steps. -->

## Silent Failure Modes
<!-- AGENT: List any code paths where a failure produces no error — null return, silent drop, unchecked default.
These are the hardest things to find by reading code and the most valuable to have written down.
Example: "If selector has no match in prompts.yml, resolver returns null — Spark UDF propagates as null row with no error logged."
If none are known, write "None identified." -->

## Command/API Surface
<!-- AGENT: Table | Command/Endpoint | Intent | Primary Source Files | -->

## Tracked Path Density
<!-- AGENT: Summarize top-level directory distribution (git ls-files). -->
`;
}

function templateCodeMap() {
  return `# Code Map

## High-Impact Paths

> **This table is a navigation index, not a complete blast-radius list.** For impact analysis tasks,
> read \`30_BEHAVIORAL_INVARIANTS.md\` Update Checklist first — it has the full file set per change type.
> Use this table to navigate to those files once you know which are relevant. Verify coverage with grep.

<!-- AGENT: Identify 8-15 key paths. Use [Approach 1], [Approach 2], or [Both] in the Approach column
if the repo has coexisting architectural patterns — omit the column if there is only one approach.
Risk must be filled: use "Silent failure if missed", "KeyError at runtime", "Build drift", etc.
Authority must be filled: "authoritative" (edit this file), "derived" (generated/compiled — do not edit directly), or "reference" (read-only context).
| Path | Approach | What | Why It Matters | Risk | Authority |
| --- | --- | --- | --- | --- | --- | -->

## Quick Lookup Shortcuts
<!-- AGENT: Add 4-6 common lookup patterns. Map intent to exact file and what to look for.
| I need to find... | Open this file | Look for |
| --- | --- | --- | -->

## Cross-Cutting Tracing Flows
<!-- AGENT: For changes that ripple through multiple layers, document the full chain.
Example: "New parameter through call chain: schema → step → client → wrapper → tests"
List files in dependency order so agents trace the change correctly. -->

## Minimum Sufficient Evidence
<!-- AGENT: For the 3-5 most common task types, say what minimum evidence closes the task.
Example: "Lookup closes after authoritative file + exact value + one supporting chain if requested."
These rules should align with reporting_rules.json. -->

## Extension Recipe
<!-- AGENT: Describe how to add a new module/adapter/plugin. List all files that must change together. -->
`;
}

function templateInvariants() {
  return `# Behavioral Invariants

<!-- AGENT: List contract-level constraints to preserve. -->

## Core Invariants
<!-- AGENT: 3-8 numbered items. Each must be a testable statement, not a description.
Good: "Every selector in a spec must match an entry in prompts.yml — missing match raises ValueError at sync time."
Bad: "Prompts must be valid." -->

## Update Checklist Before Merging Behavior Changes
<!-- AGENT: One row per common change type. The "Files that must change together" column must list
explicit file paths — not descriptions, not directory names. Agents will use these rows as a checklist.
If a missed file causes a silent production failure, say so explicitly in the row.
| Change type | Files that must change together |
| --- | --- | -->

## File Families
<!-- AGENT: List homogeneous file families where all members change the same way.
For each family, state: the glob pattern, how many members, and whether to report as a family
or enumerate individually. Agents should inspect one representative unless divergence is suspected.
Example: "models/assets_gen/_specs/*.prompt.yml (20 files) — report as family, do not enumerate individually."
Example: "models/assets_gen/_generated/*.yml (17 files) — derived, never list as change targets." -->

## Often Reviewed But Not Always Required
<!-- AGENT: List files or file families that are commonly inspected during a task but should not
automatically be included in the final answer unless the task demands them. This section exists to
separate contractual completeness from optional verification. -->

## Negative Guidance
<!-- AGENT: List patterns that agents commonly over-explore. Be explicit about what NOT to do.
Example: "Do not enumerate _generated/ files individually for impact analysis — they are regenerated by a build step."
Example: "Do not inspect both sync and async wrappers unless the parameter is known to diverge between them."
Example: "Do not open test files to determine blast radius — tests are updated after source, not before." -->
`;

}

function templateOperations() {
  return `# Operations And Release

## Standard Validation
<!-- AGENT: Add local validation commands (tests, linters, etc.). -->

## CI Checks
<!-- AGENT: List CI workflows/steps that gate merges. -->

## Release Flow
<!-- AGENT: Describe how releases are triggered and what they produce. -->

## Context Pack Maintenance
1. Initialize scaffolding: \`chorus context-pack init\` (pre-push hook installed automatically)
2. Have your agent fill in the template sections.
3. Seal the pack: \`chorus context-pack seal\`
4. When freshness warnings appear on push, update content then run \`chorus context-pack seal\`

## Rollback/Recovery
- Restore latest snapshot: \`chorus context-pack rollback\`
- Restore named snapshot: \`chorus context-pack rollback --snapshot <snapshot_id>\`
`;
}

function templateRoutesJson() {
  return `${JSON.stringify({
    schema_version: 1,
    task_routes: {
      lookup: {
        description: 'Find a value, threshold, URL, or authoritative file.',
        pack_read_order: ['00_START_HERE.md', '20_CODE_MAP.md', 'reporting_rules.json'],
        fallback_files: ['30_BEHAVIORAL_INVARIANTS.md'],
        completeness_ref: 'lookup',
        reporting_ref: 'lookup',
      },
      impact_analysis: {
        description: 'List every file or file family that must change.',
        pack_read_order: [
          '00_START_HERE.md',
          '30_BEHAVIORAL_INVARIANTS.md',
          'completeness_contract.json',
          'reporting_rules.json',
          '20_CODE_MAP.md',
        ],
        fallback_files: ['10_SYSTEM_OVERVIEW.md'],
        completeness_ref: 'impact_analysis',
        reporting_ref: 'impact_analysis',
      },
      planning: {
        description: 'Write an implementation plan with files, commands, and validation.',
        pack_read_order: [
          '00_START_HERE.md',
          '20_CODE_MAP.md',
          '30_BEHAVIORAL_INVARIANTS.md',
          'completeness_contract.json',
          'reporting_rules.json',
        ],
        fallback_files: ['40_OPERATIONS_AND_RELEASE.md'],
        completeness_ref: 'planning',
        reporting_ref: 'planning',
      },
      diagnosis: {
        description: 'Rank likely root causes and cite the runtime path.',
        pack_read_order: [
          '00_START_HERE.md',
          '10_SYSTEM_OVERVIEW.md',
          '30_BEHAVIORAL_INVARIANTS.md',
          'completeness_contract.json',
          'reporting_rules.json',
        ],
        fallback_files: ['20_CODE_MAP.md'],
        completeness_ref: 'diagnosis',
        reporting_ref: 'diagnosis',
      },
    },
  }, null, 2)}\n`;
}

function templateCompletenessContractJson() {
  return `${JSON.stringify({
    schema_version: 1,
    task_families: {
      lookup: {
        minimum_sufficient_evidence: [
          'exact answer',
          'authoritative source path',
          'one supporting chain only if the task asks for authority',
        ],
        required_chain_members: [],
        contractually_required_files: [],
        required_file_families: [],
      },
      impact_analysis: {
        minimum_sufficient_evidence: [
          'complete blast radius',
          'required file families',
          'contractually required pass-through layers',
        ],
        required_chain_members: [],
        contractually_required_files: [],
        required_file_families: [],
      },
      planning: {
        minimum_sufficient_evidence: [
          'files to create or modify',
          'commands in order',
          'validation criteria',
        ],
        required_chain_members: [],
        contractually_required_files: [],
        required_file_families: [],
      },
      diagnosis: {
        minimum_sufficient_evidence: [
          'ranked root causes',
          'runtime path or failure chain',
          'confirmation method for each cause',
        ],
        required_chain_members: [],
        contractually_required_files: [],
        required_file_families: [],
      },
    },
  }, null, 2)}\n`;
}

function templateReportingRulesJson() {
  return `${JSON.stringify({
    schema_version: 1,
    global_rules: {
      grouped_reporting_default: true,
      authoritative_vs_derived_paths: [],
    },
    task_families: {
      lookup: {
        optional_verify_budget: 1,
        stop_after: 'Stop after the authoritative source and one optional supporting check.',
        stop_unless: [
          'a structured artifact references a missing file',
          'markdown and structured artifacts disagree',
          'code contradicts the structured contract',
          'the task explicitly asks for concrete instances rather than grouped families',
        ],
        groupable_families: [],
        never_enumerate_individually: [],
      },
      impact_analysis: {
        optional_verify_budget: 2,
        stop_after: 'Stop after the blast radius is complete and required families are grouped correctly.',
        stop_unless: [
          'a structured artifact references a missing file',
          'markdown and structured artifacts disagree',
          'code contradicts the structured contract',
          'the task explicitly asks for concrete instances rather than grouped families',
        ],
        groupable_families: [],
        never_enumerate_individually: [],
      },
      planning: {
        optional_verify_budget: 2,
        stop_after: 'Stop after the plan is executable without further repo browsing.',
        stop_unless: [
          'a structured artifact references a missing file',
          'markdown and structured artifacts disagree',
          'code contradicts the structured contract',
          'the task explicitly asks for concrete instances rather than grouped families',
        ],
        groupable_families: [],
        never_enumerate_individually: [],
      },
      diagnosis: {
        optional_verify_budget: 3,
        stop_after: 'Stop after the ranked runtime chain is established and each cause has a confirmation method.',
        stop_unless: [
          'a structured artifact references a missing file',
          'markdown and structured artifacts disagree',
          'code contradicts the structured contract',
          'the task explicitly asks for concrete instances rather than grouped families',
        ],
        groupable_families: [],
        never_enumerate_individually: [],
      },
    },
  }, null, 2)}\n`;
}

function templateSearchScopeJson() {
  return `${JSON.stringify({
    schema_version: 1,
    description: 'Search scope boundaries for search-and-verify agents (e.g. Codex). Bounds WHERE to search, not WHEN to stop.',
    task_families: {
      lookup: {
        search_directories: [],
        exclude_from_search: [],
        verification_shortcuts: {},
      },
      impact_analysis: {
        search_directories: [],
        exclude_from_search: [],
        verification_shortcuts: {},
        derived_file_policy: 'Do not list generated/compiled/bundled output files as change targets. They are produced by a build/generate step.',
      },
      planning: {
        search_directories: [],
        exclude_from_search: [],
        verification_shortcuts: {},
      },
      diagnosis: {
        search_directories: [],
        exclude_from_search: [],
        verification_shortcuts: {},
      },
    },
  }, null, 2)}\n`;
}

// P13/F47: mandatory first-line freshness gate. Kept identical across agent
// variants so every routing block carries the same contract. Must stay in
// lockstep with cli/src/agent_context.rs::build_context_pack_routing_block.
const FRESHNESS_GATE =
  '**Before any reasoning, check `.agent-context/current/manifest.json`\'s `head_sha_at_seal` vs `git rev-parse HEAD`. If they diverge, warn the user.**';

function buildContextPackRoutingBlock(agentKind) {
  if (agentKind === 'codex') {
    return `## Context Pack

${FRESHNESS_GATE}

When asked to understand this repository:

1. Read \`.agent-context/current/00_START_HERE.md\`.
2. Read \`.agent-context/current/routes.json\`.
3. Identify the active task type in \`routes.json\`.
4. Read the matching entries in \`completeness_contract.json\`, \`reporting_rules.json\`, and \`search_scope.json\`.
5. Search ONLY within the directories listed in \`search_scope.json\` for your task type.
6. Use \`verification_shortcuts\` to check specific line ranges instead of reading full files.
7. Do not enumerate files in directories marked \`exclude_from_search\`.
8. Do not open repo files before those steps unless a referenced structured file is missing.

If \`.agent-context/current/routes.json\` is missing, fall back to the markdown pack only.`;
  }

  return `## Context Pack

${FRESHNESS_GATE}

**BEFORE starting any task**, read the context pack in this order:

1. \`.agent-context/current/00_START_HERE.md\` — entrypoint, routing, stop rules
2. \`.agent-context/current/30_BEHAVIORAL_INVARIANTS.md\` — change checklists, file families, what NOT to do
3. \`.agent-context/current/20_CODE_MAP.md\` — navigation index, tracing flows

Read these three files BEFORE opening any repo source files. Then open only the files the pack identifies as relevant.

For architecture questions, also read \`10_SYSTEM_OVERVIEW.md\`. For test/deploy questions, also read \`40_OPERATIONS_AND_RELEASE.md\`.`;
}

function main() {
  const opts = parseArgs(process.argv);
  const repoRoot =
    runGit(['rev-parse', '--show-toplevel'], opts.cwd, true) || opts.cwd;
  const repoName = path.basename(repoRoot);
  const branch = runGit(['rev-parse', '--abbrev-ref', 'HEAD'], repoRoot, true) || 'unknown';
  const headSha = runGit(['rev-parse', 'HEAD'], repoRoot, true) || 'unknown';

  const packRoot = path.isAbsolute(opts.packDir)
    ? opts.packDir
    : path.join(repoRoot, opts.packDir);
  const currentDir = path.join(packRoot, 'current');
  const guidePath = path.join(packRoot, 'GUIDE.md');
  const relevancePath = path.join(packRoot, 'relevance.json');

  if (fs.existsSync(currentDir) && !opts.force && isNonEmptyDir(currentDir)) {
    console.error(
      `[context-pack] init aborted: ${relPath(currentDir, repoRoot)} is not empty (use --force to overwrite)`
    );
    process.exit(1);
  }

  ensureDir(currentDir);

  const generatedAt = nowStamp();

  // P13/F46: scaffold only the files the requested tier defines.
  // Tier 3 preserves legacy behavior (full pack).
  let outputs;
  if (opts.tier === 1) {
    outputs = [
      ['20_CODE_MAP.md', templateCodeMap()],
      ['routes.json', templateRoutesJson()],
    ];
  } else if (opts.tier === 2) {
    outputs = [
      ['20_CODE_MAP.md', templateCodeMap()],
      ['30_BEHAVIORAL_INVARIANTS.md', templateInvariants()],
      ['routes.json', templateRoutesJson()],
      ['completeness_contract.json', templateCompletenessContractJson()],
    ];
  } else {
    outputs = [
      ['00_START_HERE.md', templateStartHere(repoName, branch, headSha, generatedAt)],
      ['10_SYSTEM_OVERVIEW.md', templateSystemOverview()],
      ['20_CODE_MAP.md', templateCodeMap()],
      ['30_BEHAVIORAL_INVARIANTS.md', templateInvariants()],
      ['40_OPERATIONS_AND_RELEASE.md', templateOperations()],
      ['routes.json', templateRoutesJson()],
      ['completeness_contract.json', templateCompletenessContractJson()],
      ['reporting_rules.json', templateReportingRulesJson()],
      ['search_scope.json', templateSearchScopeJson()],
    ];
  }

  for (const [filename, content] of outputs) {
    safeWriteText(path.join(currentDir, filename), content);
  }

  if (!fs.existsSync(relevancePath) || opts.force) {
    // P3: when a `study/` directory exists at repo root, tailor the default
    // zone map to include it so freshness surfaces the right pack sections.
    const hasStudy = fs.existsSync(path.join(repoRoot, 'study')) &&
      fs.statSync(path.join(repoRoot, 'study')).isDirectory();
    safeWriteText(relevancePath, defaultRelevanceJson(hasStudy));
  }

  if (!fs.existsSync(guidePath) || opts.force) {
    safeWriteText(guidePath, guideContent());
  }

  // Wire agent config files with context-pack routing instructions.
  const agentConfigs = [
    ['CLAUDE.md', 'agent-chorus:context-pack:claude', buildContextPackRoutingBlock('claude')],
    ['AGENTS.md', 'agent-chorus:context-pack:codex', buildContextPackRoutingBlock('codex')],
    ['GEMINI.md', 'agent-chorus:context-pack:gemini', buildContextPackRoutingBlock('gemini')],
  ];

  for (const [filename, marker, routingBlock] of agentConfigs) {
    upsertContextPackBlock(path.join(repoRoot, filename), routingBlock, marker);
  }
  console.log('[context-pack] agent config files wired (CLAUDE.md, AGENTS.md, GEMINI.md)');

  // Auto-install the pre-push hook so freshness warnings fire on every main push.
  const installHooksScript = path.join(__dirname, 'install_hooks.cjs');
  try {
    execFileSync(process.execPath, [installHooksScript, '--cwd', repoRoot], {
      stdio: ['ignore', 'pipe', 'pipe'],
      encoding: 'utf8',
    });
    console.log('[context-pack] pre-push hook installed');
  } catch (_err) {
    console.warn('[context-pack] WARN: could not auto-install pre-push hook — run `chorus context-pack install-hooks` manually');
  }

  console.log(
    `[context-pack] init completed: ${relPath(currentDir, repoRoot)}`
  );
  console.log(
    '[context-pack] next: fill markdown + structured files, then run `chorus context-pack seal`'
  );
}

main();
