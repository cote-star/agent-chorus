/**
 * Cursor agent adapter (native).
 *
 * Reads cursor-agent CLI transcripts directly from the projects tree:
 *   <base>/<project>/agent-transcripts/<session>/<session>.jsonl
 * where <base> defaults to ~/.cursor/projects (override via CHORUS_CURSOR_DATA_DIR
 * / BRIDGE_CURSOR_DATA_DIR). Per-session cwd is recovered (cursor_cwd.cjs) so
 * --cwd scoping works the same as codex/claude — no external bridge required.
 */

const fs = require('fs');
const path = require('path');
const {
  normalizePath, collectMatchingFiles, getFileTimestamp, redactSensitiveText,
  isSystemDirectory, cwdMatchesProject, findLatestByCwd, extractContentWithToolCalls,
} = require('./utils.cjs');
const { resolveCursorCwd } = require('./cursor_cwd.cjs');
const { readCursorTurns } = require('./cursor_parse.cjs');

// Build turns for the read path. Text-only by default; with --tool-calls, render
// tool_use/tool_result segments too. Cursor's content array matches the shape
// utils.extractContentWithToolCalls already handles, so reuse it (parity).
function readCursorTurnsRich(filePath, includeToolCalls) {
  if (!includeToolCalls) return readCursorTurns(filePath);
  let raw;
  try {
    raw = fs.readFileSync(filePath, 'utf8');
  } catch {
    return [];
  }
  const turns = [];
  for (const line of raw.split('\n')) {
    if (line.trim() === '') continue;
    let obj;
    try {
      obj = JSON.parse(line);
    } catch {
      continue;
    }
    if (obj.role !== 'user' && obj.role !== 'assistant') continue;
    const content = obj.message && obj.message.content;
    const text = extractContentWithToolCalls(content).trim();
    if (text === '') continue;
    turns.push({ role: obj.role, text });
  }
  return turns;
}

const cursorDataBase = normalizePath(
  process.env.CHORUS_CURSOR_DATA_DIR
  || process.env.BRIDGE_CURSOR_DATA_DIR
  || '~/.cursor/projects',
);

if (isSystemDirectory(cursorDataBase)) {
  throw new Error(`Refusing to scan system directory: ${cursorDataBase}`);
}

// Enumerate cursor-agent transcript files (newest first). When `id` is given,
// only paths containing it are returned.
function collectCursorTranscripts(id) {
  return collectMatchingFiles(cursorDataBase, (fullPath, name) => (
    name.endsWith('.jsonl')
    && fullPath.includes('agent-transcripts')
    && (id ? fullPath.includes(id) : true)
  ), true);
}

function selectConversationTurns(turns, lastN) {
  const assistantIndexes = [];
  for (let i = 0; i < turns.length; i += 1) {
    if (turns[i].role === 'assistant') assistantIndexes.push(i);
  }
  if (assistantIndexes.length === 0) return [];

  const selected = [];
  let lowerBound = 0;
  for (const assistantIndex of assistantIndexes.slice(-Math.max(1, lastN))) {
    let userIndex = -1;
    for (let i = assistantIndex - 1; i >= lowerBound; i -= 1) {
      if (turns[i].role === 'user') {
        userIndex = i;
        break;
      }
    }
    if (userIndex >= 0) selected.push(turns[userIndex]);
    selected.push(turns[assistantIndex]);
    lowerBound = assistantIndex + 1;
  }
  return selected;
}

function resolve(id, cwd, opts) {
  if (!fs.existsSync(cursorDataBase)) return null;
  const files = collectCursorTranscripts(id);
  if (files.length === 0) return null;

  const warnings = [];
  let targetPath;
  if (id) {
    targetPath = files[0].path;
  } else if (cwd) {
    targetPath = findLatestByCwd(files, resolveCursorCwd, cwd);
    if (!targetPath) {
      warnings.push(`No Cursor session matched cwd ${normalizePath(cwd)}; falling back to latest session.`);
      targetPath = files[0].path;
    }
  } else {
    targetPath = files[0].path;
  }
  return { path: targetPath, warnings };
}

function read(filePath, lastN, opts = {}) {
  lastN = lastN || 1;

  const turns = readCursorTurnsRich(filePath, opts.includeToolCalls === true);
  const assistantMsgs = turns.filter(t => t.role === 'assistant').map(t => t.text);
  const messageCount = assistantMsgs.length;

  let content = '';
  let messagesReturned = 1;
  let rolesIncluded = ['assistant'];

  if (opts.includeUser && assistantMsgs.length > 0) {
    const selected = selectConversationTurns(turns, lastN);
    messagesReturned = selected.length;
    rolesIncluded = ['user', 'assistant'];
    content = selected.map(m => `${m.role.toUpperCase()}:\n${m.text}`).join('\n---\n');
  } else if (lastN > 1 && assistantMsgs.length > 0) {
    const selected = assistantMsgs.slice(-lastN);
    messagesReturned = selected.length;
    content = selected.join('\n---\n');
  } else if (assistantMsgs.length > 0) {
    content = assistantMsgs[assistantMsgs.length - 1];
  } else {
    content = '[No assistant messages found]';
    messagesReturned = 0;
  }

  const sessionId = path.basename(filePath, path.extname(filePath));
  const sessionCwd = resolveCursorCwd(filePath);

  return {
    agent: 'cursor',
    source: filePath,
    content: redactSensitiveText(content),
    warnings: [],
    session_id: sessionId,
    cwd: sessionCwd || null,
    timestamp: getFileTimestamp(filePath),
    message_count: messageCount,
    messages_returned: messagesReturned,
    included_roles: rolesIncluded,
    ...(opts.includeToolCalls ? { included_tool_calls: true } : {}),
  };
}

function list(cwd, limit) {
  limit = limit || 10;
  if (!fs.existsSync(cursorDataBase)) return [];

  const files = collectCursorTranscripts(null);
  const entries = [];
  for (const f of files) {
    if (entries.length >= limit) break;

    const sessionCwd = resolveCursorCwd(f.path);
    if (cwd && !(sessionCwd && cwdMatchesProject(sessionCwd, cwd))) {
      continue;
    }

    entries.push({
      session_id: path.basename(f.path, path.extname(f.path)),
      agent: 'cursor',
      cwd: sessionCwd || null,
      modified_at: getFileTimestamp(f.path),
      file_path: f.path,
    });
  }
  return entries;
}

function search(query, cwd, limit) {
  limit = limit || 10;
  const queryLower = String(query || '').toLowerCase();
  if (!fs.existsSync(cursorDataBase)) return [];

  const files = collectCursorTranscripts(null);
  const entries = [];

  for (const f of files) {
    if (entries.length >= limit) break;

    const sessionCwd = resolveCursorCwd(f.path);
    if (cwd && !(sessionCwd && cwdMatchesProject(sessionCwd, cwd))) {
      continue;
    }

    const assistantText = readCursorTurns(f.path)
      .filter(t => t.role === 'assistant')
      .map(t => t.text)
      .join('\n');

    const lower = assistantText.toLowerCase();
    if (!lower.includes(queryLower)) continue;

    const idx = lower.indexOf(queryLower);
    const snippetStart = Math.max(0, idx - 60);
    const snippetEnd = Math.min(assistantText.length, idx + queryLower.length + 60);
    const match_snippet = assistantText.slice(snippetStart, snippetEnd).replace(/\n/g, ' ');

    entries.push({
      session_id: path.basename(f.path, path.extname(f.path)),
      agent: 'cursor',
      cwd: sessionCwd || null,
      modified_at: getFileTimestamp(f.path),
      file_path: f.path,
      match_snippet,
    });
  }

  return entries;
}

module.exports = { resolve, read, list, search };
