import fs from "node:fs";
import path from "node:path";
import { chorusRoot, readJson } from "./fs.mjs";

export function buildStatus() {
  const presenceDir = path.join(chorusRoot, "presence");
  const claimsDir = path.join(chorusRoot, "claims");
  const runtimePath = path.join(chorusRoot, "runtime", "dev-servers.json");

  const presence = fs.existsSync(presenceDir)
    ? fs.readdirSync(presenceDir).filter((f) => f.endsWith(".json"))
        .map((f) => readJson(path.join(presenceDir, f), null)).filter(Boolean)
    : [];

  const claims = fs.existsSync(claimsDir)
    ? fs.readdirSync(claimsDir).filter((f) => f.endsWith(".json"))
        .map((f) => readJson(path.join(claimsDir, f), null)).filter(Boolean)
    : [];

  const runtime = readJson(runtimePath, { version: 1, servers: [] });

  return { presence, claims, runtime };
}
