import path from "node:path";
import { chorusRoot, readJson, writeJson } from "./fs.mjs";
import { getAgentIdentity, nowIso } from "./identity.mjs";
import { writeEvent } from "./events.mjs";

function normalizeResource(resourceId) {
  return resourceId.replace(/[^a-zA-Z0-9._-]+/g, "-").toLowerCase();
}

export function claimFilePath(resourceId) {
  return path.join(chorusRoot, "claims", `${normalizeResource(resourceId)}.json`);
}

export function acquireClaim({ resourceType, resourceId, reason, mode = "exclusive", ttlSeconds = 1200 }) {
  const { agentId, sessionId } = getAgentIdentity();
  const filePath = claimFilePath(resourceId);
  const existing = readJson(filePath, null);
  const createdAt = nowIso();
  const expiresAt = new Date(Date.now() + ttlSeconds * 1000).toISOString();

  if (existing && existing.owner_agent_id !== agentId) {
    throw new Error(`Resource already claimed: ${resourceId} by ${existing.owner_agent_id}`);
  }

  const nextClaim = {
    resource_type: resourceType,
    resource_id: resourceId,
    owner_agent_id: agentId,
    owner_session_id: sessionId,
    reason,
    mode,
    created_at: createdAt,
    expires_at: expiresAt
  };

  writeJson(filePath, nextClaim);
  writeEvent("claim_acquired", { resource_id: resourceId, mode, reason });
  return nextClaim;
}

export function releaseClaim(resourceId) {
  const filePath = claimFilePath(resourceId);
  writeJson(filePath, {
    released: true,
    resource_id: resourceId,
    released_at: nowIso()
  });
  writeEvent("claim_released", { resource_id: resourceId });
}
