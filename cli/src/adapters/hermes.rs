use super::{AgentAdapter, ReadOptions};
use crate::agents::{self, Session};
use anyhow::Result;
use serde_json::Value;

/// Hermes adapter (provisional scaffold — UNTESTED).
/// Hermes is not yet installed; its real transcript format is unconfirmed. This
/// adapter is wired for parity and returns cleanly when no Hermes data exists.
pub struct HermesAdapter;

impl AgentAdapter for HermesAdapter {
    fn read_session_with_options(
        &self,
        id: Option<&str>,
        cwd: &str,
        _chats_dir: Option<&str>,
        last_n: usize,
        opts: ReadOptions,
    ) -> Result<Session> {
        agents::read_hermes_session_with_options(id, cwd, last_n, opts)
    }

    fn list_sessions(&self, cwd: Option<&str>, limit: usize) -> Result<Vec<Value>> {
        agents::list_hermes_sessions(cwd, limit)
    }

    fn search_sessions(&self, query: &str, cwd: Option<&str>, limit: usize) -> Result<Vec<Value>> {
        agents::search_hermes_sessions(query, cwd, limit)
    }
}
