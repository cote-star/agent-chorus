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

import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { readJson, writeJson, getChorusRoot } from "./lib/fs.mjs";
import { nowIso } from "./lib/identity.mjs";

const POLL_INTERVAL_MS  = 8_000;
const BRIDGE_EVERY_N    = 4;     // sync bridge every ~32s per project
const KNOWN_AGENTS      = ["claude", "codex", "gemini"];
const RELAY_CONFIG_PATH = path.join(os.homedir(), ".agent-chorus", "relay-config.json");
const BRIDGE_SCRIPT     = path.join(path.dirname(fileURLToPath(import.meta.url)), "bridge.mjs");

// ── Config ─────────────────────────────────────────────────────────────────

function loadConfig() {
  if (!fs.existsSync(RELAY_CONFIG_PATH)) {
    log(`No relay config found at ${RELAY_CONFIG_PATH}`);
    log(`Create it with: { "projects": [{ "name": "my-project", "path": "/abs/path" }] }`);
    return { projects: [] };
  }
  return readJson(RELAY_CONFIG_PATH, { projects: [] });
}

// ── CLI dispatch ───────────────────────────────────────────────────────────

/**
 * Returns { cmd, stdin } for runCommand.
 * cmd is a shell command string; shell: true resolves .cmd/.ps1 shims on Windows.
 * Double-quote escaping is used (cmd.exe compatible via shell:true on Windows).
 */
function buildCliInvocation(agentName, directive, projectPath) {
  // shell: true uses cmd.exe on Windows — escape double-quotes inside the arg.
  const dq = (s) => `"${s.replace(/"/g, '\\"')}"`;
  switch (agentName) {
    case "claude":
      // shell:true finds claude.cmd shim; directive passed as positional arg.
      return { cmd: `claude --print ${dq(directive)}`, stdin: null };
    case "codex":
      // codex exec reads from stdin when PROMPT arg is omitted.
      return { cmd: `codex exec -C ${dq(projectPath)} -s workspace-write`, stdin: directive };
    default:
      return null; // Gemini has no confirmed non-interactive mode
  }
}

/**
 * Runs a shell command string with { shell: true } so Windows .cmd/.ps1 shims
 * resolve correctly without requiring an explicit PowerShell or cmd.exe wrapper.
 */
function runCommand(cmd, stdin = null, env = {}) {
  return new Promise((resolve, reject) => {
    const proc = spawn(cmd, {
      shell: true,
      stdio: ["pipe", "pipe", "pipe"],
      env: { ...process.env, ...env },
    });
    let stdout = "";
    let stderr = "";
    proc.stdout.on("data", (d) => { stdout += d; });
    proc.stderr.on("data", (d) => { stderr += d; });
    proc.on("close", (code) => {
      if (code !== 0) reject(new Error(`Exit ${code}: ${stderr.slice(0, 600)}`));
      else resolve(stdout);
    });
    proc.on("error", reject);
    if (stdin) {
      proc.stdin.write(stdin, "utf8");
    }
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

async function processTask(chorusRoot, projectPath, agentName, taskFile) {
  const task = readJson(taskFile, null);
  if (!task || task.status !== "pending") return;

  // Mark received atomically to prevent double-dispatch
  writeJson(taskFile, { ...task, status: "received", received_at: nowIso() });
  log(`Dispatch [${task.task_id}] ${task.from} → ${agentName}@${path.basename(projectPath)}: "${task.directive.slice(0, 70)}"`);

  const invocation = buildCliInvocation(agentName, buildDirective(task), projectPath);
  if (!invocation) {
    writeJson(taskFile, { ...task, status: "failed", failed_at: nowIso(),
      error: `No non-interactive CLI for: ${agentName}` });
    log(`Skipped — no CLI for ${agentName}`);
    return;
  }

  const { cmd, stdin } = invocation;
  // CHORUS_PROJECT_ROOT tells scripts inside the spawned agent which project to operate on
  const env = { CHORUS_PROJECT_ROOT: projectPath };

  try {
    const result = await runCommand(cmd, stdin, env);
    const fresh  = readJson(taskFile, task);
    writeJson(taskFile, { ...fresh, status: "done", done_at: nowIso(), result: result.trim() });
    log(`Done [${task.task_id}]`);
    if (!task.is_reply && task.from && task.from !== agentName) {
      sendReply(chorusRoot, task, result.trim());
    }
  } catch (err) {
    const fresh = readJson(taskFile, task);
    writeJson(taskFile, { ...fresh, status: "failed", failed_at: nowIso(),
      error: err.message.slice(0, 500) });
    log(`Failed [${task.task_id}]: ${err.message.slice(0, 120)}`);
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
