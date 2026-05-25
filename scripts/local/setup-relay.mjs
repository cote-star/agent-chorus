/**
 * Installs the chorus relay daemon as a Windows Task Scheduler task.
 * Runs once at user login, no window, restarts on failure.
 *
 * Usage:
 *   node scripts/local/setup-relay.mjs           # install
 *   node scripts/local/setup-relay.mjs --remove  # uninstall
 *   node scripts/local/setup-relay.mjs --status  # check
 */

import { execSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const TASK_NAME     = "ChorusRelayDaemon";
const RELAY_SCRIPT  = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "relay.mjs");
const CONFIG_PATH   = path.join(os.homedir(), ".agent-chorus", "relay-config.json");
const LOG_PATH      = path.join(os.homedir(), ".agent-chorus", "relay.log");

const arg = process.argv[2];

if (arg === "--remove") {
  remove();
} else if (arg === "--status") {
  status();
} else {
  install();
}

function install() {
  ensureConfig();

  // Find node.exe on PATH
  const nodeExe = which("node");
  if (!nodeExe) {
    console.error("node not found on PATH. Install Node.js first.");
    process.exit(1);
  }

  // Wrap in cmd /c so we get stdout/stderr redirection into the log file.
  // No /rl HIGHEST — that requires elevation and the relay doesn't need it.
  const tr = `cmd /c ""${nodeExe}" "${RELAY_SCRIPT}" >> "${LOG_PATH}" 2>&1"`;
  const cmd = [
    "schtasks", "/create",
    "/tn",  `"${TASK_NAME}"`,
    "/tr",  `"${tr}"`,
    "/sc",  "ONLOGON",
    "/f",
    "/it",
  ].join(" ");

  try {
    execSync(cmd, { stdio: "pipe", shell: true });
    console.log(`✓ Task "${TASK_NAME}" installed — starts at next login.`);
    console.log(`  Log: ${LOG_PATH}`);
    console.log(`  Config: ${CONFIG_PATH}`);
    console.log("");
    console.log("To start immediately without logging out:");
    console.log(`  schtasks /run /tn ${TASK_NAME}`);
  } catch (err) {
    console.error("schtasks failed:", err.stderr?.toString() || err.message);
    console.error("You may need to run this script as Administrator.");
    process.exit(1);
  }
}

function remove() {
  try {
    execSync(`schtasks /delete /tn ${TASK_NAME} /f`, { stdio: "pipe", shell: true });
    console.log(`✓ Task "${TASK_NAME}" removed.`);
  } catch (err) {
    console.error("Remove failed:", err.stderr?.toString() || err.message);
  }
}

function status() {
  try {
    const out = execSync(`schtasks /query /tn ${TASK_NAME} /fo LIST`, { encoding: "utf8", shell: true });
    console.log(out);
  } catch {
    console.log(`Task "${TASK_NAME}" not installed.`);
  }
}

function ensureConfig() {
  fs.mkdirSync(path.dirname(CONFIG_PATH), { recursive: true });
  if (!fs.existsSync(CONFIG_PATH)) {
    fs.writeFileSync(CONFIG_PATH, JSON.stringify({
      version: 1,
      projects: []
    }, null, 2) + "\n", "utf8");
    console.log(`Created empty config at ${CONFIG_PATH}`);
    console.log(`Add project paths before starting:`);
    console.log(`  { "projects": [{ "name": "my-project", "path": "C:\\\\path\\\\to\\\\project" }] }`);
  }
}

function which(cmd) {
  try {
    const result = spawnSync("where", [cmd], { encoding: "utf8", shell: true });
    return result.stdout.trim().split("\n")[0].trim() || null;
  } catch {
    return null;
  }
}
