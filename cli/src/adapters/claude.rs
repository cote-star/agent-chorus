use super::{AgentAdapter, ReadOptions};
use crate::agents::{self, Session};
use anyhow::Result;
use serde_json::Value;

pub struct ClaudeAdapter;

impl AgentAdapter for ClaudeAdapter {
    fn read_session_with_options(
        &self,
        id: Option<&str>,
        cwd: &str,
        _chats_dir: Option<&str>,
        last_n: usize,
        opts: ReadOptions,
    ) -> Result<Session> {
        agents::read_claude_session_with_options(id, cwd, last_n, opts)
    }

    fn list_sessions(&self, cwd: Option<&str>, limit: usize) -> Result<Vec<Value>> {
        agents::list_claude_sessions(cwd, limit)
    }

    fn search_sessions(&self, query: &str, cwd: Option<&str>, limit: usize) -> Result<Vec<Value>> {
        agents::search_claude_sessions(query, cwd, limit)
    }
}
