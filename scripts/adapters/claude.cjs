/**
 * Claude agent adapter.
 */

const fs = require('fs');
const path = require('path');
const {
  normalizePath, collectMatchingFiles, readJsonlLines,
  findLatestByCwd, cwdMatchesProject, getFileTimestamp, extractClaudeText, redactSensitiveText, isSystemDirectory,
} = require('./utils.cjs');

const claudeProjectsBase = normalizePath(process.env.CHORUS_CLAUDE_PROJECTS_DIR || process.env.BRIDGE_CLAUDE_PROJECTS_DIR || '~/.claude/projects');

if (isSystemDirectory(claudeProjectsBase)) {
  throw new Error(`Refusing to scan system directory: ${claudeProjectsBase}`);
}

function getClaudeSessionCwd(filePath) {
  try {
    const lines = readJsonlLines(filePath);
    for (const line of lines) {
      try {
        const json = JSON.parse(line);
        if (typeof json.cwd === 'string') return normalizePath(json.cwd);
      } catch (error) { /* skip */ }
    }
  } catch (error) {
    return null;
  }
  return null;
}

function resolve(id, cwd, opts) {
  const warnings = [];
  if (!fs.existsSync(claudeProjectsBase)) return null;

  if (id) {
    const files = collectMatchingFiles(
      claudeProjectsBase,
      (fullPath, name) => name.endsWith('.jsonl') && fullPath.includes(id),
      true
    );
    return files.length > 0 ? { path: files[0].path, warnings } : null;
  }

  const files = collectMatchingFiles(claudeProjectsBase, (_fp, name) => name.endsWith('.jsonl'), true);
  if (files.length === 0) return null;

  const scoped = findLatestByCwd(files, getClaudeSessionCwd, cwd);
  if (scoped) return { path: scoped, warnings };

  warnings.push(`Warning: no Claude session matched cwd ${cwd}; falling back to latest session.`);
  return { path: files[0].path, warnings };
}

function read(filePath, lastN) {
  lastN = lastN || 1;
  const lines = readJsonlLines(filePath);
  const messages = [];
  let skipped = 0;
  let sessionCwd = null;

  for (const line of lines) {
    try {
      const json = JSON.parse(line);
      if (typeof json.cwd === 'string' && !sessionCwd) sessionCwd = json.cwd;
      const message = json.message || json;
      if (json.type === 'assistant' || message.role === 'assistant') {
        const content = message.content !== undefined ? message.content : json.content;
        const text = extractClaudeText(content);
        if (text) messages.push(text);
      }
    } catch (error) {
      skipped += 1;
    }
  }

  const warnings = [];
  if (skipped > 0) {
    warnings.push(`Warning: skipped ${skipped} unparseable line(s) in ${filePath}`);
  }

  const messageCount = messages.length;
  const sessionId = path.basename(filePath, path.extname(filePath));
  let content;
  let messagesReturned = 1;

  if (messages.length > 0) {
    if (lastN > 1) {
      const selected = messages.slice(-lastN);
      messagesReturned = selected.length;
      content = selected.join('\n---\n');
    } else {
      content = messages[messages.length - 1];
    }
  } else {
    content = `Could not extract assistant messages. Showing last 20 raw lines:\n${lines.slice(-20).join('\n')}`;
    messagesReturned = 0;
  }

  return {
    agent: 'claude',
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
  if (!fs.existsSync(claudeProjectsBase)) return [];
  const files = collectMatchingFiles(claudeProjectsBase, (_fp, name) => name.endsWith('.jsonl'), true);
  const expectedCwd = cwd ? normalizePath(cwd) : null;
  const entries = [];
  for (const f of files) {
    const fileCwd = getClaudeSessionCwd(f.path) || null;
    if (expectedCwd && !cwdMatchesProject(fileCwd, expectedCwd)) {
      continue;
    }

    entries.push({
      session_id: path.basename(f.path, path.extname(f.path)),
      agent: 'claude',
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
  if (!fs.existsSync(claudeProjectsBase)) return [];

  const files = collectMatchingFiles(claudeProjectsBase, (_fp, name) => name.endsWith('.jsonl'), true);
  const entries = [];

  for (const f of files) {
    if (entries.length >= limit) break;

    const fileCwd = getClaudeSessionCwd(f.path) || null;
    if (expectedCwd && !cwdMatchesProject(fileCwd, expectedCwd)) {
      continue;
    }

    // Read JSONL and extract only assistant text content
    let assistantText = '';
    try {
      const lines = readJsonlLines(f.path);
      for (const line of lines) {
        try {
          const obj = JSON.parse(line);
          if (obj.type === 'assistant' && obj.message && obj.message.content) {
            for (const block of obj.message.content) {
              if (block.type === 'text' && block.text) {
                assistantText += block.text + '\n';
              }
            }
          }
        } catch (_e) { /* skip malformed */ }
      }
    } catch (_e) {
      continue;
    }

    if (!assistantText.toLowerCase().includes(queryLower)) {
      continue;
    }

    // Extract match snippet
    const lowerText = assistantText.toLowerCase();
    const idx = lowerText.indexOf(queryLower);
    const snippetStart = Math.max(0, idx - 60);
    const snippetEnd = Math.min(assistantText.length, idx + queryLower.length + 60);
    const match_snippet = assistantText.slice(snippetStart, snippetEnd).replace(/\n/g, ' ');

    entries.push({
      session_id: path.basename(f.path, path.extname(f.path)),
      agent: 'claude',
      cwd: fileCwd,
      modified_at: getFileTimestamp(f.path),
      file_path: f.path,
      match_snippet,
    });
  }

  return entries;
}

module.exports = { resolve, read, list, search };
