#!/usr/bin/env node
'use strict';

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { execFileSync } = require('child_process');

// P11 / F34: manifest schema version emitted by this seal script. Must stay
// in lockstep with CURRENT_SCHEMA_VERSION in cli/src/agent_context.rs so that
// the Node and Rust tracks produce byte-identical manifest shapes.
const CURRENT_SCHEMA_VERSION = 1;

// P11 / F36: chorus version recorded in the manifest. Read from the workspace
// package.json so we don't hard-code a drifting string.
function readChorusVersion() {
  try {
    const pkgPath = path.resolve(__dirname, '..', '..', 'package.json');
    const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));
    return typeof pkg.version === 'string' ? pkg.version : null;
  } catch (_err) {
    return null;
  }
}

const REQUIRED_FILES = [
  '00_START_HERE.md',
  '10_SYSTEM_OVERVIEW.md',
  '20_CODE_MAP.md',
  '30_BEHAVIORAL_INVARIANTS.md',
  '40_OPERATIONS_AND_RELEASE.md',
];

const STRUCTURED_FILES = [
  'routes.json',
  'completeness_contract.json',
  'reporting_rules.json',
];

const TASK_FAMILIES = ['lookup', 'impact_analysis', 'planning', 'diagnosis'];

function parseArgs(argv) {
  const opts = {
    reason: 'manual-seal',
    base: null,
    head: null,
    packDir: process.env.CHORUS_CONTEXT_PACK_DIR || process.env.BRIDGE_CONTEXT_PACK_DIR || '.agent-context',
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
  safeWriteText,
  safeWriteTextAtomic,
} = require('./cp_utils.cjs');

/**
 * Update the Snapshot metadata lines in 00_START_HERE.md so they stay in sync
 * with manifest.json.  Only touches Branch, HEAD commit, and Generated at —
 * preserves everything else (Repo line, user-written content).
 */
function updateStartHereSnapshot(currentDir, branch, headSha, generatedAt) {
  const startHerePath = path.join(currentDir, '00_START_HERE.md');
  if (!fs.existsSync(startHerePath)) return;

  let content = fs.readFileSync(startHerePath, 'utf8');
  content = content.replace(
    /^- Branch at generation: `.+`$/m,
    `- Branch at generation: \`${branch}\``
  );
  content = content.replace(
    /^- HEAD commit: `.+`$/m,
    `- HEAD commit: \`${headSha || 'unknown'}\``
  );
  content = content.replace(
    /^- Generated at: `.+`$/m,
    `- Generated at: \`${generatedAt}\``
  );
  safeWriteTextAtomic(startHerePath, content);
}

function sha256(input) {
  return crypto.createHash('sha256').update(input).digest('hex');
}

function readJson(filePath) {
  if (!fs.existsSync(filePath)) return null;
  return JSON.parse(fs.readFileSync(filePath, 'utf8'));
}

function hasStructuredLayer(currentDir) {
  return fs.existsSync(path.join(currentDir, 'routes.json'));
}

function requiredFilesForMode(currentDir) {
  return hasStructuredLayer(currentDir)
    ? REQUIRED_FILES.concat(STRUCTURED_FILES)
    : REQUIRED_FILES.slice();
}

function walkFiles(rootDir, currentDir = rootDir, acc = []) {
  for (const entry of fs.readdirSync(currentDir, { withFileTypes: true })) {
    if (entry.name === '.git') continue;
    const absolutePath = path.join(currentDir, entry.name);
    if (entry.isDirectory()) {
      walkFiles(rootDir, absolutePath, acc);
    } else if (entry.isFile()) {
      acc.push(path.relative(rootDir, absolutePath).replace(/\\/g, '/'));
    }
  }
  return acc;
}

function globToRegExp(pattern) {
  const normalized = pattern.replace(/\\/g, '/');
  let regex = '^';
  for (let i = 0; i < normalized.length; i += 1) {
    const char = normalized[i];
    const next = normalized[i + 1];
    if (char === '*') {
      if (next === '*') {
        regex += '.*';
        i += 1;
      } else {
        regex += '[^/]*';
      }
    } else if (char === '?') {
      regex += '.';
    } else if ('\\.[]{}()+-^$|'.includes(char)) {
      regex += `\\${char}`;
    } else {
      regex += char;
    }
  }
  regex += '$';
  return new RegExp(regex);
}

function resolvePatternMatches(repoRoot, pattern) {
  const normalized = pattern.replace(/\\/g, '/');
  if (!/[?*]/.test(normalized)) {
    return fs.existsSync(path.join(repoRoot, normalized)) ? [normalized] : [];
  }
  const matcher = globToRegExp(normalized);
  return walkFiles(repoRoot).filter((filePath) => matcher.test(filePath));
}

function readRequiredJson(filePath, label) {
  try {
    return JSON.parse(fs.readFileSync(filePath, 'utf8'));
  } catch (error) {
    throw new Error(`[context-pack] seal failed: could not parse ${label}: ${error.message}`);
  }
}

function validateStructuredLayer(repoRoot, currentDir) {
  const routesPath = path.join(currentDir, 'routes.json');
  if (!fs.existsSync(routesPath)) {
    return;
  }

  const completenessPath = path.join(currentDir, 'completeness_contract.json');
  const reportingPath = path.join(currentDir, 'reporting_rules.json');

  for (const requiredPath of [completenessPath, reportingPath]) {
    if (!fs.existsSync(requiredPath)) {
      throw new Error(
        `[context-pack] seal failed: structured mode requires ${path.relative(repoRoot, requiredPath)}`
      );
    }
  }

  const routes = readRequiredJson(routesPath, 'routes.json');
  const completeness = readRequiredJson(completenessPath, 'completeness_contract.json');
  const reporting = readRequiredJson(reportingPath, 'reporting_rules.json');

  if (!routes.task_routes || typeof routes.task_routes !== 'object') {
    throw new Error('[context-pack] seal failed: routes.json must define task_routes');
  }
  if (!completeness.task_families || typeof completeness.task_families !== 'object') {
    throw new Error('[context-pack] seal failed: completeness_contract.json must define task_families');
  }
  if (!reporting.task_families || typeof reporting.task_families !== 'object') {
    throw new Error('[context-pack] seal failed: reporting_rules.json must define task_families');
  }

  for (const task of TASK_FAMILIES) {
    const route = routes.task_routes[task];
    const completenessEntry = completeness.task_families[task];
    const reportingEntry = reporting.task_families[task];

    if (!route) {
      throw new Error(`[context-pack] seal failed: routes.json is missing task_routes.${task}`);
    }
    if (!completenessEntry) {
      throw new Error(`[context-pack] seal failed: completeness_contract.json is missing task_families.${task}`);
    }
    if (!reportingEntry) {
      throw new Error(`[context-pack] seal failed: reporting_rules.json is missing task_families.${task}`);
    }

    if (route.completeness_ref !== task) {
      throw new Error(`[context-pack] seal failed: routes.json completeness_ref for ${task} must equal ${task}`);
    }
    if (route.reporting_ref !== task) {
      throw new Error(`[context-pack] seal failed: routes.json reporting_ref for ${task} must equal ${task}`);
    }

    for (const ref of route.pack_read_order || []) {
      const targetPath = path.join(currentDir, ref);
      if (!fs.existsSync(targetPath)) {
        throw new Error(`[context-pack] seal failed: routes.json references missing pack file ${ref}`);
      }
    }
    for (const ref of route.fallback_files || []) {
      const targetPath = path.join(currentDir, ref);
      if (!fs.existsSync(targetPath)) {
        throw new Error(`[context-pack] seal failed: routes.json references missing fallback file ${ref}`);
      }
    }

    for (const pattern of completenessEntry.contractually_required_files || []) {
      if (resolvePatternMatches(repoRoot, pattern).length === 0) {
        throw new Error(`[context-pack] seal failed: completeness_contract.json pattern did not match any files: ${pattern}`);
      }
    }
    for (const pattern of completenessEntry.required_file_families || []) {
      if (resolvePatternMatches(repoRoot, pattern).length === 0) {
        throw new Error(`[context-pack] seal failed: completeness_contract.json family did not match any files: ${pattern}`);
      }
    }
    for (const pattern of completenessEntry.required_chain_members || []) {
      if (resolvePatternMatches(repoRoot, pattern).length === 0) {
        throw new Error(`[context-pack] seal failed: completeness_contract.json chain member did not match any files: ${pattern}`);
      }
    }

    const optionalBudget = reportingEntry.optional_verify_budget;
    if (!Number.isInteger(optionalBudget) || optionalBudget < 0) {
      throw new Error(`[context-pack] seal failed: reporting_rules.json optional_verify_budget must be a non-negative integer for ${task}`);
    }

    for (const pattern of reportingEntry.groupable_families || []) {
      if (resolvePatternMatches(repoRoot, pattern).length === 0) {
        throw new Error(`[context-pack] seal failed: reporting_rules.json groupable family did not match any files: ${pattern}`);
      }
    }
    for (const pattern of reportingEntry.never_enumerate_individually || []) {
      if (resolvePatternMatches(repoRoot, pattern).length === 0) {
        throw new Error(`[context-pack] seal failed: reporting_rules.json anti-enumeration pattern did not match any files: ${pattern}`);
      }
    }
  }

  for (const entry of reporting.global_rules?.authoritative_vs_derived_paths || []) {
    if (!entry || typeof entry !== 'object') {
      throw new Error('[context-pack] seal failed: reporting_rules.json authoritative_vs_derived_paths entries must be objects');
    }
    if (typeof entry.pattern !== 'string' || typeof entry.role !== 'string') {
      throw new Error('[context-pack] seal failed: reporting_rules.json authoritative_vs_derived_paths entries must contain pattern and role');
    }
    if (resolvePatternMatches(repoRoot, entry.pattern).length === 0) {
      throw new Error(`[context-pack] seal failed: reporting_rules.json path rule did not match any files: ${entry.pattern}`);
    }
    if (entry.role === 'authoritative' && entry.pattern.includes('_generated/')) {
      throw new Error('[context-pack] seal failed: generated files cannot be marked as authoritative edit targets');
    }
  }

  // Validate search_scope.json if present (not required — backward compat)
  const searchScopePath = path.join(currentDir, 'search_scope.json');
  if (fs.existsSync(searchScopePath)) {
    const scope = readRequiredJson(searchScopePath, 'search_scope.json');
    if (scope.task_families && typeof scope.task_families === 'object') {
      for (const task of TASK_FAMILIES) {
        const entry = scope.task_families[task];
        if (!entry) continue;
        for (const dir of entry.search_directories || []) {
          const dirPath = path.join(repoRoot, dir);
          if (!fs.existsSync(dirPath)) {
            throw new Error(`[context-pack] seal failed: search_scope.json references missing directory ${dir}`);
          }
        }
        if (entry.verification_shortcuts && typeof entry.verification_shortcuts === 'object') {
          for (const filePath of Object.keys(entry.verification_shortcuts)) {
            const baseFile = filePath.split(':')[0];
            const fileOnDisk = path.join(repoRoot, baseFile);
            if (!fs.existsSync(fileOnDisk)) {
              throw new Error(`[context-pack] seal failed: search_scope.json verification_shortcuts references missing file ${filePath}`);
            }
          }
        }
      }
    }
  }
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

  // P11 / F36: sha256 of the node script performing the seal. Gives the
  // manifest a forensic fingerprint of the tool that produced it without
  // pulling in the whole chorus binary path.
  let verifierSha256 = null;
  try {
    verifierSha256 = sha256(fs.readFileSync(__filename));
  } catch (_err) {
    // fall through — null is a valid value
  }

  return {
    value: {
      schema_version: CURRENT_SCHEMA_VERSION,
      chorus_version: readChorusVersion(),
      skill_version: null,
      verifier_sha256: verifierSha256,
      generated_at: generatedAt,
      repo_name: repoName,
      repo_root: '.',
      branch,
      head_sha: headSha || null,
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

/**
 * Content quality warnings — advisory only, never block the seal.
 * Returns an array of warning strings (empty = all good).
 */
function checkContentQuality(currentDir) {
  const warnings = [];

  // CODE_MAP: check for Risk column and non-empty values
  const codeMapPath = path.join(currentDir, '20_CODE_MAP.md');
  if (fs.existsSync(codeMapPath)) {
    const codeMap = fs.readFileSync(codeMapPath, 'utf8');
    const hasRiskHeader = /\|\s*Risk\b/i.test(codeMap);
    if (!hasRiskHeader) {
      warnings.push('20_CODE_MAP.md: no Risk column found — add a Risk column to each table row (e.g. "Silent failure if missed")');
    } else {
      // Count table data rows (not header/separator) missing a risk value in the last column
      const tableRows = codeMap.split('\n').filter((l) => /^\|/.test(l) && !/^\|\s*[-:]+/.test(l) && !/Risk/i.test(l));
      const emptyRisk = tableRows.filter((row) => {
        const cells = row.split('|').map((c) => c.trim()).filter(Boolean);
        return cells.length > 0 && cells[cells.length - 1] === '';
      });
      if (emptyRisk.length > 0) {
        warnings.push(`20_CODE_MAP.md: ${emptyRisk.length} row(s) have an empty Risk column — fill with "Silent failure if missed", "KeyError at runtime", etc.`);
      }
    }
  }

  // BEHAVIORAL_INVARIANTS: check for at least one checklist row with a file path
  const invariantsPath = path.join(currentDir, '30_BEHAVIORAL_INVARIANTS.md');
  if (fs.existsSync(invariantsPath)) {
    const invariants = fs.readFileSync(invariantsPath, 'utf8');
    const tableRows = invariants.split('\n').filter((l) => /^\|/.test(l) && !/^\|\s*[-:]+/.test(l) && !/Change.*type/i.test(l) && !/Files.*must/i.test(l));
    if (tableRows.length === 0) {
      warnings.push('30_BEHAVIORAL_INVARIANTS.md: Update Checklist has no rows — add at least one change-type row with explicit file paths');
    } else {
      // Check if any row contains an explicit file path (contains a dot in a path-like token)
      const hasFilePath = tableRows.some((row) => /\b\w[\w/.-]+\.\w+/.test(row));
      if (!hasFilePath) {
        warnings.push('30_BEHAVIORAL_INVARIANTS.md: checklist rows do not appear to name explicit file paths — rows should list files by path, not just description');
      }
    }
  }

  // SYSTEM_OVERVIEW: check for runtime behavior or silent failure modes section
  const overviewPath = path.join(currentDir, '10_SYSTEM_OVERVIEW.md');
  if (fs.existsSync(overviewPath)) {
    const overview = fs.readFileSync(overviewPath, 'utf8');
    const hasRuntimeSection = /##\s+(Runtime|Silent Failure)/i.test(overview);
    if (!hasRuntimeSection) {
      warnings.push('10_SYSTEM_OVERVIEW.md: no Runtime Architecture or Silent Failure Modes section found — agents need runtime behavior documented to diagnose silent failures');
    }
  }

  return warnings;
}

function isHookInstalled(repoRoot) {
  const hooksPath = runGit(['config', '--get', 'core.hooksPath'], repoRoot, true);
  const hooksDir = hooksPath
    ? path.join(repoRoot, hooksPath)
    : path.join(repoRoot, '.githooks');
  const prePushPath = path.join(hooksDir, 'pre-push');
  if (!fs.existsSync(prePushPath)) return false;
  const content = fs.readFileSync(prePushPath, 'utf8');
  return (
    content.includes('# --- agent-chorus:pre-push:start ---') ||
    content.includes('# --- agent-bridge:pre-push:start ---')
  );
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

    const requiredFiles = requiredFilesForMode(currentDir);

    for (const file of requiredFiles) {
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

    validateStructuredLayer(repoRoot, currentDir);

    const generatedAt = new Date().toISOString();

    // Update 00_START_HERE.md snapshot metadata BEFORE collecting file checksums
    // so the manifest reflects the updated content.
    updateStartHereSnapshot(currentDir, branch, headSha, generatedAt);

    const filesMeta = collectFilesMeta(currentDir, requiredFiles);

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

    const qualityWarnings = checkContentQuality(currentDir);
    for (const w of qualityWarnings) {
      console.warn(`[context-pack] WARN: ${w}`);
    }

    if (!isHookInstalled(repoRoot)) {
      console.warn(
        '[context-pack] WARN: pre-push hook is not installed — run `chorus context-pack install-hooks` to enable staleness detection on main pushes'
      );
    }
  } catch (error) {
    console.error(error.message || error);
    process.exit(1);
  } finally {
    releaseLock();
  }
}

main();
