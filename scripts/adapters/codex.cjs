/**
 * Codex agent adapter.
 */

const fs = require('fs');
const path = require('path');
const {
  normalizePath, collectMatchingFiles, readJsonlLines,
  findLatestByCwd, cwdMatchesProject, getFileTimestamp, extractText, redactSensitiveText, isSystemDirectory,
} = require('./utils.cjs');

const codexSessionsBase = normalizePath(process.env.CHORUS_CODEX_SESSIONS_DIR || process.env.BRIDGE_CODEX_SESSIONS_DIR || '~/.codex/sessions');

if (isSystemDirectory(codexSessionsBase)) {
  throw new Error(`Refusing to scan system directory: ${codexSessionsBase}`);
}

function getCodexSessionCwd(filePath) {
  try {
    const firstLine = readJsonlLines(filePath)[0];
    if (!firstLine) return null;
    const json = JSON.parse(firstLine);
    if (json.type === 'session_meta' && json.payload && typeof json.payload.cwd === 'string') {
      return normalizePath(json.payload.cwd);
    }
  } catch (error) {
    return null;
  }
  return null;
}

function resolve(id, cwd, opts) {
  const warnings = [];
  if (!fs.existsSync(codexSessionsBase)) return null;

  if (id) {
    const files = collectMatchingFiles(
      codexSessionsBase,
      (fullPath, name) => name.endsWith('.jsonl') && fullPath.includes(id),
      true
    );
    return files.length > 0 ? { path: files[0].path, warnings } : null;
  }

  const files = collectMatchingFiles(codexSessionsBase, (_fp, name) => name.endsWith('.jsonl'), true);
  if (files.length === 0) return null;

  const scoped = findLatestByCwd(files, getCodexSessionCwd, cwd);
  if (scoped) return { path: scoped, warnings };

  warnings.push(`Warning: no Codex session matched cwd ${cwd}; falling back to latest session.`);
  return { path: files[0].path, warnings };
}

function read(filePath, lastN) {
  lastN = lastN || 1;
  const lines = readJsonlLines(filePath);
  const messages = [];
  let skipped = 0;
  let sessionCwd = null;
  let sessionId = null;

  for (const line of lines) {
    try {
      const json = JSON.parse(line);
      if (json.type === 'session_meta' && json.payload) {
        if (typeof json.payload.cwd === 'string') sessionCwd = json.payload.cwd;
        if (typeof json.payload.session_id === 'string') sessionId = json.payload.session_id;
      }
      if (json.type === 'response_item' && json.payload && json.payload.type === 'message') {
        messages.push(json.payload);
      } else if (json.type === 'event_msg' && json.payload && json.payload.type === 'agent_message') {
        messages.push({ role: 'assistant', content: json.payload.message });
      }
    } catch (error) {
      skipped += 1;
    }
  }

  const warnings = [];
  if (skipped > 0) {
    warnings.push(`Warning: skipped ${skipped} unparseable line(s) in ${filePath}`);
  }

  const assistantMsgs = messages.filter(m => (m.role || '').toLowerCase() === 'assistant');
  const messageCount = assistantMsgs.length;
  if (!sessionId) sessionId = path.basename(filePath, path.extname(filePath));

  let content = '';
  let messagesReturned = 1;
  if (messages.length > 0) {
    if (lastN > 1 && assistantMsgs.length > 0) {
      const selected = assistantMsgs.slice(-lastN);
      messagesReturned = selected.length;
      content = selected.map(m => extractText(m.content) || '[No text content]').join('\n---\n');
    } else {
      const selected = assistantMsgs.length > 0 ? assistantMsgs[assistantMsgs.length - 1] : messages[messages.length - 1];
      content = extractText(selected.content) || '[No text content]';
    }
  } else {
    content = `Could not extract structured messages. Showing last 20 raw lines:\n${lines.slice(-20).join('\n')}`;
    messagesReturned = 0;
  }

  return {
    agent: 'codex',
    source: filePath,
    content: redactSensitiveText(content),
    warnings,
    session_id: sessionId,
    cwd: sessionCwd,
    timestamp: getFileTimestamp(filePath),
    message_count: messageCount,
    messages_returned: messagesReturned,
  };
}

function list(cwd, limit) {
  limit = limit || 10;
  if (!fs.existsSync(codexSessionsBase)) return [];
  const files = collectMatchingFiles(codexSessionsBase, (_fp, name) => name.endsWith('.jsonl'), true);
  const expectedCwd = cwd ? normalizePath(cwd) : null;
  const entries = [];
  for (const f of files) {
    const fileCwd = getCodexSessionCwd(f.path) || null;
    if (expectedCwd && !cwdMatchesProject(fileCwd, expectedCwd)) {
      continue;
    }

    entries.push({
      session_id: path.basename(f.path, path.extname(f.path)),
      agent: 'codex',
      cwd: fileCwd,
      modified_at: getFileTimestamp(f.path),
      file_path: f.path,
    });

    if (entries.length >= limit) break;
  }
  return entries;
}

function search(query, cwd, limit) {
  limit = limit || 10;
  const expectedCwd = cwd ? normalizePath(cwd) : null;
  const queryLower = String(query || '').toLowerCase();
  if (!fs.existsSync(codexSessionsBase)) return [];

  const files = collectMatchingFiles(codexSessionsBase, (_fp, name) => name.endsWith('.jsonl'), true);
  const entries = [];

  for (const f of files) {
    if (entries.length >= limit) break;

    const fileCwd = getCodexSessionCwd(f.path) || null;
    if (expectedCwd && !cwdMatchesProject(fileCwd, expectedCwd)) {
      continue;
    }

    let assistantText = '';
    try {
      const lines = readJsonlLines(f.path);
      for (const line of lines) {
        try {
          const obj = JSON.parse(line);
          if (obj.role === 'assistant' && obj.content) {
            assistantText += (typeof obj.content === 'string' ? obj.content : JSON.stringify(obj.content)) + '\n';
          }
        } catch (_e) { /* skip */ }
      }
    } catch (_e) {
      continue;
    }

    if (!assistantText.toLowerCase().includes(queryLower)) {
      continue;
    }

    const lowerText = assistantText.toLowerCase();
    const idx = lowerText.indexOf(queryLower);
    const snippetStart = Math.max(0, idx - 60);
    const snippetEnd = Math.min(assistantText.length, idx + queryLower.length + 60);
    const match_snippet = assistantText.slice(snippetStart, snippetEnd).replace(/\n/g, ' ');

    entries.push({
      session_id: path.basename(f.path, path.extname(f.path)),
      agent: 'codex',
      cwd: fileCwd,
      modified_at: getFileTimestamp(f.path),
      file_path: f.path,
      match_snippet,
    });
  }

  return entries;
}

module.exports = { resolve, read, list, search };
