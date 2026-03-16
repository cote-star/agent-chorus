#!/usr/bin/env node
'use strict';

const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

function parseArgs(argv) {
  const out = {
    snapshot: null,
    packDir: process.env.CHORUS_CONTEXT_PACK_DIR || process.env.BRIDGE_CONTEXT_PACK_DIR || '.agent-context',
    cwd: process.cwd(),
  };

  for (let i = 2; i < argv.length; i += 1) {
    const token = argv[i];
    const [name, inlineValue] = token.startsWith('--') ? token.split('=', 2) : [token, null];
    const next = inlineValue != null ? inlineValue : argv[i + 1];

    switch (name) {
      case '--snapshot':
        out.snapshot = next || null;
        if (inlineValue == null) i += 1;
        break;
      case '--pack-dir':
        out.packDir = next || out.packDir;
        if (inlineValue == null) i += 1;
        break;
      case '--cwd':
        out.cwd = next ? path.resolve(next) : out.cwd;
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

function listSnapshotIds(snapshotsDir) {
  if (!fs.existsSync(snapshotsDir)) return [];
  return fs
    .readdirSync(snapshotsDir, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .sort();
}

function main() {
  const args = parseArgs(process.argv);
  const repoRoot = runGit(['rev-parse', '--show-toplevel'], args.cwd, true) || args.cwd;
  const packRoot = path.resolve(repoRoot, args.packDir);
  const currentDir = path.join(packRoot, 'current');
  const snapshotsDir = path.join(packRoot, 'snapshots');

  const snapshotIds = listSnapshotIds(snapshotsDir);
  if (snapshotIds.length === 0) {
    process.stderr.write(`[context-pack] no snapshots found in ${path.relative(repoRoot, snapshotsDir)}\n`);
    process.exit(1);
  }

  const targetSnapshot = args.snapshot || snapshotIds[snapshotIds.length - 1];
  if (!snapshotIds.includes(targetSnapshot)) {
    process.stderr.write(`[context-pack] snapshot not found: ${targetSnapshot}\n`);
    process.exit(1);
  }

  const sourceDir = path.join(snapshotsDir, targetSnapshot);
  fs.rmSync(currentDir, { recursive: true, force: true });
  fs.mkdirSync(currentDir, { recursive: true });
  fs.cpSync(sourceDir, currentDir, { recursive: true });

  process.stdout.write(
    `[context-pack] restored snapshot ${targetSnapshot} -> ${path.relative(repoRoot, currentDir)}\n`
  );
}

main();
