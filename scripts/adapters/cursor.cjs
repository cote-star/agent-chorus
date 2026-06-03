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
const cursorApp = require('./cursor_app.cjs');

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
  // Assemble candidates from BOTH surfaces (cursor-agent CLI JSONL +
  // Cursor IDE store.db), newest-first, then pick by id or cwd-match.
  // Each candidate carries the surface tag so `read` knows which reader
  // to dispatch to.
  const cliFiles = fs.existsSync(cursorDataBase) ? collectCursorTranscripts(id) : [];
  const appBase = cursorApp.cursorAppBaseDir();
  const appSessions = cursorApp.collectCursorAppSessions(appBase);

  const candidates = [];
  for (const f of cliFiles) {
    candidates.push({
      surface: 'cli',
      path: f.path,
      mtime: getFileTimestamp(f.path),
      resolveCwd: () => resolveCursorCwd(f.path),
    });
  }
  for (const s of appSessions) {
    if (id && !s.agent_id.includes(id) && !s.db_path.includes(id)) continue;
    candidates.push({
      surface: 'app',
      path: s.db_path,
      agent_id: s.agent_id,
      mtime: cursorApp.cursorAppModifiedIso(s.db_path),
      resolveCwd: () => cursorApp.cursorAppSessionWorkspace(s.db_path),
    });
  }
  if (candidates.length === 0) return null;
  candidates.sort((a, b) => String(b.mtime || '').localeCompare(String(a.mtime || '')));

  const warnings = [];
  let target;
  if (id) {
    target = candidates[0];
  } else if (cwd) {
    target = candidates.find((c) => {
      const sc = c.resolveCwd();
      return sc && cwdMatchesProject(sc, cwd);
    });
    if (!target) {
      warnings.push(`No Cursor session matched cwd ${normalizePath(cwd)}; falling back to latest session.`);
      target = candidates[0];
    }
  } else {
    target = candidates[0];
  }
  return target.surface === 'app'
    ? { path: target.path, surface: 'app', agent_id: target.agent_id, warnings }
    : { path: target.path, surface: 'cli', warnings };
}

function read(filePath, lastN, opts = {}) {
  lastN = lastN || 1;

  // Surface detection: paths ending in `store.db` are Cursor IDE sessions;
  // anything else is a cursor-agent CLI JSONL transcript.
  const isApp = filePath.endsWith('/store.db') || filePath.endsWith(path.sep + 'store.db');

  const turns = isApp
    ? cursorApp.readCursorAppTurns(filePath, opts.includeToolCalls === true)
    : readCursorTurnsRich(filePath, opts.includeToolCalls === true);

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

  let sessionId;
  let sessionCwd;
  let timestamp;
  if (isApp) {
    // For Cursor IDE sessions, session_id is the UUID directory (matches the
    // `agentId` field stored in meta); cwd comes from the embedded
    // Workspace Path header.
    sessionId = path.basename(path.dirname(filePath));
    sessionCwd = cursorApp.cursorAppSessionWorkspace(filePath);
    timestamp = cursorApp.cursorAppModifiedIso(filePath);
  } else {
    sessionId = path.basename(filePath, path.extname(filePath));
    sessionCwd = resolveCursorCwd(filePath);
    timestamp = getFileTimestamp(filePath);
  }

  return {
    agent: 'cursor',
    source: filePath,
    content: redactSensitiveText(content),
    warnings: [],
    session_id: sessionId,
    cwd: sessionCwd || null,
    timestamp,
    message_count: messageCount,
    messages_returned: messagesReturned,
    included_roles: rolesIncluded,
    ...(opts.includeToolCalls ? { included_tool_calls: true } : {}),
  };
}

function list(cwd, limit) {
  limit = limit || 10;
  const entries = [];

  // Surface 1: cursor-agent CLI JSONL.
  if (fs.existsSync(cursorDataBase)) {
    const files = collectCursorTranscripts(null);
    for (const f of files) {
      const sessionCwd = resolveCursorCwd(f.path);
      if (cwd && !(sessionCwd && cwdMatchesProject(sessionCwd, cwd))) continue;
      entries.push({
        session_id: path.basename(f.path, path.extname(f.path)),
        agent: 'cursor',
        source: 'cli',
        cwd: sessionCwd || null,
        modified_at: getFileTimestamp(f.path),
        file_path: f.path,
      });
    }
  }

  // Surface 2: Cursor IDE store.db.
  const appBase = cursorApp.cursorAppBaseDir();
  if (fs.existsSync(appBase)) {
    const sessions = cursorApp.collectCursorAppSessions(appBase);
    for (const s of sessions) {
      const sessionCwd = cursorApp.cursorAppSessionWorkspace(s.db_path);
      if (cwd && !(sessionCwd && cwdMatchesProject(sessionCwd, cwd))) continue;
      entries.push({
        session_id: s.agent_id,
        agent: 'cursor',
        source: 'app',
        cwd: sessionCwd || null,
        modified_at: cursorApp.cursorAppModifiedIso(s.db_path),
        file_path: s.db_path,
      });
    }
  }

  // Newest-first across both surfaces, then truncate.
  entries.sort((a, b) => String(b.modified_at || '').localeCompare(String(a.modified_at || '')));
  return entries.slice(0, limit);
}

function search(query, cwd, limit) {
  limit = limit || 10;
  const queryLower = String(query || '').toLowerCase();
  const entries = [];

  // Surface 1: cursor-agent CLI JSONL.
  if (fs.existsSync(cursorDataBase)) {
    const files = collectCursorTranscripts(null);
    for (const f of files) {
      const sessionCwd = resolveCursorCwd(f.path);
      if (cwd && !(sessionCwd && cwdMatchesProject(sessionCwd, cwd))) continue;
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
        source: 'cli',
        cwd: sessionCwd || null,
        modified_at: getFileTimestamp(f.path),
        file_path: f.path,
        match_snippet,
      });
    }
  }

  // Surface 2: Cursor IDE store.db.
  const appBase = cursorApp.cursorAppBaseDir();
  if (fs.existsSync(appBase)) {
    const sessions = cursorApp.collectCursorAppSessions(appBase);
    for (const s of sessions) {
      const sessionCwd = cursorApp.cursorAppSessionWorkspace(s.db_path);
      if (cwd && !(sessionCwd && cwdMatchesProject(sessionCwd, cwd))) continue;
      const assistantText = cursorApp.readCursorAppTurns(s.db_path, false)
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
        session_id: s.agent_id,
        agent: 'cursor',
        source: 'app',
        cwd: sessionCwd || null,
        modified_at: cursorApp.cursorAppModifiedIso(s.db_path),
        file_path: s.db_path,
        match_snippet,
      });
    }
  }

  // Newest-first, truncate.
  entries.sort((a, b) => String(b.modified_at || '').localeCompare(String(a.modified_at || '')));
  return entries.slice(0, limit);
}

module.exports = { resolve, read, list, search };
