use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::adapters;
use crate::agents;

/// Summary output matching the Node implementation.
pub struct SummaryResult {
    pub agent: String,
    pub session_id: String,
    pub cwd: String,
    pub source: String,
    pub message_count: usize,
    pub duration_estimate: Option<String>,
    pub user_requests: Vec<String>,
    pub files_referenced: Vec<String>,
    pub tool_calls_by_type: BTreeMap<String, usize>,
    pub last_response_snippet: Option<String>,
    pub warnings: Vec<String>,
}

impl SummaryResult {
    pub fn to_json(&self) -> Value {
        json!({
            "chorus_output_version": 1,
            "agent": self.agent,
            "session_id": self.session_id,
            "cwd": self.cwd,
            "source": self.source,
            "message_count": self.message_count,
            "duration_estimate": self.duration_estimate,
            "user_requests": self.user_requests,
            "files_referenced": self.files_referenced,
            "tool_calls_by_type": self.tool_calls_by_type,
            "last_response_snippet": self.last_response_snippet,
            "warnings": self.warnings,
        })
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("Session: {}\n", self.session_id));
        let dur = self
            .duration_estimate
            .as_deref()
            .map(|d| format!(" | Duration: {}", d))
            .unwrap_or_default();
        out.push_str(&format!(
            "Agent: {} | Messages: {}{}\n",
            self.agent, self.message_count, dur
        ));
        out.push_str(&format!(
            "CWD: {}\n",
            if self.cwd.is_empty() {
                "(unknown)"
            } else {
                &self.cwd
            }
        ));

        if !self.user_requests.is_empty() {
            out.push_str("\nUser requests:\n");
            for req in &self.user_requests {
                out.push_str(&format!("  - {}\n", req));
            }
        }
        if !self.tool_calls_by_type.is_empty() {
            out.push_str("\nTool calls:\n");
            let mut sorted: Vec<_> = self.tool_calls_by_type.iter().collect();
            sorted.sort_by(|a, b| b.1.cmp(a.1));
            for (name, count) in sorted {
                out.push_str(&format!("  {}: {}\n", name, count));
            }
        }
        if !self.files_referenced.is_empty() {
            out.push_str("\nFiles referenced:\n");
            for f in self.files_referenced.iter().take(20) {
                out.push_str(&format!("  {}\n", f));
            }
            if self.files_referenced.len() > 20 {
                out.push_str(&format!(
                    "  ... and {} more\n",
                    self.files_referenced.len() - 20
                ));
            }
        }
        if let Some(ref snippet) = self.last_response_snippet {
            out.push_str(&format!("\nLast response: {}\n", snippet));
        }
        out
    }

    pub fn to_markdown(&self) -> String {
        let label = capitalize(&self.agent);
        let mut lines = Vec::new();
        lines.push(format!("## {} Session Summary", label));
        lines.push(String::new());
        lines.push("| Field | Value |".to_string());
        lines.push("|---|---|".to_string());
        lines.push(format!("| Session | `{}` |", self.session_id));
        lines.push(format!(
            "| CWD | `{}` |",
            if self.cwd.is_empty() {
                "(unknown)"
            } else {
                &self.cwd
            }
        ));
        lines.push(format!("| Messages | {} |", self.message_count));
        if let Some(ref d) = self.duration_estimate {
            lines.push(format!("| Duration | {} |", d));
        }
        lines.push(String::new());

        if !self.user_requests.is_empty() {
            lines.push("### User Requests".to_string());
            for req in &self.user_requests {
                lines.push(format!("- {}", req.replace('\n', " ")));
            }
            lines.push(String::new());
        }
        if !self.tool_calls_by_type.is_empty() {
            lines.push("### Tool Calls".to_string());
            lines.push("| Tool | Count |".to_string());
            lines.push("|---|---|".to_string());
            let mut sorted: Vec<_> = self.tool_calls_by_type.iter().collect();
            sorted.sort_by(|a, b| b.1.cmp(a.1));
            for (name, count) in sorted {
                lines.push(format!("| {} | {} |", name, count));
            }
            lines.push(String::new());
        }
        if !self.files_referenced.is_empty() {
            lines.push("### Files Referenced".to_string());
            for f in self.files_referenced.iter().take(20) {
                lines.push(format!("- `{}`", f));
            }
            if self.files_referenced.len() > 20 {
                lines.push(format!(
                    "- *... and {} more*",
                    self.files_referenced.len() - 20
                ));
            }
            lines.push(String::new());
        }
        if let Some(ref snippet) = self.last_response_snippet {
            lines.push("### Last Response".to_string());
            lines.push(format!("> {}", snippet.replace('\n', "\n> ")));
        }
        lines.join("\n")
    }
}

/// Build a summary by reading the raw JSONL/JSON session file.
pub fn build_summary(
    agent: &str,
    id: Option<&str>,
    cwd: &str,
    chats_dir: Option<&str>,
) -> Result<SummaryResult> {
    let adapter = adapters::get_adapter(agent)
        .with_context(|| format!("Unsupported agent: {}", agent))?;

    // Use the adapter to resolve + read the session (this gives us the file path and warnings)
    let session = adapter.read_session(id, cwd, chats_dir, 1)?;
    let source_path = session.source.clone();
    let session_warnings = session.warnings.clone();

    // Now parse the raw file for summary extraction
    let path = Path::new(&source_path);
    let lines = agents::read_jsonl_lines(path).unwrap_or_default();

    let mut user_requests: Vec<String> = Vec::new();
    let mut tool_call_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut file_paths: BTreeSet<String> = BTreeSet::new();
    let mut assistant_count = 0usize;
    let mut last_assistant_text = String::new();
    let mut session_cwd: Option<String> = None;
    let mut first_timestamp: Option<String> = None;
    let mut last_timestamp: Option<String> = None;

    for line in &lines {
        let json: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Extract timestamps
        let ts = json
            .get("timestamp")
            .or_else(|| json.get("created_at"))
            .and_then(|v| {
                if let Some(s) = v.as_str() {
                    Some(s.to_string())
                } else if let Some(n) = v.as_f64() {
                    // Unix timestamp
                    let secs = n as u64;
                    Some(format!("{}Z", secs))
                } else {
                    None
                }
            });
        if let Some(ref t) = ts {
            if first_timestamp.is_none() {
                first_timestamp = Some(t.clone());
            }
            last_timestamp = Some(t.clone());
        }

        // CWD extraction
        if session_cwd.is_none() {
            if let Some(c) = json.get("cwd").and_then(|v| v.as_str()) {
                session_cwd = Some(c.to_string());
            }
            if let Some(c) = json
                .pointer("/payload/cwd")
                .or_else(|| json.pointer("/type"))
                .and_then(|_| json.pointer("/payload/cwd"))
                .and_then(|v| v.as_str())
            {
                session_cwd = Some(c.to_string());
            }
        }

        // Claude-format messages
        let message = json.get("message").unwrap_or(&json);
        let role = message
            .get("role")
            .or_else(|| json.get("type"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();

        if role == "user" || role == "human" {
            let content = message
                .get("content")
                .or_else(|| json.get("content"))
                .cloned()
                .unwrap_or(Value::Null);
            let text = agents::extract_claude_text(&content);
            let text = if text.is_empty() {
                agents::extract_text(&content)
            } else {
                text
            };
            if !text.is_empty() && user_requests.len() < 5 {
                let truncated: String = text.chars().take(150).collect();
                user_requests.push(truncated);
            }
        }

        if role == "assistant" {
            let content = message
                .get("content")
                .or_else(|| json.get("content"))
                .cloned()
                .unwrap_or(Value::Null);
            let text = agents::extract_claude_text(&content);
            if !text.is_empty() {
                assistant_count += 1;
                last_assistant_text = text;
            }
            // Extract tool calls from content array
            if let Some(arr) = content.as_array() {
                extract_tool_call_summary(arr, &mut tool_call_counts);
                extract_file_paths_from_content(arr, &mut file_paths);
            }
        }

        // Codex-format: response_item with payload.type == "message"
        if json.get("type").and_then(|v| v.as_str()) == Some("response_item") {
            if let Some(payload) = json.get("payload") {
                if payload.get("type").and_then(|v| v.as_str()) == Some("message") {
                    let payload_role = payload
                        .get("role")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_lowercase();
                    if payload_role == "user" {
                        let text = payload
                            .get("content")
                            .map(agents::extract_text)
                            .unwrap_or_default();
                        if !text.is_empty() && user_requests.len() < 5 {
                            let truncated: String = text.chars().take(150).collect();
                            user_requests.push(truncated);
                        }
                    }
                    if payload_role == "assistant" {
                        let text = payload
                            .get("content")
                            .map(agents::extract_text)
                            .unwrap_or_default();
                        if !text.is_empty() {
                            assistant_count += 1;
                            last_assistant_text = text;
                        }
                    }
                }
            }
        }
    }

    // Duration estimate
    let duration_estimate = compute_duration(&first_timestamp, &last_timestamp);

    // Session ID from filename
    let session_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();

    let snippet = if last_assistant_text.is_empty() {
        None
    } else {
        let s: String = last_assistant_text.chars().take(300).collect();
        Some(agents::redact_sensitive_text(&s))
    };

    Ok(SummaryResult {
        agent: agent.to_string(),
        session_id,
        cwd: session_cwd
            .unwrap_or_else(|| cwd.to_string()),
        source: source_path,
        message_count: assistant_count,
        duration_estimate,
        user_requests,
        files_referenced: file_paths.into_iter().collect(),
        tool_calls_by_type: tool_call_counts,
        last_response_snippet: snippet,
        warnings: session_warnings,
    })
}

/// Extract tool call counts from a Claude-style content array.
fn extract_tool_call_summary(content: &[Value], counts: &mut BTreeMap<String, usize>) {
    for block in content {
        let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if block_type == "tool_use" {
            let name = block
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            *counts.entry(name.to_string()).or_insert(0) += 1;
        }
        // Codex function_call format
        if block_type == "function_call" {
            let name = block
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            *counts.entry(name.to_string()).or_insert(0) += 1;
        }
    }
}

/// Extract file paths from tool_use blocks in a content array.
fn extract_file_paths_from_content(content: &[Value], paths: &mut BTreeSet<String>) {
    for block in content {
        let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if block_type == "tool_use" || block_type == "function_call" {
            if let Some(input) = block.get("input").or_else(|| block.get("arguments")) {
                // Look for common file path fields
                for key in &["file_path", "path", "filePath", "file", "filename"] {
                    if let Some(p) = input.get(*key).and_then(|v| v.as_str()) {
                        if !p.is_empty() {
                            paths.insert(p.to_string());
                        }
                    }
                }
                // Handle string arguments that might be JSON
                if let Some(args_str) = input.as_str() {
                    if let Ok(args_json) = serde_json::from_str::<Value>(args_str) {
                        for key in &["file_path", "path", "filePath", "file", "filename"] {
                            if let Some(p) = args_json.get(*key).and_then(|v| v.as_str()) {
                                if !p.is_empty() {
                                    paths.insert(p.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn compute_duration(first: &Option<String>, last: &Option<String>) -> Option<String> {
    let f = first.as_ref()?;
    let l = last.as_ref()?;

    // Try parsing ISO 8601 timestamps
    let parse_ts = |s: &str| -> Option<i64> {
        // Handle "YYYY-MM-DDThh:mm:ss.sssZ" or similar
        let parts: Vec<&str> = s.splitn(2, 'T').collect();
        if parts.len() < 2 {
            // Maybe it's a pure unix timestamp
            return s.trim_end_matches('Z').parse::<i64>().ok();
        }
        let date_parts: Vec<&str> = parts[0].split('-').collect();
        let time_str = parts[1].trim_end_matches('Z');
        let time_str = time_str.split('.').next().unwrap_or("");
        let time_parts: Vec<&str> = time_str.split(':').collect();

        if date_parts.len() < 3 || time_parts.len() < 3 {
            return None;
        }

        let year: i64 = date_parts[0].parse().ok()?;
        let month: i64 = date_parts[1].parse().ok()?;
        let day: i64 = date_parts[2].parse().ok()?;
        let hour: i64 = time_parts[0].parse().ok()?;
        let minute: i64 = time_parts[1].parse().ok()?;
        let second: i64 = time_parts[2].parse().ok()?;

        // Rough epoch seconds (good enough for duration diffs)
        Some(
            ((year - 1970) * 365 * 86400)
                + (month * 30 * 86400)
                + (day * 86400)
                + (hour * 3600)
                + (minute * 60)
                + second,
        )
    };

    let f_secs = parse_ts(f)?;
    let l_secs = parse_ts(l)?;
    let diff = l_secs - f_secs;
    if diff <= 0 {
        return None;
    }
    let mins = diff / 60;
    if mins < 1 {
        Some("< 1 min".to_string())
    } else {
        Some(format!("~{} min", mins))
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}
