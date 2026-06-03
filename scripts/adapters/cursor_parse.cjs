/**
 * Cursor transcript parsing (Node parity with cli/src/cursor_parse.rs).
 *
 * Flattens cursor-agent JSONL lines `{"role","message":{"content":[...]}}` into
 * ordered {role, text} turns, keeping only segments whose type == "text".
 *
 * FULL SPEC: docs/adapters/CURSOR_HERMES_NATIVE_ADAPTER.md  §6 Unit B'.
 * Implementer: fill the function bodies + the --selftest assertions below.
 * Do NOT change the exported names/shapes and do NOT edit any other file.
 * Allowed: Node built-ins (fs, path, assert) only. No new npm deps.
 */

const fs = require('fs');

/**
 * Flatten a Cursor transcript line's `message` value into plain text.
 * - object with content: [ {type:"text", text}, ... ] -> concat text segments in order
 * - object with content: "<string>" -> that string
 * - string -> the string as-is
 * - else -> ""
 * @param {*} message @returns {string}
 */
function flattenCursorMessage(message) {
  throw new Error("Unit B': not implemented — see docs/adapters/CURSOR_HERMES_NATIVE_ADAPTER.md §6");
}

/**
 * Read a Cursor transcript (.jsonl) into [{role, text}] turns. Skips non-JSON
 * lines, roles other than user/assistant, and empty (trimmed) text. Preserves
 * order. Does NOT redact (done downstream by the integrator).
 * @param {string} filePath @returns {{role:string,text:string}[]}
 */
function readCursorTurns(filePath) {
  throw new Error("Unit B': not implemented");
}

module.exports = { flattenCursorMessage, readCursorTurns };

if (require.main === module && process.argv.includes('--selftest')) {
  // Implementer: add assertions per spec §6 Unit B' (use require('assert') + fs temp files).
  throw new Error("Unit B': selftest not implemented");
}
