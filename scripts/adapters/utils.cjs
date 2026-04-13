/**
 * Shared utility functions for agent adapters.
 */

const fs = require('fs');
const path = require('path');
const os = require('os');
const crypto = require('crypto');

const MAX_FILE_SIZE = 50 * 1024 * 1024; // 50 MB
const MAX_SCAN_FILES = 1000;

function expandHome(filepath) {
  if (!filepath) return filepath;
  if (filepath === '~') return os.homedir();
  if (filepath.startsWith('~/')) {
    return path.join(os.homedir(), filepath.slice(2));
  }
  return filepath;
}

function normalizePath(filepath) {
  return path.resolve(expandHome(filepath));
}

function hashPath(filepath) {
  return crypto.createHash('sha256').update(normalizePath(filepath)).digest('hex');
}

function collectMatchingFiles(dirPath, predicate, recursive = false) {
  if (!dirPath || !fs.existsSync(dirPath)) return [];

  const matches = [];

  function search(currentDir) {
    if (matches.length >= MAX_SCAN_FILES) return;

    let entries = [];
    try {
      entries = fs.readdirSync(currentDir, { withFileTypes: true });
    } catch (error) {
      return;
    }

    for (const entry of entries) {
      if (matches.length >= MAX_SCAN_FILES) return;

      const fullPath = path.join(currentDir, entry.name);
      // Skip symlinked directories by default (Phase 6)
      if (entry.isDirectory()) {
        if (entry.isSymbolicLink()) continue;
        if (recursive) search(fullPath);
        continue;
      }

      if (!predicate(fullPath, entry.name)) continue;

      try {
        // Prefer nanosecond precision to keep "latest" selection stable
        // across runtimes and filesystems.
        let mtimeNs;
        try {
          const statBig = fs.statSync(fullPath, { bigint: true });
          mtimeNs = statBig.mtimeNs;
        } catch (_error) {
          const stat = fs.statSync(fullPath);
          mtimeNs = BigInt(Math.trunc(stat.mtimeMs * 1e6));
        }
        matches.push({ path: fullPath, mtimeNs });
      } catch (error) {
        // Ignore entries that disappear while scanning.
      }
    }
  }

  search(dirPath);
  matches.sort((a, b) => {
    if (b.mtimeNs !== a.mtimeNs) {
      return b.mtimeNs > a.mtimeNs ? 1 : -1;
    }
    return String(a.path).localeCompare(String(b.path));
  });
  return matches;
}

function readJsonlLines(filePath) {
  const stat = fs.statSync(filePath);
  if (stat.size > MAX_FILE_SIZE) {
    throw new Error(`Skipped ${filePath} (exceeds ${MAX_FILE_SIZE / (1024 * 1024)}MB size limit)`);
  }
  const lines = fs.readFileSync(filePath, 'utf-8').split('\n').filter(Boolean);
  // Concurrent-read safety: if another process is actively writing to this
  // JSONL file, the last line may be truncated mid-JSON.  Drop it if it
  // doesn't look like a complete JSON value.
  if (lines.length > 0) {
    const last = lines[lines.length - 1].trimEnd();
    if (last.length > 0 && !/[}\]"0-9]$/.test(last)) {
      lines.pop();
    }
  }
  return lines;
}

function cwdMatchesProject(sessionCwd, expectedCwd) {
  if (!sessionCwd || !expectedCwd) return false;
  const a = normalizePath(sessionCwd);
  const b = normalizePath(expectedCwd);
  // Exact match OR session cwd is ancestor of expected OR expected is ancestor of session cwd
  return a === b || b.startsWith(a + '/') || a.startsWith(b + '/');
}

function findLatestByCwd(files, cwdExtractor, expectedCwd) {
  for (const file of files) {
    const fileCwd = cwdExtractor(file.path);
    if (fileCwd && cwdMatchesProject(fileCwd, expectedCwd)) {
      return file.path;
    }
  }
  return null;
}

function getFileTimestamp(filePath) {
  try {
    const stat = fs.statSync(filePath);
    return stat.mtime.toISOString();
  } catch (error) {
    return null;
  }
}

function extractText(value) {
  if (typeof value === 'string') return value;
  if (!Array.isArray(value)) return '';

  return value
    .map(part => {
      if (typeof part === 'string') return part;
      if (part && typeof part.text === 'string') return part.text;
      return '';
    })
    .join('');
}

function extractClaudeText(value) {
  if (typeof value === 'string') return value;
  if (!Array.isArray(value)) return '';

  return value
    .filter(part => part && part.type === 'text')
    .map(part => part.text || '')
    .join('');
}

function extractClaudeContentWithToolCalls(value) {
  if (typeof value === 'string') return value;
  if (!Array.isArray(value)) return '';

  return value
    .map(part => {
      if (!part) return '';
      if (part.type === 'text') return part.text || '';
      if (part.type === 'tool_use') {
        const name = part.name || 'unknown';
        let inputStr = '';
        try {
          inputStr = JSON.stringify(part.input, null, 2);
        } catch (_) {
          inputStr = String(part.input || '');
        }
        return `[TOOL: ${name}]\n${inputStr}\n[/TOOL]`;
      }
      if (part.type === 'tool_result') {
        const toolId = part.tool_use_id || '';
        const content = typeof part.content === 'string'
          ? part.content
          : (Array.isArray(part.content)
            ? part.content.map(c => c.text || '').join('')
            : '');
        return `[TOOL_RESULT: ${toolId}]\n${content}\n[/TOOL_RESULT]`;
      }
      return '';
    })
    .filter(Boolean)
    .join('\n');
}

function extractContentWithToolCalls(value) {
  if (typeof value === 'string') return value;
  if (!Array.isArray(value)) return '';

  return value
    .map(part => {
      if (typeof part === 'string') return part;
      if (part && typeof part.text === 'string') return part.text;
      if (part && part.type === 'function_call') {
        const name = part.name || 'unknown';
        let argStr = '';
        try {
          argStr = typeof part.arguments === 'string'
            ? part.arguments
            : JSON.stringify(part.arguments, null, 2);
        } catch (_) {
          argStr = String(part.arguments || '');
        }
        return `[TOOL: ${name}]\n${argStr}\n[/TOOL]`;
      }
      if (part && part.type === 'tool_use') {
        const name = part.name || 'unknown';
        let inputStr = '';
        try {
          inputStr = JSON.stringify(part.input, null, 2);
        } catch (_) {
          inputStr = String(part.input || '');
        }
        return `[TOOL: ${name}]\n${inputStr}\n[/TOOL]`;
      }
      return '';
    })
    .filter(Boolean)
    .join('\n');
}

function extractToolCallSummary(value) {
  if (!Array.isArray(value)) return {};
  const counts = {};
  for (const part of value) {
    if (!part) continue;
    if (part.type === 'tool_use' && part.name) {
      counts[part.name] = (counts[part.name] || 0) + 1;
    }
    if (part.type === 'function_call' && part.name) {
      counts[part.name] = (counts[part.name] || 0) + 1;
    }
  }
  return counts;
}

function extractFilePaths(value) {
  if (!Array.isArray(value)) return [];
  const paths = new Set();
  for (const part of value) {
    if (!part) continue;
    if ((part.type === 'tool_use' || part.type === 'function_call') && part.input) {
      const input = typeof part.input === 'string' ? (() => { try { return JSON.parse(part.input); } catch (_) { return {}; } })() : part.input;
      if (input.file_path) paths.add(input.file_path);
      if (input.path) paths.add(input.path);
    }
  }
  return [...paths];
}

function redactSensitiveText(input) {
  let output = String(input || '');
  // OpenAI keys (sk-proj-, sk-ant-, sk-...) with hyphens allowed
  output = output.replace(/\bsk-[A-Za-z0-9_-]{20,}/g, 'sk-[REDACTED]');
  // AWS access keys
  output = output.replace(/\bAKIA[0-9A-Z]{16}\b/g, 'AKIA[REDACTED]');
  // GitHub tokens
  output = output.replace(/\b(ghp_|gho_|ghs_|ghr_)[A-Za-z0-9_]{20,}/g, '$1[REDACTED]');
  output = output.replace(/\bgithub_pat_[A-Za-z0-9_]{20,}/g, 'github_pat_[REDACTED]');
  // Google API keys
  output = output.replace(/\bAIza[A-Za-z0-9_-]{20,}/g, 'AIza[REDACTED]');
  // Slack tokens
  output = output.replace(/\b(xoxb-|xoxp-|xoxs-)[A-Za-z0-9-]{10,}/g, '$1[REDACTED]');
  // Bearer tokens
  output = output.replace(/\bBearer\s+[A-Za-z0-9._-]{10,}/gi, 'Bearer [REDACTED]');
  // JWT-like tokens (three base64url segments)
  output = output.replace(/\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}/g, '[REDACTED_JWT]');
  // PEM private keys
  output = output.replace(/-----BEGIN\s+(RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----[\s\S]*?-----END\s+(RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----/g, '[REDACTED_PEM_KEY]');
  // Connection strings — redact userinfo portion
  output = output.replace(/((?:postgres|mysql|mongodb|redis|amqp):\/\/)[^\s"']+/gi, '$1[REDACTED]');
  // Secret assignments (api_key, api-key, apikey, token, secret, password)
  output = output.replace(
    /\b(api[_-]?key|token|secret|password)\b\s*[:=]\s*["']?[^"'\s]+["']?/gi,
    (_, key) => `${key}=[REDACTED]`
  );
  return output;
}

/**
 * Redact sensitive text with an audit trail of what was redacted.
 * @param {string} input
 * @returns {{ text: string, redactions: Array<{pattern: string, count: number}> }}
 */
function redactSensitiveTextWithAudit(input) {
  let output = String(input || '');
  const redactions = [];

  function countAndReplace(regex, replacement, patternLabel) {
    let count = 0;
    output = output.replace(regex, (...args) => {
      count += 1;
      if (typeof replacement === 'function') return replacement(...args);
      return replacement;
    });
    if (count > 0) redactions.push({ pattern: patternLabel, count });
  }

  countAndReplace(/\bsk-[A-Za-z0-9_-]{20,}/g, 'sk-[REDACTED]', 'openai_key');
  countAndReplace(/\bAKIA[0-9A-Z]{16}\b/g, 'AKIA[REDACTED]', 'aws_access_key');
  countAndReplace(/\b(ghp_|gho_|ghs_|ghr_)[A-Za-z0-9_]{20,}/g, '$1[REDACTED]', 'github_token');
  countAndReplace(/\bgithub_pat_[A-Za-z0-9_]{20,}/g, 'github_pat_[REDACTED]', 'github_pat');
  countAndReplace(/\bAIza[A-Za-z0-9_-]{20,}/g, 'AIza[REDACTED]', 'google_api_key');
  countAndReplace(/\b(xoxb-|xoxp-|xoxs-)[A-Za-z0-9-]{10,}/g, '$1[REDACTED]', 'slack_token');
  countAndReplace(/\bBearer\s+[A-Za-z0-9._-]{10,}/gi, 'Bearer [REDACTED]', 'bearer_token');
  countAndReplace(/\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}/g, '[REDACTED_JWT]', 'jwt_token');
  countAndReplace(/-----BEGIN\s+(RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----[\s\S]*?-----END\s+(RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----/g, '[REDACTED_PEM_KEY]', 'pem_key');
  countAndReplace(/((?:postgres|mysql|mongodb|redis|amqp):\/\/)[^\s"']+/gi, '$1[REDACTED]', 'connection_string');
  countAndReplace(
    /\b(api[_-]?key|token|secret|password)\b\s*[:=]\s*["']?[^"'\s]+["']?/gi,
    (_, key) => `${key}=[REDACTED]`,
    'secret_assignment'
  );

  return { text: output, redactions };
}

const SYSTEM_DIRS = new Set(['/etc', '/usr', '/var', '/bin', '/sbin', '/System', '/Library',
  '/Windows', '/Windows/System32', '/Program Files', '/Program Files (x86)']);

function isSystemDirectory(dirPath) {
  const resolved = path.resolve(dirPath);
  // macOS temp dirs live under /var/folders or /private/var/folders — allow those
  if (resolved.startsWith('/var/folders/') || resolved.startsWith('/private/var/folders/')) return false;
  for (const sysDir of SYSTEM_DIRS) {
    if (resolved === sysDir || resolved.startsWith(sysDir + path.sep)) return true;
  }
  return false;
}

module.exports = {
  MAX_FILE_SIZE,
  MAX_SCAN_FILES,
  expandHome,
  normalizePath,
  hashPath,
  collectMatchingFiles,
  readJsonlLines,
  findLatestByCwd,
  cwdMatchesProject,
  getFileTimestamp,
  extractText,
  extractClaudeText,
  extractClaudeContentWithToolCalls,
  extractContentWithToolCalls,
  extractToolCallSummary,
  extractFilePaths,
  redactSensitiveText,
  redactSensitiveTextWithAudit,
  isSystemDirectory,
};
