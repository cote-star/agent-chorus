//! Session diff — compare two sessions from the same agent.

use crate::adapters;
use anyhow::{Context, Result};
use serde::Serialize;

/// Result of diffing two sessions.
#[derive(Debug, Serialize)]
pub struct DiffResult {
    pub agent: String,
    pub session_a: String,
    pub session_b: String,
    pub added_lines: usize,
    pub removed_lines: usize,
    pub equal_lines: usize,
    pub hunks: Vec<DiffHunk>,
}

/// A single hunk in the diff output.
#[derive(Debug, Serialize)]
pub struct DiffHunk {
    pub tag: String,     // "add", "remove", "equal"
    pub content: String, // the line(s)
}

/// Compute a line-level diff between two sessions.
pub fn diff_sessions(
    agent: &str,
    id_a: &str,
    id_b: &str,
    cwd: &str,
    last_n: usize,
) -> Result<DiffResult> {
    let adapter = adapters::get_adapter(agent)
        .with_context(|| format!("Unsupported agent: {}", agent))?;

    let session_a = adapter
        .read_session(Some(id_a), cwd, None, last_n)
        .with_context(|| format!("Failed to read session A: {}", id_a))?;
    let session_b = adapter
        .read_session(Some(id_b), cwd, None, last_n)
        .with_context(|| format!("Failed to read session B: {}", id_b))?;

    let lines_a: Vec<&str> = session_a.content.lines().collect();
    let lines_b: Vec<&str> = session_b.content.lines().collect();

    let hunks = compute_line_diff(&lines_a, &lines_b);

    let added = hunks.iter().filter(|h| h.tag == "add").count();
    let removed = hunks.iter().filter(|h| h.tag == "remove").count();
    let equal = hunks.iter().filter(|h| h.tag == "equal").count();

    Ok(DiffResult {
        agent: agent.to_string(),
        session_a: session_a.session_id.unwrap_or_else(|| id_a.to_string()),
        session_b: session_b.session_id.unwrap_or_else(|| id_b.to_string()),
        added_lines: added,
        removed_lines: removed,
        equal_lines: equal,
        hunks,
    })
}

/// Simple LCS-based line diff.
fn compute_line_diff(a: &[&str], b: &[&str]) -> Vec<DiffHunk> {
    let m = a.len();
    let n = b.len();

    // Build LCS table
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            if a[i - 1] == b[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else {
                dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
            }
        }
    }

    // Backtrack to produce hunks
    let mut hunks = Vec::new();
    let mut i = m;
    let mut j = n;

    while i > 0 || j > 0 {
        if i > 0 && j > 0 && a[i - 1] == b[j - 1] {
            hunks.push(DiffHunk {
                tag: "equal".to_string(),
                content: a[i - 1].to_string(),
            });
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            hunks.push(DiffHunk {
                tag: "add".to_string(),
                content: b[j - 1].to_string(),
            });
            j -= 1;
        } else {
            hunks.push(DiffHunk {
                tag: "remove".to_string(),
                content: a[i - 1].to_string(),
            });
            i -= 1;
        }
    }

    hunks.reverse();
    hunks
}
