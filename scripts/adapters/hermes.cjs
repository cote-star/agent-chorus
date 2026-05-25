/**
 * Hermes agent adapter.
 *
 * Hermes does not currently expose a stable JSONL transcript format in this
 * environment, so this adapter reads Hermes's own session-log markdown files
 * from ~/.hermes/sessions and the project-local wiki/session-logs mirrors.
 * It exists primarily so `chorus send --from hermes ...` and
 * `chorus messages --agent hermes ...` work with the same validation path as
 * the sibling agents.
 */

const fs = require('fs');
const path = require('path');
const {
  normalizePath, collectMatchingFiles, cwdMatchesProject, getFileTimestamp,
  redactSensitiveText, isSystemDirectory,
} = require('./utils.cjs');

const hermesSessionsBase = normalizePath(process.env.CHORUS_HERMES_SESSIONS_DIR || process.env.BRIDGE_HERMES_SESSIONS_DIR || '~/.hermes/sessions');

if (isSystemDirectory(hermesSessionsBase)) {
  throw new Error(`Refusing to scan system directory: ${hermesSessionsBase}`);
}

function candidateBases(cwd) {
  const bases = [];
  if (fs.existsSync(hermesSessionsBase)) bases.push(hermesSessionsBase);
  if (cwd) {
    const normalizedCwd = normalizePath(cwd);
    // Check project-local session-log mirror (e.g. session-logs/ at repo root)
    const localMirror = path.join(normalizedCwd, 'session-logs');
    if (fs.existsSync(localMirror)) bases.push(localMirror);
  }
  return [...new Set(bases)];
}

function collectSessionFiles(cwd) {
  const files = [];
  for (const base of candidateBases(cwd)) {
    files.push(...collectMatchingFiles(base, (_fp, name) => name.endsWith('.md') || name.endsWith('.jsonl'), true));
  }
  files.sort((a, b) => {
    if (b.mtimeNs !== a.mtimeNs) return b.mtimeNs > a.mtimeNs ? 1 : -1;
    return String(a.path).localeCompare(String(b.path));
  });
  return files;
}

function getHermesSessionCwd(filePath) {
  const normalized = normalizePath(filePath);
  if (normalized.includes('/.hermes/sessions/')) return null;
  const marker = '/session-logs/';
  const idx = normalized.indexOf(marker);
  if (idx >= 0) return normalized.slice(0, idx);
  return null;
}

function resolve(id, cwd, opts) {
  const warnings = [];
  const files = collectSessionFiles(cwd);
  if (files.length === 0) return null;
  if (id) {
    const match = files.find(f => f.path.includes(id));
    return match ? { path: match.path, warnings } : null;
  }
  const expectedCwd = cwd ? normalizePath(cwd) : null;
  if (expectedCwd) {
    const scoped = files.find(f => {
      const fileCwd = getHermesSessionCwd(f.path);
      return fileCwd && cwdMatchesProject(fileCwd, expectedCwd);
    });
    if (scoped) return { path: scoped.path, warnings };
    warnings.push(`Warning: no Hermes session log matched cwd ${expectedCwd}; falling back to latest Hermes session/log.`);
  }
  return { path: files[0].path, warnings };
}

function read(filePath, lastN) {
  const raw = fs.readFileSync(filePath, 'utf8');
  const lines = raw.split('\n');
  const content = lines.slice(-(lastN || 1) * 80).join('\n');
  return {
    agent: 'hermes',
    source: filePath,
    content: redactSensitiveText(content),
    warnings: [],
    session_id: path.basename(filePath, path.extname(filePath)),
    cwd: getHermesSessionCwd(filePath),
    timestamp: getFileTimestamp(filePath),
    message_count: 1,
    messages_returned: 1,
  };
}

function list(cwd, limit) {
  limit = limit || 10;
  const expectedCwd = cwd ? normalizePath(cwd) : null;
  const entries = [];
  for (const f of collectSessionFiles(cwd)) {
    const fileCwd = getHermesSessionCwd(f.path);
    if (expectedCwd && fileCwd && !cwdMatchesProject(fileCwd, expectedCwd)) continue;
    entries.push({
      session_id: path.basename(f.path, path.extname(f.path)),
      agent: 'hermes',
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
  const queryLower = String(query || '').toLowerCase();
  const entries = [];
  for (const f of collectSessionFiles(cwd)) {
    if (entries.length >= limit) break;
    let text = '';
    try { text = fs.readFileSync(f.path, 'utf8'); } catch (_e) { continue; }
    const lower = text.toLowerCase();
    const idx = lower.indexOf(queryLower);
    if (idx < 0) continue;
    const start = Math.max(0, idx - 80);
    const end = Math.min(text.length, idx + queryLower.length + 80);
    entries.push({
      session_id: path.basename(f.path, path.extname(f.path)),
      agent: 'hermes',
      cwd: getHermesSessionCwd(f.path),
      modified_at: getFileTimestamp(f.path),
      file_path: f.path,
      match_snippet: redactSensitiveText(text.slice(start, end).replace(/\n/g, ' ')),
    });
  }
  return entries;
}

module.exports = { resolve, read, list, search };
