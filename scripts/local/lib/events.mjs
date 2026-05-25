import path from "node:path";
import { appendJsonLine, chorusRoot } from "./fs.mjs";
import { getAgentIdentity, nowIso } from "./identity.mjs";

export function writeEvent(type, payload = {}) {
  const { agentId, sessionId } = getAgentIdentity();
  const day = nowIso().slice(0, 10);
  const filePath = path.join(chorusRoot, "events", `${day}.jsonl`);
  appendJsonLine(filePath, {
    timestamp: nowIso(),
    agent_id: agentId,
    session_id: sessionId,
    type,
    payload
  });
}
