/**
 * Hermes agent adapter (provisional scaffold — UNTESTED).
 *
 * Hermes is not yet installed and its on-disk transcript format is unconfirmed.
 * This adapter is wired for parity but has NOT been validated against real data.
 * Assumed shape (claude-like JSONL; override root via CHORUS_HERMES_DATA_DIR):
 *   <base>/**\/*.jsonl  with lines {"role":"user"|"assistant","content":"<str>","cwd":"<path?>"}
 * Revisit once Hermes is available. Returns cleanly when no data exists.
 */

const fs = require('fs');
const path = require('path');
const {
  normalizePath, collectMatchingFiles, getFileTimestamp, redactSensitiveText,
  isSystemDirectory, cwdMatchesProject, findLatestByCwd,
} = require('./utils.cjs');

const hermesDataBase = normalizePath(
  process.env.CHORUS_HERMES_DATA_DIR
  || process.env.BRIDGE_HERMES_DATA_DIR
  || '~/.hermes/sessions',
);

if (isSystemDirectory(hermesDataBase)) {
  throw new Error(`Refusing to scan system directory: ${hermesDataBase}`);
}

function jsonlObjects(filePath) {
  let raw;
  try {
    raw = fs.readFileSync(filePath, 'utf8');
  } catch {
    return [];
  }
  const out = [];
  for (const line of raw.split('\n')) {
    if (line.trim() === '') continue;
    try {
      out.push(JSON.parse(line));
    } catch {
      /* skip */
    }
  }
  return out;
}

function collectHermesSessions(id) {
  if (!fs.existsSync(hermesDataBase)) return [];
  return collectMatchingFiles(hermesDataBase, (fullPath, name) => (
    name.endsWith('.jsonl') && (id ? fullPath.includes(id) : true)
  ), true);
}

function getHermesCwd(filePath) {
  for (const o of jsonlObjects(filePath)) {
    if (o && typeof o.cwd === 'string') return o.cwd;
  }
  return null;
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
      if (turns[i].role === 'user') { userIndex = i; break; }
    }
    if (userIndex >= 0) selected.push(turns[userIndex]);
    selected.push(turns[assistantIndex]);
    lowerBound = assistantIndex + 1;
  }
  return selected;
}

function resolve(id, cwd, opts) {
  if (!fs.existsSync(hermesDataBase)) return null;
  const files = collectHermesSessions(id);
  if (files.length === 0) return null;

  const warnings = [];
  let targetPath;
  if (id) {
    targetPath = files[0].path;
  } else if (cwd) {
    targetPath = findLatestByCwd(files, getHermesCwd, cwd);
    if (!targetPath) {
      warnings.push(`No Hermes session matched cwd ${normalizePath(cwd)}; falling back to latest session.`);
      targetPath = files[0].path;
    }
  } else {
    targetPath = files[0].path;
  }
  return { path: targetPath, warnings };
}

function read(filePath, lastN, opts = {}) {
  lastN = lastN || 1;

  const turns = [];
  for (const o of jsonlObjects(filePath)) {
    if (o.role !== 'user' && o.role !== 'assistant') continue;
    const text = typeof o.content === 'string' ? o.content : JSON.stringify(o.content || '');
    if (!text.trim()) continue;
    turns.push({ role: o.role, text });
  }
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

  return {
    agent: 'hermes',
    source: filePath,
    content: redactSensitiveText(content),
    warnings: [],
    session_id: path.basename(filePath, path.extname(filePath)),
    cwd: getHermesCwd(filePath),
    timestamp: getFileTimestamp(filePath),
    message_count: messageCount,
    messages_returned: messagesReturned,
    included_roles: rolesIncluded,
  };
}

function list(cwd, limit) {
  limit = limit || 10;
  if (!fs.existsSync(hermesDataBase)) return [];
  const files = collectHermesSessions(null);
  const entries = [];
  for (const f of files) {
    if (entries.length >= limit) break;
    const sessionCwd = getHermesCwd(f.path);
    if (cwd && !(sessionCwd && cwdMatchesProject(sessionCwd, cwd))) continue;
    entries.push({
      session_id: path.basename(f.path, path.extname(f.path)),
      agent: 'hermes',
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
  if (!fs.existsSync(hermesDataBase)) return [];
  const files = collectHermesSessions(null);
  const entries = [];
  for (const f of files) {
    if (entries.length >= limit) break;
    const sessionCwd = getHermesCwd(f.path);
    if (cwd && !(sessionCwd && cwdMatchesProject(sessionCwd, cwd))) continue;
    const assistantText = jsonlObjects(f.path)
      .filter(o => o.role === 'assistant' && typeof o.content === 'string')
      .map(o => o.content)
      .join('\n');
    const lower = assistantText.toLowerCase();
    if (!lower.includes(queryLower)) continue;
    const idx = lower.indexOf(queryLower);
    const match_snippet = assistantText
      .slice(Math.max(0, idx - 60), Math.min(assistantText.length, idx + queryLower.length + 60))
      .replace(/\n/g, ' ');
    entries.push({
      session_id: path.basename(f.path, path.extname(f.path)),
      agent: 'hermes',
      cwd: sessionCwd || null,
      modified_at: getFileTimestamp(f.path),
      file_path: f.path,
      match_snippet,
    });
  }
  return entries;
}

module.exports = { resolve, read, list, search };
