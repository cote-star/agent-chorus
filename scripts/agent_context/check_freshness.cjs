#!/usr/bin/env node
'use strict';

const fs = require('fs');
const { execFileSync } = require('child_process');
const path = require('path');
const relevance = require('./relevance.cjs');

function parseArgs(argv) {
  const options = {
    base: 'origin/main',
    cwd: process.cwd(),
  };

  for (let i = 2; i < argv.length; i += 1) {
    const token = argv[i];
    const [name, inlineValue] = token.startsWith('--') ? token.split('=', 2) : [token, null];
    const next = inlineValue != null ? inlineValue : argv[i + 1];

    switch (name) {
      case '--base':
        if (next) options.base = next;
        if (inlineValue == null) i += 1;
        break;
      case '--cwd':
        if (next) options.cwd = next;
        if (inlineValue == null) i += 1;
        break;
      default:
        if (!token.startsWith('--')) {
          options.base = token;
        }
        break;
    }
  }

  return options;
}

function runGit(args, cwd, allowFailure = false) {
  try {
    return execFileSync('git', args, {
      cwd,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
    }).trim();
  } catch (error) {
    if (allowFailure) return '';
    throw error;
  }
}

// P9 F27: detect whether cwd is inside a git repository.
function isGitRepo(cwd) {
  return runGit(['rev-parse', '--git-dir'], cwd, true) !== '';
}

// P9 F24: detect a shallow clone (CI fetch-depth=1).
function isShallowRepo(cwd) {
  return runGit(['rev-parse', '--is-shallow-repository'], cwd, true) === 'true';
}

// P9 F25: detect initial-commit (no HEAD~1 to diff against).
function commitCount(cwd) {
  const raw = runGit(['rev-list', '--count', 'HEAD'], cwd, true);
  const n = Number.parseInt(raw, 10);
  return Number.isFinite(n) ? n : null;
}

function getChangedFiles(base, cwd) {
  const withBase = runGit(['diff', '--name-only', `${base}...HEAD`], cwd, true);
  if (withBase) {
    return withBase.split('\n').map((line) => line.trim()).filter(Boolean);
  }

  const fallback = runGit(['diff', '--name-only', 'HEAD~1'], cwd, true);
  return fallback.split('\n').map((line) => line.trim()).filter(Boolean);
}


function main() {
  const options = parseArgs(process.argv);

  // P9 F27: non-git directory → explicit skipped, not silent pass.
  if (!isGitRepo(options.cwd)) {
    process.stdout.write('SKIPPED agent-context-freshness (non-git)\n');
    return;
  }

  // P9 F24: shallow clone → skipped with guidance rather than empty-diff "pass".
  if (isShallowRepo(options.cwd)) {
    process.stdout.write(
      'SKIPPED agent-context-freshness (shallow-clone: increase fetch-depth to >=20)\n'
    );
    return;
  }

  // P9 F25: initial commit → no HEAD~1 to diff against; surface explicitly.
  if (commitCount(options.cwd) === 1) {
    process.stdout.write('SKIPPED agent-context-freshness (initial-commit)\n');
    return;
  }

  const changedFiles = getChangedFiles(options.base, options.cwd);
  const config = relevance.loadRelevanceConfig(options.cwd);
  // P3: optionally load the zone map so we can surface affected pack sections.
  // Falls back to legacy include/exclude relevance when zones are absent.
  const zoneMap = relevance.loadZoneMap(options.cwd);

  let packTouched = false;
  const relevant = [];
  const affectedSet = new Set();

  for (const filePath of changedFiles) {
    if (filePath.startsWith('.agent-context/current/')) {
      packTouched = true;
      continue;
    }

    if (zoneMap && zoneMap.length > 0) {
      const sections = relevance.resolveAffectedSections(filePath, zoneMap);
      if (sections.length > 0) {
        relevant.push(filePath);
        for (const s of sections) affectedSet.add(s);
      }
    } else if (relevance.isRelevant(filePath, config)) {
      relevant.push(filePath);
    }
  }

  if (relevant.length === 0) {
    process.stdout.write('PASS agent-context-freshness (no context-relevant files changed)\n');
    return;
  }

  if (packTouched) {
    process.stdout.write('PASS agent-context-freshness (agent-context was updated)\n');
    return;
  }

  process.stdout.write(
    `WARNING: ${relevant.length} context-relevant file(s) changed but .agent-context/current/ was not updated:\n`
  );
  for (const filePath of relevant) {
    process.stdout.write(`  - ${filePath}\n`);
  }
  // P3: surface affected pack sections so agents know which files to patch.
  const affectedSorted = [...affectedSet].sort();
  if (affectedSorted.length > 0) {
    process.stdout.write('\nAffected pack sections:\n');
    for (const s of affectedSorted) {
      process.stdout.write(`  - ${s}\n`);
    }
  }
  process.stdout.write('\n');
  process.stdout.write(
    'Consider: update pack content with your agent, then run chorus agent-context seal\n'
  );

  // P6: persist the warning so a later pack-only push can detect
  // "warning appears addressed". Mirrors the Rust-side
  // `write_last_freshness_state` in cli/src/agent_context.rs.
  try {
    const repoRoot = runGit(['rev-parse', '--show-toplevel'], options.cwd, true) || options.cwd;
    const currentDir = path.join(repoRoot, '.agent-context', 'current');
    if (fs.existsSync(currentDir)) {
      const statePath = path.join(currentDir, '.last_freshness.json');
      const payload = {
        changed_files: relevant,
        affected_sections: affectedSorted,
        timestamp: Math.floor(Date.now() / 1000),
      };
      fs.writeFileSync(statePath, JSON.stringify(payload, null, 2), 'utf8');
    }
  } catch (_err) {
    // Best-effort: state-file failure must not break freshness reporting.
  }
}

main();
