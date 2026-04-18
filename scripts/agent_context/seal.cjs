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
  appendJsonl,
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

// P8 — hostile input guards. Keep these constants in sync with the Rust
// helper `read_file_for_pack` in cli/src/agent_context.rs.
const MAX_PACK_FILE_BYTES = 5_000_000;
const BINARY_SNIFF_BYTES = 8_192;

// TODO(P8): Node-side parity for F20 (symlink escape) and F22 (glob path
// traversal) still needs to land. This helper covers F19 (binary/non-UTF-8)
// and F23 (size) for the seal hashing path, which is the blocker. See
// cli/src/agent_context.rs:~1175 for the full Rust implementation.
function readFileForPack(absolutePath) {
  const stat = fs.statSync(absolutePath);
  if (stat.size > MAX_PACK_FILE_BYTES) {
    return { ok: false, reason: `file too large (${stat.size} bytes, limit ${MAX_PACK_FILE_BYTES})` };
  }
  const buf = fs.readFileSync(absolutePath); // Buffer (raw bytes)
  const sniffLen = Math.min(buf.length, BINARY_SNIFF_BYTES);
  for (let i = 0; i < sniffLen; i += 1) {
    if (buf[i] === 0) {
      return { ok: false, reason: 'binary content (NUL bytes detected)' };
    }
  }
  // utf8 lossy: Buffer#toString('utf8') already replaces invalid sequences
  // with U+FFFD, matching String::from_utf8_lossy semantics.
  return { ok: true, content: buf.toString('utf8'), bytes: stat.size };
}

function collectFilesMeta(currentDir, relativePaths) {
  const out = [];
  for (const relativePath of relativePaths) {
    const absolutePath = path.join(currentDir, relativePath);
    const result = readFileForPack(absolutePath);
    if (!result.ok) {
      console.error(`[context-pack] WARN: skipping pack file ${relativePath}: ${result.reason}`);
      continue;
    }
    out.push({
      path: relativePath,
      path_lower: relativePath.toLowerCase(),
      sha256: sha256(result.content),
      bytes: result.bytes,
      words: (result.content.match(/\S+/g) || []).length,
    });
  }
  return out;
}

// P1 — semantic baseline helpers (Node parity).  Rust remains the reference
// implementation; these helpers produce byte-identical JSON fields for the
// simple cases and leave more complex parsing (full Python AST, Rust/TS
// tokenizer) as TODO(P1) until a team needs them.
function p1ResolveFamilyCounts(repoRoot, currentDir) {
  const patterns = new Set();
  const harvest = (cfgPath, key) => {
    if (!fs.existsSync(cfgPath)) return;
    let cfg;
    try {
      cfg = JSON.parse(fs.readFileSync(cfgPath, 'utf8'));
    } catch (_) {
      return;
    }
    const families = cfg && cfg.task_families;
    if (!families || typeof families !== 'object') return;
    for (const name of Object.keys(families)) {
      const entry = families[name];
      if (!entry || typeof entry !== 'object') continue;
      const list = Array.isArray(entry[key]) ? entry[key] : [];
      for (const p of list) {
        if (typeof p === 'string') patterns.add(p);
      }
    }
  };
  harvest(path.join(currentDir, 'completeness_contract.json'), 'required_file_families');
  harvest(path.join(currentDir, 'reporting_rules.json'), 'groupable_families');
  const out = {};
  for (const pattern of Array.from(patterns).sort()) {
    out[pattern] = resolvePatternMatches(repoRoot, pattern).length;
  }
  return out;
}

const P1_PROSE_NOUNS = [
  'study docs',
  'study doc',
  'scripts',
  'script',
  'tests',
  'test',
  'files',
  'file',
  'API symbols',
  'API symbol',
  'brands',
  'brand',
];

function p1ExtractDeclaredCounts(currentDir) {
  const out = [];
  if (!fs.existsSync(currentDir)) return out;
  const entries = fs
    .readdirSync(currentDir, { withFileTypes: true })
    .filter((e) => e.isFile() && e.name.endsWith('.md'))
    .map((e) => e.name)
    .sort();
  for (const name of entries) {
    let text;
    try {
      text = fs.readFileSync(path.join(currentDir, name), 'utf8');
    } catch (_) {
      continue;
    }
    let ignore = false;
    const lines = text.split(/\r?\n/);
    for (let i = 0; i < lines.length; i += 1) {
      const line = lines[i];
      if (line.includes('<!-- count-claim: end -->') || line.includes('<!-- count-claim: /ignore -->')) {
        ignore = false;
        continue;
      }
      if (line.includes('<!-- count-claim: ignore -->')) {
        ignore = true;
        continue;
      }
      if (ignore) continue;
      const re = /(\d+)\s+(study docs?|scripts?|tests?|files?|API symbols?|brands?)(?![A-Za-z0-9_])/g;
      let match;
      while ((match = re.exec(line)) !== null) {
        out.push({
          noun: match[2],
          count: Number(match[1]),
          file: name,
          line: i + 1,
        });
      }
    }
  }
  return out;
}

// TODO(P1): port full Rust/Python/TypeScript signature parsers. The Node
// parity intentionally records `{}` until then; manifest consumers already
// handle an empty map (P1 spec: stub other languages gracefully).
function p1ShortcutSignatures(_repoRoot, _currentDir) {
  return {};
}

function p1DependenciesSnapshot(repoRoot) {
  const out = {};
  const candidates = [
    ['pyproject', 'pyproject.toml'],
    ['cargo', 'Cargo.toml'],
    ['npm', 'package.json'],
  ];
  for (const [key, filename] of candidates) {
    const p = path.join(repoRoot, filename);
    if (!fs.existsSync(p)) continue;
    try {
      const bytes = fs.readFileSync(p);
      out[key] = sha256(bytes);
    } catch (_) {
      /* ignore */
    }
  }
  return out;
}

// P11-drift / F38 — Node parity for `tool_hashes`. Snapshots SHA256 of every
// regular file under `<currentDir>/tools/` so `check-tool-integrity` (Rust
// side) has an authoritative baseline even when the pack was sealed by the
// Node wrapper. Missing `tools/` → empty object (not an error). Keys are
// sorted to keep the serialized JSON deterministic across runs.
function p11ComputeToolHashes(currentDir) {
  const out = {};
  const toolsDir = path.join(currentDir, 'tools');
  if (!fs.existsSync(toolsDir)) return out;
  let entries;
  try {
    entries = fs.readdirSync(toolsDir, { withFileTypes: true });
  } catch (_) {
    return out;
  }
  const names = [];
  for (const entry of entries) {
    if (!entry.isFile()) continue;
    names.push(entry.name);
  }
  names.sort();
  for (const name of names) {
    try {
      const bytes = fs.readFileSync(path.join(toolsDir, name));
      out[name] = sha256(bytes);
    } catch (_) {
      /* skip unreadable file; next seal will pick it up */
    }
  }
  return out;
}

// ---- P5: count SSOT via seal-time template expansion ----------------------
// Node parity: mirrors the Rust helpers in cli/src/agent_context.rs. Slug
// derivation, handlebar expansion, and numeric-claim scanning must stay
// byte-identical so Rust and Node tracks produce the same sealed bytes.

const P5_PROSE_NOUNS_ORDERED = [
  'study docs',
  'study doc',
  'scripts',
  'script',
  'tests',
  'test',
  'files',
  'file',
  'API symbols',
  'API symbol',
  'brands',
  'brand',
];

function p5SlugForCountKey(pattern) {
  let buf = '';
  for (const ch of pattern) {
    if (ch === '*' || ch === '?') continue;
    if (/[A-Za-z0-9_]/.test(ch)) {
      buf += ch;
    } else {
      buf += '_';
    }
  }
  return buf.replace(/_+/g, '_').replace(/^_+|_+$/g, '');
}

function p5ExpandCountHandlebars(content, slugCounts) {
  // Replace {{ counts.<slug> }} (whitespace tolerated) with the integer
  // value. Unknown slugs fall through untouched so the authoring mistake is
  // visible in the sealed pack and in the drift check.
  return content.replace(/\{\{\s*counts\.([A-Za-z0-9_]+)\s*\}\}/g, (match, slug) => {
    if (Object.prototype.hasOwnProperty.call(slugCounts, slug)) {
      return String(slugCounts[slug]);
    }
    return match;
  });
}

function p5DeriveCountMaps(familyCounts) {
  const slugMap = {};
  for (const [glob, count] of Object.entries(familyCounts)) {
    const slug = p5SlugForCountKey(glob);
    if (!slug) continue;
    slugMap[slug] = (slugMap[slug] || 0) + count;
  }
  const nounMap = {};
  for (const noun of P5_PROSE_NOUNS_ORDERED) {
    const nounLower = noun.toLowerCase();
    const parts = nounLower.split(/\s+/).filter(Boolean);
    let accum = 0;
    let matched = false;
    for (const [glob, count] of Object.entries(familyCounts)) {
      const slug = p5SlugForCountKey(glob).toLowerCase();
      const tokens = slug.split('_').filter(Boolean);
      const hit = parts.some((np) => {
        const singular = np.replace(/s$/, '');
        return tokens.some((t) => t === np || t === singular);
      });
      if (hit) {
        accum += count;
        matched = true;
      }
    }
    if (matched) nounMap[noun] = accum;
  }
  return { slugMap, nounMap };
}

function p5ExtractNumericClaims(content, nounCounts, fileLabel) {
  const out = [];
  let ignore = false;
  const lines = content.split(/\r?\n/);
  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i];
    if (line.includes('<!-- count-claim: end -->') || line.includes('<!-- count-claim: /ignore -->')) {
      ignore = false;
      continue;
    }
    if (line.includes('<!-- count-claim: ignore -->')) {
      ignore = true;
      continue;
    }
    if (ignore) continue;
    const re = /(\d+)\s+(study docs?|scripts?|tests?|files?|API symbols?|brands?)(?![A-Za-z0-9_])/g;
    let match;
    while ((match = re.exec(line)) !== null) {
      const claimed = Number(match[1]);
      const noun = match[2];
      const singular = noun.replace(/s$/, '');
      let auth = nounCounts[noun];
      if (auth === undefined) auth = nounCounts[singular];
      if (auth === undefined) auth = nounCounts[`${noun}s`];
      if (auth === undefined) continue;
      if (claimed !== auth) {
        out.push({
          file: fileLabel,
          line: i + 1,
          claimed_count: claimed,
          authoritative_count: auth,
          noun,
        });
      }
    }
  }
  return out;
}

function p5ApplyCountTemplates(currentDir, requiredFiles, slugCounts, nounCounts) {
  const reports = [];
  for (const rel of requiredFiles) {
    if (!rel.endsWith('.md')) continue;
    const abs = path.join(currentDir, rel);
    let original;
    try {
      original = fs.readFileSync(abs, 'utf8');
    } catch (_) {
      continue;
    }
    const expanded = p5ExpandCountHandlebars(original, slugCounts);
    const mismatches = p5ExtractNumericClaims(expanded, nounCounts, rel);
    reports.push({ file: rel, abs, original, expanded, mismatches });
  }
  return reports;
}

function buildManifest({
  generatedAt,
  repoRoot,
  repoName,
  branch,
  detached,
  headSha,
  reason,
  baseSha,
  filesMeta,
  currentDir,
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

  // P9 F26: emit `branch: null` when HEAD is detached rather than leaking the
  // literal string "HEAD" into the manifest. A prior merge dropped this
  // helper and left `branchValue` undefined; reintroduce it here.
  const branchValue =
    detached || !branch || branch === 'HEAD' ? null : String(branch);

  // P1 — semantic baseline.
  const familyCounts = p1ResolveFamilyCounts(repoRoot, currentDir);
  const declaredCounts = p1ExtractDeclaredCounts(currentDir);
  const shortcutSignatures = p1ShortcutSignatures(repoRoot, currentDir);
  const dependenciesSnapshot = p1DependenciesSnapshot(repoRoot);
  // P11-drift / F38 — per-file SHA256 of shipped helper scripts under
  // `.agent-context/current/tools/`. Empty object when the pack does not
  // ship tools; `check-tool-integrity` treats that as "nothing to verify".
  const toolHashes = p11ComputeToolHashes(currentDir);

  return {
    value: {
      schema_version: CURRENT_SCHEMA_VERSION,
      chorus_version: readChorusVersion(),
      skill_version: null,
      verifier_sha256: verifierSha256,
      generated_at: generatedAt,
      repo_name: repoName,
      repo_root: '.',
      branch: branchValue,
      detached: Boolean(detached),
      head_sha: headSha || null,
      head_sha_at_seal: headSha || null,
      post_commit_sha: null,
      build_reason: reason,
      base_sha: baseSha || null,
      changed_files: [],
      files_count: filesMeta.length,
      words_total: wordsTotal,
      bytes_total: bytesTotal,
      pack_checksum: packChecksum,
      stable_checksum: stableChecksum,
      files: filesMeta,
      family_counts: familyCounts,
      declared_counts: declaredCounts,
      shortcut_signatures: shortcutSignatures,
      dependencies_snapshot: dependenciesSnapshot,
      tool_hashes: toolHashes,
    },
    stable_checksum: stableChecksum,
    pack_checksum: packChecksum,
  };
}

// P9 F27: detect whether cwd is inside a git repository.
function isGitRepo(cwd) {
  return runGit(['rev-parse', '--git-dir'], cwd, true) !== '';
}

// P9 F26: resolve current branch, reporting detached HEAD explicitly rather
// than leaking the literal string "HEAD" into the manifest.
function resolveBranch(cwd) {
  let symbolicOk = true;
  try {
    execFileSync('git', ['symbolic-ref', '-q', 'HEAD'], {
      cwd,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
  } catch (e) {
    symbolicOk = false;
  }
  const abbrev = runGit(['rev-parse', '--abbrev-ref', 'HEAD'], cwd, true) || '';
  if (!symbolicOk || abbrev === 'HEAD') {
    return { branch: null, detached: true };
  }
  if (!abbrev) {
    return { branch: null, detached: false };
  }
  return { branch: abbrev, detached: false };
}

// P9 F28: return warning strings for zone paths (search_scope.json) that resolve
// to git-ignored files. Silent on missing config / invalid JSON.
function collectGitignoreZoneWarnings(repoRoot, currentDir) {
  const warnings = [];
  const seen = new Set();
  const scopePath = path.join(currentDir, 'search_scope.json');
  if (!fs.existsSync(scopePath)) return warnings;
  let scope;
  try {
    scope = JSON.parse(fs.readFileSync(scopePath, 'utf8'));
  } catch (_) {
    return warnings;
  }
  const families = scope && scope.task_families;
  if (!families || typeof families !== 'object') return warnings;

  const isIgnored = (rel) => {
    try {
      const res = require('child_process').spawnSync(
        'git',
        ['check-ignore', '-q', '--', rel],
        { cwd: repoRoot, stdio: ['ignore', 'ignore', 'ignore'] }
      );
      return res.status === 0;
    } catch (_) {
      return false;
    }
  };

  for (const name of Object.keys(families)) {
    const entry = families[name];
    if (!entry || typeof entry !== 'object') continue;
    const dirs = Array.isArray(entry.search_directories) ? entry.search_directories : [];
    for (const dir of dirs) {
      if (typeof dir !== 'string') continue;
      const abs = path.join(repoRoot, dir);
      if (fs.existsSync(abs) && isIgnored(dir)) {
        const msg = `zone path '${dir}' matches git-ignored file '${dir}' — update .gitignore or remove the zone`;
        if (!seen.has(msg)) {
          seen.add(msg);
          warnings.push(msg);
        }
      }
    }
    const shortcuts = entry.verification_shortcuts && typeof entry.verification_shortcuts === 'object'
      ? entry.verification_shortcuts
      : null;
    if (shortcuts) {
      for (const key of Object.keys(shortcuts)) {
        const rel = key.split(':')[0] || key;
        const abs = path.join(repoRoot, rel);
        if (fs.existsSync(abs) && isIgnored(rel)) {
          const msg = `zone path '${key}' matches git-ignored file '${rel}' — update .gitignore or remove the zone`;
          if (!seen.has(msg)) {
            seen.add(msg);
            warnings.push(msg);
          }
        }
      }
    }
  }
  return warnings;
}

function appendHistory(historyPath, entry) {
  // Atomic-per-line: appendJsonl rotates the file when it crosses the F55
  // thresholds before the append. The seal lock (F29) serializes concurrent
  // writers, so we do not need extra synchronization here.
  appendJsonl(historyPath, entry);
}

// P12 / F42 — audit-trail helpers.

// Resolve the git committer identity. Parity with Rust's
// `git_committer_identity`. Returns `"name <email>"` or an empty string.
function gitCommitterIdentity(repoRoot) {
  let name = '';
  let email = '';
  try {
    name = execFileSync('git', ['config', 'user.name'], {
      cwd: repoRoot,
      stdio: ['ignore', 'pipe', 'ignore'],
    })
      .toString()
      .trim();
  } catch (_e) {
    name = '';
  }
  try {
    email = execFileSync('git', ['config', 'user.email'], {
      cwd: repoRoot,
      stdio: ['ignore', 'pipe', 'ignore'],
    })
      .toString()
      .trim();
  } catch (_e) {
    email = '';
  }
  if (!name && !email) return '';
  if (name && !email) return name;
  if (!name && email) return `<${email}>`;
  return `${name} <${email}>`;
}

// Split markdown into a map keyed by H2 heading; body text preserved so the
// caller can compare two maps for changed sections.
function splitMarkdownH2Sections(text) {
  const out = new Map();
  let heading = null;
  let body = [];
  for (const line of text.split('\n')) {
    if (line.startsWith('## ')) {
      if (heading != null) out.set(heading, body.join('\n') + '\n');
      heading = line.slice(3).trim();
      body = [];
      continue;
    }
    if (heading != null) body.push(line);
  }
  if (heading != null) out.set(heading, body.join('\n') + '\n');
  return out;
}

function mostRecentSnapshotDir(snapshotsDir) {
  if (!fs.existsSync(snapshotsDir)) return null;
  const names = fs
    .readdirSync(snapshotsDir, { withFileTypes: true })
    .filter((d) => d.isDirectory())
    .map((d) => d.name)
    .sort();
  if (names.length === 0) return null;
  return path.join(snapshotsDir, names[names.length - 1]);
}

// Compute the H2 section keys that changed vs the most recent snapshot.
// Keys are prefixed by file (e.g. `20_CODE_MAP.md#Contexts`). Empty array on
// first-seal or when snapshots are unreadable.
function computeProseDiffSections(snapshotsDir, currentDir) {
  const latest = mostRecentSnapshotDir(snapshotsDir);
  if (!latest) return [];
  const changed = [];
  const seen = new Set();
  for (const fileName of REQUIRED_FILES) {
    const prevPath = path.join(latest, fileName);
    const curPath = path.join(currentDir, fileName);
    const prev = fs.existsSync(prevPath) ? fs.readFileSync(prevPath, 'utf8') : '';
    const cur = fs.existsSync(curPath) ? fs.readFileSync(curPath, 'utf8') : '';
    if (prev === cur) continue;
    const prevSections = splitMarkdownH2Sections(prev);
    const curSections = splitMarkdownH2Sections(cur);
    for (const [heading, body] of curSections.entries()) {
      if (prevSections.get(heading) !== body) {
        const key = `${fileName}#${heading}`;
        if (!seen.has(key)) {
          seen.add(key);
          changed.push(key);
        }
      }
    }
    for (const heading of prevSections.keys()) {
      if (!curSections.has(heading)) {
        const key = `${fileName}#${heading}`;
        if (!seen.has(key)) {
          seen.add(key);
          changed.push(key);
        }
      }
    }
  }
  return changed;
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

// F29: bounded wait with exponential backoff. The lock covers the entire
// read-manifest -> write-files -> write-history transaction in seal().
const LOCK_WAIT_MS = 10_000;
const LOCK_BACKOFF_INITIAL_MS = 50;
const LOCK_BACKOFF_MAX_MS = 500;

function sleepBusy(ms) {
  const end = Date.now() + ms;
  // Avoid requiring async/await; seal.cjs is a sync pipeline. This is a
  // simple busy wait used only when the lock is contended.
  while (Date.now() < end) {
    // eslint-disable-next-line no-empty
  }
}

function acquireLock(lockPath) {
  const start = Date.now();
  let backoff = LOCK_BACKOFF_INITIAL_MS;
  for (;;) {
    try {
      const fd = fs.openSync(lockPath, 'wx');
      fs.writeFileSync(fd, `${process.pid}\n`);
      try {
        fs.fsyncSync(fd);
      } catch (_) {
        /* ignore */
      }
      fs.closeSync(fd);
      return () => {
        try {
          fs.unlinkSync(lockPath);
        } catch (_) {
          /* ignore */
        }
      };
    } catch (error) {
      if (error.code === 'EEXIST') {
        let holderAlive = true;
        try {
          const pidContent = fs.readFileSync(lockPath, 'utf8').trim();
          const pid = parseInt(pidContent, 10);
          if (!isNaN(pid) && !isProcessRunning(pid)) {
            console.error(`[context-pack] WARNING: cleaned stale lock (pid ${pid} no longer running)`);
            fs.unlinkSync(lockPath);
            holderAlive = false;
          }
        } catch (_) {
          /* fall through to the timeout check */
        }
        if (!holderAlive) {
          continue;
        }
        if (Date.now() - start >= LOCK_WAIT_MS) {
          throw new Error(
            `[context-pack] another seal is in progress (lock: ${lockPath}); waited ${Math.floor(
              LOCK_WAIT_MS / 1000
            )}s`
          );
        }
        sleepBusy(backoff);
        backoff = Math.min(backoff * 2, LOCK_BACKOFF_MAX_MS);
        continue;
      }
      throw new Error(`[context-pack] another seal is in progress (lock: ${lockPath}): ${error.message}`);
    }
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

  // P9 F27: non-git directory → fail loudly rather than silently producing a
  // manifest with empty branch/head_sha and no freshness signal.
  if (!isGitRepo(opts.cwd)) {
    console.error(
      `[context-pack] seal failed: not a git repository (cwd: ${opts.cwd})`
    );
    process.exit(1);
  }

  const repoRoot = runGit(['rev-parse', '--show-toplevel'], opts.cwd, true) || opts.cwd;
  const repoName = path.basename(repoRoot);
  // P9 F26: resolve branch + detect detached HEAD.
  const { branch: resolvedBranch, detached } = resolveBranch(repoRoot);
  const branch = resolvedBranch || '';
  if (detached) {
    process.stderr.write(
      '[context-pack] NOTICE: HEAD is detached — manifest recorded as branch: null, detached: true\n'
    );
  }
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

    // P5 — expand `{{counts.<slug>}}` handlebars and detect stale prose
    // numeric claims before collectFilesMeta hashes the files. The expanded
    // bytes are what get sealed into the manifest, so prose and manifest
    // agree by construction. Mirrors the Rust track in cli/src/agent_context.rs.
    const p5FamilyCounts = p1ResolveFamilyCounts(repoRoot, currentDir);
    const { slugMap: p5SlugCounts, nounMap: p5NounCounts } = p5DeriveCountMaps(p5FamilyCounts);
    const p5Reports = p5ApplyCountTemplates(currentDir, requiredFiles, p5SlugCounts, p5NounCounts);
    const p5Mismatches = [];
    for (const report of p5Reports) {
      if (report.original !== report.expanded) {
        safeWriteTextAtomic(report.abs, report.expanded);
      }
      for (const m of report.mismatches) p5Mismatches.push(m);
    }
    if (p5Mismatches.length > 0) {
      const lines = p5Mismatches.map(
        (m) => `  - ${m.file}:${m.line}: claimed ${m.claimed_count} ${m.noun}, authoritative ${m.authoritative_count}`
      );
      const msg =
        '[context-pack] seal failed: prose numeric claims disagree with authoritative family_counts:\n' +
        lines.join('\n');
      if (opts.force) {
        process.stderr.write(`${msg}\n`);
        process.stderr.write(
          '[context-pack] WARN: --force downgraded count-claim failures to warnings\n'
        );
      } else {
        throw new Error(
          `${msg}\n  Fix: update prose to {{counts.<slug>}} or surround with <!-- count-claim: ignore --> / <!-- count-claim: end -->. Use --force to override.`
        );
      }
    }

    const filesMeta = collectFilesMeta(currentDir, requiredFiles);

    const manifest = buildManifest({
      generatedAt,
      repoRoot,
      repoName,
      branch,
      detached,
      headSha,
      reason: opts.reason,
      baseSha: opts.base,
      filesMeta,
      currentDir,
    });

    // P9 F28: warn if any zone path is git-ignored.
    for (const w of collectGitignoreZoneWarnings(repoRoot, currentDir)) {
      process.stderr.write(`[context-pack] WARN: ${w}\n`);
    }

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

      // P12 / F42 — audit trail. Parity with Rust: committer identity,
      // the set of H2 sections whose prose changed vs the previous snapshot,
      // and an explicit `seal_reason` mirror of `reason`. MUST be computed
      // before copyDir below so mostRecentSnapshotDir returns the previous
      // snapshot, not the freshly written one.
      const proseDiffSections = computeProseDiffSections(snapshotsDir, currentDir);
      const sealedBy = gitCommitterIdentity(repoRoot);

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
        sealed_by: sealedBy,
        prose_diff_sections: proseDiffSections,
        seal_reason: opts.reason,
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
