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

function read(filePath, lastN) {
  lastN = lastN || 1;
  const raw = fs.readFileSync(filePath, 'utf-8');
  let content = '';
  let messageCount = 0;

  try {
    const json = JSON.parse(raw);
    if (Array.isArray(json.messages)) {
      const assistantMsgs = json.messages.filter(m => m.role === 'assistant');
      messageCount = assistantMsgs.length;
      content = assistantMsgs.length > 0
        ? (assistantMsgs[assistantMsgs.length - 1].content || '[No text content]')
        : '[No assistant messages found]';
    } else if (typeof json.content === 'string') {
      content = json.content;
      messageCount = 1;
    } else {
      content = JSON.stringify(json, null, 2);
    }
  } catch (error) {
    const lines = raw.split('\n').filter(Boolean);
    const msgs = [];
    for (const line of lines) {
      try {
        const json = JSON.parse(line);
        if (json.role === 'assistant' && typeof json.content === 'string') {
          msgs.push(json.content);
        }
      } catch (e) { /* skip */ }
    }
    messageCount = msgs.length;
    content = msgs.length > 0 ? msgs[msgs.length - 1] : lines.slice(-20).join('\n');
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
    messages_returned: 1,
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
