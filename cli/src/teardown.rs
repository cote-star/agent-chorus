//! Teardown — reverse setup by removing managed blocks, scaffolding, and hooks.

use anyhow::Result;
use serde_json::json;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Provider instruction file configuration.
struct Provider {
    agent: &'static str,
    target_file: &'static str,
}

const PROVIDERS: &[Provider] = &[
    Provider { agent: "codex", target_file: "AGENTS.md" },
    Provider { agent: "claude", target_file: "CLAUDE.md" },
    Provider { agent: "gemini", target_file: "GEMINI.md" },
];

/// Result of the teardown operation.
pub struct TeardownResult {
    pub cwd: String,
    pub dry_run: bool,
    pub global: bool,
    pub operations: Vec<serde_json::Value>,
    pub warnings: Vec<String>,
    pub changed: usize,
}

/// Run teardown for the given cwd.
pub fn run_teardown(cwd: &str, dry_run: bool, global: bool) -> Result<TeardownResult> {
    let cwd_path = Path::new(cwd);
    let mut operations = Vec::new();
    let mut warnings = Vec::new();

    // Validate target directory is not a system path
    if is_system_directory(cwd_path) {
        anyhow::bail!("Refusing to run teardown in system directory: {}", cwd);
    }

    // 1. Remove managed blocks from provider instruction files
    for provider in PROVIDERS {
        let target_path = cwd_path.join(provider.target_file);
        let marker_prefix = format!("agent-chorus:{}", provider.agent);
        let result = remove_managed_block(&target_path, &marker_prefix, dry_run);
        operations.push(json!({
            "type": "integration",
            "path": target_path.to_string_lossy(),
            "status": result.status,
            "note": result.message,
        }));
    }

    // 2. Remove .agent-chorus/ directory (scaffolding)
    let setup_root = cwd_path.join(".agent-chorus");
    if setup_root.exists() {
        if !dry_run {
            let _ = fs::remove_dir_all(&setup_root);
        }
        operations.push(json!({
            "type": "directory",
            "path": setup_root.to_string_lossy(),
            "status": "deleted",
            "note": "Removed scaffolding directory",
        }));
    } else {
        operations.push(json!({
            "type": "directory",
            "path": setup_root.to_string_lossy(),
            "status": "unchanged",
            "note": "Scaffolding directory does not exist",
        }));
    }

    // 3. Remove .agent-chorus/ from .gitignore
    let gitignore_path = cwd_path.join(".gitignore");
    if gitignore_path.exists() {
        match fs::read_to_string(&gitignore_path) {
            Ok(content) => {
                let filtered: Vec<&str> = content
                    .lines()
                    .filter(|l| l.trim() != ".agent-chorus/" && l.trim() != ".agent-chorus")
                    .collect();
                let new_content = filtered.join("\n") + "\n";
                if new_content != content {
                    let status = if dry_run {
                        "planned"
                    } else {
                        fs::write(&gitignore_path, &new_content).ok();
                        "updated"
                    };
                    operations.push(json!({
                        "type": "gitignore",
                        "path": ".gitignore",
                        "status": status,
                        "note": "Removed .agent-chorus/ from .gitignore"
                    }));
                } else {
                    operations.push(json!({
                        "type": "gitignore",
                        "path": ".gitignore",
                        "status": "unchanged",
                        "note": "No .agent-chorus/ entry found"
                    }));
                }
            }
            Err(e) => {
                operations.push(json!({
                    "type": "gitignore",
                    "path": ".gitignore",
                    "status": "error",
                    "note": format!("Could not read .gitignore: {}", e)
                }));
            }
        }
    } else {
        operations.push(json!({
            "type": "gitignore",
            "path": ".gitignore",
            "status": "skipped",
            "note": "No .gitignore found"
        }));
    }

    // 4. Remove pre-push hook sentinel
    let hook_result = remove_hook_sentinel(cwd_path, dry_run);
    operations.push(json!({
        "type": "hook",
        "path": hook_result.path,
        "status": hook_result.status,
        "note": hook_result.message,
    }));

    // 5. Warn about .agent-context/ (never auto-delete)
    let agent_context_dir = cwd_path.join(".agent-context");
    if agent_context_dir.exists() {
        warnings.push(format!(
            "Context pack at {} preserved (contains project data). Remove manually if desired.",
            agent_context_dir.display()
        ));
        operations.push(json!({
            "type": "context-pack",
            "path": agent_context_dir.to_string_lossy(),
            "status": "preserved",
            "note": "Contains project data; not removed by teardown",
        }));
    }

    // 6. If --global: remove cache directory
    if global {
        if let Some(cache_dir) = dirs::cache_dir().map(|d| d.join("agent-chorus")) {
            if cache_dir.exists() {
                if !dry_run {
                    let _ = fs::remove_dir_all(&cache_dir);
                }
                operations.push(json!({
                    "type": "cache",
                    "path": cache_dir.to_string_lossy(),
                    "status": "deleted",
                    "note": "Removed global update-check cache",
                }));
            } else {
                operations.push(json!({
                    "type": "cache",
                    "path": cache_dir.to_string_lossy(),
                    "status": "unchanged",
                    "note": "Global cache does not exist",
                }));
            }
        }
    }

    let changed = operations
        .iter()
        .filter(|op| {
            let status = op.get("status").and_then(|s| s.as_str()).unwrap_or("");
            status == "deleted" || status == "updated"
        })
        .count();

    Ok(TeardownResult {
        cwd: cwd.to_string(),
        dry_run,
        global,
        operations,
        warnings,
        changed,
    })
}

// --- Helper types and functions ---

struct BlockResult {
    status: &'static str,
    message: &'static str,
}

struct HookResult {
    path: String,
    status: &'static str,
    message: &'static str,
}

fn remove_managed_block(file_path: &Path, marker_prefix: &str, dry_run: bool) -> BlockResult {
    if !file_path.exists() {
        return BlockResult {
            status: "unchanged",
            message: "File does not exist",
        };
    }

    let existing = match fs::read_to_string(file_path) {
        Ok(s) => s,
        Err(_) => {
            return BlockResult {
                status: "unchanged",
                message: "Could not read file",
            };
        }
    };

    // Check both current and legacy marker prefixes
    let legacy_prefix = marker_prefix.replace("agent-chorus:", "agent-bridge:");
    let prefixes = [marker_prefix.to_string(), legacy_prefix];

    let mut content = existing;
    let mut removed = false;

    for prefix in &prefixes {
        let start_marker = format!("<!-- {}:start -->", prefix);
        let end_marker = format!("<!-- {}:end -->", prefix);

        let mut safety = 0;
        while safety < 10 {
            let start_idx = content.find(&start_marker);
            let end_idx = content.find(&end_marker);

            match (start_idx, end_idx) {
                (Some(si), Some(ei)) if ei > si => {
                    let before = content[..si].trim_end().to_string();
                    let after = content[ei + end_marker.len()..].trim_start().to_string();
                    content = if !before.is_empty() && !after.is_empty() {
                        format!("{}\n\n{}", before, after)
                    } else if !before.is_empty() {
                        before
                    } else {
                        after
                    };
                    // Collapse excessive newlines
                    while content.contains("\n\n\n") {
                        content = content.replace("\n\n\n", "\n\n");
                    }
                    removed = true;
                    safety += 1;
                }
                _ => break,
            }
        }
    }

    if !removed {
        return BlockResult {
            status: "unchanged",
            message: "No managed block found",
        };
    }

    let trimmed = content.trim().to_string();

    if !dry_run {
        if trimmed.is_empty() {
            let _ = fs::remove_file(file_path);
        } else {
            let _ = fs::write(file_path, format!("{}\n", trimmed));
        }
    }

    if trimmed.is_empty() {
        BlockResult {
            status: "deleted",
            message: "File deleted (was only managed block)",
        }
    } else {
        BlockResult {
            status: "updated",
            message: "Managed block removed",
        }
    }
}

fn remove_hook_sentinel_from_file(hook_path: &Path, dry_run: bool) -> Option<HookResult> {
    if !hook_path.exists() {
        return None;
    }

    let existing = match fs::read_to_string(hook_path) {
        Ok(s) => s,
        Err(_) => return None,
    };

    let sentinel_pairs = [
        (
            "# --- agent-chorus:pre-push:start ---",
            "# --- agent-chorus:pre-push:end ---",
        ),
        (
            "# --- agent-bridge:pre-push:start ---",
            "# --- agent-bridge:pre-push:end ---",
        ),
    ];

    let mut content = existing;
    let mut removed = false;

    for (start_sentinel, end_sentinel) in &sentinel_pairs {
        if let (Some(si), Some(ei)) = (content.find(start_sentinel), content.find(end_sentinel)) {
            if ei > si {
                let before = content[..si].trim_end().to_string();
                let after = content[ei + end_sentinel.len()..].trim_start().to_string();
                content = if !before.is_empty() && !after.is_empty() {
                    format!("{}\n\n{}", before, after)
                } else if !before.is_empty() {
                    before
                } else {
                    after
                };
                while content.contains("\n\n\n") {
                    content = content.replace("\n\n\n", "\n\n");
                }
                removed = true;
            }
        }
    }

    if !removed {
        return None;
    }

    let trimmed = content.trim();
    let is_effectively_empty = trimmed.is_empty()
        || trimmed == "#!/usr/bin/env bash"
        || trimmed == "#!/bin/bash"
        || trimmed == "#!/bin/sh";

    if !dry_run {
        if is_effectively_empty {
            let _ = fs::remove_file(hook_path);
            // Try to clean up empty hooks directory
            if let Some(parent) = hook_path.parent() {
                if let Ok(entries) = fs::read_dir(parent) {
                    if entries.count() == 0 {
                        let _ = fs::remove_dir(parent);
                    }
                }
            }
        } else {
            let _ = fs::write(hook_path, format!("{}\n", trimmed));
        }
    }

    let (status, message) = if is_effectively_empty {
        ("deleted", "Hook file deleted (was only chorus sentinel)")
    } else {
        ("updated", "Hook sentinel removed")
    };

    Some(HookResult {
        path: hook_path.to_string_lossy().to_string(),
        status,
        message,
    })
}

fn remove_hook_sentinel(cwd: &Path, dry_run: bool) -> HookResult {
    let mut candidates: Vec<PathBuf> = Vec::new();

    // 1. Check core.hooksPath from git config
    if let Ok(output) = Command::new("git")
        .args(["config", "--get", "core.hooksPath"])
        .current_dir(cwd)
        .output()
    {
        if output.status.success() {
            let config_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let hooks_dir = if Path::new(&config_path).is_absolute() {
                PathBuf::from(&config_path)
            } else {
                cwd.join(&config_path)
            };
            candidates.push(hooks_dir.join("pre-push"));
        }
    }

    // 2. .githooks/ in project root (chorus default)
    candidates.push(cwd.join(".githooks").join("pre-push"));

    // 3. .git/hooks/ (git default)
    candidates.push(cwd.join(".git").join("hooks").join("pre-push"));

    // Deduplicate paths
    let mut seen = HashSet::new();
    for candidate in &candidates {
        let resolved = candidate.canonicalize().unwrap_or_else(|_| candidate.clone());
        let key = resolved.to_string_lossy().to_string();
        if seen.contains(&key) {
            continue;
        }
        seen.insert(key);

        if let Some(result) = remove_hook_sentinel_from_file(candidate, dry_run) {
            return result;
        }
    }

    let default_path = candidates
        .first()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| cwd.join(".githooks/pre-push").to_string_lossy().to_string());

    HookResult {
        path: default_path,
        status: "unchanged",
        message: "No hook sentinel found",
    }
}

fn is_system_directory(dir: &Path) -> bool {
    let s = dir.to_string_lossy();
    // macOS temp dirs live under /var/folders — allow those
    if s.starts_with("/var/folders/") || s.starts_with("/private/var/folders/") {
        return false;
    }
    let system_prefixes = [
        "/etc", "/usr", "/var", "/bin", "/sbin", "/System", "/Library",
        "/Windows", "/Windows/System32", "/Program Files", "/Program Files (x86)",
    ];
    for prefix in system_prefixes {
        if s.as_ref() == prefix
            || s.starts_with(&format!("{}/", prefix))
            || s.starts_with(&format!("{}\\", prefix))
        {
            return true;
        }
    }
    false
}
