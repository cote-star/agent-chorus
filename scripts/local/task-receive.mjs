import fs from "node:fs";
import path from "node:path";
import { chorusRoot, readJson, writeJson } from "./lib/fs.mjs";
import { getAgentIdentity, nowIso } from "./lib/identity.mjs";
import { writeEvent } from "./lib/events.mjs";

const { agentName } = getAgentIdentity();
const inboxDir = path.join(chorusRoot, "tasks", "inbox", agentName);
const markReceived = process.argv.includes("--mark-received");

if (!fs.existsSync(inboxDir)) {
  console.log(JSON.stringify([]));
  process.exit(0);
}

const tasks = fs.readdirSync(inboxDir)
  .filter(f => f.endsWith(".json"))
  .map(f => readJson(path.join(inboxDir, f), null))
  .filter(t => t?.status === "pending");

if (markReceived) {
  tasks.forEach(t => {
    const filePath = path.join(inboxDir, `${t.task_id}.json`);
    writeJson(filePath, { ...t, status: "received", received_at: nowIso() });
    writeEvent("task_received", { task_id: t.task_id, from: t.from });
  });
}

console.log(JSON.stringify(tasks, null, 2));
