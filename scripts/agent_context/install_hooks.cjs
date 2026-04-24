#!/usr/bin/env node
'use strict';

const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

const SENTINEL_START = '# --- agent-chorus:pre-push:start ---';
const SENTINEL_END = '# --- agent-chorus:pre-push:end ---';
// Legacy sentinels for backward compatibility during migration
const LEGACY_SENTINEL_START = '# --- agent-bridge:pre-push:start ---';
const LEGACY_SENTINEL_END = '# --- agent-bridge:pre-push:end ---';

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

function buildChorusSection() {
  // Kept in sync with `build_pre_push_hook_section` in
  // cli/src/agent_context.rs. P6: pack-only pushes skip the freshness cycle,
  // and a pack-only follow-up push can report "warning appears addressed"
  // by consulting `.agent-context/current/.last_freshness.json`.
  return `remote_name="\${1:-origin}"
remote_url="\${2:-unknown}"

# P6: when the push range only touches \`.agent-context/\`, skip the freshness
# cycle entirely. This avoids the noise loop where a code push warns "pack is
# stale", the agent updates the pack and pushes, and the hook re-warns about
# the pack's own commit.
pack_only_skip() {
  local base="$1"
  local head="$2"

  if [[ -z "$base" || -z "$head" || "$base" == "0000000000000000000000000000000000000000" ]]; then
    return 1
  fi

  local changed
  changed="$(git diff --name-only "\${base}..\${head}" 2>/dev/null || true)"
  if [[ -z "$changed" ]]; then
    return 1
  fi

  if printf '%s\\n' "$changed" | grep -vE '^\\.agent-context/' >/dev/null 2>&1; then
    return 1
  fi

  echo "[context-pack] pack-only push, skipping freshness check"

  local state_file=".agent-context/current/.last_freshness.json"
  if [[ -f "$state_file" ]]; then
    local sections=""
    if command -v jq >/dev/null 2>&1; then
      sections="$(jq -r '.affected_sections[]?' "$state_file" 2>/dev/null | tr '\\n' ' ')"
    elif command -v python3 >/dev/null 2>&1; then
      sections="$(python3 -c "import json,sys
try:
    d=json.load(open('$state_file'))
    for s in d.get('affected_sections', []):
        print(s)
except Exception:
    pass" 2>/dev/null | tr '\\n' ' ')"
    fi

    if [[ -n "\${sections// /}" ]]; then
      local covered=""
      local missing=""
      for section in $sections; do
        if printf '%s\\n' "$changed" | grep -F ".agent-context/current/\${section}" >/dev/null 2>&1; then
          covered="\${covered}\${section} "
        else
          missing="\${missing}\${section} "
        fi
      done
      if [[ -n "\${covered// /}" ]]; then
        echo "[context-pack] warning appears addressed: sections [\${covered% }] updated"
      fi
      if [[ -n "\${missing// /}" ]]; then
        echo "[context-pack] note: sections still referenced by last warning: [\${missing% }]"
      fi
    fi
  fi

  return 0
}

run_context_sync() {
  local local_ref="$1"
  local local_sha="$2"
  local remote_ref="$3"
  local remote_sha="$4"

  if command -v chorus >/dev/null 2>&1; then
    chorus context-pack sync-main \\
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

  echo "[context-pack] WARN: chorus command not found; skipping context-pack sync"
}

while read -r local_ref local_sha remote_ref remote_sha; do
  if [[ "$local_ref" == "refs/heads/main" || "$remote_ref" == "refs/heads/main" ]]; then
    echo "[context-pack] validating main push for \${remote_name} (\${remote_url})"
    if pack_only_skip "$remote_sha" "$local_sha"; then
      continue
    fi
    run_context_sync "$local_ref" "$local_sha" "$remote_ref" "$remote_sha" 2>&1 || {
      echo "[context-pack] WARN: sync-main failed; push is continuing (fail-open)" >&2
    }
  fi
done`;
}

function main() {
  const options = parseArgs(process.argv);
  const repoRoot = runGit(['rev-parse', '--show-toplevel'], options.cwd, true);

  if (!repoRoot) {
    throw new Error(`Not a git repository (cwd: ${options.cwd})`);
  }

  const existingHooksPath = runGit(['config', '--get', 'core.hooksPath'], repoRoot, true);

  // Determine hooks directory — prefer existing if set, otherwise use .githooks
  let hooksDir;
  if (existingHooksPath) {
    if (existingHooksPath !== '.githooks') {
      process.stdout.write(`[context-pack] NOTE: core.hooksPath is '${existingHooksPath}'; appending chorus hook there.\n`);
    }
    hooksDir = path.join(repoRoot, existingHooksPath);
  } else {
    hooksDir = path.join(repoRoot, '.githooks');
  }

  const prePushPath = path.join(hooksDir, 'pre-push');
  const chorusSection = `${SENTINEL_START}\n${buildChorusSection()}\n${SENTINEL_END}`;

  let finalContent;
  if (fs.existsSync(prePushPath)) {
    const existing = fs.readFileSync(prePushPath, 'utf8');
    // Detect new or legacy sentinels
    let sentStart, sentEnd;
    if (existing.includes(SENTINEL_START) && existing.includes(SENTINEL_END)) {
      sentStart = SENTINEL_START; sentEnd = SENTINEL_END;
    } else if (existing.includes(LEGACY_SENTINEL_START) && existing.includes(LEGACY_SENTINEL_END)) {
      sentStart = LEGACY_SENTINEL_START; sentEnd = LEGACY_SENTINEL_END;
    } else {
      sentStart = null; sentEnd = null;
    }
    if (sentStart && sentEnd) {
      // Replace existing chorus section
      const startIdx = existing.indexOf(sentStart);
      let endIdx = existing.indexOf(sentEnd) + sentEnd.length;
      if (existing[endIdx] === '\n') endIdx++;
      finalContent = existing.slice(0, startIdx) + chorusSection + '\n' + existing.slice(endIdx);
    } else {
      // Append chorus section to existing hook
      let content = existing;
      if (!content.endsWith('\n')) content += '\n';
      content += '\n' + chorusSection + '\n';
      finalContent = content;
    }
  } else {
    // Create new hook file with shebang
    finalContent = `#!/usr/bin/env bash\nset -euo pipefail\n\n${chorusSection}\n`;
  }

  const contentUnchanged = fs.existsSync(prePushPath) && fs.readFileSync(prePushPath, 'utf8') === finalContent;

  if (!options.dryRun) {
    fs.mkdirSync(hooksDir, { recursive: true });
    fs.writeFileSync(prePushPath, finalContent, 'utf8');
    fs.chmodSync(prePushPath, 0o755);
    // Only set core.hooksPath if it wasn't already configured
    if (!existingHooksPath) {
      runGit(['config', 'core.hooksPath', '.githooks'], repoRoot);
    }
  }

  const statusLabel = options.dryRun ? 'planned' : (contentUnchanged ? 'unchanged' : 'updated');
  process.stdout.write(`[context-pack] ${statusLabel}: ${path.relative(repoRoot, prePushPath)}\n`);
  if (!options.dryRun) {
    process.stdout.write('[context-pack] pre-push hook is active\n');
  }
}

main();
