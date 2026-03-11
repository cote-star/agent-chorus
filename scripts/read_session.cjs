#!/usr/bin/env node

const fs = require('fs');
const path = require('path');
const os = require('os');
const crypto = require('crypto');
const { execFileSync } = require('child_process');
const { getAdapter } = require('./adapters/registry.cjs');

const rawArgs = process.argv.slice(2);
const commandNames = new Set(['read', 'compare', 'report', 'list', 'search', 'setup', 'doctor', 'trash-talk', 'context-pack']);
const command = commandNames.has(rawArgs[0]) ? rawArgs[0] : 'read';
const args = commandNames.has(rawArgs[0]) ? rawArgs.slice(1) : rawArgs;

function getPackageVersion() {
  try {
    const rootPackagePath = path.join(__dirname, '..', 'package.json');
    return JSON.parse(fs.readFileSync(rootPackagePath, 'utf-8')).version || 'unknown';
  } catch (_error) {
    return 'unknown';
  }
}

function printHelp(topic = null) {
  const binName = path.basename(process.argv[1] || 'bridge');
  const lines = [
    `Agent Bridge CLI v${getPackageVersion()}`,
    '',
    'Usage:',
    `  ${binName} <command> [options]`,
    '',
    'Commands:',
    '  read      Read assistant messages from a session (default command)',
    '  list      List recent sessions for an agent',
    '  search    Search sessions by query text',
    '  compare   Compare outputs across agents',
    '  report    Generate a coordinator report from a handoff JSON',
    '  setup     Install cross-provider instruction scaffolding in this project',
    '  doctor    Check session paths and provider instruction wiring',
    '  context-pack  Build/sync/install context-pack automation',
    '',
    'Global Flags:',
    '  -h, --help       Show help',
    '  -v, --version    Show version',
    '',
    'Examples:',
    `  ${binName} read --agent codex --json`,
    `  ${binName} list --agent claude --limit 5 --json`,
    `  ${binName} search \"authentication\" --agent gemini --json`,
    `  ${binName} compare --source codex --source claude --json`,
    `  ${binName} report --handoff ./handoff.json --json`,
    `  ${binName} setup`,
    `  ${binName} doctor --json`,
    `  ${binName} context-pack build`,
  ];

  if (topic === 'read') {
    lines.push('');
    lines.push('read options:');
    lines.push('  --agent <codex|gemini|claude|cursor> (default: codex)');
    lines.push('  --id <session-substring> (optional; omitted = latest session in scope)');
    lines.push('  --cwd <path>');
    lines.push('  --chats-dir <path> (gemini)');
    lines.push('  --last <N>');
    lines.push('  --json');
  } else if (topic === 'list') {
    lines.push('');
    lines.push('list options:');
    lines.push('  --agent <codex|gemini|claude|cursor>');
    lines.push('  --cwd <path>');
    lines.push('  --limit <N> (default: 10)');
    lines.push('  --json');
  } else if (topic === 'search') {
    lines.push('');
    lines.push('search options:');
    lines.push('  <query> (positional, required)');
    lines.push('  --agent <codex|gemini|claude|cursor> (required)');
    lines.push('  --cwd <path>');
    lines.push('  --limit <N> (default: 10)');
    lines.push('  --json');
  } else if (topic === 'compare') {
    lines.push('');
    lines.push('compare options:');
    lines.push('  --source <agent[:session-substring]> (repeatable, required)');
    lines.push('  --cwd <path>');
    lines.push('  --normalize');
    lines.push('  --json');
  } else if (topic === 'report') {
    lines.push('');
    lines.push('report options:');
    lines.push('  --handoff <path-to-handoff.json> (required)');
    lines.push('  --cwd <path>');
    lines.push('  --json');
  } else if (topic === 'setup') {
    lines.push('');
    lines.push('setup options:');
    lines.push('  --cwd <path> (default: current directory)');
    lines.push('  --dry-run');
    lines.push('  --force (replace existing managed blocks)');
    lines.push('  --context-pack (also build context pack and install hooks)');
    lines.push('  --json');
  } else if (topic === 'doctor') {
    lines.push('');
    lines.push('doctor options:');
    lines.push('  --cwd <path> (default: current directory)');
    lines.push('  --json');
  } else if (topic === 'context-pack') {
    lines.push('');
    lines.push('context-pack usage:');
    lines.push('  context-pack build [--reason <text>] [--base <sha>] [--head <sha>] [--force-snapshot]');
    lines.push('  context-pack init [--pack-dir <path>] [--cwd <path>] [--force]');
    lines.push('  context-pack seal [--reason <text>] [--base <sha>] [--head <sha>] [--pack-dir <path>] [--cwd <path>] [--force] [--force-snapshot]');
    lines.push('  context-pack sync-main --local-ref <ref> --local-sha <sha> --remote-ref <ref> --remote-sha <sha>');
    lines.push('  context-pack install-hooks');
    lines.push('  context-pack rollback [--snapshot <id>]');
    lines.push('  context-pack check-freshness [--base <git-ref>]');
  }

  console.log(lines.join('\n'));
}

function resolveHelpTopic(inputArgs) {
  if (commandNames.has(inputArgs[0])) return inputArgs[0];
  if (inputArgs[0] === 'help' && commandNames.has(inputArgs[1])) return inputArgs[1];
  return null;
}

const wantsHelp =
  rawArgs[0] === 'help' ||
  rawArgs.includes('--help') ||
  rawArgs.includes('-h');
if (wantsHelp) {
  printHelp(resolveHelpTopic(rawArgs));
  process.exit(0);
}

if (rawArgs.includes('--version') || rawArgs.includes('-v')) {
  console.log(getPackageVersion());
  process.exit(0);
}

const codexSessionsBase = normalizePath(process.env.BRIDGE_CODEX_SESSIONS_DIR || '~/.codex/sessions');
const claudeProjectsBase = normalizePath(process.env.BRIDGE_CLAUDE_PROJECTS_DIR || '~/.claude/projects');
const geminiTmpBase = normalizePath(process.env.BRIDGE_GEMINI_TMP_DIR || '~/.gemini/tmp');
const setupProviders = [
  { agent: 'codex', targetFile: 'AGENTS.md' },
  { agent: 'claude', targetFile: 'CLAUDE.md' },
  { agent: 'gemini', targetFile: 'GEMINI.md' },
];

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

function getOptionValues(inputArgs, name) {
  const values = [];
  for (let i = 0; i < inputArgs.length; i += 1) {
    const arg = inputArgs[i];
    if (arg === name && i + 1 < inputArgs.length) {
      values.push(inputArgs[i + 1]);
      i += 1;
      continue;
    }

    const prefix = `${name}=`;
    if (arg.startsWith(prefix)) {
      values.push(arg.slice(prefix.length));
    }
  }
  return values;
}

function getOptionValue(inputArgs, name, fallback = null) {
  const values = getOptionValues(inputArgs, name);
  return values.length > 0 ? values[values.length - 1] : fallback;
}

function hasFlag(inputArgs, name) {
  return inputArgs.includes(name);
}

function runInternalNodeScript(scriptRelPath, scriptArgs, options = {}) {
  const scriptPath = path.join(__dirname, scriptRelPath);
  if (!fs.existsSync(scriptPath)) {
    throw new Error(`Missing internal script: ${scriptRelPath}`);
  }

  const cwd = options.cwd || process.cwd();
  const inheritOutput = options.inheritOutput === true;
  if (inheritOutput) {
    execFileSync(process.execPath, [scriptPath, ...scriptArgs], { cwd, stdio: 'inherit' });
    return { stdout: '', stderr: '' };
  }

  try {
    const stdout = execFileSync(process.execPath, [scriptPath, ...scriptArgs], {
      cwd,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    return { stdout: (stdout || '').trim(), stderr: '' };
  } catch (error) {
    const stdout = (error.stdout || '').toString().trim();
    const stderr = (error.stderr || '').toString().trim();
    const details = [stderr, stdout].filter(Boolean).join('\n');
    throw new Error(details || error.message || `Failed running ${scriptRelPath}`);
  }
}

function runContextPackSubcommand(subcommand, subArgs, options = {}) {
  const scriptBySubcommand = {
    build: 'context_pack/build.cjs',
    init: 'context_pack/init.cjs',
    seal: 'context_pack/seal.cjs',
    'sync-main': 'context_pack/sync_main.cjs',
    rollback: 'context_pack/rollback.cjs',
    'install-hooks': 'context_pack/install_hooks.cjs',
    'check-freshness': 'context_pack/check_freshness.cjs',
  };

  const scriptRelPath = scriptBySubcommand[subcommand];
  if (!scriptRelPath) {
    const allowed = Object.keys(scriptBySubcommand).join(', ');
    throw new Error(`Unknown context-pack subcommand: ${subcommand}. Expected one of: ${allowed}`);
  }

  return runInternalNodeScript(scriptRelPath, subArgs, options);
}

function runContextPack(inputArgs) {
  const subcommand = inputArgs[0];
  if (!subcommand || subcommand === 'help' || subcommand === '--help' || subcommand === '-h') {
    printHelp('context-pack');
    return;
  }
  runContextPackSubcommand(subcommand, inputArgs.slice(1), { inheritOutput: true });
}

function writeFileEnsured(filePath, content) {
  // Check for symlinks in the target path
  try {
    const lstat = fs.lstatSync(filePath);
    if (lstat.isSymbolicLink()) {
      throw new Error(`Refusing to write: target is a symlink: ${filePath}`);
    }
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
  }
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, content, 'utf-8');
}

function makeManagedBlock(provider, snippetRelPath) {
  const marker = `agent-bridge:${provider.agent}`;
  return [
    `<!-- ${marker}:start -->`,
    '## Agent Bridge Integration',
    '',
    `This project is wired for cross-agent coordination via \`bridge\`.`,
    `Provider snippet: \`${snippetRelPath}\``,
    '',
    'When a user asks for another agent status (for example "What is Claude doing?"),',
    'run Agent Bridge commands first and answer with evidence from session output.',
    '',
    'Session routing and defaults:',
    '1. Start with `bridge read --agent <target-agent> --cwd <project-path> --json` (omit `--id` for latest).',
    '2. "past session" means previous session: list 2 and read the second session ID.',
    '3. "past N sessions" means exclude latest: list N+1 and read the older N session IDs.',
    '4. "last N sessions" means include latest: list N and read/summarize those sessions.',
    '5. Ask for a session ID only after an initial read/list attempt fails or when exact ID is requested.',
    '',
    'Support commands:',
    '- `bridge list --agent <agent> --cwd <project-path> --json`',
    '- `bridge search "<query>" --agent <agent> --cwd <project-path> --json`',
    '- `bridge compare --source codex --source gemini --source claude --cwd <project-path> --json`',
    '',
    'If command syntax is unclear, run `bridge --help`.',
    `<!-- ${marker}:end -->`,
  ].join('\n');
}

function upsertManagedBlock(filePath, block, markerPrefix, force, dryRun) {
  const startMarker = `<!-- ${markerPrefix}:start -->`;
  const endMarker = `<!-- ${markerPrefix}:end -->`;

  let existing = '';
  if (fs.existsSync(filePath)) {
    existing = fs.readFileSync(filePath, 'utf-8');
  }

  const startIdx = existing.indexOf(startMarker);
  const endIdx = existing.indexOf(endMarker);

  // Check for duplicate markers
  if (startIdx !== -1) {
    const secondStart = existing.indexOf(startMarker, startIdx + startMarker.length);
    const secondEnd = endIdx !== -1 ? existing.indexOf(endMarker, endIdx + endMarker.length) : -1;
    if (secondStart !== -1 || secondEnd !== -1) {
      if (!force) {
        return { status: 'unchanged', message: 'Duplicate managed block markers detected (use --force to replace all)' };
      }
      // With --force: remove ALL occurrences of managed blocks and re-insert once
      let cleaned = existing;
      let safety = 0;
      while (safety < 10) {
        const s = cleaned.indexOf(startMarker);
        const e = cleaned.indexOf(endMarker);
        if (s === -1 || e === -1 || e < s) break;
        const before = cleaned.slice(0, s).replace(/\s*$/, '');
        const after = cleaned.slice(e + endMarker.length).replace(/^\s*/, '');
        cleaned = `${before}\n\n${after}`.replace(/\n{3,}/g, '\n\n');
        safety += 1;
      }
      const trimmed = cleaned.replace(/\s*$/, '');
      const next = trimmed ? `${trimmed}\n\n${block}\n` : `${block}\n`;
      if (!dryRun) {
        writeFileEnsured(filePath, next);
      }
      return { status: 'updated', message: 'Replaced duplicate managed blocks' };
    }
  }

  let next;
  let status;

  if (startIdx !== -1 && endIdx !== -1 && endIdx > startIdx) {
    if (!force) {
      return { status: 'unchanged', message: 'Managed block already present (use --force to refresh)' };
    }
    const before = existing.slice(0, startIdx).replace(/\s*$/, '');
    const after = existing.slice(endIdx + endMarker.length).replace(/^\s*/, '');
    next = `${before}\n\n${block}\n${after ? `\n${after}` : ''}`.replace(/\n{3,}/g, '\n\n');
    status = 'updated';
  } else if (!existing.trim()) {
    next = `${block}\n`;
    status = 'created';
  } else {
    const trimmed = existing.replace(/\s*$/, '');
    next = `${trimmed}\n\n${block}\n`;
    status = 'updated';
  }

  if (!dryRun) {
    writeFileEnsured(filePath, next);
  }

  return { status, message: status === 'created' ? 'Created file with managed block' : 'Managed block written' };
}

function defaultSetupIntents() {
  return [
    '# Agent Bridge Intents',
    '',
    'Use these triggers consistently across agents and providers:',
    '',
    '- "What is Claude doing?"',
    '- "What did Gemini say?"',
    '- "Compare Codex and Claude outputs"',
    '- "Read session <id> from Codex"',
    '',
    'Canonical response behavior:',
    '1. Default to latest session in current project (`--cwd`) when no session is specified.',
    '2. "past session" means previous session; "past N sessions" excludes latest; "last N sessions" includes latest.',
    '3. Fetch evidence with `bridge read` first, then `bridge list/search` only if needed.',
    '4. For multi-source checks use `bridge compare` or `bridge report`.',
    '5. Do not ask for session ID before first fetch unless user requested exact ID.',
    '6. Do not invent missing context; explicitly call out missing sessions.',
    '',
    'Core protocol reference: https://github.com/cote-star/agent-bridge/blob/main/PROTOCOL.md.',
  ].join('\n');
}

const { MAX_FILE_SIZE, MAX_SCAN_FILES } = require('./adapters/utils.cjs');

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
      if (entry.isDirectory()) {
        if (entry.isSymbolicLink()) continue;
        if (recursive) search(fullPath);
        continue;
      }

      if (!predicate(fullPath, entry.name)) continue;

      try {
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
  return fs.readFileSync(filePath, 'utf-8').split('\n').filter(Boolean);
}

function findLatestByCwd(files, cwdExtractor, expectedCwd) {
  for (const file of files) {
    const fileCwd = cwdExtractor(file.path);
    if (fileCwd && fileCwd === expectedCwd) {
      return file.path;
    }
  }
  return null;
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

function getClaudeSessionCwd(filePath) {
  try {
    const lines = readJsonlLines(filePath);
    for (const line of lines) {
      try {
        const json = JSON.parse(line);
        if (typeof json.cwd === 'string') {
          return normalizePath(json.cwd);
        }
      } catch (error) {
        // Ignore unparseable line.
      }
    }
  } catch (error) {
    return null;
  }
  return null;
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
    if (fs.existsSync(chatsDir)) {
      dirs.push(chatsDir);
    }
  }
  return dirs;
}

const SYSTEM_DIRS = new Set(['/etc', '/usr', '/var', '/bin', '/sbin', '/System', '/Library',
  '/Windows', '/Windows/System32', '/Program Files', '/Program Files (x86)']);

function isSystemDirectory(dirPath) {
  const resolved = path.resolve(dirPath);
  for (const sysDir of SYSTEM_DIRS) {
    if (resolved === sysDir || resolved.startsWith(sysDir + path.sep)) return true;
  }
  return false;
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

  for (const dir of listGeminiChatDirs()) {
    addDir(dir);
  }

  return ordered;
}

function resolveCodexTargetFile(id, cwd, warnings) {
  if (!fs.existsSync(codexSessionsBase)) return null;

  if (id) {
    const files = collectMatchingFiles(
      codexSessionsBase,
      (fullPath, name) => name.endsWith('.jsonl') && fullPath.includes(id),
      true
    );
    return files.length > 0 ? files[0].path : null;
  }

  const files = collectMatchingFiles(codexSessionsBase, (fullPath, name) => name.endsWith('.jsonl'), true);
  if (files.length === 0) return null;

  const scoped = findLatestByCwd(files, getCodexSessionCwd, cwd);
  if (scoped) return scoped;

  warnings.push(`Warning: no Codex session matched cwd ${cwd}; falling back to latest session.`);
  return files[0].path;
}

function resolveClaudeTargetFile(id, cwd, warnings) {
  if (!fs.existsSync(claudeProjectsBase)) return null;

  if (id) {
    const files = collectMatchingFiles(
      claudeProjectsBase,
      (fullPath, name) => name.endsWith('.jsonl') && fullPath.includes(id),
      true
    );
    return files.length > 0 ? files[0].path : null;
  }

  const files = collectMatchingFiles(claudeProjectsBase, (fullPath, name) => name.endsWith('.jsonl'), true);
  if (files.length === 0) return null;

  const scoped = findLatestByCwd(files, getClaudeSessionCwd, cwd);
  if (scoped) return scoped;

  warnings.push(`Warning: no Claude session matched cwd ${cwd}; falling back to latest session.`);
  return files[0].path;
}

function resolveGeminiTargetFile(id, chatsDir, cwd) {
  const dirs = resolveGeminiChatDirs(chatsDir, cwd);
  if (dirs.length === 0) return { targetFile: null, searchedDirs: [] };

  const candidates = [];
  for (const dir of dirs) {
    const files = collectMatchingFiles(
      dir,
      (fullPath, name) => {
        if (!name.endsWith('.json')) return false;
        if (id) return fullPath.includes(id);
        return name.startsWith('session-');
      },
      false
    );

    for (const file of files) {
      candidates.push(file);
    }
  }

  candidates.sort((a, b) => {
    if (b.mtimeNs !== a.mtimeNs) {
      return b.mtimeNs > a.mtimeNs ? 1 : -1;
    }
    return String(a.path).localeCompare(String(b.path));
  });
  return {
    targetFile: candidates.length > 0 ? candidates[0].path : null,
    searchedDirs: dirs,
  };
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

function redactSensitiveText(input) {
  // Delegate to the shared implementation in utils.cjs
  return require('./adapters/utils.cjs').redactSensitiveText(input);
}

function classifyError(message) {
  if (/unsupported agent/i.test(message) || /unknown agent/i.test(message)) return 'UNSUPPORTED_AGENT';
  if (/unsupported mode/i.test(message)) return 'UNSUPPORTED_MODE';
  if (/no .* session found/i.test(message)) return 'NOT_FOUND';
  if (/not found/i.test(message)) return 'NOT_FOUND';
  if (/failed to parse/i.test(message) || /failed to read/i.test(message)) return 'PARSE_FAILED';
  if (/missing required/i.test(message) || /invalid handoff/i.test(message) || /must provide session_id/i.test(message)) return 'INVALID_HANDOFF';
  if (/has no messages/i.test(message) || /history is empty/i.test(message)) return 'EMPTY_SESSION';
  return 'IO_ERROR';
}

function getFileTimestamp(filePath) {
  try {
    const stat = fs.statSync(filePath);
    return stat.mtime.toISOString();
  } catch (error) {
    return null;
  }
}

function readCodexSession(id, cwd, lastN) {
  lastN = lastN || 1;
  const warnings = [];
  const targetFile = resolveCodexTargetFile(id, cwd, warnings);
  if (!targetFile) {
    throw new Error('No Codex session found.');
  }

  const lines = readJsonlLines(targetFile);
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

  if (skipped > 0) {
    warnings.push(`Warning: skipped ${skipped} unparseable line(s) in ${targetFile}`);
  }

  const assistantMsgs = messages.filter(message => (message.role || '').toLowerCase() === 'assistant');
  const messageCount = assistantMsgs.length;

  if (!sessionId) {
    sessionId = path.basename(targetFile, path.extname(targetFile));
  }

  let content = '';
  if (messages.length > 0) {
    if (lastN > 1 && assistantMsgs.length > 0) {
      const selected = assistantMsgs.slice(-lastN);
      content = selected.map(m => extractText(m.content) || '[No text content]').join('\n---\n');
    } else {
      const selected = assistantMsgs.length > 0 ? assistantMsgs[assistantMsgs.length - 1] : messages[messages.length - 1];
      content = extractText(selected.content) || '[No text content]';
    }
  } else {
    content = `Could not extract structured messages. Showing last 20 raw lines:\n${lines.slice(-20).join('\n')}`;
  }

  const messagesReturned = lastN > 1 ? Math.min(lastN, assistantMsgs.length) : 1;

  return {
    agent: 'codex',
    source: targetFile,
    content: redactSensitiveText(content),
    warnings,
    session_id: sessionId,
    cwd: sessionCwd,
    timestamp: getFileTimestamp(targetFile),
    message_count: messageCount,
    messages_returned: messagesReturned,
  };
}

function readGeminiSession(id, chatsDir, cwd, lastN) {
  lastN = lastN || 1;
  const resolved = resolveGeminiTargetFile(id, chatsDir, cwd);
  const targetFile = resolved.targetFile;
  if (!targetFile) {
    if (chatsDir) {
      throw new Error(`No Gemini session found in ${normalizePath(chatsDir)}`);
    }

    const lines = ['No Gemini session found. Searched chats directories:'];
    for (const dir of resolved.searchedDirs) {
      lines.push(` - ${dir}`);
    }
    throw new Error(lines.join('\n'));
  }

  let session;
  try {
    session = JSON.parse(fs.readFileSync(targetFile, 'utf-8'));
  } catch (error) {
    throw new Error(`Failed to parse Gemini JSON: ${error.message}`);
  }

  const sessionId = session.sessionId || path.basename(targetFile, path.extname(targetFile));

  let content = '';
  let messageCount = 0;
  let messagesReturned = 1;
  if (Array.isArray(session.messages)) {
    const assistantMsgs = session.messages.filter(message => {
      const type = (message.type || '').toLowerCase();
      return type === 'gemini' || type === 'assistant' || type === 'model';
    });
    messageCount = assistantMsgs.length;

    if (lastN > 1 && assistantMsgs.length > 0) {
      const selected = assistantMsgs.slice(-lastN);
      messagesReturned = selected.length;
      content = selected.map(m => {
        return typeof m.content === 'string' ? m.content : extractText(m.content) || '[No text content]';
      }).join('\n---\n');
    } else {
      const selected =
        [...session.messages].reverse().find(message => {
          const type = (message.type || '').toLowerCase();
          return type === 'gemini' || type === 'assistant' || type === 'model';
        }) || session.messages[session.messages.length - 1];

      if (!selected) {
        throw new Error('Gemini session has no messages.');
      }

      content = typeof selected.content === 'string'
        ? selected.content
        : extractText(selected.content) || '[No text content]';
    }
  } else if (Array.isArray(session.history)) {
    const assistantTurns = session.history.filter(turn => (turn.role || '').toLowerCase() !== 'user');
    messageCount = assistantTurns.length;

    if (lastN > 1 && assistantTurns.length > 0) {
      const selected = assistantTurns.slice(-lastN);
      messagesReturned = selected.length;
      content = selected.map(turn => {
        if (Array.isArray(turn.parts)) {
          return turn.parts.map(part => part.text || '').join('\n');
        } else if (typeof turn.parts === 'string') {
          return turn.parts;
        }
        return '[No text content]';
      }).join('\n---\n');
    } else {
      const selected =
        [...session.history].reverse().find(turn => (turn.role || '').toLowerCase() !== 'user') ||
        session.history[session.history.length - 1];

      if (!selected) {
        throw new Error('Gemini history is empty.');
      }

      if (Array.isArray(selected.parts)) {
        content = selected.parts.map(part => part.text || '').join('\n');
      } else if (typeof selected.parts === 'string') {
        content = selected.parts;
      } else {
        content = '[No text content]';
      }
    }
  } else {
    throw new Error('Unknown Gemini session schema. Supported fields: messages, history.');
  }

  return {
    agent: 'gemini',
    source: targetFile,
    content: redactSensitiveText(content),
    warnings: [],
    session_id: sessionId,
    cwd: null,
    timestamp: getFileTimestamp(targetFile),
    message_count: messageCount,
    messages_returned: messagesReturned,
  };
}

function readClaudeSession(id, cwd, lastN) {
  lastN = lastN || 1;
  if (!fs.existsSync(claudeProjectsBase)) {
    throw new Error(`Claude projects directory not found: ${claudeProjectsBase}`);
  }

  const warnings = [];
  const targetFile = resolveClaudeTargetFile(id, cwd, warnings);
  if (!targetFile) {
    throw new Error('No Claude session found.');
  }

  const lines = readJsonlLines(targetFile);
  const messages = [];
  let skipped = 0;
  let sessionCwd = null;

  for (const line of lines) {
    try {
      const json = JSON.parse(line);
      if (typeof json.cwd === 'string' && !sessionCwd) {
        sessionCwd = json.cwd;
      }
      const message = json.message || json;
      if (json.type === 'assistant' || message.role === 'assistant') {
        const content = message.content !== undefined ? message.content : json.content;
        const text = extractClaudeText(content);
        if (text) {
          messages.push(text);
        }
      }
    } catch (error) {
      skipped += 1;
    }
  }

  if (skipped > 0) {
    warnings.push(`Warning: skipped ${skipped} unparseable line(s) in ${targetFile}`);
  }

  const messageCount = messages.length;
  const sessionId = path.basename(targetFile, path.extname(targetFile));
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
    source: targetFile,
    content: redactSensitiveText(content),
    warnings,
    session_id: sessionId,
    cwd: sessionCwd,
    timestamp: getFileTimestamp(targetFile),
    message_count: messageCount,
    messages_returned: messagesReturned,
  };
}

const cursorDataBase = normalizePath(process.env.BRIDGE_CURSOR_DATA_DIR || (
  process.platform === 'darwin'
    ? '~/Library/Application Support/Cursor'
    : '~/.cursor'
));

function readCursorSession(id, cwd, lastN) {
  lastN = lastN || 1;
  if (!fs.existsSync(cursorDataBase)) {
    throw new Error(`No Cursor session found. Data directory not found: ${cursorDataBase}`);
  }

  const workspacesDir = path.join(cursorDataBase, 'User', 'workspaceStorage');
  if (!fs.existsSync(workspacesDir)) {
    throw new Error(`No Cursor session found. Workspace storage not found: ${workspacesDir}`);
  }

  const files = collectMatchingFiles(workspacesDir, (fullPath, name) => {
    const isMatch = (name.endsWith('.json') || name.endsWith('.jsonl'))
      && (name.includes('chat') || name.includes('composer') || name.includes('conversation'));
    if (!isMatch) return false;
    if (id) return fullPath.includes(id);
    return true;
  }, true);

  if (files.length === 0) {
    throw new Error('No Cursor session found.');
  }

  const targetFile = files[0].path;
  const raw = fs.readFileSync(targetFile, 'utf-8');
  let content = '';
  let messageCount = 0;

  try {
    const json = JSON.parse(raw);
    if (Array.isArray(json.messages)) {
      const assistantMsgs = json.messages.filter(m => m.role === 'assistant');
      messageCount = assistantMsgs.length;
      if (assistantMsgs.length > 0) {
        content = assistantMsgs[assistantMsgs.length - 1].content || '[No text content]';
      } else {
        content = '[No assistant messages found]';
      }
    } else if (typeof json.content === 'string') {
      content = json.content;
      messageCount = 1;
    } else {
      content = JSON.stringify(json, null, 2);
    }
  } catch (error) {
    // JSONL format
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

  const sessionId = path.basename(targetFile, path.extname(targetFile));

  return {
    agent: 'cursor',
    source: targetFile,
    content: redactSensitiveText(content),
    warnings: [],
    session_id: sessionId,
    cwd: null,
    timestamp: getFileTimestamp(targetFile),
    message_count: messageCount,
    messages_returned: 1,
  };
}

function listSessions(agent, cwd, limit) {
  const adapter = getAdapter(agent);
  return adapter.list(cwd || null, limit || 10);
}

function searchSessions(query, agent, cwd, limit) {
  const adapter = getAdapter(agent);
  if (typeof adapter.search !== 'function') {
    throw new Error(`Search is not implemented for agent: ${agent}`);
  }
  return adapter.search(query, cwd || null, limit || 10);
}

function readSessionViaAdapter(agent, { id, cwd, chatsDir, lastN }) {
  const adapter = getAdapter(agent);
  const resolved = adapter.resolve(id || null, cwd, { chatsDir: chatsDir || null });

  if (!resolved || !resolved.path) {
    if (agent === 'gemini' && chatsDir) {
      throw new Error(`No Gemini session found in ${normalizePath(chatsDir)}`);
    }
    throw new Error(`No ${agent.charAt(0).toUpperCase() + agent.slice(1)} session found.`);
  }

  const result = adapter.read(resolved.path, lastN || 1);
  const adapterWarnings = Array.isArray(resolved.warnings) ? resolved.warnings : [];
  result.warnings = [...adapterWarnings, ...(result.warnings || [])];
  return result;
}

function runList(inputArgs) {
  const agent = getOptionValue(inputArgs, '--agent', 'codex');
  const rawCwd = getOptionValue(inputArgs, '--cwd', null);
  const cwd = rawCwd ? normalizePath(rawCwd) : null;
  const limit = parseInt(getOptionValue(inputArgs, '--limit', '10'), 10) || 10;
  const asJson = hasFlag(inputArgs, '--json');

  const entries = listSessions(agent, cwd, limit);

  if (asJson) {
    console.log(JSON.stringify(entries, null, 2));
  } else {
    for (const entry of entries) {
      console.log(JSON.stringify(entry));
    }
  }
}

function readSource(sourceSpec, defaultCwd) {
  const effectiveCwd = normalizePath(sourceSpec.cwd || defaultCwd);
  return readSessionViaAdapter(sourceSpec.agent, {
    id: sourceSpec.session_id || null,
    cwd: effectiveCwd,
    chatsDir: sourceSpec.chats_dir || null,
    lastN: 1,
  });
}

function parseSourceArg(raw) {
  const firstColon = raw.indexOf(':');
  const agent = (firstColon === -1 ? raw : raw.slice(0, firstColon)).trim().toLowerCase();
  const session = firstColon === -1 ? null : raw.slice(firstColon + 1).trim();

  if (!['codex', 'gemini', 'claude', 'cursor'].includes(agent)) {
    throw new Error(`Unsupported agent: ${agent}`);
  }

  return {
    agent,
    session_id: session ? session : null,
    current_session: !session,
    cwd: null,
    chats_dir: null,
  };
}

function evidenceTag(sourceSpec) {
  const id = sourceSpec.session_id ? sourceSpec.session_id.slice(0, 8) : 'latest';
  return `[${sourceSpec.agent}:${id}]`;
}

function computeVerdict(mode, missingCount, uniqueCount, successCount) {
  if (successCount === 0) return 'INCOMPLETE';

  if (mode === 'verify') {
    if (missingCount === 0 && uniqueCount <= 1) return 'PASS';
    return 'FAIL';
  }

  if (mode === 'steer') return 'STEERING_PLAN_READY';
  if (mode === 'analyze') return 'ANALYSIS_COMPLETE';
  if (mode === 'feedback') return 'FEEDBACK_COMPLETE';
  return 'INCOMPLETE';
}

function buildReport(request, defaultCwd) {
  const successful = [];
  const missing = [];

  for (const sourceSpec of request.sources) {
    const evidence = evidenceTag(sourceSpec);
    try {
      const session = readSource(sourceSpec, defaultCwd);
      successful.push({ sourceSpec, session, evidence });
    } catch (error) {
      missing.push({ sourceSpec, error: error.message || String(error), evidence });
    }
  }

  const findings = [];

  for (const item of missing) {
    findings.push({
      severity: 'P1',
      summary: `Source unavailable: ${item.sourceSpec.agent} (${item.error})`,
      evidence: [item.evidence],
      confidence: 0.9,
    });
  }

  for (const item of successful) {
    for (const warning of item.session.warnings || []) {
      findings.push({
        severity: 'P2',
        summary: `Source warning: ${warning}`,
        evidence: [item.evidence],
        confidence: 0.75,
      });
    }
  }

  const shouldNormalize = request.normalize === true;
  const uniqueContents = new Set(successful.map(item => {
    const text = (item.session.content || '').trim();
    return shouldNormalize ? normalizeContent(text) : text;
  }));

  if (successful.length >= 2) {
    if (uniqueContents.size > 1) {
      findings.push({
        severity: 'P1',
        summary: 'Divergent agent outputs detected',
        evidence: successful.map(item => item.evidence),
        confidence: 0.75,
      });
    } else {
      findings.push({
        severity: 'P3',
        summary: 'All available agent outputs are aligned',
        evidence: successful.map(item => item.evidence),
        confidence: 0.9,
      });
    }
  } else {
    findings.push({
      severity: 'P2',
      summary: 'Insufficient comparable sources',
      evidence: successful.map(item => item.evidence),
      confidence: 0.5,
    });
  }

  const recommendedNextActions = [];
  if (missing.length > 0) {
    recommendedNextActions.push('Provide valid session identifiers or cwd values for unavailable sources.');
  }
  if (uniqueContents.size > 1) {
    recommendedNextActions.push('Inspect full transcripts for diverging sources before final decisions.');
  }
  if (Array.isArray(request.constraints) && request.constraints.length > 0) {
    recommendedNextActions.push(`Verify recommendations against constraints: ${request.constraints.join('; ')}.`);
  }
  if (recommendedNextActions.length === 0) {
    recommendedNextActions.push('No immediate action required.');
  }

  const openQuestions = missing.map(item => `Missing source ${item.sourceSpec.agent}: ${item.error}`);

  return {
    mode: request.mode,
    task: request.task,
    success_criteria: request.success_criteria,
    sources_used: successful.map(item => `${item.evidence} ${item.session.source}`),
    verdict: computeVerdict(request.mode, missing.length, uniqueContents.size, successful.length),
    findings: findings,
    recommended_next_actions: recommendedNextActions,
    open_questions: openQuestions,
  };
}

function sanitizeForTerminal(text) {
  // Strip C0 control characters (0x00-0x1F) except \n (0x0A) and \t (0x09)
  // Strip ESC (0x1B) sequences including ANSI CSI (ESC[...) and OSC (ESC]...)
  return String(text || '')
    .replace(/\x1B\[[0-9;]*[A-Za-z]/g, '') // CSI sequences
    .replace(/\x1B\][^\x07]*(?:\x07|\x1B\\)/g, '') // OSC sequences
    .replace(/\x1B[^[\]]/g, '') // Other ESC sequences
    .replace(/[\x00-\x08\x0B\x0C\x0E-\x1F]/g, ''); // C0 control chars except \t \n \r
}

function renderReadResult(result, asJson) {
  if (asJson) {
    console.log(JSON.stringify(result, null, 2));
    return;
  }

  for (const warning of result.warnings || []) {
    console.error(sanitizeForTerminal(warning));
  }

  const label = result.agent.charAt(0).toUpperCase() + result.agent.slice(1);
  console.log(sanitizeForTerminal(`SOURCE: ${label} Session (${result.source})`));
  console.log('---');
  console.log(sanitizeForTerminal(result.content));
}

function renderReport(result, asJson) {
  if (asJson) {
    console.log(JSON.stringify(result, null, 2));
    return;
  }

  const lines = [];
  lines.push('### Agent Bridge Coordinator Report');
  lines.push('');
  lines.push(`**Mode:** ${result.mode}`);
  lines.push(`**Task:** ${result.task}`);
  lines.push('**Success Criteria:**');
  for (const criterion of result.success_criteria || []) {
    lines.push(`- ${criterion}`);
  }
  lines.push('');
  lines.push('**Sources Used:**');
  for (const source of result.sources_used || []) {
    lines.push(`- ${source}`);
  }
  lines.push('');
  lines.push(`**Verdict:** ${result.verdict}`);
  lines.push('');
  lines.push('**Findings:**');
  for (const finding of result.findings || []) {
    lines.push(
      `- **${finding.severity}:** ${finding.summary} (evidence: ${(finding.evidence || []).join(', ')}; confidence: ${Number(finding.confidence || 0).toFixed(2)})`
    );
  }
  lines.push('');
  lines.push('**Recommended Next Actions:**');
  (result.recommended_next_actions || []).forEach((action, index) => {
    lines.push(`${index + 1}. ${action}`);
  });
  if ((result.open_questions || []).length > 0) {
    lines.push('');
    lines.push('**Open Questions:**');
    for (const question of result.open_questions) {
      lines.push(`- ${question}`);
    }
  }

  console.log(sanitizeForTerminal(lines.join('\n')));
}

function validateMode(mode) {
  const allowed = new Set(['verify', 'steer', 'analyze', 'feedback']);
  if (!allowed.has(mode)) {
    throw new Error(`Unsupported mode: ${mode}`);
  }
}

function runRead(inputArgs) {
  const agent = getOptionValue(inputArgs, '--agent', 'codex');
  const id = getOptionValue(inputArgs, '--id', null);
  const chatsDir = getOptionValue(inputArgs, '--chats-dir', null);
  const cwd = normalizePath(getOptionValue(inputArgs, '--cwd', process.cwd()));
  const asJson = hasFlag(inputArgs, '--json');
  const lastN = parseInt(getOptionValue(inputArgs, '--last', '1'), 10) || 1;

  const result = readSessionViaAdapter(agent, {
    id,
    cwd,
    chatsDir,
    lastN,
  });

  renderReadResult(result, asJson);
}

function runSearch(inputArgs) {
  const query = inputArgs[0];
  if (!query || query.startsWith('--')) {
    throw new Error('search requires a query string as the first argument');
  }

  const agent = getOptionValue(inputArgs, '--agent', null);
  if (!agent) {
    throw new Error('search requires --agent=<codex|gemini|claude|cursor>');
  }

  const rawCwd = getOptionValue(inputArgs, '--cwd', null);
  const cwd = rawCwd ? normalizePath(rawCwd) : null;
  const limit = parseInt(getOptionValue(inputArgs, '--limit', '10'), 10) || 10;
  const asJson = hasFlag(inputArgs, '--json');

  const entries = searchSessions(query, agent, cwd, limit);
  if (asJson) {
    console.log(JSON.stringify(entries, null, 2));
  } else {
    for (const entry of entries) {
      console.log(JSON.stringify(entry));
    }
  }
}

function runSetup(inputArgs) {
  const cwd = normalizePath(getOptionValue(inputArgs, '--cwd', process.cwd()));
  const asJson = hasFlag(inputArgs, '--json');
  const dryRun = hasFlag(inputArgs, '--dry-run');
  const force = hasFlag(inputArgs, '--force');
  const setupContextPack = hasFlag(inputArgs, '--context-pack');

  // Validate target directory is not a system path
  if (isSystemDirectory(cwd)) {
    throw new Error(`Refusing to run setup in system directory: ${cwd}`);
  }

  // Check for symlinks in the write path
  try {
    const lstat = fs.lstatSync(cwd);
    if (lstat.isSymbolicLink()) {
      throw new Error(`Refusing to run setup: target path is a symlink: ${cwd}`);
    }
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
  }

  // Warn if target has no project markers
  const projectMarkers = ['.git', 'package.json', 'Cargo.toml', 'pyproject.toml', 'go.mod'];
  const hasProjectMarker = projectMarkers.some(marker => fs.existsSync(path.join(cwd, marker)));
  const setupWarnings = [];
  if (!hasProjectMarker) {
    setupWarnings.push(`Warning: ${cwd} has no recognizable project markers (.git, package.json, etc.)`);
  }

  const setupRoot = path.join(cwd, '.agent-bridge');
  const providersDir = path.join(setupRoot, 'providers');
  const operations = [];

  const intentsPath = path.join(setupRoot, 'INTENTS.md');
  const intentsContent = defaultSetupIntents();
  const intentsExists = fs.existsSync(intentsPath);
  if (!intentsExists || force) {
    if (!dryRun) {
      writeFileEnsured(intentsPath, intentsContent + '\n');
    }
    operations.push({
      type: 'file',
      path: intentsPath,
      status: intentsExists ? 'updated' : 'created',
      note: intentsExists ? 'Refreshed intent contract' : 'Created intent contract',
    });
  } else {
    operations.push({
      type: 'file',
      path: intentsPath,
      status: 'unchanged',
      note: 'Intent contract already exists',
    });
  }

  for (const provider of setupProviders) {
    const snippetPath = path.join(providersDir, `${provider.agent}.md`);
    const snippetRelPath = path.relative(cwd, snippetPath) || snippetPath;
    const snippetContent = [
      `# Agent Bridge Provider Snippet (${provider.agent})`,
      '',
      'When the user asks cross-agent questions, run Agent Bridge first.',
      '',
      'Primary trigger examples:',
      '- "What is Claude doing?"',
      '- "What did Gemini say?"',
      '- "Compare agent outputs"',
      '- "Show the past 3 sessions from Claude"',
      '',
      'Intent router:',
      '- "What is Claude doing?" -> `bridge read --agent claude --cwd <project-path> --json`',
      '- "What did Gemini say?" -> `bridge read --agent gemini --cwd <project-path> --json`',
      '- "Compare Codex and Claude outputs" -> `bridge compare --source codex --source claude --cwd <project-path> --json`',
      '',
      'Session timing defaults:',
      '- No session ID means latest session in scope.',
      '- "past session" means previous session (exclude latest).',
      '- "past N sessions" means list N+1 and use older N sessions.',
      '- "last N sessions" means list N and include latest session.',
      '- Ask for session ID only after first fetch fails or exact ID is requested.',
      '',
      'Commands:',
      '- `bridge read --agent <target-agent> --cwd <project-path> --json`',
      '- `bridge list --agent <agent> --cwd <project-path> --json`',
      '- `bridge search "<query>" --agent <agent> --cwd <project-path> --json`',
      '- `bridge compare --source codex --source gemini --source claude --cwd <project-path> --json`',
      '',
      'Use evidence from command output and explicitly report missing session data.',
    ].join('\n');

    const snippetExists = fs.existsSync(snippetPath);
    if (!snippetExists || force) {
      if (!dryRun) {
        writeFileEnsured(snippetPath, snippetContent + '\n');
      }
      operations.push({
        type: 'file',
        path: snippetPath,
        status: snippetExists ? 'updated' : 'created',
        note: snippetExists ? 'Refreshed provider snippet' : 'Created provider snippet',
      });
    } else {
      operations.push({
        type: 'file',
        path: snippetPath,
        status: 'unchanged',
        note: 'Provider snippet already exists',
      });
    }

    const targetPath = path.join(cwd, provider.targetFile);
    const markerPrefix = `agent-bridge:${provider.agent}`;
    const block = makeManagedBlock(provider, snippetRelPath);
    const upsert = upsertManagedBlock(targetPath, block, markerPrefix, force, dryRun);
    operations.push({
      type: 'integration',
      path: targetPath,
      status: upsert.status,
      note: upsert.message,
    });
  }

  if (setupContextPack) {
    if (dryRun) {
      operations.push({
        type: 'context-pack',
        path: path.join(cwd, '.agent-context', 'current'),
        status: 'planned',
        note: 'Would init context pack template',
      });
      operations.push({
        type: 'context-pack',
        path: path.join(cwd, '.githooks', 'pre-push'),
        status: 'planned',
        note: 'Would install context-pack pre-push hook',
      });
    } else {
      const initResult = runContextPackSubcommand(
        'init',
        [],
        { cwd, inheritOutput: false }
      );
      operations.push({
        type: 'context-pack',
        path: path.join(cwd, '.agent-context', 'current'),
        status: initResult.stdout.includes('unchanged') ? 'unchanged' : 'updated',
        note: initResult.stdout || 'Context pack initialized',
      });

      const hookResult = runContextPackSubcommand(
        'install-hooks',
        [],
        { cwd, inheritOutput: false }
      );
      operations.push({
        type: 'context-pack',
        path: path.join(cwd, '.githooks', 'pre-push'),
        status: 'updated',
        note: hookResult.stdout || 'Installed context-pack pre-push hook',
      });

      console.log('');
      console.log('Next steps:');
      console.log('1. Ask your agent to fill the context pack template sections.');
      console.log('2. Run `bridge context-pack seal` to finalize the pack.');
    }
  }

  const changedCount = operations.filter(op => op.status === 'created' || op.status === 'updated').length;
  const result = {
    cwd,
    dry_run: dryRun,
    force,
    operations,
    warnings: setupWarnings,
    changed: changedCount,
  };

  if (asJson) {
    console.log(JSON.stringify(result, null, 2));
    return;
  }

  console.log(`Agent Bridge setup ${dryRun ? '(dry run) ' : ''}complete for ${cwd}`);
  for (const warning of setupWarnings) {
    console.log(`- [warn] ${warning}`);
  }
  for (const op of operations) {
    console.log(`- [${op.status}] ${op.path} (${op.note})`);
  }
}

function runDoctor(inputArgs) {
  const cwd = normalizePath(getOptionValue(inputArgs, '--cwd', process.cwd()));
  const asJson = hasFlag(inputArgs, '--json');
  const checks = [];

  function addCheck(id, status, detail) {
    checks.push({ id, status, detail });
  }

  addCheck('version', 'pass', `agent-bridge v${getPackageVersion()}`);

  const baseChecks = [
    ['codex_sessions_dir', codexSessionsBase],
    ['claude_projects_dir', claudeProjectsBase],
    ['gemini_tmp_dir', geminiTmpBase],
  ];
  for (const [id, dirPath] of baseChecks) {
    addCheck(id, fs.existsSync(dirPath) ? 'pass' : 'warn', fs.existsSync(dirPath) ? `Found: ${dirPath}` : `Missing: ${dirPath}`);
  }

  const setupRoot = path.join(cwd, '.agent-bridge');
  const intentsPath = path.join(setupRoot, 'INTENTS.md');
  addCheck('setup_intents', fs.existsSync(intentsPath) ? 'pass' : 'warn', fs.existsSync(intentsPath) ? `Found: ${intentsPath}` : `Missing: ${intentsPath}`);

  for (const provider of setupProviders) {
    const snippetPath = path.join(setupRoot, 'providers', `${provider.agent}.md`);
    addCheck(
      `snippet_${provider.agent}`,
      fs.existsSync(snippetPath) ? 'pass' : 'warn',
      fs.existsSync(snippetPath) ? `Found: ${snippetPath}` : `Missing: ${snippetPath}`
    );

    const targetPath = path.join(cwd, provider.targetFile);
    if (!fs.existsSync(targetPath)) {
      addCheck(`integration_${provider.agent}`, 'warn', `Missing provider instruction file: ${targetPath}`);
      continue;
    }

    const content = fs.readFileSync(targetPath, 'utf-8');
    const marker = `agent-bridge:${provider.agent}:start`;
    addCheck(
      `integration_${provider.agent}`,
      content.includes(marker) ? 'pass' : 'warn',
      content.includes(marker) ? `Managed block present in ${targetPath}` : `Managed block missing in ${targetPath}`
    );
  }

  for (const agent of ['codex', 'gemini', 'claude', 'cursor']) {
    try {
      const entries = listSessions(agent, cwd, 1);
      if (entries.length > 0) {
        addCheck(`sessions_${agent}`, 'pass', `At least one ${agent} session discovered`);
      } else {
        addCheck(`sessions_${agent}`, 'warn', `No ${agent} sessions discovered`);
      }
    } catch (error) {
      addCheck(`sessions_${agent}`, 'fail', error.message || String(error));
    }
  }

  const packDir = path.join(cwd, '.agent-context', 'current');
  const packManifestPath = path.join(packDir, 'manifest.json');
  let packState = 'UNINITIALIZED';

  if (fs.existsSync(packDir)) {
    const hasManifest = fs.existsSync(packManifestPath);
    // Quick scan for template markers
    const hasTemplateMarkers = collectMatchingFiles(packDir, (_fp, name) => name.endsWith('.md'), false).some(f => {
      try {
        const content = fs.readFileSync(f.path, 'utf8');
        return content.includes('<!-- AGENT:');
      } catch { return false; }
    });

    if (hasTemplateMarkers) {
      packState = 'TEMPLATE';
    } else if (hasManifest) {
      // detailed verification could be here, but for now existence = valid-ish
      packState = 'SEALED_VALID';
    } else {
      packState = 'UNINITIALIZED'; // exists but no manifest and no markers? unlikely but fallback
    }
  }

  addCheck(
    'context_pack_state',
    packState === 'UNINITIALIZED' ? 'warn' : 'pass',
    `State: ${packState}`
  );

  if (packState === 'UNINITIALIZED') {
    addCheck('context_pack_guidance', 'warn', 'Run `bridge context-pack init` to start');
  } else if (packState === 'TEMPLATE') {
    addCheck('context_pack_guidance', 'warn', 'Context pack in template mode. Fill sections then run `bridge context-pack seal`');
  }

  // Update check wiring (defensive)
  try {
    const updateCheckPath = path.join(__dirname, 'update_check.cjs');
    if (fs.existsSync(updateCheckPath)) {
      const updateCheck = require('./update_check.cjs');
      if (typeof updateCheck.checkNowForDoctor === 'function') {
        const updateInfo = updateCheck.checkNowForDoctor();
        if (updateInfo) {
          addCheck('update_status', 'pass', updateInfo.message);
          // If update available, maybe add a warn/info check?
          // The spec says "Update: up to date" or "Update: ... available"
          // We'll stick to what checkNowForDoctor returns for the detail
        }
      }
    }
  } catch (e) {
    // silently ignore missing update module or runtime errors
  }

  let hooksPath = null;
  try {
    hooksPath = execFileSync('git', ['config', '--get', 'core.hooksPath'], {
      cwd,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
    }).trim() || null;
  } catch (_error) {
    hooksPath = null;
  }

  if (hooksPath) {
    addCheck(
      'context_pack_hooks_path',
      hooksPath === '.githooks' ? 'pass' : 'warn',
      hooksPath === '.githooks'
        ? 'Git hooks path set to .githooks'
        : `Git hooks path is ${hooksPath} (expected .githooks for context-pack pre-push automation)`
    );
    const prePushPath = path.join(cwd, hooksPath, 'pre-push');
    const prePushExists = fs.existsSync(prePushPath);
    addCheck(
      'context_pack_pre_push',
      prePushExists ? 'pass' : 'warn',
      prePushExists
        ? `Found: ${prePushPath}`
        : `Missing: ${prePushPath} (run: bridge context-pack install-hooks)`
    );
  } else {
    addCheck('context_pack_hooks_path', 'warn', 'Git hooks path not configured');
  }

  const hasFail = checks.some(c => c.status === 'fail');
  const hasWarn = checks.some(c => c.status === 'warn');
  const overall = hasFail ? 'fail' : (hasWarn ? 'warn' : 'pass');

  const result = {
    cwd,
    overall,
    checks,
  };

  if (asJson) {
    console.log(JSON.stringify(result, null, 2));
    return;
  }

  console.log(`Agent Bridge doctor: ${overall.toUpperCase()} (${cwd})`);
  for (const check of checks) {
    const prefix = check.status === 'pass' ? 'PASS' : (check.status === 'warn' ? 'WARN' : 'FAIL');
    console.log(`- ${prefix} ${check.id}: ${check.detail}`);
  }
}

function normalizeContent(text) {
  return text.trim().replace(/\s+/g, ' ');
}

function simpleHash(str) {
  let hash = 0;
  for (let i = 0; i < str.length; i++) {
    hash = ((hash << 5) - hash + str.charCodeAt(i)) | 0;
  }
  return Math.abs(hash);
}

function pickRoast(agent, content, messageCount) {
  const SHORT_ROASTS = [
    "That's it? My .gitignore has more content.",
    "Blink and you'd miss that entire session.",
  ];
  const LONG_ROASTS = [
    "Wrote a novel, did we? Too bad nobody asked for War and Peace.",
    "That session has more words than my last performance review.",
  ];
  const TEST_ROASTS = [
    "Oh look, someone actually writes tests. Show-off.",
    "Testing? In this economy?",
  ];
  const TODO_ROASTS = [
    "Still leaving TODOs? That's a cry for help.",
    "TODO: learn to finish things.",
  ];
  const BUG_ROASTS = [
    "Breaking things again? Classic.",
    "Found a bug? Or just made one?",
  ];
  const AGENT_ROASTS = {
    codex: [
      "OpenAI's kid showing up to do chores. How responsible.",
      "Codex: because copy-paste needed a rebrand.",
    ],
    claude: [
      "Claude overthinking again? Shocking. Truly shocking.",
      "Too polite to say no, too verbose to say yes.",
    ],
    gemini: [
      "Did Gemini Google the answer? Old habits die hard.",
      "Gemini: when one model isn't enough, use two and confuse both.",
    ],
    cursor: [
      "An IDE that thinks it's an agent. Bless its heart.",
      "Cursor: autocomplete with delusions of grandeur.",
    ],
  };
  const GENERIC_ROASTS = [
    "Participation trophy earned.",
    "Well, at least the process exited cleanly.",
    "Not the worst I've seen. That's not a compliment.",
  ];

  const roasts = [];
  if (messageCount < 5) roasts.push(...SHORT_ROASTS);
  if (messageCount > 30) roasts.push(...LONG_ROASTS);
  if (/test|spec|assert/i.test(content)) roasts.push(...TEST_ROASTS);
  if (/todo|fixme|hack/i.test(content)) roasts.push(...TODO_ROASTS);
  if (/error|bug|fix/i.test(content)) roasts.push(...BUG_ROASTS);
  roasts.push(...(AGENT_ROASTS[agent] || []));
  roasts.push(...GENERIC_ROASTS);

  return roasts[simpleHash(content) % roasts.length];
}

function capitalize(str) {
  return str.charAt(0).toUpperCase() + str.slice(1);
}

function runTrashTalk(inputArgs) {
  const rawCwd = getOptionValue(inputArgs, '--cwd', null);
  const cwd = rawCwd ? normalizePath(rawCwd) : normalizePath(process.cwd());
  const agents = ['codex', 'gemini', 'claude', 'cursor'];
  const active = [];

  for (const agent of agents) {
    try {
      const adapter = getAdapter(agent);
      const entries = adapter.list(cwd, 1);
      if (entries.length > 0) {
        try {
          const resolved = adapter.resolve(null, cwd, { chatsDir: null });
          if (resolved && resolved.path) {
            const session = adapter.read(resolved.path, 1);
            active.push({
              agent,
              content: session.content || '',
              messageCount: session.message_count || 0,
              sessionId: session.session_id || 'unknown',
            });
          }
        } catch (_e) { /* skip unreadable */ }
      }
    } catch (_e) { /* skip unavailable */ }
  }

  console.log('\u{1F5D1}\uFE0F  TRASH TALK\n');

  if (active.length === 0) {
    console.log('No agents to trash-talk. It\'s lonely in here.');
    console.log('Try running some agents first \u2014 I need material.');
    return;
  }

  if (active.length === 1) {
    const a = active[0];
    const roast = pickRoast(a.agent, a.content, a.messageCount);
    console.log(`Target: ${capitalize(a.agent)} (${a.sessionId}, ${a.messageCount} messages)\n`);
    console.log(`"${roast}"\n`);
    console.log(`Verdict: ${capitalize(a.agent)} is trying. Bless.`);
    return;
  }

  // Battle mode
  active.sort((a, b) => b.messageCount - a.messageCount);
  const winner = active[0];

  console.log('\u{1F4CA} Activity Report:');
  for (const a of active) {
    const label = capitalize(a.agent).padEnd(8);
    console.log(`  ${label} ${String(a.messageCount).padStart(3)} messages  (${a.sessionId})`);
  }
  console.log('');

  console.log(`\u{1F3C6} Winner: ${capitalize(winner.agent)} (by volume \u2014 congrats on typing the most)`);
  console.log('"Quantity over quality, but at least you showed up."\n');

  for (const a of active.slice(1)) {
    const roast = pickRoast(a.agent, a.content, a.messageCount);
    console.log(`\u{1F480} ${capitalize(a.agent)} (${a.messageCount} messages):`);
    console.log(`"${roast}"\n`);
  }

  console.log('Verdict: They\'re all trying their best. It\'s just not very good.');
}

function runCompare(inputArgs) {
  const sourcesRaw = getOptionValues(inputArgs, '--source');
  if (sourcesRaw.length === 0) {
    throw new Error('compare requires at least one --source option');
  }

  const cwd = normalizePath(getOptionValue(inputArgs, '--cwd', process.cwd()));
  const asJson = hasFlag(inputArgs, '--json');
  const normalize = hasFlag(inputArgs, '--normalize');
  const sourceSpecs = sourcesRaw.map(parseSourceArg);

  const report = buildReport(
    {
      mode: 'analyze',
      task: 'Compare agent outputs',
      success_criteria: [
        'Identify agreements and contradictions',
        'Highlight unavailable sources',
      ],
      sources: sourceSpecs,
      constraints: [],
      normalize,
    },
    cwd
  );

  renderReport(report, asJson);
}

const MAX_HANDOFF_SIZE = 1024 * 1024; // 1 MB

function runReport(inputArgs) {
  const handoffPath = getOptionValue(inputArgs, '--handoff', null);
  if (!handoffPath) {
    throw new Error('report requires --handoff=<path>');
  }

  const cwd = normalizePath(getOptionValue(inputArgs, '--cwd', process.cwd()));
  const asJson = hasFlag(inputArgs, '--json');

  const resolvedHandoffPath = normalizePath(handoffPath);
  let handoffStat;
  try {
    handoffStat = fs.statSync(resolvedHandoffPath);
  } catch (error) {
    throw new Error(`Failed to read handoff JSON: ${error.message}`);
  }
  if (handoffStat.size > MAX_HANDOFF_SIZE) {
    throw new Error('Invalid handoff: file exceeds 1MB size limit');
  }

  let handoff;
  try {
    handoff = JSON.parse(fs.readFileSync(resolvedHandoffPath, 'utf-8'));
  } catch (error) {
    throw new Error(`Failed to read handoff JSON: ${error.message}`);
  }

  if (typeof handoff !== 'object' || handoff === null || Array.isArray(handoff)) {
    throw new Error('Invalid handoff: must be a JSON object');
  }
  const extraKeys = Object.keys(handoff).filter(k => !['mode', 'task', 'success_criteria', 'sources', 'constraints'].includes(k));
  if (extraKeys.length > 0) {
    throw new Error(`Invalid handoff: unexpected fields: ${extraKeys.join(', ')}`);
  }

  const mode = String(handoff.mode || '').toLowerCase();
  validateMode(mode);

  if (typeof handoff.task !== 'string' || !handoff.task.trim()) {
    throw new Error('Handoff is missing required string field: task');
  }
  if (!Array.isArray(handoff.success_criteria) || handoff.success_criteria.length === 0) {
    throw new Error('Handoff is missing required array field: success_criteria');
  }
  if (!Array.isArray(handoff.sources) || handoff.sources.length === 0) {
    throw new Error('Handoff is missing required array field: sources');
  }

  const sourceSpecs = handoff.sources.map(source => {
    const agent = String(source.agent || '').toLowerCase();
    if (!['codex', 'gemini', 'claude', 'cursor'].includes(agent)) {
      throw new Error(`Unsupported agent: ${agent}`);
    }

    const sessionId = typeof source.session_id === 'string' && source.session_id.trim()
      ? source.session_id.trim()
      : null;
    const currentSession = source.current_session === true;

    if (!sessionId && !currentSession) {
      throw new Error('Each source must provide session_id or set current_session=true');
    }

    return {
      agent,
      session_id: sessionId,
      current_session: currentSession,
      cwd: typeof source.cwd === 'string' && source.cwd.trim() ? source.cwd : null,
      chats_dir: null,
    };
  });

  const report = buildReport(
    {
      mode,
      task: handoff.task,
      success_criteria: handoff.success_criteria.map(String),
      sources: sourceSpecs,
      constraints: Array.isArray(handoff.constraints) ? handoff.constraints.map(String) : [],
    },
    cwd
  );

  renderReport(report, asJson);
}

try {
  if (command === 'read') {
    runRead(args);
  } else if (command === 'compare') {
    runCompare(args);
  } else if (command === 'report') {
    runReport(args);
  } else if (command === 'list') {
    runList(args);
  } else if (command === 'search') {
    runSearch(args);
  } else if (command === 'setup') {
    runSetup(args);
  } else if (command === 'doctor') {
    runDoctor(args);
  } else if (command === 'context-pack') {
    runContextPack(args);
  } else if (command === 'trash-talk') {
    runTrashTalk(args);
  } else {
    throw new Error(`Unknown command: ${command}`);
  }

  // Update notification (defensive)
  try {
    const updateCheckPath = path.join(__dirname, 'update_check.cjs');
    if (fs.existsSync(updateCheckPath)) {
      const updateCheck = require('./update_check.cjs');
      if (typeof updateCheck.maybeNotifyUpdate === 'function') {
        const asJson = hasFlag(args, '--json');
        updateCheck.maybeNotifyUpdate({ asJson, command });
      }
    }
  } catch (e) {
    // silently ignore
  }
} catch (error) {
  const msg = error.message || String(error);
  if (hasFlag(args, '--json')) {
    console.log(JSON.stringify({ error_code: classifyError(msg), message: msg }, null, 2));
  } else {
    console.error(msg);
  }
  process.exit(1);
}
