/**
 * Gemini agent adapter.
 */

const fs = require('fs');
const path = require('path');
const {
  normalizePath, hashPath, collectMatchingFiles,
  getFileTimestamp, extractText, redactSensitiveText, isSystemDirectory,
} = require('./utils.cjs');

const geminiTmpBase = normalizePath(process.env.CHORUS_GEMINI_TMP_DIR || process.env.BRIDGE_GEMINI_TMP_DIR || '~/.gemini/tmp');

function compareByMtimeDesc(a, b) {
  if (b.mtimeNs !== a.mtimeNs) {
    return b.mtimeNs > a.mtimeNs ? 1 : -1;
  }
  return String(a.path).localeCompare(String(b.path));
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

function listGeminiChatDirs() {
  if (!fs.existsSync(geminiTmpBase)) return [];
  let entries = [];
  try {
    entries = fs.readdirSync(geminiTmpBase, { withFileTypes: true });
  } catch (error) {
    return [];
  }

  const dirs = [];
  for (const entry of entries) {
    if (!entry.isDirectory()) continue;
    const chatsDir = path.join(geminiTmpBase, entry.name, 'chats');
    if (fs.existsSync(chatsDir)) dirs.push(chatsDir);
  }
  return dirs;
}

function resolveGeminiChatDirs(chatsDir, cwd) {
  if (chatsDir) {
    const expanded = normalizePath(chatsDir);
    if (isSystemDirectory(expanded)) {
      throw new Error(`Refusing to scan system directory: ${expanded}`);
    }
    return fs.existsSync(expanded) ? [expanded] : [];
  }
  const ordered = [];
  const seen = new Set();
  function addDir(dirPath) {
    if (!dirPath || seen.has(dirPath) || !fs.existsSync(dirPath)) return;
    ordered.push(dirPath);
    seen.add(dirPath);
  }
  const scopedHash = hashPath(cwd);
  addDir(path.join(geminiTmpBase, scopedHash, 'chats'));
  for (const dir of listGeminiChatDirs()) addDir(dir);
  return ordered;
}

function resolve(id, cwd, opts) {
  const chatsDir = opts && opts.chatsDir ? opts.chatsDir : null;
  const dirs = resolveGeminiChatDirs(chatsDir, cwd);
  if (dirs.length === 0) return null;

  const warnings = [];
  if (dirs.length > 1 && !chatsDir) {
    warnings.push('Warning: Gemini sessions from multiple projects may be mixed. Use --chats-dir to scope to a specific project.');
  }
  const candidates = [];
  for (const dir of dirs) {
    const files = collectMatchingFiles(dir, (fullPath, name) => {
      // read() now dispatches on extension: .json -> single-document parser,
      // .jsonl -> line-delimited parser. Accept both so newer Gemini CLI
      // sessions (.jsonl) are readable.
      if (!(name.endsWith('.json') || name.endsWith('.jsonl'))) return false;
      if (id) return fullPath.includes(id);
      return name.startsWith('session-');
    }, false);
    for (const file of files) candidates.push(file);
  }
  candidates.sort(compareByMtimeDesc);
  return candidates.length > 0 ? { path: candidates[0].path, warnings, searchedDirs: dirs } : null;
}

// Parse line-delimited JSON Gemini sessions.
//
// Shape:
//   - First line is a header: {sessionId, projectHash, kind, ...}
//   - Message lines: {id, timestamp, type: 'user'|'gemini', content: <string|array>}
//   - Metadata lines: {"$set": {...}} — skip
//
// Gemini's streaming producer can emit the same assistant message twice
// (mid-stream + final) with identical `id`. Dedupe on id while preserving
// first-seen order.
function readJsonl(filePath, lastN, opts = {}) {
  lastN = lastN || 1;
  const raw = fs.readFileSync(filePath, 'utf-8');
  const lines = raw.split('\n').filter(l => l.trim().length > 0);

  let sessionId = null;
  const turns = [];
  const seenIds = new Set();
  let skipped = 0;

  for (const line of lines) {
    let obj;
    try {
      obj = JSON.parse(line);
    } catch (_e) {
      skipped += 1;
      continue;
    }

    // Header line — grab sessionId and move on.
    if (sessionId == null && typeof obj.sessionId === 'string') {
      sessionId = obj.sessionId;
      continue;
    }

    // Metadata events carry only `$set` — skip.
    if (obj.$set !== undefined && obj.type === undefined) continue;

    const type = (obj.type || obj.role || '').toLowerCase();
    const role = (type === 'gemini' || type === 'assistant' || type === 'model')
      ? 'assistant'
      : (type === 'user' ? 'user' : null);
    if (!role) continue;

    if (typeof obj.id === 'string') {
      if (seenIds.has(obj.id)) continue;
      seenIds.add(obj.id);
    }

    let text;
    if (typeof obj.content === 'string') {
      text = obj.content;
    } else {
      const fromContent = extractText(obj.content);
      if (fromContent) {
        text = fromContent;
      } else {
        const fromParts = extractText(obj.parts);
        text = fromParts || '[No text content]';
      }
    }
    turns.push({ role, text });
  }

  if (!sessionId) {
    sessionId = path.basename(filePath, path.extname(filePath));
  }

  const assistantMsgs = turns.filter(t => t.role === 'assistant').map(t => t.text);
  const messageCount = assistantMsgs.length;

  const warnings = [];
  if (skipped > 0) {
    warnings.push(`Warning: skipped ${skipped} unparseable line(s) in ${filePath}`);
  }

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
  } else {
    if (assistantMsgs.length === 0) throw new Error('Gemini session has no assistant messages.');
    content = assistantMsgs[assistantMsgs.length - 1];
  }

  return {
    agent: 'gemini',
    source: filePath,
    content: redactSensitiveText(content),
    warnings,
    session_id: sessionId,
    cwd: null,
    timestamp: getFileTimestamp(filePath),
    message_count: messageCount,
    messages_returned: messagesReturned,
    included_roles: rolesIncluded,
  };
}

function read(filePath, lastN, opts = {}) {
  lastN = lastN || 1;
  // Dispatch on extension. Newer Gemini CLI writes line-delimited .jsonl;
  // older ones write a single JSON document. Both funnel into the same
  // Session shape via shared output below.
  if (filePath.endsWith('.jsonl')) {
    return readJsonl(filePath, lastN, opts);
  }
  let session;
  try {
    session = JSON.parse(fs.readFileSync(filePath, 'utf-8'));
  } catch (error) {
    throw new Error(`Failed to parse Gemini JSON: ${error.message}`);
  }

  const sessionId = session.sessionId || path.basename(filePath, path.extname(filePath));
  let content = '';
  let messageCount = 0;
  let messagesReturned = 1;
  let rolesIncluded = ['assistant'];
  const turns = [];

  if (Array.isArray(session.messages)) {
    for (const msg of session.messages) {
      const type = (msg.type || msg.role || '').toLowerCase();
      const role = (type === 'gemini' || type === 'assistant' || type === 'model')
        ? 'assistant'
        : (type === 'user' ? 'user' : null);
      if (!role) continue;
      const text = typeof msg.content === 'string'
        ? msg.content
        : (extractText(msg.content || msg.parts) || '[No text content]');
      if (text) turns.push({ role, text });
    }
    const assistantMsgs = turns.filter(t => t.role === 'assistant').map(t => t.text);
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
      if (assistantMsgs.length === 0) throw new Error('Gemini session has no assistant messages.');
      content = assistantMsgs[assistantMsgs.length - 1];
    }
  } else if (Array.isArray(session.history)) {
    for (const turn of session.history) {
      const role = (turn.role || '').toLowerCase() === 'user' ? 'user' : 'assistant';
      let text = '';
      if (Array.isArray(turn.parts)) text = turn.parts.map(p => p.text || '').join('\n');
      else if (typeof turn.parts === 'string') text = turn.parts;
      else text = '[No text content]';
      if (text) turns.push({ role, text });
    }
    const assistantTurns = turns.filter(t => t.role === 'assistant').map(t => t.text);
    messageCount = assistantTurns.length;

    if (opts.includeUser && assistantTurns.length > 0) {
      const selected = selectConversationTurns(turns, lastN);
      messagesReturned = selected.length;
      rolesIncluded = ['user', 'assistant'];
      content = selected.map(m => `${m.role.toUpperCase()}:\n${m.text}`).join('\n---\n');
    } else if (lastN > 1 && assistantTurns.length > 0) {
      const selected = assistantTurns.slice(-lastN);
      messagesReturned = selected.length;
      content = selected.join('\n---\n');
    } else {
      if (assistantTurns.length === 0) throw new Error('Gemini history is empty.');
      content = assistantTurns[assistantTurns.length - 1];
    }
  } else {
    throw new Error('Unknown Gemini session schema. Supported fields: messages, history.');
  }

  return {
    agent: 'gemini',
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

// Best-effort inference of cwd from the Gemini session's scope segment.
// Layout: .../tmp/<scope>/chats/session-*.json[l]
// If <scope> is a named directory (e.g. "play"), return it as the cwd hint.
// If <scope> is a hex hash (>=40 hex chars — SHA-256 of an absolute path),
// we can't reverse it without a scope map; still return the scope dir as
// the cwd bucket and surface the hash via a separate scope_hash field.
function inferGeminiScope(sessionPath) {
  // parent() -> <scope>/chats ; parent() again -> <scope>
  const chatsDir = path.dirname(sessionPath);
  const scopeDir = path.dirname(chatsDir);
  const scopeName = path.basename(scopeDir);
  if (!scopeName || scopeName === '.' || scopeName === path.sep) {
    return { cwd: null, scopeHash: null };
  }
  const isHexHash = scopeName.length >= 40 && /^[0-9a-f]+$/.test(scopeName);
  if (isHexHash) {
    return { cwd: scopeName, scopeHash: scopeName };
  }
  return { cwd: scopeName, scopeHash: null };
}

function list(cwd, limit) {
  limit = limit || 10;
  const dirs = cwd
    ? (() => {
      const scoped = path.join(geminiTmpBase, hashPath(cwd), 'chats');
      return fs.existsSync(scoped) ? [scoped] : [];
    })()
    : listGeminiChatDirs();
  const candidates = [];
  for (const dir of dirs) {
    const files = collectMatchingFiles(
      dir,
      (fp, name) => (name.endsWith('.json') || name.endsWith('.jsonl')) && name.startsWith('session-'),
      false,
    );
    for (const f of files) candidates.push(f);
  }
  candidates.sort(compareByMtimeDesc);
  return candidates.slice(0, limit).map(f => {
    const scope = inferGeminiScope(f.path);
    const entry = {
      session_id: path.basename(f.path, path.extname(f.path)),
      agent: 'gemini',
      cwd: scope.cwd,
      modified_at: getFileTimestamp(f.path),
      file_path: f.path,
    };
    if (scope.scopeHash) entry.scope_hash = scope.scopeHash;
    return entry;
  });
}

function search(query, cwd, limit) {
  limit = limit || 10;
  const queryLower = String(query || '').toLowerCase();
  const dirs = cwd
    ? (() => {
      const scoped = path.join(geminiTmpBase, hashPath(cwd), 'chats');
      return fs.existsSync(scoped) ? [scoped] : [];
    })()
    : listGeminiChatDirs();
  const candidates = [];
  for (const dir of dirs) {
    const files = collectMatchingFiles(
      dir,
      (fp, name) => (name.endsWith('.json') || name.endsWith('.jsonl')) && name.startsWith('session-'),
      false,
    );
    for (const f of files) candidates.push(f);
  }
  candidates.sort(compareByMtimeDesc);

  const entries = [];
  for (const f of candidates) {
    if (entries.length >= limit) break;

    let content;
    try {
      content = fs.readFileSync(f.path, 'utf-8');
    } catch (error) {
      continue;
    }

    let assistantText = '';
    try {
      const session = JSON.parse(content);
      if (Array.isArray(session.messages)) {
        // Gemini CLI uses { type: 'gemini', content: '...' }
        for (const msg of session.messages) {
          const type = (msg.type || msg.role || '').toLowerCase();
          if (type === 'gemini' || type === 'model' || type === 'assistant') {
            if (typeof msg.content === 'string') {
              assistantText += msg.content + '\n';
            } else if (Array.isArray(msg.parts)) {
              for (const part of msg.parts) {
                if (part.text) assistantText += part.text + '\n';
              }
            }
          }
        }
      }
      // Also handle the history-based format (Gemini API style)
      if (!assistantText && Array.isArray(session.history)) {
        for (const turn of session.history) {
          if ((turn.role || '').toLowerCase() !== 'user' && Array.isArray(turn.parts)) {
            for (const part of turn.parts) {
              if (part.text) assistantText += part.text + '\n';
            }
          }
        }
      }
    } catch (_e) {
      // Fallback to raw content if parsing fails
      assistantText = content;
    }

    if (!assistantText.toLowerCase().includes(queryLower)) {
      continue;
    }

    const lowerText = assistantText.toLowerCase();
    const idx = lowerText.indexOf(queryLower);
    const snippetStart = Math.max(0, idx - 60);
    const snippetEnd = Math.min(assistantText.length, idx + queryLower.length + 60);
    const match_snippet = assistantText.slice(snippetStart, snippetEnd).replace(/\n/g, ' ');

    const scope = inferGeminiScope(f.path);
    const entry = {
      session_id: path.basename(f.path, path.extname(f.path)),
      agent: 'gemini',
      cwd: scope.cwd,
      modified_at: getFileTimestamp(f.path),
      file_path: f.path,
      match_snippet,
    };
    if (scope.scopeHash) entry.scope_hash = scope.scopeHash;
    entries.push(entry);
  }

  return entries;
}

module.exports = { resolve, read, list, search };
