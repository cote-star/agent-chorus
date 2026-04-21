use anyhow::Result;
use serde_json::{json, Value};

use crate::adapters;
use crate::utils;

/// A single entry in the cross-agent timeline.
#[derive(Debug)]
pub struct TimelineEntry {
    pub timestamp: Option<String>,
    pub agent: String,
    pub session_id: String,
    pub cwd: Option<String>,
    pub snippet: Option<String>,
}

/// Full timeline result matching the Node implementation.
pub struct TimelineResult {
    pub timeline: Vec<TimelineEntry>,
    pub agents_included: Vec<String>,
    pub cwd: String,
    pub warnings: Vec<String>,
}

impl TimelineResult {
    pub fn to_json(&self) -> Value {
        let entries: Vec<Value> = self
            .timeline
            .iter()
            .map(|e| {
                json!({
                    "timestamp": e.timestamp,
                    "agent": e.agent,
                    "session_id": e.session_id,
                    "cwd": e.cwd,
                    "snippet": e.snippet,
                })
            })
            .collect();

        json!({
            "chorus_output_version": 1,
            "timeline": entries,
            "agents_included": self.agents_included,
            "cwd": self.cwd,
            "warnings": self.warnings,
        })
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("Timeline for {}\n", self.cwd));
        out.push_str(&format!(
            "Agents: {}\n\n",
            if self.agents_included.is_empty() {
                "(none found)".to_string()
            } else {
                self.agents_included.join(", ")
            }
        ));

        for entry in &self.timeline {
            let ts = entry
                .timestamp
                .as_deref()
                .map(|t| {
                    let short: String = t.chars().take(16).collect();
                    short.replace('T', " ")
                })
                .unwrap_or_else(|| "?".to_string());
            let snip = entry
                .snippet
                .as_deref()
                .map(|s| {
                    let short: String = s.chars().take(80).collect();
                    short.replace('\n', " ")
                })
                .unwrap_or_default();

            out.push_str(&format!(
                "{}  [{}]  {}\n",
                ts, entry.agent, entry.session_id
            ));
            if !snip.is_empty() {
                out.push_str(&format!("  {}\n", snip));
            }
        }

        if !self.warnings.is_empty() {
            out.push_str("\nWarnings:\n");
            for w in &self.warnings {
                out.push_str(&format!("  {}\n", w));
            }
        }

        out
    }

    pub fn to_markdown(&self) -> String {
        let mut lines = Vec::new();
        lines.push("## Agent Timeline".to_string());
        lines.push(String::new());
        lines.push(format!("**CWD:** `{}`", self.cwd));
        lines.push(format!(
            "**Agents:** {}",
            if self.agents_included.is_empty() {
                "(none)".to_string()
            } else {
                self.agents_included.join(", ")
            }
        ));
        lines.push(String::new());
        lines.push("| Time | Agent | Session | Snippet |".to_string());
        lines.push("|---|---|---|---|".to_string());

        for entry in &self.timeline {
            let ts = entry
                .timestamp
                .as_deref()
                .map(|t| {
                    let short: String = t.chars().take(16).collect();
                    short.replace('T', " ")
                })
                .unwrap_or_else(|| "?".to_string());
            let snip = entry
                .snippet
                .as_deref()
                .map(|s| {
                    let short: String = s.chars().take(80).collect();
                    short.replace('\n', " ").replace('|', "\\|")
                })
                .unwrap_or_default();
            let sid: String = entry.session_id.chars().take(30).collect();
            lines.push(format!(
                "| {} | {} | `{}` | {} |",
                ts, entry.agent, sid, snip
            ));
        }

        if !self.warnings.is_empty() {
            lines.push(String::new());
            lines.push("**Warnings:**".to_string());
            for w in &self.warnings {
                lines.push(format!("- {}", w));
            }
        }

        lines.join("\n")
    }
}

const ALL_AGENTS: &[&str] = &["claude", "codex", "gemini", "cursor"];

/// Build a cross-agent timeline.
pub fn build_timeline(
    agents: &[String],
    cwd: &str,
    limit_per_agent: usize,
) -> Result<TimelineResult> {
    let agent_list: Vec<&str> = if agents.is_empty() {
        ALL_AGENTS.to_vec()
    } else {
        agents.iter().map(|s| s.as_str()).collect()
    };

    let normalized_cwd = utils::normalize_path(cwd)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| cwd.to_string());

    let mut entries = Vec::new();
    let mut agents_included = Vec::new();
    let mut warnings = Vec::new();

    for agent in &agent_list {
        let adapter = match adapters::get_adapter(agent) {
            Some(a) => a,
            None => {
                warnings.push(format!("{}: unsupported agent", agent));
                continue;
            }
        };

        match adapter.list_sessions(Some(&normalized_cwd), limit_per_agent) {
            Ok(sessions) => {
                if !sessions.is_empty() {
                    agents_included.push(agent.to_string());
                }
                for session in &sessions {
                    let session_id = session
                        .get("session_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let ts = session
                        .get("modified_at")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let session_cwd = session
                        .get("cwd")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let file_path = session
                        .get("file_path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");

                    // Try to read a snippet from the session
                    let snippet = if !file_path.is_empty() {
                        read_snippet(agent, file_path)
                    } else {
                        None
                    };

                    entries.push(TimelineEntry {
                        timestamp: ts,
                        agent: agent.to_string(),
                        session_id,
                        cwd: session_cwd,
                        snippet,
                    });
                }
            }
            Err(e) => {
                warnings.push(format!("{}: {}", agent, e));
            }
        }
    }

    // Sort by timestamp descending (newest first)
    entries.sort_by(|a, b| {
        let ta = a.timestamp.as_deref().unwrap_or("");
        let tb = b.timestamp.as_deref().unwrap_or("");
        tb.cmp(ta)
    });

    Ok(TimelineResult {
        timeline: entries,
        agents_included,
        cwd: normalized_cwd,
        warnings,
    })
}

/// Try to read the first assistant snippet from a session file.
fn read_snippet(agent: &str, file_path: &str) -> Option<String> {
    let adapter = adapters::get_adapter(agent)?;
    let session = adapter
        .read_session(
            Some(
                std::path::Path::new(file_path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(""),
            ),
            ".",
            None,
            1,
        )
        .ok()?;

    if session.content.is_empty() {
        None
    } else {
        let short: String = session.content.chars().take(200).collect();
        Some(short)
    }
}
