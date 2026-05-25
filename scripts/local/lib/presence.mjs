import path from "node:path";
import { chorusRoot, writeJson } from "./fs.mjs";
import { getAgentIdentity, nowIso } from "./identity.mjs";
import { writeEvent } from "./events.mjs";

export function writePresence(partial = {}) {
  const { agentId, agentName, sessionId } = getAgentIdentity();
  const filePath = path.join(chorusRoot, "presence", `${agentId}.json`);
  const current = {
    agent_id: agentId,
    agent_name: agentName,
    role: partial.role ?? null,
    status: partial.status ?? "working",
    task_summary: partial.task_summary ?? null,
    claimed_resources: partial.claimed_resources ?? [],
    touched_files: partial.touched_files ?? [],
    started_at: partial.started_at ?? nowIso(),
    last_heartbeat_at: nowIso(),
    workspace: process.env.CHORUS_PROJECT_ROOT || process.cwd(),
    session_id: sessionId
  };
  writeJson(filePath, current);
  writeEvent("heartbeat", { status: current.status, task_summary: current.task_summary });
  return current;
}
