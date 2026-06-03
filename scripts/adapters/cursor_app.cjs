/**
 * Cursor IDE (app) adapter — reads sessions stored as SQLite databases.
 *
 * Mirrors `cli/src/cursor_app.rs`. See that file's module-level doc for
 * the full format specification (meta + blobs tables, hex-encoded JSON in
 * `meta.value`, protobuf-style root blob enumerating child message SHAs,
 * claude-shaped message JSON, "Workspace Path:" header for cwd recovery).
 *
 * On-disk layout:
 *   ~/.cursor/chats/<dir-hash>/<session-uuid>/store.db    (SQLite)
 *
 * Override the base directory via `CHORUS_CURSOR_APP_DATA_DIR` or
 * `BRIDGE_CURSOR_APP_DATA_DIR` (bridge fallback for backward compat).
 *
 * SQLite access uses Node's built-in `node:sqlite` (Node >= 22.5). On
 * older Node, this module gracefully returns no sessions and the rest of
 * the cursor adapter falls back to JSONL-only behavior — same end state
 * as a user without Cursor IDE installed. Doctor surfaces the gap via
 * the `sessions_cursor_app` check.
 */

const fs = require('fs');
const path = require('path');
const { normalizePath } = require('./utils.cjs');

// Optional dependency: Node 22.5+ ships node:sqlite as experimental.
// Older Node returns null and the app surface is invisible (graceful).
let nodeSqlite = null;
try {
  // eslint-disable-next-line global-require
  nodeSqlite = require('node:sqlite');
} catch (_err) {
  nodeSqlite = null;
}

function cursorAppBaseDir() {
  return normalizePath(
    process.env.CHORUS_CURSOR_APP_DATA_DIR
    || process.env.BRIDGE_CURSOR_APP_DATA_DIR
    || '~/.cursor/chats',
  );
}

function isSqliteAvailable() {
  return nodeSqlite !== null && typeof nodeSqlite.DatabaseSync === 'function';
}

function openDb(dbPath) {
  if (!isSqliteAvailable()) return null;
  try {
    return new nodeSqlite.DatabaseSync(dbPath, { readOnly: true });
  } catch (_err) {
    return null;
  }
}

function decodeHex(hex) {
  if (typeof hex !== 'string' || hex.length % 2 !== 0) return null;
  try {
    return Buffer.from(hex, 'hex');
  } catch (_err) {
    return null;
  }
}

function readMetaJson(db) {
  try {
    const row = db.prepare('SELECT value FROM meta LIMIT 1').get();
    if (!row || typeof row.value !== 'string') return null;
    const buf = decodeHex(row.value);
    if (!buf) return null;
    return JSON.parse(buf.toString('utf8'));
  } catch (_err) {
    return null;
  }
}

function readBlob(db, id) {
  try {
    const row = db.prepare('SELECT data FROM blobs WHERE id = ?').get(id);
    if (!row || !row.data) return null;
    return Buffer.isBuffer(row.data) ? row.data : Buffer.from(row.data);
  } catch (_err) {
    return null;
  }
}

// Parse a protobuf-like length-delimited stream. Accepts any wire-type-2
// field whose payload is exactly 32 bytes (SHA-256 of a child blob);
// skips other wire types / payload sizes for forward compatibility.
function parseRootBlobChain(buf) {
  const out = [];
  let i = 0;
  while (i < buf.length) {
    const tag = readVarint(buf, i);
    if (!tag) break;
    i = tag.next;
    const wireType = tag.value & 0x07;
    if (wireType === 2) {
      const lenInfo = readVarint(buf, i);
      if (!lenInfo) break;
      i = lenInfo.next;
      const payloadLen = Number(lenInfo.value);
      if (i + payloadLen > buf.length) break;
      if (payloadLen === 32) {
        out.push(buf.slice(i, i + payloadLen).toString('hex'));
      }
      i += payloadLen;
    } else if (wireType === 0) {
      const v = readVarint(buf, i);
      if (!v) break;
      i = v.next;
    } else if (wireType === 1) {
      i += 8;
    } else if (wireType === 5) {
      i += 4;
    } else {
      break;
    }
  }
  return out;
}

function readVarint(buf, start) {
  let result = 0n;
  let shift = 0n;
  for (let i = 0; i < 10; i += 1) {
    if (start + i >= buf.length) return null;
    const byte = buf[start + i];
    result |= BigInt(byte & 0x7f) << shift;
    if ((byte & 0x80) === 0) {
      return { value: Number(result), next: start + i + 1 };
    }
    shift += 7n;
  }
  return null;
}

function extractTextOnly(content) {
  if (typeof content === 'string') return content;
  if (Array.isArray(content)) {
    const parts = [];
    for (const seg of content) {
      if (seg && seg.type === 'text' && typeof seg.text === 'string') {
        parts.push(seg.text);
      }
    }
    return parts.join('\n');
  }
  return '';
}

/**
 * Enumerate Cursor IDE sessions under the chats base. Returns one entry
 * per discoverable store.db, newest mtime first. Returns [] when Node
 * lacks node:sqlite or the base directory is absent.
 */
function collectCursorAppSessions(base = cursorAppBaseDir()) {
  if (!isSqliteAvailable() || !fs.existsSync(base)) return [];
  const out = [];
  let hashDirs;
  try {
    hashDirs = fs.readdirSync(base, { withFileTypes: true });
  } catch (_err) {
    return out;
  }
  for (const hashEntry of hashDirs) {
    if (!hashEntry.isDirectory()) continue;
    const hashDir = path.join(base, hashEntry.name);
    let uuidDirs;
    try {
      uuidDirs = fs.readdirSync(hashDir, { withFileTypes: true });
    } catch (_err) {
      continue;
    }
    for (const uuidEntry of uuidDirs) {
      if (!uuidEntry.isDirectory()) continue;
      const uuidDir = path.join(hashDir, uuidEntry.name);
      const dbPath = path.join(uuidDir, 'store.db');
      try {
        if (!fs.statSync(dbPath).isFile()) continue;
      } catch (_err) {
        continue;
      }
      const db = openDb(dbPath);
      if (!db) continue;
      const meta = readMetaJson(db);
      try { db.close(); } catch (_err) {}
      if (!meta || typeof meta.agentId !== 'string') continue;
      out.push({
        agent_id: meta.agentId,
        db_path: dbPath,
        name: meta.name || null,
        mode: meta.mode || null,
        created_at_ms: typeof meta.createdAt === 'number' ? meta.createdAt : null,
      });
    }
  }
  out.sort((a, b) => mtime(b.db_path) - mtime(a.db_path));
  return out;
}

function mtime(p) {
  try {
    return fs.statSync(p).mtime.getTime();
  } catch (_err) {
    return 0;
  }
}

/**
 * Read all conversation turns from a Cursor IDE store.db, in order.
 * Returns [{role, text}, ...]. Returns [] on any failure.
 */
function readCursorAppTurns(dbPath, includeToolCalls) {
  const db = openDb(dbPath);
  if (!db) return [];
  try {
    const meta = readMetaJson(db);
    if (!meta || typeof meta.latestRootBlobId !== 'string') return [];
    const root = readBlob(db, meta.latestRootBlobId);
    if (!root) return [];
    const childIds = parseRootBlobChain(root);
    const turns = [];

    // Reuse the shared content extractor when --tool-calls is requested
    // so cursor IDE output renders tool_use/tool_result identical to the
    // cursor-agent CLI and claude paths. Required for invariant 1 (Node/Rust
    // parity) and invariant 4 (boundary markers + version).
    const { extractContentWithToolCalls } = require('./utils.cjs');

    for (const id of childIds) {
      const data = readBlob(db, id);
      if (!data) continue;
      let v;
      try {
        v = JSON.parse(data.toString('utf8'));
      } catch (_err) {
        continue;
      }
      const role = v && v.role;
      if (role !== 'user' && role !== 'assistant') continue;
      const text = includeToolCalls
        ? extractContentWithToolCalls(v.content)
        : extractTextOnly(v.content);
      const trimmed = (text || '').trim();
      if (!trimmed) continue;
      turns.push({ role, text: trimmed });
    }
    return turns;
  } finally {
    try { db.close(); } catch (_err) {}
  }
}

/**
 * Recover the workspace path embedded in the first user-role message's
 * `Workspace Path: <path>` header. Returns null if not discoverable.
 */
function cursorAppSessionWorkspace(dbPath) {
  const turns = readCursorAppTurns(dbPath, false);
  for (const t of turns) {
    if (t.role !== 'user') continue;
    for (const line of t.text.split('\n')) {
      const trimmed = line.replace(/^\s+/, '');
      if (trimmed.startsWith('Workspace Path:')) {
        const value = trimmed.slice('Workspace Path:'.length).trim();
        if (value) return value;
      }
    }
  }
  return null;
}

function cursorAppModifiedIso(dbPath) {
  try {
    const t = fs.statSync(dbPath).mtime;
    return t.toISOString();
  } catch (_err) {
    return null;
  }
}

module.exports = {
  cursorAppBaseDir,
  isSqliteAvailable,
  collectCursorAppSessions,
  readCursorAppTurns,
  cursorAppSessionWorkspace,
  cursorAppModifiedIso,
};
