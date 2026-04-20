//! `chorus checkpoint` — broadcast the current git state to every other agent's inbox.
//!
//! Writes one JSONL entry per recipient via the existing `messaging::send_message`
//! primitive. Guards on `.agent-chorus/` presence so the command is safe to call
//! unconditionally (e.g. from a SessionEnd hook installed globally).

use anyhow::Result;
use serde::Serialize;
use std::path::Path;
use std::process::Command;

use crate::messaging;

/// Ordered roster of all agents chorus knows how to message.
pub const ALL_AGENTS: &[&str] = &["claude", "codex", "gemini", "cursor"];

/// Result of a checkpoint broadcast, suitable for `--json` output.
#[derive(Debug, Serialize)]
pub struct CheckpointResult {
    pub ok: bool,
    pub from: String,
    pub recipients: Vec<String>,
    pub message: String,
}

/// Run a checkpoint broadcast from `from` to every other valid agent rooted at `cwd`.
///
/// Returns `Ok(None)` when `.agent-chorus/` is not present in `cwd` — the caller
/// should treat that as a silent no-op (consistent with other install-safe
/// subcommands like `verify` and the pre-push hook wiring).
pub fn run(from: &str, cwd: &str, message_override: Option<&str>) -> Result<Option<CheckpointResult>> {
    let cwd_path = Path::new(cwd);
    let guard = cwd_path.join(".agent-chorus");
    if !guard.exists() {
        return Ok(None);
    }

    let message = match message_override {
        Some(m) => m.to_string(),
        None => compose_state_message(from, cwd_path),
    };

    let mut recipients = Vec::new();
    for agent in ALL_AGENTS {
        if *agent == from {
            continue;
        }
        messaging::send_message(from, agent, &message, cwd)?;
        recipients.push((*agent).to_string());
    }

    Ok(Some(CheckpointResult {
        ok: true,
        from: from.to_string(),
        recipients,
        message,
    }))
}

/// Compose the default checkpoint message from lightweight git state.
///
/// All `git` invocations soft-fail to conservative defaults so the checkpoint
/// still broadcasts even when git is missing or the working tree is not a repo.
fn compose_state_message(from: &str, cwd: &Path) -> String {
    let branch = git_oneline(cwd, &["branch", "--show-current"]).unwrap_or_else(|| "unknown".to_string());
    let uncommitted = git_uncommitted_count(cwd).unwrap_or_else(|| "0".to_string());
    let last_commit = git_oneline(cwd, &["log", "-1", "--format=%h %s"]).unwrap_or_else(|| "none".to_string());

    let branch = if branch.is_empty() { "unknown".to_string() } else { branch };
    let last_commit = if last_commit.is_empty() { "none".to_string() } else { last_commit };

    format!(
        "{} session ended. Branch: {} | Uncommitted: {} | Last commit: {}",
        from, branch, uncommitted, last_commit
    )
}

/// Run `git <args>` in `cwd` and return the trimmed first line of stdout on success.
/// Returns `None` if git is missing, the command fails, or output is empty.
fn git_oneline(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).current_dir(cwd).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let first = text.lines().next().unwrap_or("").trim().to_string();
    if first.is_empty() {
        None
    } else {
        Some(first)
    }
}

/// Return the number of modified lines reported by `git status --short`, as a
/// string. Uses stdlib line counting to avoid relying on an external `wc`.
fn git_uncommitted_count(cwd: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["status", "--short"])
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let count = text.lines().filter(|l| !l.trim().is_empty()).count();
    Some(count.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::path::PathBuf;

    fn test_dir(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("chorus_checkpoint_test_{}", name));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create test dir");
        dir
    }

    fn read_jsonl_lines(path: &Path) -> Vec<String> {
        if !path.exists() {
            return Vec::new();
        }
        fs::read_to_string(path)
            .unwrap()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.to_string())
            .collect()
    }

    #[test]
    fn checkpoint_writes_state_message_to_all_other_agents() {
        let dir = test_dir("writes_all_others");
        fs::create_dir_all(dir.join(".agent-chorus")).unwrap();

        let cwd = dir.to_string_lossy().to_string();
        let result = run("claude", &cwd, None).expect("checkpoint ok").expect("not guarded");

        assert_eq!(result.from, "claude");
        let mut expected: Vec<String> = ["codex", "gemini", "cursor"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        let mut got = result.recipients.clone();
        got.sort();
        expected.sort();
        assert_eq!(got, expected, "checkpoint should address the three other agents");

        for agent in &["codex", "gemini", "cursor"] {
            let file = dir
                .join(".agent-chorus")
                .join("messages")
                .join(format!("{}.jsonl", agent));
            let lines = read_jsonl_lines(&file);
            assert_eq!(lines.len(), 1, "expected one line for {} at {}", agent, file.display());
            let json: serde_json::Value = serde_json::from_str(&lines[0]).expect("valid JSON");
            assert_eq!(json["from"], "claude");
            assert_eq!(json["to"], *agent);
            let content = json["content"].as_str().unwrap_or("");
            assert!(content.starts_with("claude session ended."), "unexpected content: {}", content);
        }

        // Claude's own inbox must NOT have been written.
        let self_file = dir.join(".agent-chorus").join("messages").join("claude.jsonl");
        assert!(!self_file.exists(), "checkpoint should never write to the sender's own inbox");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn checkpoint_guards_on_missing_agent_context_dir() {
        let dir = test_dir("guards_missing");
        // Deliberately do NOT create .agent-chorus/

        let cwd = dir.to_string_lossy().to_string();
        let result = run("claude", &cwd, None).expect("checkpoint ok");
        assert!(result.is_none(), "checkpoint should be a silent no-op when .agent-chorus/ is missing");

        // Nothing created.
        assert!(!dir.join(".agent-chorus").exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn checkpoint_honors_custom_message_override() {
        let dir = test_dir("honors_override");
        fs::create_dir_all(dir.join(".agent-chorus")).unwrap();

        let cwd = dir.to_string_lossy().to_string();
        let override_msg = "Payment refactor half-done; types still broken";
        let result = run("codex", &cwd, Some(override_msg))
            .expect("checkpoint ok")
            .expect("not guarded");

        assert_eq!(result.message, override_msg);
        for agent in &["claude", "gemini", "cursor"] {
            let file = dir
                .join(".agent-chorus")
                .join("messages")
                .join(format!("{}.jsonl", agent));
            let lines = read_jsonl_lines(&file);
            assert_eq!(lines.len(), 1, "expected one line for {}", agent);
            let json: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
            assert_eq!(json["content"].as_str().unwrap(), override_msg);
            assert_eq!(json["from"].as_str().unwrap(), "codex");
        }

        let _ = fs::remove_dir_all(&dir);
    }
}
