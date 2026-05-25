import os from "node:os";
import crypto from "node:crypto";

export function nowIso() {
  return new Date().toISOString();
}

export function getAgentIdentity() {
  const agentName =
    process.env.CHORUS_AGENT_NAME ||
    process.env.CODEX_AGENT_NAME ||
    process.env.CLAUDE_AGENT_NAME ||
    process.env.GEMINI_AGENT_NAME ||
    "unknown-agent";

  const agentId =
    process.env.CHORUS_AGENT_ID ||
    `${agentName}-${os.hostname().toLowerCase()}`;

  const sessionId =
    process.env.CHORUS_SESSION_ID ||
    crypto.randomUUID();

  return { agentName, agentId, sessionId };
}
