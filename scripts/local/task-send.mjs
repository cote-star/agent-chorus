import crypto from "node:crypto";
import path from "node:path";
import { chorusRoot, writeJson } from "./lib/fs.mjs";
import { getAgentIdentity, nowIso } from "./lib/identity.mjs";
import { writeEvent } from "./lib/events.mjs";

const args = process.argv.slice(2);
const get = (flag) => {
  const i = args.indexOf(flag);
  return i !== -1 ? args[i + 1] : null;
};

const to         = get("--to");
const directive  = get("--directive");
const priority   = get("--priority") || "normal";
const contextRef = get("--context-ref") || null;

if (!to || !directive) {
  console.error("Usage: node scripts/local/task-send.mjs --to <agent> --directive <text> [--priority low|normal|high|urgent] [--context-ref <path>]");
  process.exit(1);
}

const { agentName } = getAgentIdentity();
const taskId = crypto.randomUUID();

const task = {
  task_id:                taskId,
  from:                   agentName,
  to,
  directive,
  context_checkpoint_ref: contextRef,
  priority,
  created_at:             nowIso(),
  status:                 "pending",
  result:                 null
};

const filePath = path.join(chorusRoot, "tasks", "inbox", to, `${taskId}.json`);
writeJson(filePath, task);
writeEvent("task_sent", { task_id: taskId, to, priority });

console.log(JSON.stringify(task, null, 2));
