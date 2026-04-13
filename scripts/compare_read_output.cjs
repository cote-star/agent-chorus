#!/usr/bin/env node

const fs = require('fs');

const [leftPath, rightPath, label] = process.argv.slice(2);
if (!leftPath || !rightPath) {
  console.error('Usage: compare_read_output.cjs <left.json> <right.json> [label]');
  process.exit(1);
}

const leftJson = JSON.parse(fs.readFileSync(leftPath, 'utf-8'));
const rightJson = JSON.parse(fs.readFileSync(rightPath, 'utf-8'));

const path = require('path');

function normalizeSourcePath(str) {
  // Replace full paths with just basename: "[tag] /full/path/file.ext" -> "[tag] file.ext"
  return str.replace(/ \/[^ ]*\/([^ ]+)$/g, ' $1').replace(/ \/([^ ]+)$/g, ' $1');
}

function canonicalize(value, key) {
  // Normalize sources_used paths to basenames (must check before Array.isArray)
  if (key === 'sources_used' && Array.isArray(value)) {
    return value.map(v => typeof v === 'string' ? normalizeSourcePath(v) : canonicalize(v));
  }

  if (Array.isArray(value)) {
    const mapped = value.map((v) => canonicalize(v));

    // Normalize ordering for list/search style outputs where entries are objects
    // with session_id/agent/file_path. Mtime ordering is environment-dependent.
    if (mapped.every((entry) =>
      entry &&
      typeof entry === 'object' &&
      !Array.isArray(entry) &&
      typeof entry.session_id === 'string' &&
      typeof entry.agent === 'string' &&
      Object.prototype.hasOwnProperty.call(entry, 'file_path')
    )) {
      mapped.sort((a, b) =>
        String(a.session_id).localeCompare(String(b.session_id)) ||
        String(a.agent).localeCompare(String(b.agent)) ||
        String(a.file_path).localeCompare(String(b.file_path))
      );
    }

    return mapped;
  }

  if (value && typeof value === 'object') {
    // Node-only fields not yet in Rust — skip for parity comparison
    const skipKeys = new Set(['included_roles', 'included_tool_calls']);
    const out = {};
    for (const k of Object.keys(value).sort()) {
      if (skipKeys.has(k)) continue;
      out[k] = canonicalize(value[k], k);
    }
    return out;
  }

  // Normalize source paths to basenames to avoid absolute-path mismatches
  if (key === 'source' && typeof value === 'string') {
    return path.basename(value);
  }

  // Strip timestamp for golden file comparison (varies by env)
  if (key === 'timestamp') {
    return null;
  }

  // Strip modified_at precision differences between runtimes
  if (key === 'modified_at') {
    return null;
  }


  // Strip file_path for golden file comparison
  if (key === 'file_path' && typeof value === 'string') {
    return path.basename(value);
  }

  return value;
}

const leftCanonical = canonicalize(leftJson);
const rightCanonical = canonicalize(rightJson);

if (JSON.stringify(leftCanonical) !== JSON.stringify(rightCanonical)) {
  console.error(`Mismatch${label ? ` (${label})` : ''}`);
  console.error('Left:', JSON.stringify(leftCanonical, null, 2));
  console.error('Right:', JSON.stringify(rightCanonical, null, 2));
  process.exit(1);
}

console.log(`PASS${label ? ` ${label}` : ''}`);
