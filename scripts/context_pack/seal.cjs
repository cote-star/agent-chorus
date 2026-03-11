#!/usr/bin/env node
'use strict';

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { execFileSync } = require('child_process');

const REQUIRED_FILES = [
  '00_START_HERE.md',
  '10_SYSTEM_OVERVIEW.md',
  '20_CODE_MAP.md',
  '30_BEHAVIORAL_INVARIANTS.md',
  '40_OPERATIONS_AND_RELEASE.md',
];

function parseArgs(argv) {
  const opts = {
    reason: 'manual-seal',
    base: null,
    head: null,
    packDir: process.env.BRIDGE_CONTEXT_PACK_DIR || '.agent-context',
    cwd: process.cwd(),
    force: false,
    forceSnapshot: false,
  };

  for (let i = 2; i < argv.length; i += 1) {
    const token = argv[i];
    const [name, inline] = token.startsWith('--') ? token.split('=', 2) : [token, null];
    const next = inline != null ? inline : argv[i + 1];
    switch (name) {
      case '--reason':
        opts.reason = next || opts.reason;
        if (inline == null) i += 1;
        break;
      case '--base':
        opts.base = next || null;
        if (inline == null) i += 1;
        break;
      case '--head':
        opts.head = next || null;
        if (inline == null) i += 1;
        break;
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
      case '--force-snapshot':
        opts.forceSnapshot = true;
        break;
      default:
        break;
    }
  }

  return opts;
}

const {
  runGit,
  ensureDir,
  isProcessRunning,
  safeWriteTextAtomic,
} = require('./cp_utils.cjs');

function sha256(input) {
  return crypto.createHash('sha256').update(input).digest('hex');
}

function readJson(filePath) {
  if (!fs.existsSync(filePath)) return null;
  return JSON.parse(fs.readFileSync(filePath, 'utf8'));
}

function sha256(input) {
  return crypto.createHash('sha256').update(input).digest('hex');
}

function readJson(filePath) {
  if (!fs.existsSync(filePath)) return null;
  return JSON.parse(fs.readFileSync(filePath, 'utf8'));
}

function collectFilesMeta(currentDir, relativePaths) {
  return relativePaths.map((relativePath) => {
    const absolutePath = path.join(currentDir, relativePath);
    const content = fs.readFileSync(absolutePath, 'utf8');
    return {
      path: relativePath,
      sha256: sha256(content),
      bytes: fs.statSync(absolutePath).size,
      words: (content.match(/\S+/g) || []).length,
    };
  });
}

function buildManifest({
  generatedAt,
  repoRoot,
  repoName,
  branch,
  headSha,
  reason,
  baseSha,
  filesMeta,
}) {
  const packChecksum = sha256(filesMeta.map((m) => `${m.path}:${m.sha256}`).join('\n'));
  const stableChecksum = sha256(
    filesMeta
      .filter((m) => m.path !== '00_START_HERE.md')
      .map((m) => `${m.path}:${m.sha256}`)
      .join('\n')
  );

  const wordsTotal = filesMeta.reduce((sum, m) => sum + m.words, 0);
  const bytesTotal = filesMeta.reduce((sum, m) => sum + m.bytes, 0);

  return {
    value: {
      schema_version: 1,
      generated_at: generatedAt,
      repo_name: repoName,
      repo_root: repoRoot,
      branch,
      head_sha: headSha || null,
      package_version: 'unknown',
      cargo_version: 'unknown',
      build_reason: reason,
      base_sha: baseSha || null,
      changed_files: [],
      files_count: filesMeta.length,
      words_total: wordsTotal,
      bytes_total: bytesTotal,
      pack_checksum: packChecksum,
      stable_checksum: stableChecksum,
      files: filesMeta,
    },
    stable_checksum: stableChecksum,
    pack_checksum: packChecksum,
  };
}

function appendHistory(historyPath, entry) {
  ensureDir(path.dirname(historyPath));
  fs.appendFileSync(historyPath, `${JSON.stringify(entry)}\n`, 'utf8');
}

function copyDir(source, destination) {
  ensureDir(path.dirname(destination));
  fs.cpSync(source, destination, { recursive: true });
}

function shortSha(sha) {
  if (!sha || /^0{40}$/.test(sha)) return 'none';
  return sha.slice(0, 12);
}

function compactTimestamp(iso) {
  return iso.replace(/[-:]/g, '').replace(/\.\d+Z$/, 'Z');
}

function acquireLock(lockPath) {
  try {
    const fd = fs.openSync(lockPath, 'wx');
    fs.writeFileSync(fd, `${process.pid}\n`);
    return () => {
      try {
        fs.unlinkSync(lockPath);
      } catch (_) {
        /* ignore */
      }
    };
  } catch (error) {
    if (error.code === 'EEXIST') {
      try {
        const pidContent = fs.readFileSync(lockPath, 'utf8').trim();
        const pid = parseInt(pidContent, 10);
        if (!isNaN(pid) && !isProcessRunning(pid)) {
          console.error(`[context-pack] WARNING: cleaned stale lock (pid ${pid} no longer running)`);
          fs.unlinkSync(lockPath);
          return acquireLock(lockPath);
        }
      } catch (readError) {
        // Fall through to original error if we can't read/process the lockfile
      }
    }
    throw new Error(`[context-pack] another seal is in progress (lock: ${lockPath}): ${error.message}`);
  }
}

function main() {
  const opts = parseArgs(process.argv);
  const repoRoot = runGit(['rev-parse', '--show-toplevel'], opts.cwd, true) || opts.cwd;
  const repoName = path.basename(repoRoot);
  const branch = runGit(['rev-parse', '--abbrev-ref', 'HEAD'], repoRoot, true) || 'unknown';
  const headSha = opts.head || runGit(['rev-parse', 'HEAD'], repoRoot, true) || null;

  const packRoot = path.isAbsolute(opts.packDir)
    ? opts.packDir
    : path.join(repoRoot, opts.packDir);
  const currentDir = path.join(packRoot, 'current');
  const snapshotsDir = path.join(packRoot, 'snapshots');
  const historyPath = path.join(packRoot, 'history.jsonl');
  const manifestPath = path.join(currentDir, 'manifest.json');
  const lockPath = path.join(packRoot, 'seal.lock');

  if (!fs.existsSync(currentDir)) {
    console.error(
      `[context-pack] seal failed: ${path.relative(repoRoot, currentDir)} does not exist (run init first)`
    );
    process.exit(1);
  }

  const releaseLock = acquireLock(lockPath);
  try {
    ensureDir(snapshotsDir);

    for (const file of REQUIRED_FILES) {
      const filePath = path.join(currentDir, file);
      if (!fs.existsSync(filePath)) {
        throw new Error(
          `[context-pack] seal failed: missing required file ${path.relative(repoRoot, filePath)}`
        );
      }
      if (!opts.force) {
        const content = fs.readFileSync(filePath, 'utf8');
        if (content.includes('<!-- AGENT:')) {
          throw new Error(
            `[context-pack] seal failed: template markers remain in ${path.relative(
              repoRoot,
              filePath
            )} (use --force to override)`
          );
        }
      }
    }

    const generatedAt = new Date().toISOString();
    const filesMeta = collectFilesMeta(currentDir, REQUIRED_FILES);

    const manifest = buildManifest({
      generatedAt,
      repoRoot,
      repoName,
      branch,
      headSha,
      reason: opts.reason,
      baseSha: opts.base,
      filesMeta,
    });

    const previous = readJson(manifestPath);
    const previousStable = previous?.stable_checksum;
    const previousHead = previous?.head_sha;

    safeWriteTextAtomic(manifestPath, `${JSON.stringify(manifest.value, null, 2)}\n`);

    const changed =
      opts.forceSnapshot ||
      !previous ||
      previousStable !== manifest.stable_checksum ||
      previousHead !== headSha;

    if (changed) {
      let snapshotId = `${compactTimestamp(generatedAt)}_${shortSha(headSha)}`;
      let snapshotDir = path.join(snapshotsDir, snapshotId);
      let counter = 1;
      while (fs.existsSync(snapshotDir)) {
        snapshotId = `${compactTimestamp(generatedAt)}_${shortSha(headSha)}-${counter}`;
        snapshotDir = path.join(snapshotsDir, snapshotId);
        counter += 1;
      }

      copyDir(currentDir, snapshotDir);

      appendHistory(historyPath, {
        snapshot_id: snapshotId,
        generated_at: generatedAt,
        branch,
        head_sha: headSha,
        base_sha: opts.base,
        reason: opts.reason,
        changed_files: [],
        pack_checksum: manifest.pack_checksum,
      });

      console.log(
        `[context-pack] sealed: ${path.relative(repoRoot, packRoot)} (snapshot ${snapshotId})`
      );
    } else {
      console.log('[context-pack] unchanged; no new snapshot created');
    }
  } catch (error) {
    console.error(error.message || error);
    process.exit(1);
  } finally {
    releaseLock();
  }
}

main();
