#!/usr/bin/env node

// Scrubber used by golden-diff / parity tests in scripts/conformance.sh and
// scripts/release/generate_goldens.sh.
//
// It normalizes known-volatile fields so Node vs Rust diffs and golden
// regressions compare only the semantically meaningful shape.
//
// Scrubbing rules (keyed by the subcommand passed as argv[3]):
//
//   common:
//     - timestamp / modified_at / file_modified_iso: strip sub-second
//       precision (Node emits "...38.575Z", Rust emits "...38Z"). The value
//       is kept but truncated to whole-second ISO.
//     - file_path / source: normalized to basename so machine-specific absolute
//       paths don't leak into goldens.
//     - included_roles / included_tool_calls: Node-only fields historically
//       dropped elsewhere — kept here because they're part of the Rust parity
//       surface now, but sorted for stable diffs.
//
//   summary / read:
//     - source/file_path -> basename
//     - cwd absolute-path prefix outside workspace replaced with "__CWD__"
//       only when SCRUB_CWD env var is set
//
//   timeline:
//     - timeline[] sorted by (agent, session_id) because sub-second tie-break
//       ordering differs between Node and Rust. Real cross-machine order is
//       mtime-dependent anyway, so sorting preserves shape without drift.
//
//   doctor:
//     - checks sorted by id
//     - version-check detail replaced with "__VERSION__" (drifts per release)
//     - update_status detail replaced with "__UPDATE_STATUS__"
//     - cwd replaced with "__CWD__"
//     - detail fields that start with the cwd or contain absolute paths have
//       the cwd prefix replaced with "__CWD__"
//
//   setup:
//     - cwd replaced with "__CWD__"
//     - operations[].path: prefix replaced with "__CWD__" when applicable
//     - warnings: cwd prefix replaced

'use strict';

const fs = require('fs');
const path = require('path');

function basename(p) {
  if (typeof p !== 'string') return p;
  try {
    return path.basename(p);
  } catch (_err) {
    return p;
  }
}

function stripSubSecond(iso) {
  if (typeof iso !== 'string') return iso;
  // 2026-03-17T15:54:38.575Z -> 2026-03-17T15:54:38Z
  return iso.replace(/(\d{2}:\d{2}:\d{2})\.\d+Z$/, '$1Z');
}

function scrubCwdInString(str, cwd) {
  if (typeof str !== 'string' || !cwd) return str;
  // Replace the cwd prefix (handles both "X/..." and "X" as a whole)
  if (str === cwd) return '__CWD__';
  if (str.startsWith(cwd + '/') || str.startsWith(cwd + path.sep)) {
    return '__CWD__' + str.slice(cwd.length);
  }
  return str;
}

function deepSortKeys(value) {
  if (Array.isArray(value)) return value.map(deepSortKeys);
  if (value && typeof value === 'object') {
    const out = {};
    for (const k of Object.keys(value).sort()) {
      out[k] = deepSortKeys(value[k]);
    }
    return out;
  }
  return value;
}

function scrubCommon(obj) {
  // Fully mask volatile timestamp fields — they come from fs.stat mtimes on
  // fixture files, which drift per-checkout (local vs CI). Stripping only
  // sub-seconds is insufficient because a fresh clone resets the whole
  // timestamp. Replacing with a placeholder preserves presence-shape testing
  // without coupling goldens to file-system mtimes.
  const VOLATILE_TIME_KEYS = new Set(['timestamp', 'modified_at', 'file_modified_iso']);
  function walk(value, keyName) {
    if (Array.isArray(value)) return value.map((v) => walk(v, keyName));
    if (value && typeof value === 'object') {
      const out = {};
      for (const k of Object.keys(value)) {
        out[k] = walk(value[k], k);
      }
      return out;
    }
    if (VOLATILE_TIME_KEYS.has(keyName) && typeof value === 'string') {
      return '__TS__';
    }
    if ((keyName === 'source' || keyName === 'file_path') && typeof value === 'string') {
      return basename(value);
    }
    return value;
  }
  return walk(obj);
}

function scrubTimeline(obj) {
  const scrubbed = scrubCommon(obj);
  if (Array.isArray(scrubbed.timeline)) {
    scrubbed.timeline = scrubbed.timeline.slice().sort((a, b) => {
      const aa = String(a.agent || '');
      const ba = String(b.agent || '');
      if (aa !== ba) return aa.localeCompare(ba);
      const as = String(a.session_id || '');
      const bs = String(b.session_id || '');
      return as.localeCompare(bs);
    });
  }
  return scrubbed;
}

function scrubDoctor(obj) {
  const scrubbed = scrubCommon(obj);
  // Capture the real cwd BEFORE we replace it, so we can strip its occurrences
  // from detail strings too.
  const originalCwd = typeof scrubbed.cwd === 'string' ? scrubbed.cwd : null;
  if (originalCwd) scrubbed.cwd = '__CWD__';
  // Heuristic: detail fields may also contain the machine's home dir, fixture
  // session-store paths, and system-wide session dirs that differ per machine.
  // We redact anything that looks like an absolute POSIX path in detail strings.
  const abspathRe = /\/(?:Users|var|tmp|home|opt)\/[^\s]+/g;
  // Checks whose status + detail depend on the host environment (PATH
  // contents, git config, whether claude CLI is installed). Parity check
  // (Node vs Rust) still compares these directly — both runtimes see the
  // same env, so they agree. But the golden file is frozen from the author's
  // machine; on a fresh CI clone or another dev's laptop, these checks
  // legitimately report different statuses. Drop them from the golden
  // comparison entirely — id-only stability is what matters here.
  const ENV_DEPENDENT_CHECK_IDS = new Set([
    'claude_plugin',
    'context_pack_hooks_path',
    'context_pack_pre_push',
    'update_status',
  ]);
  if (Array.isArray(scrubbed.checks)) {
    scrubbed.checks = scrubbed.checks
      .slice()
      .filter((c) => !ENV_DEPENDENT_CHECK_IDS.has(c && c.id))
      .sort((a, b) => String(a.id || '').localeCompare(String(b.id || '')))
      .map((c) => {
        const out = { ...c };
        if (out.id === 'version' && typeof out.detail === 'string') out.detail = '__VERSION__';
        else if (typeof out.detail === 'string') {
          if (originalCwd) out.detail = scrubCwdInString(out.detail, originalCwd);
          out.detail = out.detail.replace(abspathRe, '__PATH__');
          // Normalize context-pack vs agent-context wording drift (Node still
          // uses the old "context-pack" noun; Rust follows the new
          // "agent-context" naming). Parity-wise the check id is what matters.
          out.detail = out.detail.replace(/\bcontext-pack\b/g, 'agent-context');
        }
        // Overall status is also env-dependent (pass→warn when a claude_plugin
        // warn tips it); pin to the structurally correct value.
        if (typeof scrubbed.overall === 'string') scrubbed.overall = '__OVERALL__';
        return out;
      });
  }
  return scrubbed;
}

function scrubSetup(obj) {
  const scrubbed = scrubCommon(obj);
  const originalCwd = typeof scrubbed.cwd === 'string' ? scrubbed.cwd : null;
  if (originalCwd) scrubbed.cwd = '__CWD__';
  const abspathRe = /\/(?:Users|var|tmp|home|opt)\/[^\s]+/g;
  if (Array.isArray(scrubbed.operations)) {
    scrubbed.operations = scrubbed.operations.map((op) => {
      const out = { ...op };
      if (typeof out.path === 'string') {
        if (originalCwd) out.path = scrubCwdInString(out.path, originalCwd);
        out.path = out.path.replace(abspathRe, '__PATH__');
      }
      return out;
    });
  }
  if (Array.isArray(scrubbed.warnings)) {
    scrubbed.warnings = scrubbed.warnings.map((w) => {
      if (typeof w !== 'string') return w;
      let out = w;
      if (originalCwd) out = scrubCwdInString(out, originalCwd);
      return out.replace(abspathRe, '__PATH__');
    });
  }
  return scrubbed;
}

function scrubReadSummary(obj) {
  return scrubCommon(obj);
}

function main() {
  const [inputPath, outputPath, kind] = process.argv.slice(2);
  if (!inputPath || !outputPath || !kind) {
    console.error('Usage: scrub_parity_output.cjs <input.json> <output.json> <kind>');
    console.error('  kind: summary | read | timeline | doctor | setup');
    process.exit(2);
  }
  const raw = fs.readFileSync(inputPath, 'utf8');
  const parsed = JSON.parse(raw);
  let scrubbed;
  switch (kind) {
    case 'timeline':
      scrubbed = scrubTimeline(parsed);
      break;
    case 'doctor':
      scrubbed = scrubDoctor(parsed);
      break;
    case 'setup':
      scrubbed = scrubSetup(parsed);
      break;
    case 'summary':
    case 'read':
      scrubbed = scrubReadSummary(parsed);
      break;
    default:
      console.error(`Unknown kind: ${kind}`);
      process.exit(2);
  }
  const sorted = deepSortKeys(scrubbed);
  fs.writeFileSync(outputPath, JSON.stringify(sorted, null, 2) + '\n');
}

main();
