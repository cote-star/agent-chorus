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
    // P13/F58 — Node parity for `--latest-good`. Resolves via the manifest's
    // `last_known_good_sha` field.
    latestGood: false,
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
      case '--latest-good':
        out.latestGood = true;
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

/**
 * P13/F58 — look up the snapshot whose history.jsonl entry's head_sha matches
 * the target SHA. Mirrors cli/src/agent_context.rs::find_snapshot_for_head_sha.
 * Returns null when no match is found.
 */
function findSnapshotForHeadSha(packRoot, targetSha) {
  const scanFile = (p) => {
    if (!fs.existsSync(p)) return null;
    let raw;
    try {
      raw = fs.readFileSync(p, 'utf8');
    } catch (_) {
      return null;
    }
    let best = null;
    for (const line of raw.split('\n')) {
      const trimmed = line.trim();
      if (!trimmed) continue;
      let value;
      try {
        value = JSON.parse(trimmed);
      } catch (_) {
        continue;
      }
      if (value && value.head_sha === targetSha && value.snapshot_id) {
        best = value.snapshot_id;
      }
    }
    return best;
  };

  const active = scanFile(path.join(packRoot, 'history.jsonl'));
  if (active) return active;

  const indexPath = path.join(packRoot, 'history_index.json');
  if (fs.existsSync(indexPath)) {
    try {
      const index = JSON.parse(fs.readFileSync(indexPath, 'utf8'));
      if (Array.isArray(index?.files)) {
        for (const entry of index.files) {
          if (entry && typeof entry.name === 'string') {
            const hit = scanFile(path.join(packRoot, entry.name));
            if (hit) return hit;
          }
        }
      }
    } catch (_) {
      // swallow
    }
  }
  return null;
}

function main() {
  const args = parseArgs(process.argv);
  if (args.latestGood && args.snapshot) {
    process.stderr.write(
      '[context-pack] rollback: --latest-good and --snapshot are mutually exclusive\n'
    );
    process.exit(1);
  }
  const repoRoot = runGit(['rev-parse', '--show-toplevel'], args.cwd, true) || args.cwd;
  const packRoot = path.resolve(repoRoot, args.packDir);
  const currentDir = path.join(packRoot, 'current');
  const snapshotsDir = path.join(packRoot, 'snapshots');

  const snapshotIds = listSnapshotIds(snapshotsDir);
  if (snapshotIds.length === 0) {
    process.stderr.write(`[context-pack] no snapshots found in ${path.relative(repoRoot, snapshotsDir)}\n`);
    process.exit(1);
  }

  let targetSnapshot;
  if (args.latestGood) {
    // P13/F58 — resolve the manifest's last_known_good_sha then walk history.
    const manifestPath = path.join(currentDir, 'manifest.json');
    let manifest;
    try {
      manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
    } catch (_) {
      manifest = null;
    }
    const goodSha = manifest && typeof manifest.last_known_good_sha === 'string'
      ? manifest.last_known_good_sha
      : null;
    if (!goodSha) {
      process.stderr.write(
        '[context-pack] rollback --latest-good failed: manifest.json has no `last_known_good_sha` ' +
          '(run `verify --ci` on a green commit first)\n'
      );
      process.exit(1);
    }
    const resolved = findSnapshotForHeadSha(packRoot, goodSha);
    if (!resolved) {
      process.stderr.write(
        `[context-pack] rollback --latest-good failed: no snapshot matches last_known_good_sha \`${goodSha}\`\n`
      );
      process.exit(1);
    }
    targetSnapshot = resolved;
  } else {
    targetSnapshot = args.snapshot || snapshotIds[snapshotIds.length - 1];
  }

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
