#!/usr/bin/env bash
set -euo pipefail

tmp_json="$(mktemp)"
npm_cache_dir="$(mktemp -d)"
trap 'rm -f "$tmp_json"; rm -rf "$npm_cache_dir"' EXIT

npm_config_cache="$npm_cache_dir" NPM_CONFIG_CACHE="$npm_cache_dir" npm pack --dry-run --json --silent > "$tmp_json"

node - "$tmp_json" <<'NODE'
const fs = require('fs');

const dryRunPath = process.argv[2];
const raw = fs.readFileSync(dryRunPath, 'utf8');
const parsed = JSON.parse(raw);
const files = new Set((parsed?.[0]?.files ?? []).map((entry) => entry.path));

const requiredPaths = [
  'scripts/read_session.cjs',
  'scripts/adapters/registry.cjs',
  'scripts/agent_context/build.cjs',
  'scripts/agent_context/install_hooks.cjs',
  'docs/architecture.svg',
  'README.md',
  'PROTOCOL.md',
  'SKILL.md',
  'LICENSE',
  'schemas/read-output.schema.json',
  '.claude-plugin/plugin.json',
  '.claude-plugin/marketplace.json',
  'skills/agent-chorus/SKILL.md',
];

const forbiddenPrefixes = [
  'fixtures/',
  'cli/',
  'references/',
  '.github/',
  '.agent-context/',
  'node_modules/',
];

const missing = requiredPaths.filter((path) => !files.has(path));
const forbidden = [...files].filter((path) =>
  forbiddenPrefixes.some((prefix) => path.startsWith(prefix))
);

if (missing.length > 0 || forbidden.length > 0) {
  if (missing.length > 0) {
    console.error('Missing required package paths:');
    for (const path of missing) {
      console.error(`- ${path}`);
    }
  }
  if (forbidden.length > 0) {
    console.error('Forbidden package paths detected:');
    for (const path of forbidden) {
      console.error(`- ${path}`);
    }
  }
  process.exit(1);
}

console.log(`PASS package-contents (${files.size} files)`);
NODE
