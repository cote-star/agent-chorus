// UserPromptSubmit hook: reads pending tasks from all configured project inboxes
// and injects them as system context for the current session.
// Claude Code / Codex prepend stdout of this hook to the prompt.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { getChorusRoot, readJson, writeJson } from "./lib/fs.mjs";
import { getAgentIdentity, nowIso } from "./lib/identity.mjs";
import { writeEvent } from "./lib/events.mjs";

const RELAY_CONFIG_PATH = path.join(os.homedir(), ".agent-chorus", "relay-config.json");

const { agentName } = getAgentIdentity();

// Collect project roots to check:
// 1. CHORUS_PROJECT_ROOT env var (set by relay for non-interactive spawns)
// 2. All projects in relay-config.json (for interactive sessions at user root)
// 3. process.cwd() as final fallback
const projectRoots = new Set();

if (process.env.CHORUS_PROJECT_ROOT) {
  projectRoots.add(process.env.CHORUS_PROJECT_ROOT);
}

const config = readJson(RELAY_CONFIG_PATH, { projects: [] });
for (const p of config.projects) {
  if (fs.existsSync(p.path)) projectRoots.add(p.path);
}

if (projectRoots.size === 0) {
  projectRoots.add(process.cwd());
}

const allPending = [];

for (const projectRoot of projectRoots) {
  const chorusRoot = getChorusRoot(projectRoot);
  const inboxDir = path.join(chorusRoot, "tasks", "inbox", agentName);
  if (!fs.existsSync(inboxDir)) continue;

  const tasks = fs.readdirSync(inboxDir)
    .filter(f => f.endsWith(".json"))
    .map(f => {
      try { return { file: path.join(inboxDir, f), task: readJson(path.join(inboxDir, f), null) }; }
      catch { return null; }
    })
    .filter(t => t?.task?.status === "pending");

  for (const { file, task } of tasks) {
    writeJson(file, { ...task, status: "received", received_at: nowIso() });
    try { writeEvent("task_injected", { task_id: task.task_id, from: task.from }); } catch {}
    allPending.push({ ...task, _project: path.basename(projectRoot) });
  }
}

if (allPending.length === 0) process.exit(0);

const lines = allPending.map(t =>
  `[AGENT TASK | project=${t._project} | from=${t.from} | priority=${t.priority} | id=${t.task_id}]\n${t.directive}`
);

console.log("=== PENDING AGENT TASKS ===");
lines.forEach(l => console.log(l));
console.log("=== END AGENT TASKS ===");
