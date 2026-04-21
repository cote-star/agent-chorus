pub mod codex;
pub mod gemini;
pub mod claude;
pub mod cursor;

use crate::agents::Session;
use anyhow::Result;
use serde_json::Value;

/// Per-read rendering options that agent adapters honor when building the
/// session content. These are CLI-layer-neutral — format selection (json/md)
/// is handled above the adapter layer.
#[derive(Debug, Default, Clone, Copy)]
pub struct ReadOptions {
    /// When true, interleave each user prompt with the assistant turn that
    /// followed it. When false, only assistant turns are emitted.
    pub include_user: bool,
    /// When true, tool-call blocks are rendered into the content with their
    /// input arguments. When false, tool calls are elided (Claude's text-only
    /// extractor).
    pub include_tool_calls: bool,
}

/// Trait for agent adapters. Each agent implementation provides
/// file resolution, session reading, and listing capabilities.
pub trait AgentAdapter {
    fn read_session(
        &self,
        id: Option<&str>,
        cwd: &str,
        chats_dir: Option<&str>,
        last_n: usize,
    ) -> Result<Session> {
        self.read_session_with_options(id, cwd, chats_dir, last_n, ReadOptions::default())
    }

    fn read_session_with_options(
        &self,
        id: Option<&str>,
        cwd: &str,
        chats_dir: Option<&str>,
        last_n: usize,
        opts: ReadOptions,
    ) -> Result<Session>;

    fn list_sessions(&self, cwd: Option<&str>, limit: usize) -> Result<Vec<Value>>;
    fn search_sessions(&self, query: &str, cwd: Option<&str>, limit: usize) -> Result<Vec<Value>>;
}

/// Returns the adapter for the given agent name.
pub fn get_adapter(agent: &str) -> Option<Box<dyn AgentAdapter>> {
    match agent {
        "codex" => Some(Box::new(codex::CodexAdapter)),
        "gemini" => Some(Box::new(gemini::GeminiAdapter)),
        "claude" => Some(Box::new(claude::ClaudeAdapter)),
        "cursor" => Some(Box::new(cursor::CursorAdapter)),
        _ => None,
    }
}
