/**
 * Cursor workspace-cwd resolution (Node parity with cli/src/cursor_cwd.rs).
 *
 * 1. read `<project>/.workspace-trusted` -> "workspacePath" (authoritative), else
 * 2. demangle the project dir name against the real filesystem.
 *
 * FULL SPEC: docs/adapters/CURSOR_HERMES_NATIVE_ADAPTER.md  §6 Unit A'.
 * Implementer: fill the function bodies + the --selftest assertions below.
 * Do NOT change the exported names/shapes and do NOT edit any other file.
 * Allowed: Node built-ins (fs, path, assert) only. No new npm deps.
 */

const fs = require('fs');
const path = require('path');

function isDirectory(p) {
  try {
    return fs.existsSync(p) && fs.statSync(p).isDirectory();
  } catch {
    return false;
  }
}

/**
 * Walk `tokens` (a path split on '-') as a chain of EXISTING directories under
 * `base`. A single real dir name may span several tokens (names can contain '-').
 * Returns the deepest matched absolute path string iff the FULL token list is
 * consumed by existing directories; otherwise null. Backtracking, longest-first.
 * @param {string} base @param {string[]} tokens @returns {string|null}
 */
function walkExisting(base, tokens) {
  if (tokens.length === 0) {
    return isDirectory(base) ? base : null;
  }
  for (let j = tokens.length; j >= 1; j--) {
    const name = tokens.slice(0, j).join('-');
    const child = path.join(base, name);
    if (isDirectory(child)) {
      const got = walkExisting(child, tokens.slice(j));
      if (got !== null) return got;
    }
  }
  return null;
}

/**
 * Demangle a Cursor project dir name into the real absolute path, fs-walking from '/'.
 * @param {string} projectName @returns {string|null}
 */
function demangleProjectDir(projectName) {
  return walkExisting('/', projectName.split('-'));
}

/**
 * Resolve the originating workspace cwd for a transcript file at
 * <...>/<project>/agent-transcripts/<session>/<session>.jsonl
 * Order: (1) <project>/.workspace-trusted -> workspacePath; (2) demangle; (3) null.
 * @param {string} transcriptPath @returns {string|null}
 */
function resolveCursorCwd(transcriptPath) {
  const projectDir = path.dirname(path.dirname(path.dirname(transcriptPath)));

  const trustedPath = path.join(projectDir, '.workspace-trusted');
  try {
    if (fs.existsSync(trustedPath)) {
      const raw = fs.readFileSync(trustedPath, 'utf-8');
      const obj = JSON.parse(raw);
      if (
        obj &&
        typeof obj.workspacePath === 'string' &&
        obj.workspacePath.length > 0
      ) {
        return obj.workspacePath;
      }
    }
  } catch {
    // fall through to demangle
  }

  const projectName = path.basename(projectDir);
  if (projectName) {
    return demangleProjectDir(projectName);
  }
  return null;
}

module.exports = { walkExisting, demangleProjectDir, resolveCursorCwd };

if (require.main === module && process.argv.includes('--selftest')) {
  const assert = require('assert');
  const os = require('os');

  function freshDir(name) {
    const dir = path.join(os.tmpdir(), name);
    fs.rmSync(dir, { recursive: true, force: true });
    fs.mkdirSync(dir, { recursive: true });
    return dir;
  }

  const baseSimple = freshDir('cursor_cwd_walk_simple');
  fs.mkdirSync(path.join(baseSimple, 'a', 'b'), { recursive: true });
  assert.strictEqual(walkExisting(baseSimple, ['a', 'b']), path.join(baseSimple, 'a', 'b'));

  const baseDashed = freshDir('cursor_cwd_walk_dashed');
  const dashedTarget = path.join(baseDashed, 'trust-stream', 'trust-stream-backend');
  fs.mkdirSync(dashedTarget, { recursive: true });
  assert.strictEqual(
    walkExisting(baseDashed, ['trust', 'stream', 'trust', 'stream', 'backend']),
    dashedTarget,
  );

  const baseNope = freshDir('cursor_cwd_walk_nope');
  assert.strictEqual(walkExisting(baseNope, ['nope']), null);

  const basePlayFoo = freshDir('cursor_cwd_walk_play_dash');
  fs.mkdirSync(path.join(basePlayFoo, 'play-foo'), { recursive: true });
  assert.strictEqual(
    walkExisting(basePlayFoo, ['play', 'foo']),
    path.join(basePlayFoo, 'play-foo'),
  );

  const basePlaySlashFoo = freshDir('cursor_cwd_walk_play_slash');
  fs.mkdirSync(path.join(basePlaySlashFoo, 'play', 'foo'), { recursive: true });
  assert.strictEqual(
    walkExisting(basePlaySlashFoo, ['play', 'foo']),
    path.join(basePlaySlashFoo, 'play', 'foo'),
  );

  const workspacePath = freshDir('cursor_cwd_resolve_workspace');
  const session = 'sess-abc';
  const projectDir = freshDir('cursor_cwd_resolve_project');
  const transcriptPath = path.join(
    projectDir,
    'agent-transcripts',
    session,
    `${session}.jsonl`,
  );
  fs.mkdirSync(path.dirname(transcriptPath), { recursive: true });
  fs.writeFileSync(transcriptPath, '');
  fs.writeFileSync(
    path.join(projectDir, '.workspace-trusted'),
    JSON.stringify({ workspacePath }),
  );
  assert.strictEqual(resolveCursorCwd(transcriptPath), workspacePath);

  const missingProject = 'cursor-cwd-nonexistent-project-zzzz-no-match';
  const nullProjectDir = path.join(
    os.tmpdir(),
    'cursor_cwd_resolve_null',
    missingProject,
  );
  fs.rmSync(nullProjectDir, { recursive: true, force: true });
  fs.mkdirSync(path.join(nullProjectDir, 'agent-transcripts', session), {
    recursive: true,
  });
  const nullTranscript = path.join(
    nullProjectDir,
    'agent-transcripts',
    session,
    `${session}.jsonl`,
  );
  fs.writeFileSync(nullTranscript, '');
  assert.strictEqual(resolveCursorCwd(nullTranscript), null);

  console.log('cursor_cwd selftest: OK');
}
