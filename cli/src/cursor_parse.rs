//! Cursor transcript parsing.
//!
//! Flattens cursor-agent JSONL transcript lines `{"role","message":{"content":[...]}}`
//! into ordered (role, text) turns, keeping only `type=="text"` segments.
//!
//! FULL SPEC: docs/adapters/CURSOR_HERMES_NATIVE_ADAPTER.md  §6 Unit B.
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
    match message {
        Value::Object(map) => {
            let content = match map.get("content") {
                Some(c) => c,
                None => return String::new(),
            };
            match content {
                Value::Array(segments) => segments
                    .iter()
                    .filter_map(|seg| {
                        let obj = seg.as_object()?;
                        if obj.get("type")?.as_str()? != "text" {
                            return None;
                        }
                        obj.get("text")?.as_str().map(str::to_string)
                    })
                    .collect(),
                Value::String(s) => s.clone(),
                _ => String::new(),
            }
        }
        Value::String(s) => s.clone(),
        _ => String::new(),
    }
}

/// Read a Cursor transcript (.jsonl) into turns. Skips: non-JSON lines, lines whose
/// role is not "user"/"assistant", and turns whose flattened text is empty (after
/// trim). Preserves order. Do NOT redact here (done downstream by the integrator).
pub(crate) fn read_cursor_turns(path: &Path) -> Vec<CursorTurn> {
    let contents = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let mut turns = Vec::new();
    for line in contents.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let Some(role) = value.get("role").and_then(|r| r.as_str()) else {
            continue;
        };
        if role != "user" && role != "assistant" {
            continue;
        }
        let text = flatten_cursor_message(value.get("message").unwrap_or(&Value::Null));
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        turns.push(CursorTurn {
            role: role.to_string(),
            text: text.to_string(),
        });
    }
    turns
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    fn fixture(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("chorus_cursorparse_{}", name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn flatten_content_array_text_and_tool_use() {
        let message = json!({
            "content": [
                {"type": "text", "text": "first"},
                {"type": "tool_use", "name": "Read", "input": {"path": "/x"}},
                {"type": "text", "text": "second"}
            ]
        });
        assert_eq!(flatten_cursor_message(&message), "firstsecond");
    }

    #[test]
    fn flatten_content_string() {
        assert_eq!(
            flatten_cursor_message(&json!({"content": "hello"})),
            "hello"
        );
    }

    #[test]
    fn flatten_message_string() {
        assert_eq!(
            flatten_cursor_message(&Value::String("raw".into())),
            "raw"
        );
    }

    #[test]
    fn flatten_number_and_null() {
        assert_eq!(flatten_cursor_message(&json!(42)), "");
        assert_eq!(flatten_cursor_message(&json!(null)), "");
    }

    #[test]
    fn read_cursor_turns_filters_and_orders() {
        let dir = fixture("read_turns");
        let path = dir.join("transcript.jsonl");
        let user_line = r#"{"role":"user","message":{"content":[{"type":"text","text":"user says hi"}]}}"#;
        let assistant_line = r#"{"role":"assistant","message":{"content":[{"type":"text","text":"assistant reply"},{"type":"tool_use","name":"Read","input":{"path":"foo"}}]}}"#;
        let tool_line = r#"{"role":"tool","message":{"content":[{"type":"text","text":"ignored"}]}}"#;
        let invalid_line = "not json";
        let empty_assistant = r#"{"role":"assistant","message":{"content":[{"type":"tool_use","name":"Grep","input":{"pattern":"x"}}]}}"#;

        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "{}", user_line).unwrap();
        writeln!(f, "{}", assistant_line).unwrap();
        writeln!(f, "{}", tool_line).unwrap();
        writeln!(f, "{}", invalid_line).unwrap();
        writeln!(f, "{}", empty_assistant).unwrap();

        let turns = read_cursor_turns(&path);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].role, "user");
        assert_eq!(turns[0].text, "user says hi");
        assert_eq!(turns[1].role, "assistant");
        assert_eq!(turns[1].text, "assistant reply");
    }
}
