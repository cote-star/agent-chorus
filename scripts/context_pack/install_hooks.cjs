#!/usr/bin/env node
'use strict';

const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

function parseArgs(argv) {
  const options = {
    cwd: process.cwd(),
    dryRun: false,
  };

  for (let i = 2; i < argv.length; i += 1) {
    const token = argv[i];
    const [name, inlineValue] = token.startsWith('--') ? token.split('=', 2) : [token, null];
    const next = inlineValue != null ? inlineValue : argv[i + 1];

    switch (name) {
      case '--cwd':
        if (next) options.cwd = path.resolve(next);
        if (inlineValue == null) i += 1;
        break;
      case '--dry-run':
        options.dryRun = true;
        break;
      default:
        break;
    }
  }

  return options;
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

function buildPrePushHook() {
  return `#!/usr/bin/env bash
set -euo pipefail

remote_name="\${1:-origin}"
remote_url="\${2:-unknown}"

run_context_sync() {
  local local_ref="$1"
  local local_sha="$2"
  local remote_ref="$3"
  local remote_sha="$4"

  if command -v bridge >/dev/null 2>&1; then
    bridge context-pack sync-main \\
      --local-ref "$local_ref" \\
      --local-sha "$local_sha" \\
      --remote-ref "$remote_ref" \\
      --remote-sha "$remote_sha"
    return
  fi

  if [[ -f scripts/read_session.cjs ]]; then
    node scripts/read_session.cjs context-pack sync-main \\
      --local-ref "$local_ref" \\
      --local-sha "$local_sha" \\
      --remote-ref "$remote_ref" \\
      --remote-sha "$remote_sha"
    return
  fi

  echo "[context-pack] WARN: bridge command not found; skipping context-pack sync"
}

while read -r local_ref local_sha remote_ref remote_sha; do
  if [[ "$local_ref" == "refs/heads/main" || "$remote_ref" == "refs/heads/main" ]]; then
    echo "[context-pack] validating main push for \${remote_name} (\${remote_url})"
    run_context_sync "$local_ref" "$local_sha" "$remote_ref" "$remote_sha" 2>&1 || {
      echo "[context-pack] WARN: sync-main failed; push is continuing (fail-open)" >&2
    }
  fi
done
`;
}

function main() {
  const options = parseArgs(process.argv);
  const repoRoot = runGit(['rev-parse', '--show-toplevel'], options.cwd, true);

  if (!repoRoot) {
    throw new Error(`Not a git repository (cwd: ${options.cwd})`);
  }

  const existingHooksPath = runGit(['config', '--get', 'core.hooksPath'], repoRoot, true);
  if (existingHooksPath && existingHooksPath !== '.githooks') {
    process.stdout.write(`[context-pack] WARNING: core.hooksPath is already set to '${existingHooksPath}'\n`);
    process.stdout.write('[context-pack] Overriding to .githooks; previous hooks path will be replaced.\n');
  }

  const hooksDir = path.join(repoRoot, '.githooks');
  const prePushPath = path.join(hooksDir, 'pre-push');
  const content = buildPrePushHook();
  const hadExistingHook = fs.existsSync(prePushPath);
  const contentUnchanged = hadExistingHook && fs.readFileSync(prePushPath, 'utf8') === content;

  if (!options.dryRun) {
    fs.mkdirSync(hooksDir, { recursive: true });
    fs.writeFileSync(prePushPath, content, 'utf8');
    fs.chmodSync(prePushPath, 0o755);
    runGit(['config', 'core.hooksPath', '.githooks'], repoRoot);
  }

  const statusLabel = options.dryRun ? 'planned' : (contentUnchanged ? 'unchanged' : 'updated');
  process.stdout.write(`[context-pack] ${statusLabel}: ${path.relative(repoRoot, prePushPath)}\n`);
  if (!options.dryRun) {
    process.stdout.write('[context-pack] git hooks path set to .githooks\n');
    process.stdout.write('[context-pack] pre-push hook is active\n');
  }
}

main();
