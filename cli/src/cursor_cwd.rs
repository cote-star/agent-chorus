//! Cursor workspace-cwd resolution.
//!
//! Implements the per-session cwd recovery for the native Cursor adapter:
//! 1. read `<project>/.workspace-trusted` -> "workspacePath" (authoritative), else
//! 2. demangle the project dir name against the real filesystem.
//!
//! FULL SPEC: docs/adapters/CURSOR_HERMES_NATIVE_ADAPTER.md  §6 Unit A.
//! Implementer: fill the function bodies + add the required `#[cfg(test)]` tests.
//! Do NOT change these signatures and do NOT edit any other file.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// Walk `tokens` (a path split on '-') as a chain of EXISTING directories under
/// `base`, where a single real directory name may itself span several tokens
/// (because real names can contain '-'). Returns the deepest matched path iff the
/// FULL token list is consumed by existing directories; otherwise None.
/// Backtracking, longest-match-first.
pub(crate) fn walk_existing(base: &Path, tokens: &[&str]) -> Option<PathBuf> {
    let _ = (base, tokens);
    todo!("Unit A: implement per docs/adapters/CURSOR_HERMES_NATIVE_ADAPTER.md §6")
}

/// Demangle a Cursor project dir name (e.g.
/// "Users-e059303-sandbox-work-trust-stream-trust-stream-backend") into the real
/// absolute path it maps to, by fs-walking from "/". None if no existing path matches.
pub(crate) fn demangle_project_dir(project_name: &str) -> Option<PathBuf> {
    let _ = project_name;
    todo!("Unit A")
}

/// Resolve the originating workspace cwd for a transcript file at
/// `<...>/<project>/agent-transcripts/<session>/<session>.jsonl`.
/// Order: (1) <project>/.workspace-trusted -> "workspacePath"; (2) demangle the
/// <project> dir name; (3) None.
pub(crate) fn resolve_cursor_cwd(transcript_path: &Path) -> Option<PathBuf> {
    let _ = transcript_path;
    todo!("Unit A")
}
