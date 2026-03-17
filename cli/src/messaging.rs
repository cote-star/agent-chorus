//! Agent-to-agent messaging — simple JSONL message queue.
//!
//! Messages are stored in `.agent-chorus/messages/<target-agent>.jsonl`.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// Valid agent names for messaging.
const VALID_AGENTS: &[&str] = &["codex", "gemini", "claude", "cursor"];

/// Validate that an agent name is recognized.
fn validate_agent(name: &str, context: &str) -> Result<()> {
    if VALID_AGENTS.contains(&name) {
        Ok(())
    } else {
        bail!(
            "Unknown agent for {}: {}. Valid: {}",
            context,
            name,
            VALID_AGENTS.join(", ")
        )
    }
}

/// A single message between agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub from: String,
    pub to: String,
    pub timestamp: String,
    pub content: String,
    pub cwd: String,
}

/// Resolve the messages directory for a given cwd.
fn messages_dir(cwd: &Path) -> PathBuf {
    cwd.join(".agent-chorus").join("messages")
}

/// Resolve the message file path for a target agent.
fn message_file(cwd: &Path, agent: &str) -> PathBuf {
    messages_dir(cwd).join(format!("{}.jsonl", agent))
}

/// Send a message from one agent to another.
pub fn send_message(from: &str, to: &str, content: &str, cwd: &str) -> Result<Message> {
    validate_agent(from, "--from")?;
    validate_agent(to, "--to")?;
    let cwd_path = Path::new(cwd);
    let dir = messages_dir(cwd_path);
    fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create messages dir: {}", dir.display()))?;

    let msg = Message {
        from: from.to_string(),
        to: to.to_string(),
        timestamp: chrono_now(),
        content: content.to_string(),
        cwd: cwd.to_string(),
    };

    let file = message_file(cwd_path, to);
    let line = serde_json::to_string(&msg)?;
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
        .with_context(|| format!("Failed to open message file: {}", file.display()))?;
    use std::io::Write;
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)?;
    writeln!(f, "{}", line)?;

    Ok(msg)
}

/// Read all messages for a given agent.
pub fn read_messages(agent: &str, cwd: &str) -> Result<Vec<Message>> {
    validate_agent(agent, "--agent")?;
    let cwd_path = Path::new(cwd);
    let file = message_file(cwd_path, agent);

    if !file.exists() {
        return Ok(Vec::new());
    }

    let reader = BufReader::new(
        fs::File::open(&file)
            .with_context(|| format!("Failed to open message file: {}", file.display()))?,
    );

    let mut messages = Vec::new();
    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<Message>(trimmed) {
            Ok(msg) => messages.push(msg),
            Err(_) => continue, // skip malformed lines
        }
    }

    Ok(messages)
}

/// Clear all messages for a given agent.
pub fn clear_messages(agent: &str, cwd: &str) -> Result<usize> {
    validate_agent(agent, "--agent")?;
    let cwd_path = Path::new(cwd);
    let file = message_file(cwd_path, agent);

    if !file.exists() {
        return Ok(0);
    }

    // Count messages before clearing
    let count = read_messages(agent, cwd)?.len();
    fs::remove_file(&file)
        .with_context(|| format!("Failed to remove message file: {}", file.display()))?;

    Ok(count)
}

/// Generate an ISO 8601 timestamp.
fn chrono_now() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    // Format as ISO 8601 without external crate
    let secs = now.as_secs();
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Simple date calculation (good enough for timestamps)
    let mut y = 1970i64;
    let mut remaining_days = days as i64;
    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        y += 1;
    }
    let month_days = if is_leap(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut m = 0usize;
    for (i, &md) in month_days.iter().enumerate() {
        if remaining_days < md as i64 {
            m = i;
            break;
        }
        remaining_days -= md as i64;
    }

    let millis = now.subsec_millis();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        y,
        m + 1,
        remaining_days + 1,
        hours,
        minutes,
        seconds,
        millis
    )
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0)
}
