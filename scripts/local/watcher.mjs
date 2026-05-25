import { execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chorusRoot, readJson } from "./lib/fs.mjs";
import { getAgentIdentity } from "./lib/identity.mjs";

const { agentName } = getAgentIdentity();
const inboxDir = path.join(chorusRoot, "tasks", "inbox", agentName);
const bridgePath = path.join(path.dirname(fileURLToPath(import.meta.url)), "bridge.mjs");

fs.mkdirSync(inboxDir, { recursive: true });

console.log(`[chorus:watch] agent=${agentName}  inbox=${inboxDir}`);
console.log(`[chorus:watch] Bridge sync every 30s. Ctrl+C to stop.\n`);

function printTask(filePath) {
  try {
    const task = readJson(filePath, null);
    if (task?.status === "pending") {
      console.log(`\n╔══ AGENT TASK RECEIVED ══════════════════════╗`);
      console.log(`  from:      ${task.from}`);
      console.log(`  priority:  ${task.priority}`);
      console.log(`  task_id:   ${task.task_id}`);
      console.log(`  directive: ${task.directive}`);
      if (task.context_checkpoint_ref) {
        console.log(`  context:   ${task.context_checkpoint_ref}`);
      }
      console.log(`╚════════════════════════════════════════════╝\n`);
    }
  } catch { /* file not ready yet */ }
}

const watcher = fs.watch(inboxDir, (event, filename) => {
  if (!filename?.endsWith(".json")) return;
  printTask(path.join(inboxDir, filename));
});

// Print any already-pending tasks on startup
fs.readdirSync(inboxDir)
  .filter(f => f.endsWith(".json"))
  .forEach(f => printTask(path.join(inboxDir, f)));

// Bridge sync every 30s
setInterval(() => {
  try {
    execSync(`node "${bridgePath}"`, { stdio: "pipe" });
    process.stdout.write(".");
  } catch (e) {
    console.error("[chorus:watch] bridge sync failed:", e.message);
  }
}, 30_000);

process.on("SIGINT", () => {
  watcher.close();
  console.log("\n[chorus:watch] stopped.");
  process.exit(0);
});
