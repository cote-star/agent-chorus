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
  if (message === null || message === undefined) {
    return '';
  }
  if (typeof message === 'string') {
    return message;
  }
  if (typeof message === 'object' && !Array.isArray(message)) {
    const content = message.content;
    if (Array.isArray(content)) {
      let out = '';
      for (const seg of content) {
        if (
          seg &&
          typeof seg === 'object' &&
          seg.type === 'text' &&
          typeof seg.text === 'string'
        ) {
          out += seg.text;
        }
      }
      return out;
    }
    if (typeof content === 'string') {
      return content;
    }
  }
  return '';
}

/**
 * Read a Cursor transcript (.jsonl) into [{role, text}] turns. Skips non-JSON
 * lines, roles other than user/assistant, and empty (trimmed) text. Preserves
 * order. Does NOT redact (done downstream by the integrator).
 * @param {string} filePath @returns {{role:string,text:string}[]}
 */
function readCursorTurns(filePath) {
  let raw;
  try {
    raw = fs.readFileSync(filePath, 'utf8');
  } catch {
    return [];
  }

  const turns = [];
  for (const line of raw.split('\n')) {
    if (line.trim() === '') {
      continue;
    }
    let obj;
    try {
      obj = JSON.parse(line);
    } catch {
      continue;
    }
    const role = obj.role;
    if (role !== 'user' && role !== 'assistant') {
      continue;
    }
    const text = flattenCursorMessage(obj.message).trim();
    if (text === '') {
      continue;
    }
    turns.push({ role, text });
  }
  return turns;
}

module.exports = { flattenCursorMessage, readCursorTurns };

if (require.main === module && process.argv.includes('--selftest')) {
  const assert = require('assert');
  const os = require('os');

  assert.strictEqual(
    flattenCursorMessage({
      content: [
        { type: 'text', text: 'alpha' },
        { type: 'tool_use', name: 'Read', input: { path: '/x' } },
        { type: 'text', text: 'beta' },
      ],
    }),
    'alphabeta',
  );

  assert.strictEqual(flattenCursorMessage({ content: 'hello' }), 'hello');
  assert.strictEqual(flattenCursorMessage('raw'), 'raw');
  assert.strictEqual(flattenCursorMessage(42), '');
  assert.strictEqual(flattenCursorMessage(null), '');

  const tmpFile = `${os.tmpdir()}/cursor_parse_selftest_${process.pid}_${Date.now()}.jsonl`;
  const jsonl = [
    JSON.stringify({
      role: 'user',
      message: { content: [{ type: 'text', text: 'user text' }] },
    }),
    JSON.stringify({
      role: 'assistant',
      message: {
        content: [
          { type: 'text', text: 'assistant text' },
          { type: 'tool_use', name: 'Grep', input: { pattern: 'x' } },
        ],
      },
    }),
    JSON.stringify({ role: 'tool', message: { content: [{ type: 'text', text: 'skip' }] } }),
    'not json',
    JSON.stringify({
      role: 'assistant',
      message: { content: [{ type: 'tool_use', name: 'Read', input: {} }] },
    }),
  ].join('\n');
  fs.writeFileSync(tmpFile, jsonl, 'utf8');
  try {
    const turns = readCursorTurns(tmpFile);
    assert.strictEqual(turns.length, 2);
    assert.deepStrictEqual(turns, [
      { role: 'user', text: 'user text' },
      { role: 'assistant', text: 'assistant text' },
    ]);
  } finally {
    try {
      fs.unlinkSync(tmpFile);
    } catch {
      // ignore cleanup errors
    }
  }

  console.log('cursor_parse selftest: OK');
}
