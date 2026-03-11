#!/usr/bin/env node
'use strict';

const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

const ZERO_SHA_RE = /^0{40}$/;

function parseArgs(argv) {
  const out = {
    localRef: null,
    localSha: null,
    remoteRef: null,
    remoteSha: null,
  };

  for (let i = 2; i < argv.length; i += 1) {
    const token = argv[i];
    const [name, inlineValue] = token.startsWith('--') ? token.split('=', 2) : [token, null];
    const next = inlineValue != null ? inlineValue : argv[i + 1];

    switch (name) {
      case '--local-ref':
        out.localRef = next || null;
        if (inlineValue == null) i += 1;
        break;
      case '--local-sha':
        out.localSha = next || null;
        if (inlineValue == null) i += 1;
        break;
      case '--remote-ref':
        out.remoteRef = next || null;
        if (inlineValue == null) i += 1;
        break;
      case '--remote-sha':
        out.remoteSha = next || null;
        if (inlineValue == null) i += 1;
        break;
      default:
        break;
    }
  }

  return out;
}

function runGit(args, cwd, allowFailure = false) {
  try {
    return execFileSync('git', args, { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
  } catch (error) {
    if (allowFailure) return '';
    throw error;
  }
}

function isMainPush(localRef, remoteRef) {
  return localRef === 'refs/heads/main' || remoteRef === 'refs/heads/main';
}

function getChangedFiles(repoRoot, baseSha, headSha) {
  if (!headSha || ZERO_SHA_RE.test(headSha)) return [];

  let output = '';
  if (!baseSha || ZERO_SHA_RE.test(baseSha)) {
    output = runGit(['show', '--pretty=format:', '--name-only', headSha], repoRoot, true);
  } else {
    output = runGit(['diff', '--name-only', `${baseSha}..${headSha}`], repoRoot, true);
  }

  return output
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean);
}

/**
 * Load relevance rules from .agent-context/relevance.json if it exists.
 * Returns null if the file is missing or contains invalid JSON.
 * Expected format: { "include": ["pattern", ...], "exclude": ["pattern", ...] }
 */
function loadRelevanceRules(repoRoot) {
  const rulesPath = path.join(repoRoot, '.agent-context', 'relevance.json');
  try {
    const raw = fs.readFileSync(rulesPath, 'utf8');
    const rules = JSON.parse(raw);
    if (rules && typeof rules === 'object' && (Array.isArray(rules.include) || Array.isArray(rules.exclude))) {
      return rules;
    }
    return null;
  } catch (_err) {
    return null;
  }
}

/**
 * Check if a file path matches a glob-like prefix pattern.
 * Supports patterns like "scripts/", "*.md", and exact matches.
 */
function matchesPattern(normalized, pattern) {
  if (pattern.endsWith('/')) {
    return normalized.startsWith(pattern);
  }
  if (pattern.startsWith('*.')) {
    return normalized.endsWith(pattern.slice(1));
  }
  return normalized === pattern;
}

/**
 * Determine if a file is context-relevant using loaded rules or hardcoded defaults.
 */
function isContextRelevant(filePath, rules) {
  const normalized = filePath.replace(/\\/g, '/');

  if (rules) {
    const excludes = rules.exclude || [];
    for (const pattern of excludes) {
      if (matchesPattern(normalized, pattern)) return false;
    }
    const includes = rules.include || [];
    for (const pattern of includes) {
      if (matchesPattern(normalized, pattern)) return true;
    }
    return false;
  }

  // Hardcoded default fallback
  if (
    normalized.startsWith('blog/') ||
    normalized.startsWith('notes/') ||
    normalized.startsWith('drafts/') ||
    normalized.startsWith('scratch/') ||
    normalized.startsWith('tmp/') ||
    normalized.startsWith('.agent-context/') ||
    normalized.startsWith('docs/demo-')
  ) {
    return false;
  }

  if (
    normalized === 'README.md' ||
    normalized === 'PROTOCOL.md' ||
    normalized === 'CONTRIBUTING.md' ||
    normalized === 'SKILL.md' ||
    normalized === 'AGENTS.md' ||
    normalized === 'package.json' ||
    normalized === 'package-lock.json' ||
    normalized === 'cli/Cargo.toml' ||
    normalized === 'cli/Cargo.lock' ||
    normalized === 'docs/architecture.svg' ||
    normalized === 'docs/silo-tax-before-after.webp'
  ) {
    return true;
  }

  return (
    normalized.startsWith('scripts/') ||
    normalized.startsWith('cli/src/') ||
    normalized.startsWith('schemas/') ||
    normalized.startsWith('fixtures/golden/') ||
    normalized.startsWith('fixtures/session-store/') ||
    normalized.startsWith('.github/workflows/')
  );
}

function main() {
  const args = parseArgs(process.argv);
  const repoRoot = runGit(['rev-parse', '--show-toplevel'], process.cwd(), true) || process.cwd();

  if (!isMainPush(args.localRef, args.remoteRef)) {
    process.stdout.write('[context-pack] skipped (push is not targeting main)\n');
    return;
  }

  if (!args.localSha || ZERO_SHA_RE.test(args.localSha)) {
    process.stdout.write('[context-pack] skipped (main deletion or empty local sha)\n');
    return;
  }

  const changedFiles = getChangedFiles(repoRoot, args.remoteSha, args.localSha);
  const rules = loadRelevanceRules(repoRoot);
  const relevant = changedFiles.filter((f) => isContextRelevant(f, rules));

  if (relevant.length === 0) {
    process.stdout.write('[context-pack] skipped (no context-relevant file changes)\n');
    return;
  }

  // Advisory-only: warn but never block the push or auto-build
  process.stderr.write(
    "[context-pack] ADVISORY: context-relevant files changed on main push. " +
    "Update pack content with your agent, then run 'bridge context-pack seal'.\n"
  );
}

main();
