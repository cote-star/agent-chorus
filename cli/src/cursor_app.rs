//! Cursor IDE (app) adapter — reads sessions stored as SQLite databases.
//!
//! v0.15.0 shipped the JSONL adapter for the `cursor-agent` CLI
//! (`~/.cursor/projects/<project>/agent-transcripts/<session>/*.jsonl`).
//! The Cursor IDE itself writes sessions to a different location with a
//! different format:
//!
//!   `~/.cursor/chats/<dir-hash>/<session-uuid>/store.db`   (SQLite)
//!
//! Each `store.db` contains two tables:
//! - `meta(key TEXT PRIMARY KEY, value TEXT)`: one row whose `value` is
//!   hex-encoded JSON with `agentId`, `latestRootBlobId`, `name`, `mode`,
//!   `createdAt` (epoch ms).
//! - `blobs(id TEXT PRIMARY KEY, data BLOB)`: content-addressed by SHA-256.
//!   The root blob (`latestRootBlobId`) is a protobuf-style envelope with
//!   one or more repeated `bytes` fields whose values are the SHA-256 ids
//!   of the message blobs, in order. Each message blob is JSON of the
//!   shape `{"role": "user|assistant|system", "content": "..."|[...]}` —
//!   identical to Claude's message shape, so the existing claude content
//!   extractor renders tool_use / tool_result segments at parity.
//!
//! The workspace cwd for an IDE session is recovered from a header line
//! embedded in the first user-role message (`Workspace Path: <path>`).
//! Future-proof: if that line is missing we return `None` and the adapter
//! falls back to the latest-session-without-cwd-match warning path
//! mirroring the JSONL adapter.

use anyhow::{anyhow, Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::agents::ConversationTurn;
use crate::agents::extract_claude_content_with_tool_calls;

/// One Cursor IDE session as enumerated from the chats root.
///
/// `name`, `mode`, `created_at_ms` are collected from the `meta` table
/// but not yet surfaced in any chorus output. They're reserved for a
/// future `chorus list --verbose` (or similar) that exposes IDE-side
/// metadata; collecting them now means no schema change is required
/// when that lands.
#[derive(Debug, Clone)]
pub struct CursorAppSession {
    pub agent_id: String,
    pub db_path: PathBuf,
    #[allow(dead_code)]
    pub name: Option<String>,
    #[allow(dead_code)]
    pub mode: Option<String>,
    #[allow(dead_code)]
    pub created_at_ms: Option<i64>,
}

/// `~/.cursor/chats` by default; override via `CHORUS_CURSOR_APP_DATA_DIR`
/// or `BRIDGE_CURSOR_APP_DATA_DIR` (the bridge fallback is preserved for
/// backward compatibility with the legacy environment variable convention).
pub fn cursor_app_base_dir() -> PathBuf {
    if let Ok(v) = std::env::var("CHORUS_CURSOR_APP_DATA_DIR") {
        return expand_home(&v);
    }
    if let Ok(v) = std::env::var("BRIDGE_CURSOR_APP_DATA_DIR") {
        return expand_home(&v);
    }
    dirs::home_dir()
        .map(|h| h.join(".cursor").join("chats"))
        .unwrap_or_else(|| PathBuf::from("~/.cursor/chats"))
}

fn expand_home(p: &str) -> PathBuf {
    if let Some(stripped) = p.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }
    PathBuf::from(p)
}

/// Walk `<base>/<dir-hash>/<session-uuid>/store.db` and return one entry
/// per discoverable session, newest mtime first.
pub fn collect_cursor_app_sessions(base: &Path) -> Vec<CursorAppSession> {
    let mut out = Vec::new();
    let hash_iter = match std::fs::read_dir(base) {
        Ok(it) => it,
        Err(_) => return out,
    };
    for hash_entry in hash_iter.flatten() {
        let hash_dir = hash_entry.path();
        if !hash_dir.is_dir() {
            continue;
        }
        let uuid_iter = match std::fs::read_dir(&hash_dir) {
            Ok(it) => it,
            Err(_) => continue,
        };
        for uuid_entry in uuid_iter.flatten() {
            let uuid_dir = uuid_entry.path();
            let db_path = uuid_dir.join("store.db");
            if !db_path.is_file() {
                continue;
            }
            if let Some(session) = read_session_meta(&db_path) {
                out.push(session);
            }
        }
    }
    out.sort_by(|a, b| {
        let am = mtime_secs(&a.db_path);
        let bm = mtime_secs(&b.db_path);
        bm.cmp(&am)
    });
    out
}

fn mtime_secs(p: &Path) -> u64 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .map(|t| t.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0))
        .unwrap_or(0)
}

fn open_ro(db_path: &Path) -> Result<Connection> {
    Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .with_context(|| format!("opening Cursor IDE store.db: {}", db_path.display()))
}

fn read_session_meta(db_path: &Path) -> Option<CursorAppSession> {
    let conn = open_ro(db_path).ok()?;
    let value: String = conn
        .query_row("SELECT value FROM meta LIMIT 1", [], |row| row.get(0))
        .ok()?;
    let bytes = hex_decode(&value).ok()?;
    let json: Value = serde_json::from_slice(&bytes).ok()?;
    let agent_id = json.get("agentId").and_then(|v| v.as_str())?.to_string();
    Some(CursorAppSession {
        agent_id,
        db_path: db_path.to_path_buf(),
        name: json.get("name").and_then(|v| v.as_str()).map(|s| s.to_string()),
        mode: json.get("mode").and_then(|v| v.as_str()).map(|s| s.to_string()),
        created_at_ms: json.get("createdAt").and_then(|v| v.as_i64()),
    })
}

/// Decode a lowercase hex string. Mirrors Node's `Buffer.from(hex, 'hex')`.
fn hex_decode(s: &str) -> Result<Vec<u8>> {
    if s.len() % 2 != 0 {
        return Err(anyhow!("hex string length is odd"));
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for i in (0..s.len()).step_by(2) {
        let byte = u8::from_str_radix(&s[i..i + 2], 16)
            .map_err(|_| anyhow!("invalid hex at position {}", i))?;
        out.push(byte);
    }
    Ok(out)
}

/// Read the ordered list of message-blob SHAs from the root blob.
///
/// The root blob is a protobuf-style stream of `(tag, length, payload)`
/// triples; we walk it greedily and accept any length-delimited (wire
/// type 2) field whose payload is exactly 32 bytes — that's the SHA-256
/// of a child blob. Other tags / payload sizes are skipped over without
/// failing, which keeps us forward-compatible with new fields the IDE
/// adds (we observed tag 0x2a appearing after the main chain in some
/// sessions; it does not point at message blobs).
fn parse_root_blob_chain(data: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < data.len() {
        // Read varint tag.
        let (tag, tag_len) = match read_varint(&data[i..]) {
            Some(v) => v,
            None => break,
        };
        i += tag_len;
        let wire_type = (tag & 0x07) as u8;
        match wire_type {
            2 => {
                // length-delimited
                let (len, len_len) = match read_varint(&data[i..]) {
                    Some(v) => v,
                    None => break,
                };
                i += len_len;
                let payload_len = len as usize;
                if i + payload_len > data.len() {
                    break;
                }
                if payload_len == 32 {
                    let hash = hex_encode(&data[i..i + payload_len]);
                    out.push(hash);
                }
                i += payload_len;
            }
            0 => {
                // varint
                match read_varint(&data[i..]) {
                    Some((_, n)) => i += n,
                    None => break,
                }
            }
            1 => i += 8,  // 64-bit fixed
            5 => i += 4,  // 32-bit fixed
            _ => break,    // unknown wire type
        }
    }
    out
}

fn read_varint(b: &[u8]) -> Option<(u64, usize)> {
    let mut result: u64 = 0;
    let mut shift = 0u32;
    for (i, byte) in b.iter().enumerate() {
        if i >= 10 {
            return None;
        }
        result |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return Some((result, i + 1));
        }
        shift += 7;
    }
    None
}

fn hex_encode(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for byte in b {
        s.push_str(&format!("{:02x}", byte));
    }
    s
}

/// Read all conversation turns from a Cursor IDE store.db, in order.
///
/// When `include_tool_calls` is false we return only the text segments of
/// each message; when true, tool_use / tool_result segments are rendered
/// via the shared claude extractor (cursor's content shape matches claude).
pub fn read_cursor_app_turns(db_path: &Path, include_tool_calls: bool) -> Vec<ConversationTurn> {
    let conn = match open_ro(db_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let meta_value: String = match conn.query_row("SELECT value FROM meta LIMIT 1", [], |row| row.get(0)) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let meta_bytes = match hex_decode(&meta_value) {
        Ok(b) => b,
        Err(_) => return Vec::new(),
    };
    let meta_json: Value = match serde_json::from_slice(&meta_bytes) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let root_id = match meta_json.get("latestRootBlobId").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return Vec::new(),
    };

    let root_blob: Vec<u8> = match conn.query_row("SELECT data FROM blobs WHERE id = ?", [&root_id], |row| row.get(0)) {
        Ok(b) => b,
        Err(_) => return Vec::new(),
    };
    let child_ids = parse_root_blob_chain(&root_blob);

    let mut turns = Vec::new();
    for child_id in child_ids {
        let data: Vec<u8> = match conn.query_row("SELECT data FROM blobs WHERE id = ?", [&child_id], |row| row.get(0)) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let v: Value = match serde_json::from_slice(&data) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let role = match v.get("role").and_then(|r| r.as_str()) {
            Some(r) if r == "user" || r == "assistant" => r.to_string(),
            _ => continue,
        };
        let content = v.get("content").cloned().unwrap_or(Value::Null);
        let text = if include_tool_calls {
            extract_claude_content_with_tool_calls(&content)
        } else {
            extract_text_only(&content)
        };
        let text = text.trim().to_string();
        if text.is_empty() {
            continue;
        }
        turns.push(ConversationTurn { role, text });
    }
    turns
}

/// Extract only text segments from a Cursor IDE message content value.
/// Content is either a plain string or an array of `{type, text|...}` segs.
fn extract_text_only(content: &Value) -> String {
    if let Some(s) = content.as_str() {
        return s.to_string();
    }
    if let Some(arr) = content.as_array() {
        let mut parts = Vec::new();
        for seg in arr {
            if let Some(seg_type) = seg.get("type").and_then(|t| t.as_str()) {
                if seg_type == "text" {
                    if let Some(t) = seg.get("text").and_then(|t| t.as_str()) {
                        parts.push(t.to_string());
                    }
                }
            }
        }
        return parts.join("\n");
    }
    String::new()
}

/// Recover the workspace path for a Cursor IDE session by scanning the
/// first user-role message for the `Workspace Path: <path>` header that
/// the IDE injects. Returns `None` if not discoverable — caller falls
/// back to the no-cwd-match path of the JSONL adapter.
pub fn cursor_app_session_workspace(db_path: &Path) -> Option<PathBuf> {
    let conn = open_ro(db_path).ok()?;
    let meta_value: String = conn.query_row("SELECT value FROM meta LIMIT 1", [], |row| row.get(0)).ok()?;
    let meta_bytes = hex_decode(&meta_value).ok()?;
    let meta_json: Value = serde_json::from_slice(&meta_bytes).ok()?;
    let root_id = meta_json.get("latestRootBlobId").and_then(|v| v.as_str())?.to_string();

    let root_blob: Vec<u8> = conn.query_row("SELECT data FROM blobs WHERE id = ?", [&root_id], |row| row.get(0)).ok()?;
    let child_ids = parse_root_blob_chain(&root_blob);

    for child_id in child_ids {
        let data: Vec<u8> = match conn.query_row("SELECT data FROM blobs WHERE id = ?", [&child_id], |row| row.get(0)) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let v: Value = match serde_json::from_slice(&data) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let role = v.get("role").and_then(|r| r.as_str()).unwrap_or("");
        if role != "user" {
            continue;
        }
        let text = extract_text_only(&v.get("content").cloned().unwrap_or(Value::Null));
        if let Some(line) = text.lines().find(|l| l.trim_start().starts_with("Workspace Path:")) {
            let value = line.splitn(2, ':').nth(1)?.trim();
            if !value.is_empty() {
                return Some(PathBuf::from(value));
            }
        }
    }
    None
}

/// Convenience: return the path to a session's store.db given the chats
/// base directory and a session id (the UUID). Reserved for future
/// id-targeted reads that don't want to walk every meta entry.
#[allow(dead_code)]
pub fn find_session_db(base: &Path, id: &str) -> Option<PathBuf> {
    let hash_iter = std::fs::read_dir(base).ok()?;
    for hash_entry in hash_iter.flatten() {
        let candidate = hash_entry.path().join(id).join("store.db");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// ISO-8601 modified timestamp for the session's store.db.
pub fn cursor_app_modified_iso(db_path: &Path) -> Option<String> {
    crate::agents::file_modified_iso(db_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_basic() {
        assert_eq!(read_varint(&[0x00]), Some((0u64, 1)));
        assert_eq!(read_varint(&[0x05]), Some((5u64, 1)));
        // tag 0x0a = field 1, wire type 2 (length-delimited)
        assert_eq!(read_varint(&[0x0a]), Some((0x0au64, 1)));
        // 300 = 0xac 0x02 (varint)
        assert_eq!(read_varint(&[0xac, 0x02]), Some((300u64, 2)));
    }

    #[test]
    fn hex_roundtrip() {
        let bytes: Vec<u8> = (0..32).collect();
        let s = hex_encode(&bytes);
        assert_eq!(s.len(), 64);
        let back = hex_decode(&s).unwrap();
        assert_eq!(back, bytes);
    }

    #[test]
    fn root_blob_skips_unknown_tags() {
        // Build: tag=0x0a (len-delim, field 1), len=32, 32 bytes of 0x01
        // then tag=0x2a (len-delim, field 5), len=4, payload [0x00..0x03]
        let mut buf: Vec<u8> = Vec::new();
        buf.push(0x0a);
        buf.push(32);
        buf.extend([0x01u8; 32]);
        buf.push(0x2a);
        buf.push(4);
        buf.extend([0x00u8, 0x01, 0x02, 0x03]);

        let chain = parse_root_blob_chain(&buf);
        // Only the 32-byte payload should be picked up.
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0], hex_encode(&[0x01u8; 32]));
    }
}
