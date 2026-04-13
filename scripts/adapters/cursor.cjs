/**
 * Cursor agent adapter.
 */

const fs = require('fs');
const path = require('path');
const {
  normalizePath, collectMatchingFiles, getFileTimestamp, redactSensitiveText, isSystemDirectory,
} = require('./utils.cjs');

const cursorDataBase = normalizePath(process.env.CHORUS_CURSOR_DATA_DIR || process.env.BRIDGE_CURSOR_DATA_DIR || (
  process.platform === 'darwin'
    ? '~/Library/Application Support/Cursor'
    : '~/.cursor'
));

if (isSystemDirectory(cursorDataBase)) {
  throw new Error(`Refusing to scan system directory: ${cursorDataBase}`);
}

function getWorkspacesDir() {
  return path.join(cursorDataBase, 'User', 'workspaceStorage');
}

function isCursorFile(name) {
  return (name.endsWith('.json') || name.endsWith('.jsonl'))
    && (name.includes('chat') || name.includes('composer') || name.includes('conversation'));
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
  const workspacesDir = getWorkspacesDir();
  if (!fs.existsSync(workspacesDir)) return null;

  const files = collectMatchingFiles(workspacesDir, (fullPath, name) => {
    if (!isCursorFile(name)) return false;
    if (id) return fullPath.includes(id);
    return true;
  }, true);

  const warnings = ['Warning: Cursor sessions have no project scoping. Results may include sessions from unrelated projects.'];
  return files.length > 0 ? { path: files[0].path, warnings } : null;
}

function read(filePath, lastN, opts = {}) {
  lastN = lastN || 1;
  const raw = fs.readFileSync(filePath, 'utf-8');
  let content = '';
  let messageCount = 0;
  let messagesReturned = 1;
  let rolesIncluded = ['assistant'];

  try {
    const json = JSON.parse(raw);
    if (Array.isArray(json.messages)) {
      const turns = json.messages
        .filter(m => m.role === 'assistant' || m.role === 'user')
        .map(m => ({
          role: m.role,
          text: typeof m.content === 'string' ? m.content : JSON.stringify(m.content || ''),
        }));
      const assistantMsgs = turns.filter(m => m.role === 'assistant').map(m => m.text);
      messageCount = assistantMsgs.length;
      if (opts.includeUser && assistantMsgs.length > 0) {
        const selected = selectConversationTurns(turns, lastN);
        messagesReturned = selected.length;
        rolesIncluded = ['user', 'assistant'];
        content = selected.map(m => `${m.role.toUpperCase()}:\n${m.text}`).join('\n---\n');
      } else if (lastN > 1 && assistantMsgs.length > 0) {
        const selected = assistantMsgs.slice(-lastN);
        messagesReturned = selected.length;
        content = selected.join('\n---\n');
      } else {
        content = assistantMsgs.length > 0
          ? assistantMsgs[assistantMsgs.length - 1]
          : '[No assistant messages found]';
      }
    } else if (typeof json.content === 'string') {
      content = json.content;
      messageCount = 1;
    } else {
      content = JSON.stringify(json, null, 2);
    }
  } catch (error) {
    const lines = raw.split('\n').filter(Boolean);
    const turns = [];
    for (const line of lines) {
      try {
        const json = JSON.parse(line);
        if ((json.role === 'assistant' || json.role === 'user') && typeof json.content === 'string') {
          turns.push({ role: json.role, text: json.content });
        }
      } catch (e) { /* skip */ }
    }
    const assistantMsgs = turns.filter(m => m.role === 'assistant').map(m => m.text);
    messageCount = assistantMsgs.length;
    if (opts.includeUser && assistantMsgs.length > 0) {
      const selected = selectConversationTurns(turns, lastN);
      messagesReturned = selected.length;
      rolesIncluded = ['user', 'assistant'];
      content = selected.map(m => `${m.role.toUpperCase()}:\n${m.text}`).join('\n---\n');
    } else if (assistantMsgs.length > 0) {
      if (lastN > 1) {
        const selected = assistantMsgs.slice(-lastN);
        messagesReturned = selected.length;
        content = selected.join('\n---\n');
      } else {
        content = assistantMsgs[assistantMsgs.length - 1];
      }
    } else {
      content = lines.slice(-20).join('\n');
      messagesReturned = 0;
    }
  }

  const sessionId = path.basename(filePath, path.extname(filePath));

  return {
    agent: 'cursor',
    source: filePath,
    content: redactSensitiveText(content),
    warnings: [],
    session_id: sessionId,
    cwd: null,
    timestamp: getFileTimestamp(filePath),
    message_count: messageCount,
    messages_returned: messagesReturned,
    included_roles: rolesIncluded,
  };
}

function list(cwd, limit) {
  limit = limit || 10;
  if (!fs.existsSync(cursorDataBase)) return [];
  const workspacesDir = getWorkspacesDir();
  if (!fs.existsSync(workspacesDir)) return [];

  const files = collectMatchingFiles(workspacesDir, (_fp, name) => isCursorFile(name), true);
  const expectedCwd = cwd ? normalizePath(cwd).toLowerCase() : null;
  const entries = [];
  for (const f of files) {
    if (entries.length >= limit) break;

    if (expectedCwd) {
      let raw;
      try {
        raw = fs.readFileSync(f.path, 'utf-8');
      } catch (error) {
        continue;
      }
      if (!raw.toLowerCase().includes(expectedCwd)) {
        continue;
      }
    }

    entries.push({
      session_id: path.basename(f.path, path.extname(f.path)),
      agent: 'cursor',
      cwd: null,
      modified_at: getFileTimestamp(f.path),
      file_path: f.path,
    });
  }

  return entries;
}

function search(query, cwd, limit) {
  limit = limit || 10;
  const queryLower = String(query || '').toLowerCase();
  const expectedCwd = cwd ? normalizePath(cwd).toLowerCase() : null;
  if (!fs.existsSync(cursorDataBase)) return [];
  const workspacesDir = getWorkspacesDir();
  if (!fs.existsSync(workspacesDir)) return [];

  const files = collectMatchingFiles(workspacesDir, (_fp, name) => isCursorFile(name), true);
  const entries = [];

  for (const f of files) {
    if (entries.length >= limit) break;

    let raw;
    try {
      raw = fs.readFileSync(f.path, 'utf-8');
    } catch (error) {
      continue;
    }

    let assistantText = '';
    try {
      // Try parsing as JSON to extract assistant content
      const parsed = JSON.parse(raw);
      // Handle { messages: [...] } wrapper (like read() expects)
      const msgs = Array.isArray(parsed) ? parsed
        : (Array.isArray(parsed.messages) ? parsed.messages : []);
      for (const msg of msgs) {
        if (msg.role === 'assistant' && msg.content) {
          assistantText += (typeof msg.content === 'string' ? msg.content : JSON.stringify(msg.content)) + '\n';
        }
      }
    } catch (_e) {
      // Fallback: try JSONL line-by-line, then raw content
      const lines = raw.split('\n').filter(Boolean);
      for (const line of lines) {
        try {
          const obj = JSON.parse(line);
          if (obj.role === 'assistant' && typeof obj.content === 'string') {
            assistantText += obj.content + '\n';
          }
        } catch (_) { /* skip */ }
      }
      if (!assistantText) assistantText = raw;
    }

    const lower = assistantText.toLowerCase();
    if (expectedCwd && !lower.includes(expectedCwd)) {
      continue;
    }
    if (!lower.includes(queryLower)) {
      continue;
    }

    const idx = lower.indexOf(queryLower);
    const snippetStart = Math.max(0, idx - 60);
    const snippetEnd = Math.min(assistantText.length, idx + queryLower.length + 60);
    const match_snippet = assistantText.slice(snippetStart, snippetEnd).replace(/\n/g, ' ');

    entries.push({
      session_id: path.basename(f.path, path.extname(f.path)),
      agent: 'cursor',
      cwd: null,
      modified_at: getFileTimestamp(f.path),
      file_path: f.path,
      match_snippet,
    });
  }

  return entries;
}

module.exports = { resolve, read, list, search };
