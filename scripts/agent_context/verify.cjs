/**
 * Context pack integrity verification with optional freshness checking.
 * Validates manifest.json checksums against actual file content.
 * With --ci, combines integrity + freshness into a single JSON report.
 */

'use strict';

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { execFileSync } = require('child_process');
const relevance = require('./relevance.cjs');

function sha256(input) {
  return crypto.createHash('sha256').update(input).digest('hex');
}

function parseArgs(argv) {
  const opts = {
    packDir: '.agent-context',
    ci: false,
    base: 'origin/main',
    cwd: process.cwd(),
    // TODO(P10): implement --repair / --yes parity here. For now the flag is
    // accepted + surfaced so parity fixture runs don't explode on the Node side;
    // it returns a clear "not yet implemented" message. See
    // cli/src/agent_context.rs::run_repair for the Rust reference.
    repair: false,
    repairYes: false,
  };

  for (let i = 2; i < argv.length; i += 1) {
    const token = argv[i];
    const [name, inlineValue] = token.startsWith('--') ? token.split('=', 2) : [token, null];
    const next = inlineValue != null ? inlineValue : argv[i + 1];

    switch (name) {
      case '--pack-dir':
        if (next) opts.packDir = next;
        if (inlineValue == null) i += 1;
        break;
      case '--ci':
        opts.ci = true;
        break;
      case '--base':
        if (next) opts.base = next;
        if (inlineValue == null) i += 1;
        break;
      case '--cwd':
        if (next) opts.cwd = path.resolve(next);
        if (inlineValue == null) i += 1;
        break;
      case '--repair':
        opts.repair = true;
        break;
      case '--yes':
        opts.repairYes = true;
        break;
      default:
        break;
    }
  }

  return opts;
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

function getChangedFiles(base, cwd) {
  const withBase = runGit(['diff', '--name-only', `${base}...HEAD`], cwd, true);
  if (withBase) {
    return withBase.split('\n').map((line) => line.trim()).filter(Boolean);
  }

  const fallback = runGit(['diff', '--name-only', 'HEAD~1'], cwd, true);
  return fallback.split('\n').map((line) => line.trim()).filter(Boolean);
}

/**
 * Run integrity verification on the pack directory.
 * Returns { pass: boolean, passCount: number, failCount: number, details: string[] }
 */
function verifyIntegrity(packDir, quiet) {
  const currentDir = path.join(packDir, 'current');
  const manifestPath = path.join(currentDir, 'manifest.json');
  const details = [];

  if (!fs.existsSync(manifestPath)) {
    throw new Error(`[agent-context] verify failed: manifest.json not found at ${manifestPath}`);
  }

  const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
  const files = manifest.files;
  if (!Array.isArray(files)) {
    throw new Error('[agent-context] verify failed: manifest has no \'files\' array');
  }

  let passCount = 0;
  let failCount = 0;

  for (const entry of files) {
    const filePath = entry.path || 'unknown';
    const expectedHash = entry.sha256 || '';
    const actualPath = path.join(currentDir, filePath);

    if (!fs.existsSync(actualPath)) {
      if (!quiet) console.error(`  FAIL  ${filePath}  (file missing)`);
      details.push(`FAIL ${filePath} (file missing)`);
      failCount++;
      continue;
    }

    const content = fs.readFileSync(actualPath, 'utf8');
    const actualHash = sha256(content);

    if (actualHash === expectedHash) {
      if (!quiet) console.log(`  PASS  ${filePath}`);
      passCount++;
    } else {
      if (!quiet) console.error(`  FAIL  ${filePath}  (checksum mismatch)`);
      details.push(`FAIL ${filePath} (checksum mismatch)`);
      failCount++;
    }
  }

  // Verify pack_checksum if present
  if (manifest.pack_checksum) {
    const packInput = files.map(f => `${f.path || 'unknown'}:${f.sha256 || ''}`).join('\n');
    const actualPackChecksum = sha256(packInput);
    if (actualPackChecksum === manifest.pack_checksum) {
      if (!quiet) console.log('  PASS  pack_checksum');
      passCount++;
    } else {
      if (!quiet) console.error('  FAIL  pack_checksum (mismatch)');
      details.push('FAIL pack_checksum (mismatch)');
      failCount++;
    }
  }

  return { pass: failCount === 0, passCount, failCount, details };
}

/**
 * Run freshness check: detect context-relevant file changes since base ref.
 * Returns { status: 'pass'|'warn'|'skip', changedFiles: string[], packUpdated: boolean }
 */
function checkFreshness(base, cwd) {
  let changedFiles;
  try {
    changedFiles = getChangedFiles(base, cwd);
  } catch (_err) {
    // Git not available or no commits — skip freshness
    return { status: 'skip', changedFiles: [], packUpdated: false };
  }

  if (changedFiles.length === 0) {
    return { status: 'pass', changedFiles: [], packUpdated: false };
  }

  const config = relevance.loadRelevanceConfig(cwd);
  let packTouched = false;
  const relevant = [];

  for (const filePath of changedFiles) {
    if (filePath.startsWith('.agent-context/current/')) {
      packTouched = true;
      continue;
    }

    if (relevance.isRelevant(filePath, config)) {
      relevant.push(filePath);
    }
  }

  if (relevant.length === 0) {
    return { status: 'pass', changedFiles: [], packUpdated: packTouched };
  }

  if (packTouched) {
    return { status: 'pass', changedFiles: relevant, packUpdated: true };
  }

  return { status: 'warn', changedFiles: relevant, packUpdated: false };
}

/**
 * Legacy verify function (backward-compatible export).
 */
function verify(packDir) {
  const result = verifyIntegrity(packDir, false);
  const total = result.passCount + result.failCount;
  console.log(`\n  Results: ${result.passCount}/${total} passed`);

  if (!result.pass) {
    throw new Error(`[agent-context] verify failed: ${result.failCount} file(s) did not match`);
  }
  console.log('  Context pack integrity verified.');
}

// CLI entry point
if (require.main === module) {
  const opts = parseArgs(process.argv);

  // Resolve packDir relative to cwd when it's the default
  const resolvedPackDir = path.isAbsolute(opts.packDir)
    ? opts.packDir
    : path.resolve(opts.cwd, opts.packDir);

  if (opts.repair) {
    // TODO(P10): port run_repair from cli/src/agent_context.rs. Until then,
    // surface a clear exit rather than silently ignoring the flag.
    console.error(
      '[agent-context] verify --repair is not yet implemented in the Node entrypoint; ' +
        'use `chorus agent-context verify --repair` (Rust CLI) for now.'
    );
    process.exit(1);
  }

  if (opts.ci) {
    // CI mode: JSON output combining integrity + freshness
    try {
      const integrity = verifyIntegrity(resolvedPackDir, true);
      const freshness = checkFreshness(opts.base, opts.cwd);

      const integrityStatus = integrity.pass ? 'pass' : 'fail';
      const freshnessStatus = freshness.status;

      const exitCode = (integrityStatus === 'fail' || freshnessStatus === 'warn') ? 1 : 0;

      const report = {
        integrity: integrityStatus,
        freshness: freshnessStatus,
        changed_files: freshness.changedFiles,
        pack_updated: freshness.packUpdated,
        exit_code: exitCode,
      };

      process.stdout.write(JSON.stringify(report) + '\n');
      process.exit(exitCode);
    } catch (err) {
      const report = {
        integrity: 'fail',
        freshness: 'skip',
        changed_files: [],
        pack_updated: false,
        exit_code: 1,
      };
      process.stdout.write(JSON.stringify(report) + '\n');
      process.exit(1);
    }
  } else {
    // Human-readable mode: integrity + freshness info
    try {
      verify(resolvedPackDir);
    } catch (err) {
      console.error(err.message);
      process.exit(1);
    }

    // Also show freshness info in human-readable mode
    const freshness = checkFreshness(opts.base, opts.cwd);
    console.log('');
    if (freshness.status === 'pass') {
      if (freshness.changedFiles.length === 0) {
        console.log('PASS agent-context-freshness (no context-relevant files changed)');
      } else {
        console.log('PASS agent-context-freshness (agent-context was updated)');
      }
    } else if (freshness.status === 'warn') {
      console.log(
        `WARNING: ${freshness.changedFiles.length} context-relevant file(s) changed but .agent-context/current/ was not updated:`
      );
      for (const filePath of freshness.changedFiles) {
        console.log(`  - ${filePath}`);
      }
      console.log('');
      console.log('Consider: update pack content with your agent, then run chorus context-pack seal');
    } else {
      console.log('SKIP agent-context-freshness (git info unavailable)');
    }
  }
}

module.exports = { verify, verifyIntegrity, checkFreshness };
