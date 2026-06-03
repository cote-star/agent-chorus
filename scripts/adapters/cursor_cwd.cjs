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

/**
 * Walk `tokens` (a path split on '-') as a chain of EXISTING directories under
 * `base`. A single real dir name may span several tokens (names can contain '-').
 * Returns the deepest matched absolute path string iff the FULL token list is
 * consumed by existing directories; otherwise null. Backtracking, longest-first.
 * @param {string} base @param {string[]} tokens @returns {string|null}
 */
function walkExisting(base, tokens) {
  throw new Error("Unit A': not implemented — see docs/adapters/CURSOR_HERMES_NATIVE_ADAPTER.md §6");
}

/**
 * Demangle a Cursor project dir name into the real absolute path, fs-walking from '/'.
 * @param {string} projectName @returns {string|null}
 */
function demangleProjectDir(projectName) {
  throw new Error("Unit A': not implemented");
}

/**
 * Resolve the originating workspace cwd for a transcript file at
 * <...>/<project>/agent-transcripts/<session>/<session>.jsonl
 * Order: (1) <project>/.workspace-trusted -> workspacePath; (2) demangle; (3) null.
 * @param {string} transcriptPath @returns {string|null}
 */
function resolveCursorCwd(transcriptPath) {
  throw new Error("Unit A': not implemented");
}

module.exports = { walkExisting, demangleProjectDir, resolveCursorCwd };

if (require.main === module && process.argv.includes('--selftest')) {
  // Implementer: add assertions per spec §6 Unit A' (use require('assert') + fs temp dirs).
  throw new Error("Unit A': selftest not implemented");
}
