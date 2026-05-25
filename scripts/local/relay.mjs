/**
 * Chorus Relay Daemon — autonomous cross-agent task dispatcher.
 *
 * Reads ~/.agent-chorus/relay-config.json for the list of projects to monitor.
 * Polls all agent inboxes across all configured projects every POLL_INTERVAL_MS.
 * Spawns agents non-interactively when pending tasks are found.
 *
 *   claude tasks  →  claude --print "[directive]"
 *   codex  tasks  →  codex exec -C <projectPath> -s workspace-write "[directive]"
 *
 * Run once at login (see setup-relay.mjs to install auto-start):
 *   node scripts/local/relay.mjs
 */

import { spawn, execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { readJson, writeJson, getChorusRoot } from "./lib/fs.mjs";
import { nowIso } from "./lib/identity.mjs";

const POLL_INTERVAL_MS  = 8_000;
const BRIDGE_EVERY_N    = 4;     // sync bridge every ~32s per project
const KNOWN_AGENTS      = ["claude", "codex", "gemini", "hermes"];
const RELAY_CONFIG_PATH = path.join(os.homedir(), ".agent-chorus", "relay-config.json");
const BRIDGE_SCRIPT     = path.join(path.dirname(fileURLToPath(import.meta.url)), "bridge.mjs");
// Option B: per-prompt notify file read by active agent sessions via CARL rule
const RELAY_NOTIFY_FILE = path.join(os.homedir(), ".agent-chorus", "relay-notify.md");

// ── Config ─────────────────────────────────────────────────────────────────

function loadConfig() {
  if (!fs.existsSync(RELAY_CONFIG_PATH)) {
    log(`No relay config found at ${RELAY_CONFIG_PATH}`);
    log(`Create it with: { "projects": [{ "name": "my-project", "path": "/abs/path" }] }`);
    return { projects: [] };
  }
  return readJson(RELAY_CONFIG_PATH, { projects: [] });
}

// ── Active session detection ───────────────────────────────────────────────

/**
 * Returns the session ID of the most recent Claude Code session for the given
 * project path, or null if no prior sessions exist.
 * Session files: ~/.claude/projects/<path-slug>/<session-id>.jsonl
 */
function latestClaudeSessionId(projectPath) {
  const slug = projectPath.replace(/[/\\: ]/g, "-").replace(/^-+|-+$/g, "");
  const sessionDir = path.join(os.homedir(), ".claude", "projects", slug);
  if (!fs.existsSync(sessionDir)) return null;
  const files = fs.readdirSync(sessionDir)
    .filter(f => f.endsWith(".jsonl"))
    .map(f => ({ id: f.replace(/\.jsonl$/, ""), mtime: fs.statSync(path.join(sessionDir, f)).mtimeMs }))
    .sort((a, b) => b.mtime - a.mtime);
  return files.length > 0 ? files[0].id : null;
}

/**
 * Returns the session ID of the most recent Hermes CLI session, or null.
 * Calls `hermes sessions list --source cli --limit 1` via wsl.exe (5s timeout).
 * Session ID is the last whitespace-delimited token on the last non-empty line.
 */
function hermesLatestCliSessionId() {
  try {
    const out = execFileSync("wsl.exe", [
      "bash", "-c",
      "~/.local/bin/hermes sessions list --source cli --limit 1 2>/dev/null | tail -1",
    ], { encoding: "utf8", timeout: 5000 });
    const line = out.trim();
    if (!line) return null;
    const parts = line.split(/\s+/);
    const id = parts[parts.length - 1];
    // Hermes session IDs look like: 20260525_231306_b25baa
    return /^\d{8}_\d{6}_[0-9a-f]+$/.test(id) ? id : null;
  } catch { return null; }
}

/**
 * Returns an invocation descriptor for runCommand.
 *
 * Shell agents (claude, codex): { shell: true, cmd, stdin, cwd?, hitchMode }
 *   shell:true lets cmd.exe find .cmd/.ps1 shims on Windows.
 *   cwd is projectPath so --resume finds the right session directory.
 *
 * Direct agents (hermes): { shell: false, exe, args, stdin, hitchMode }
 *   wsl.exe spawned directly — no cmd.exe layer, no shell-quoting issues.
 *
 * hitchMode reflects which path was taken:
 *   "resume"      — agent resumed an existing session by ID
 *   "fresh"       — claude spawned with no prior session
 *   "fresh-spawn" — hermes spawned fresh via hermes -z
 */
function buildCliInvocation(agentName, directive, projectPath) {
  const dq = (s) => `"${s.replace(/"/g, '\\"')}"`;
  switch (agentName) {
    case "claude": {
      const sessionId  = latestClaudeSessionId(projectPath);
      const resumeFlag = sessionId ? `--resume ${sessionId}` : "";
      return {
        shell: true,
        cmd: `claude --print ${resumeFlag} ${dq(directive)}`.replace(/\s+/g, " ").trim(),
        stdin: null,
        cwd: projectPath,
        hitchMode: sessionId ? "resume" : "fresh",
      };
    }
    case "codex":
      return { shell: true, cmd: `codex exec -C ${dq(projectPath)} -s workspace-write`, stdin: directive };
    case "hermes": {
      const sessionId = hermesLatestCliSessionId();
      const resumeFlag = sessionId ? `--resume ${sessionId}` : "";
      const tmpFile = path.join(os.tmpdir(), `hermes-relay-${Date.now()}.txt`);
      fs.writeFileSync(tmpFile, directive, "utf8");
      const wslPath = tmpFile.replace(/\\/g, "/").replace(/^([A-Za-z]):/, (_, d) => `/mnt/${d.toLowerCase()}`);
      return {
        shell: false,
        exe: "wsl.exe",
        args: ["bash", "-c", `~/.local/bin/hermes ${resumeFlag} -z "$(cat '${wslPath}')" < /dev/null; rm -f '${wslPath}'`],
        stdin: null,
        hitchMode: sessionId ? "resume" : "fresh-spawn",
      };
    }
    default:
      return null; // Gemini has no confirmed non-interactive mode
  }
}

function runCommand(invocation, env = {}) {
  return new Promise((resolve, reject) => {
    const { shell, cmd, exe, args, stdin, cwd } = invocation;
    const spawnOpts = { stdio: ["pipe", "pipe", "pipe"], env: { ...process.env, ...env }, ...(cwd ? { cwd } : {}) };
    const proc = shell
      ? spawn(cmd, { ...spawnOpts, shell: true })
      : spawn(exe, args, spawnOpts);
    let stdout = "";
    let stderr = "";
    proc.stdout.on("data", (d) => { stdout += d; });
    proc.stderr.on("data", (d) => { stderr += d; });
    proc.on("close", (code) => {
      if (code !== 0) reject(new Error(`Exit ${code}: ${stderr.slice(0, 600)}`));
      else resolve(stdout);
    });
    proc.on("error", reject);
    if (stdin) proc.stdin.write(stdin, "utf8");
    proc.stdin.end();
  });
}

// ── Task handling ──────────────────────────────────────────────────────────

function sendReply(chorusRoot, originalTask, result) {
  const replyId  = `reply-${originalTask.task_id}-${Date.now()}`;
  const replyDir = path.join(chorusRoot, "tasks", "inbox", originalTask.from);
  writeJson(path.join(replyDir, `${replyId}.json`), {
    task_id:           replyId,
    from:              originalTask.to,
    to:                originalTask.from,
    directive:         `[Reply to task ${originalTask.task_id}]\n\n${result.slice(0, 3000)}`,
    priority:          "normal",
    created_at:        nowIso(),
    status:            "pending",
    is_reply:          true,
    reply_to_task_id:  originalTask.task_id,
  });
  log(`Reply queued: ${originalTask.to} → ${originalTask.from} (re: ${originalTask.task_id})`);
}

/**
 * Option B — writes task outcome to ~/.agent-chorus/relay-notify.md.
 * CARL HERMES rule checks this file before every prompt and clears it after reading.
 * Gives the active agent session near-instant awareness of relay completions.
 */
function writeRelayNotify(agentName, task, payload, status) {
  const block = [
    `## [${nowIso()}] relay → ${agentName} [${status}]`,
    `**task_id:** ${task.task_id}`,
    `**from:** ${task.from}`,
    `**directive:** ${task.directive.slice(0, 120)}`,
    `**${status === "done" ? "result" : "error"}:** ${(payload || "").slice(0, 500)}`,
    "",
  ].join("\n");
  fs.mkdirSync(path.dirname(RELAY_NOTIFY_FILE), { recursive: true });
  fs.appendFileSync(RELAY_NOTIFY_FILE, block, "utf8");
}

/**
 * Option A — sends a chorus message to the target agent so relay completions
 * are discoverable via `chorus messages --agent <agent>` at next standup.
 */
function sendChorusMessage(projectPath, agentName, task, payload, status) {
  try {
    const snippet = (payload || "").slice(0, 300).replace(/\n/g, " ");
    const msg = `[relay-${status}] task ${task.task_id} · "${task.directive.slice(0, 80)}" → ${snippet}`;
    execFileSync("chorus", [
      "send", "--from", "relay", "--to", agentName,
      "--message", msg, "--cwd", projectPath,
    ], { encoding: "utf8", timeout: 5000, shell: true });
    log(`Chorus message sent → ${agentName}`);
  } catch (err) {
    log(`Chorus send failed (non-fatal): ${err.message.slice(0, 100)}`);
  }
}

async function processTask(chorusRoot, projectPath, agentName, taskFile) {
  const task = readJson(taskFile, null);
  if (!task || task.status !== "pending") return;

  // Mark received atomically to prevent double-dispatch
  writeJson(taskFile, { ...task, status: "received", received_at: nowIso() });
  const invocation = buildCliInvocation(agentName, buildDirective(task), projectPath);
  const hitchTag = invocation?.hitchMode ? ` [${invocation.hitchMode}]` : "";
  log(`Dispatch [${task.task_id}] ${task.from} → ${agentName}@${path.basename(projectPath)}${hitchTag}: "${task.directive.slice(0, 70)}"`);

  if (!invocation) {
    writeJson(taskFile, { ...task, status: "failed", failed_at: nowIso(),
      error: `No non-interactive CLI for: ${agentName}` });
    log(`Skipped — no CLI for ${agentName}`);
    return;
  }

  // CHORUS_PROJECT_ROOT tells scripts inside the spawned agent which project to operate on
  const env = { CHORUS_PROJECT_ROOT: projectPath };

  try {
    const result = await runCommand(invocation, env);
    const fresh  = readJson(taskFile, task);
    writeJson(taskFile, { ...fresh, status: "done", done_at: nowIso(), result: result.trim() });
    log(`Done [${task.task_id}]`);
    writeRelayNotify(agentName, task, result.trim(), "done");
    sendChorusMessage(projectPath, agentName, task, result.trim(), "done");
    if (!task.is_reply && task.from && task.from !== agentName) {
      sendReply(chorusRoot, task, result.trim());
    }
  } catch (err) {
    const fresh = readJson(taskFile, task);
    writeJson(taskFile, { ...fresh, status: "failed", failed_at: nowIso(),
      error: err.message.slice(0, 500) });
    log(`Failed [${task.task_id}]: ${err.message.slice(0, 120)}`);
    writeRelayNotify(agentName, task, err.message, "failed");
    sendChorusMessage(projectPath, agentName, task, err.message, "failed");
  }
}

function buildDirective(task) {
  const lines = [task.directive];
  if (task.context) lines.push("", "=== Context ===", task.context);
  lines.push("", `[Chorus relay — task_id: ${task.task_id}, from: ${task.from}]`);
  return lines.join("\n");
}

async function pollProject(project) {
  const chorusRoot = getChorusRoot(project.path);
  for (const agentName of KNOWN_AGENTS) {
    const inboxDir = path.join(chorusRoot, "tasks", "inbox", agentName);
    if (!fs.existsSync(inboxDir)) continue;
    const files = fs.readdirSync(inboxDir).filter(f => f.endsWith(".json")).sort();
    for (const file of files) {
      const taskFile = path.join(inboxDir, file);
      try {
        const task = readJson(taskFile, null);
        if (task?.status === "pending") {
          await processTask(chorusRoot, project.path, agentName, taskFile);
        }
      } catch (err) {
        log(`Error reading ${file}: ${err.message}`);
      }
    }
  }
}

async function syncBridge(projectPath) {
  try {
    const psq = (s) => `'${s.replace(/'/g, "''")}'`;
    await runCommand(`node ${psq(BRIDGE_SCRIPT)}`, null, { CHORUS_PROJECT_ROOT: projectPath });
  } catch { /* best-effort */ }
}

// ── Main loop ──────────────────────────────────────────────────────────────

function log(msg) {
  const ts = new Date().toISOString().slice(11, 23);
  console.log(`[relay ${ts}] ${msg}`);
}

let pollCount = 0;
async function tick() {
  const config = loadConfig();
  if (config.projects.length === 0) {
    if (pollCount === 0) log("No projects configured — add entries to relay-config.json");
  }
  for (const project of config.projects) {
    if (!fs.existsSync(project.path)) {
      log(`Project path not found, skipping: ${project.path}`);
      continue;
    }
    try {
      await pollProject(project);
      if (pollCount % BRIDGE_EVERY_N === 0) await syncBridge(project.path);
    } catch (err) {
      log(`Poll error for ${project.name}: ${err.message}`);
    }
  }
  pollCount++;
  setTimeout(tick, POLL_INTERVAL_MS);
}

log("Chorus relay daemon starting");
log(`Config: ${RELAY_CONFIG_PATH}`);
log(`Poll interval: ${POLL_INTERVAL_MS}ms`);

// Initial bridge sync on startup
const config = loadConfig();
Promise.all(config.projects.map(p => syncBridge(p.path).catch(() => {})))
  .finally(() => tick());
