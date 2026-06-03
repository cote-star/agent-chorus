//! Cursor workspace-cwd resolution.
//!
//! Implements the per-session cwd recovery for the native Cursor adapter:
//! 1. read `<project>/.workspace-trusted` -> "workspacePath" (authoritative), else
//! 2. demangle the project dir name against the real filesystem.
//!
//! FULL SPEC: docs/adapters/CURSOR_HERMES_NATIVE_ADAPTER.md  §6 Unit A.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// Walk `tokens` (a path split on '-') as a chain of EXISTING directories under
/// `base`, where a single real directory name may itself span several tokens
/// (because real names can contain '-'). Returns the deepest matched path iff the
/// FULL token list is consumed by existing directories; otherwise None.
/// Backtracking, longest-match-first.
pub(crate) fn walk_existing(base: &Path, tokens: &[&str]) -> Option<PathBuf> {
    if tokens.is_empty() {
        return if base.is_dir() {
            Some(base.to_path_buf())
        } else {
            None
        };
    }
    for j in (1..=tokens.len()).rev() {
        let name = tokens[..j].join("-");
        let child = base.join(&name);
        if child.is_dir() {
            if let Some(p) = walk_existing(&child, &tokens[j..]) {
                return Some(p);
            }
        }
    }
    None
}

/// Demangle a Cursor project dir name (e.g.
/// "Users-e059303-sandbox-work-trust-stream-trust-stream-backend") into the real
/// absolute path it maps to, by fs-walking from "/". None if no existing path matches.
pub(crate) fn demangle_project_dir(project_name: &str) -> Option<PathBuf> {
    let tokens: Vec<&str> = project_name.split('-').collect();
    walk_existing(Path::new("/"), &tokens)
}

/// Resolve the originating workspace cwd for a transcript file at
/// `<...>/<project>/agent-transcripts/<session>/<session>.jsonl`.
/// Order: (1) <project>/.workspace-trusted -> "workspacePath"; (2) demangle the
/// <project> dir name; (3) None.
pub(crate) fn resolve_cursor_cwd(transcript_path: &Path) -> Option<PathBuf> {
    let project_dir = transcript_path.parent()?.parent()?.parent()?;

    let trusted_path = project_dir.join(".workspace-trusted");
    if let Ok(contents) = std::fs::read_to_string(&trusted_path) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&contents) {
            if let Some(workspace) = value.get("workspacePath").and_then(|v| v.as_str()) {
                return Some(PathBuf::from(workspace));
            }
        }
    }

    let name = project_dir.file_name()?.to_str()?;
    demangle_project_dir(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("chorus_cursorcwd_{}", name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn walk_existing_simple_chain() {
        let base = fixture("simple_chain");
        std::fs::create_dir_all(base.join("a/b")).unwrap();
        let got = walk_existing(&base, &["a", "b"]);
        assert_eq!(got, Some(base.join("a/b")));
    }

    #[test]
    fn walk_existing_dashed_chain() {
        let base = fixture("dashed_chain");
        let target = base.join("trust-stream/trust-stream-backend");
        std::fs::create_dir_all(&target).unwrap();
        let tokens = &["trust", "stream", "trust", "stream", "backend"];
        assert_eq!(walk_existing(&base, tokens), Some(target));
    }

    #[test]
    fn walk_existing_missing_dir() {
        let base = fixture("missing");
        assert_eq!(walk_existing(&base, &["nope"]), None);
    }

    #[test]
    fn walk_existing_disambiguation_dashed_dir() {
        let base = fixture("disambig_dashed");
        std::fs::create_dir_all(base.join("play-foo")).unwrap();
        assert_eq!(walk_existing(&base, &["play", "foo"]), Some(base.join("play-foo")));
    }

    #[test]
    fn walk_existing_disambiguation_nested_dirs() {
        let base = fixture("disambig_nested");
        std::fs::create_dir_all(base.join("play/foo")).unwrap();
        assert_eq!(walk_existing(&base, &["play", "foo"]), Some(base.join("play/foo")));
    }

    #[test]
    fn resolve_cursor_cwd_via_workspace_trusted() {
        let workspace = fixture("workspace_path");
        let proj = fixture("resolve_trusted_proj");
        let session = "sess-abc123";
        let transcript_dir = proj.join("agent-transcripts").join(session);
        std::fs::create_dir_all(&transcript_dir).unwrap();
        let transcript = transcript_dir.join(format!("{session}.jsonl"));
        std::fs::write(&transcript, "{}\n").unwrap();
        let trusted_json = format!(
            r#"{{"trustedAt":"2026-06-02T19:33:37.491Z","workspacePath":"{}","trustMethod":"cli-flag"}}"#,
            workspace.display()
        );
        std::fs::write(proj.join(".workspace-trusted"), trusted_json).unwrap();

        assert_eq!(resolve_cursor_cwd(&transcript), Some(workspace));
    }

    #[test]
    fn resolve_cursor_cwd_none_without_trusted_or_demangle() {
        let proj_name = "chorus-cursorcwd-nonexistent-project-xyzzy-99999";
        let projects_root = fixture("resolve_none_root");
        let project_dir = projects_root.join(proj_name);
        let session = "sess-none";
        let transcript_dir = project_dir.join("agent-transcripts").join(session);
        std::fs::create_dir_all(&transcript_dir).unwrap();
        let transcript = transcript_dir.join(format!("{session}.jsonl"));
        std::fs::write(&transcript, "{}\n").unwrap();
        assert_eq!(resolve_cursor_cwd(&transcript), None);
        assert_eq!(demangle_project_dir(proj_name), None);
    }
}
