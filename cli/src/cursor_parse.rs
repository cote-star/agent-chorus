//! Cursor transcript parsing.
//!
//! Flattens cursor-agent JSONL transcript lines `{"role","message":{"content":[...]}}`
//! into ordered (role, text) turns, keeping only `type=="text"` segments.
//!
//! FULL SPEC: docs/adapters/CURSOR_HERMES_NATIVE_ADAPTER.md  §6 Unit B.
//! Implementer: fill the function bodies + add the required `#[cfg(test)]` tests.
//! Do NOT change these signatures and do NOT edit any other file.
#![allow(dead_code)]

use serde_json::Value;
use std::path::Path;

/// One conversation turn extracted from a Cursor transcript.
pub(crate) struct CursorTurn {
    pub role: String,
    pub text: String,
}

/// Flatten a Cursor transcript line's `message` value into plain text.
/// - object with "content": [ {type:"text", text}, ... ] -> concat the "text" of
///   segments whose "type" == "text", in order (ignore tool_use / other types).
/// - object with "content": "<string>" -> that string.
/// - string -> the string as-is.
/// - anything else -> "".
pub(crate) fn flatten_cursor_message(message: &Value) -> String {
    let _ = message;
    todo!("Unit B: implement per docs/adapters/CURSOR_HERMES_NATIVE_ADAPTER.md §6")
}

/// Read a Cursor transcript (.jsonl) into turns. Skips: non-JSON lines, lines whose
/// role is not "user"/"assistant", and turns whose flattened text is empty (after
/// trim). Preserves order. Do NOT redact here (done downstream by the integrator).
pub(crate) fn read_cursor_turns(path: &Path) -> Vec<CursorTurn> {
    let _ = path;
    todo!("Unit B")
}
