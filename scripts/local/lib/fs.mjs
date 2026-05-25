import fs from "node:fs";
import path from "node:path";

/**
 * Project root for chorus local state.
 * Priority: CHORUS_PROJECT_ROOT env var → process.cwd()
 *
 * CHORUS_PROJECT_ROOT is set by relay.mjs when it spawns agents
 * non-interactively so scripts always resolve to the right project
 * regardless of where the relay itself was launched from.
 */
export function getProjectRoot() {
  return process.env.CHORUS_PROJECT_ROOT || process.cwd();
}

export function getChorusRoot(projectRoot) {
  return path.join(projectRoot ?? getProjectRoot(), ".agents", "chorus");
}

// Convenience export for scripts that run within a single project context.
// If CHORUS_PROJECT_ROOT is set this resolves correctly even when the script
// is invoked from a different working directory (e.g. by the relay daemon).
export const chorusRoot = getChorusRoot();

export function ensureDir(dirPath) {
  fs.mkdirSync(dirPath, { recursive: true });
}

export function ensureJsonFile(filePath, fallback) {
  ensureDir(path.dirname(filePath));
  if (!fs.existsSync(filePath)) {
    fs.writeFileSync(filePath, JSON.stringify(fallback, null, 2) + "\n", "utf8");
  }
}

export function readJson(filePath, fallback) {
  if (!fs.existsSync(filePath)) return fallback;
  try {
    return JSON.parse(fs.readFileSync(filePath, "utf8"));
  } catch {
    return fallback;
  }
}

export function writeJson(filePath, value) {
  ensureDir(path.dirname(filePath));
  fs.writeFileSync(filePath, JSON.stringify(value, null, 2) + "\n", "utf8");
}

export function appendJsonLine(filePath, value) {
  ensureDir(path.dirname(filePath));
  fs.appendFileSync(filePath, JSON.stringify(value) + "\n", "utf8");
}
