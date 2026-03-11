#!/usr/bin/env node
'use strict';

const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

function parseArgs(argv) {
  const opts = {
    packDir: process.env.BRIDGE_CONTEXT_PACK_DIR || '.agent-context',
    cwd: process.cwd(),
    force: false,
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
      default:
        break;
    }
  }

  return opts;
}

const { runGit, ensureDir, safeWriteText } = require('./cp_utils.cjs');

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

function defaultRelevanceJson() {
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
  ]
}
`;
}

function guideContent() {
  return `# Context Pack Generation Guide

This guide tells AI agents how to fill in the context pack templates.

## Process
1. Read each file in \`.agent-context/current/\` in numeric order.
2. For each \`<!-- AGENT: ... -->\` block, replace it with repository-derived content.
3. After filling all sections, run \`bridge context-pack seal\` to finalize (manifest + snapshot).

## Quality Criteria
- Content must be factual and verifiable from the repository.
- Prefer concise bullets over long prose.
- Keep total word count under ~2000 words across all files.
- Do not include secrets or credentials.
- If unsure, note \`TBD\` rather than inventing details.

## When to Update
- After significant architectural or contract changes.
- After adding new commands/APIs/features.
- When \`bridge context-pack check-freshness\` reports stale content.
`;
}

function templateStartHere(repoName, branch, headSha, generatedAt) {
  return `# Context Pack: Start Here

## Snapshot
- Repo: \`${repoName}\`
- Branch at generation: \`${branch}\`
- HEAD commit: \`${headSha}\`
- Generated at: \`${generatedAt}\`

## Read Order (Token-Efficient)
1. Read this file.
2. Read \`10_SYSTEM_OVERVIEW.md\` for architecture and execution paths.
3. Read \`30_BEHAVIORAL_INVARIANTS.md\` before changing behavior.
4. Use \`20_CODE_MAP.md\` to deep dive only relevant files.
5. Use \`40_OPERATIONS_AND_RELEASE.md\` for tests, release, and maintenance.

## Fast Facts
<!-- AGENT: Replace with 3-5 bullets covering product, languages/entry points, quality gate, core risk. -->

## Scope Rule
For "understand this repo end-to-end" requests:
<!-- AGENT: Provide scope/navigation rules (when to open code, what to read first). -->
`;
}

function templateSystemOverview() {
  return `# System Overview

<!-- AGENT: Fill by introspecting the repository. -->

## Product Shape
<!-- AGENT: Add package version(s), tracked file count, delivery mechanism(s). -->

## Runtime Architecture
<!-- AGENT: Describe primary execution flow in 3-5 numbered steps. -->

## Command/API Surface
<!-- AGENT: Table | Command/Endpoint | Intent | Primary Source Files | -->

## Tracked Path Density
<!-- AGENT: Summarize top-level directory distribution (git ls-files). -->
`;
}

function templateCodeMap() {
  return `# Code Map

## High-Impact Paths
<!-- AGENT: Identify 8-15 key paths.
| Path | What | Why It Matters | Change Risk |
| --- | --- | --- | --- | -->

## Extension Recipe
<!-- AGENT: Describe how to add a new module/adapter/plugin if applicable. -->
`;
}

function templateInvariants() {
  return `# Behavioral Invariants

<!-- AGENT: List contract-level constraints to preserve. -->

## Core Invariants
<!-- AGENT: 3-8 numbered items covering protocol/error/schema/flag invariants. -->

## Update Checklist Before Merging Behavior Changes
<!-- AGENT: List files/areas that must be updated together when behavior changes. -->
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
1. Initialize scaffolding: \`bridge context-pack init\`
2. Have your agent fill in the template sections.
3. Seal the pack: \`bridge context-pack seal\`
4. Install pre-push hook: \`bridge context-pack install-hooks\`
5. When freshness warnings appear, update content then run \`bridge context-pack seal\`

## Rollback/Recovery
- Restore latest snapshot: \`bridge context-pack rollback\`
- Restore named snapshot: \`bridge context-pack rollback --snapshot <snapshot_id>\`
`;
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

  const outputs = [
    ['00_START_HERE.md', templateStartHere(repoName, branch, headSha, generatedAt)],
    ['10_SYSTEM_OVERVIEW.md', templateSystemOverview()],
    ['20_CODE_MAP.md', templateCodeMap()],
    ['30_BEHAVIORAL_INVARIANTS.md', templateInvariants()],
    ['40_OPERATIONS_AND_RELEASE.md', templateOperations()],
  ];

  for (const [filename, content] of outputs) {
    safeWriteText(path.join(currentDir, filename), content);
  }

  if (!fs.existsSync(relevancePath) || opts.force) {
    safeWriteText(relevancePath, defaultRelevanceJson());
  }

  if (!fs.existsSync(guidePath) || opts.force) {
    safeWriteText(guidePath, guideContent());
  }

  console.log(
    `[context-pack] init completed: ${relPath(currentDir, repoRoot)}`
  );
  console.log(
    '[context-pack] next: ask your agent to fill AGENT sections, then run `bridge context-pack seal`'
  );
}

main();
