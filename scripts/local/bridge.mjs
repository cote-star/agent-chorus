import fs from "node:fs";
import path from "node:path";
import { chorusRoot, getProjectRoot, readJson, writeJson } from "./lib/fs.mjs";
import { buildStatus } from "./lib/status.mjs";

const projectRoot = getProjectRoot();
const agentChorusRoot = path.join(projectRoot, ".agent-chorus");
fs.mkdirSync(agentChorusRoot, { recursive: true });
fs.mkdirSync(path.join(agentChorusRoot, "providers"), { recursive: true });

const checkpointDir = path.join(chorusRoot, "checkpoints");
const checkpoints = fs.existsSync(checkpointDir)
  ? fs.readdirSync(checkpointDir)
      .filter(f => f.endsWith(".json"))
      .map(f => readJson(path.join(checkpointDir, f), null))
      .filter(Boolean)
  : [];

const status = buildStatus();

const liveContext = {
  chorus_local_version: 1,
  generated_at:         new Date().toISOString(),
  checkpoints,
  ...status
};

const liveContextPath = path.join(agentChorusRoot, "LIVE_CONTEXT.json");
writeJson(liveContextPath, liveContext);

console.log("Bridge synced →", liveContextPath);
