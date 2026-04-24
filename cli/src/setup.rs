//! Setup — initialize agent-chorus in a target directory.
//!
//! Mirrors `runSetup` in `scripts/read_session.cjs:1799`. Creates the
//! `.agent-chorus/` scaffolding, upserts managed blocks into provider
//! instruction files (`AGENTS.md`, `CLAUDE.md`, `GEMINI.md`), ensures
//! `.agent-chorus/` is git-ignored, optionally seeds an agent-context
//! pack, and auto-installs the Claude Code plugin when the `claude`
//! CLI is available.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::agent_context;

/// Provider instruction file configuration (mirrors `setupProviders` in Node).
struct Provider {
    agent: &'static str,
    target_file: &'static str,
}

const PROVIDERS: &[Provider] = &[
    Provider { agent: "codex", target_file: "AGENTS.md" },
    Provider { agent: "claude", target_file: "CLAUDE.md" },
    Provider { agent: "gemini", target_file: "GEMINI.md" },
];

/// Result of the setup operation. Serializes to match Node's JSON payload.
#[derive(Debug)]
pub struct SetupResult {
    pub cwd: String,
    pub dry_run: bool,
    pub force: bool,
    pub operations: Vec<Value>,
    pub warnings: Vec<String>,
    pub changed: usize,
}

impl SetupResult {
    pub fn to_json(&self) -> Value {
        json!({
            "cwd": self.cwd,
            "dry_run": self.dry_run,
            "force": self.force,
            "operations": self.operations,
            "warnings": self.warnings,
            "changed": self.changed,
        })
    }
}

/// Run setup for the given cwd.
pub fn run_setup(
    cwd: &str,
    dry_run: bool,
    force: bool,
    context_pack: bool,
) -> Result<SetupResult> {
    let cwd_path = PathBuf::from(cwd);
    let mut operations: Vec<Value> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // 1. Refuse system directories.
    if is_system_directory(&cwd_path) {
        return Err(anyhow!("Refusing to run setup in system directory: {}", cwd));
    }

    // 2. Refuse symlinked cwd.
    if let Ok(meta) = fs::symlink_metadata(&cwd_path) {
        if meta.file_type().is_symlink() {
            return Err(anyhow!(
                "Refusing to run setup: target path is a symlink: {}",
                cwd
            ));
        }
    }

    // 3. Warn if no recognizable project markers.
    let project_markers = [".git", "package.json", "Cargo.toml", "pyproject.toml", "go.mod"];
    let has_marker = project_markers
        .iter()
        .any(|m| cwd_path.join(m).exists());
    if !has_marker {
        warnings.push(format!(
            "Warning: {} has no recognizable project markers (.git, package.json, etc.)",
            cwd_path.display()
        ));
    }

    let setup_root = cwd_path.join(".agent-chorus");
    let providers_dir = setup_root.join("providers");

    // 4. INTENTS.md
    let intents_path = setup_root.join("INTENTS.md");
    let intents_content = default_setup_intents();
    let intents_exists = intents_path.exists();
    if !intents_exists || force {
        if !dry_run {
            write_file_ensured(&intents_path, &format!("{}\n", intents_content))?;
        }
        operations.push(json!({
            "type": "file",
            "path": intents_path.to_string_lossy(),
            "status": if intents_exists { "updated" } else { "created" },
            "note": if intents_exists { "Refreshed intent contract" } else { "Created intent contract" },
        }));
    } else {
        operations.push(json!({
            "type": "file",
            "path": intents_path.to_string_lossy(),
            "status": "unchanged",
            "note": "Intent contract already exists",
        }));
    }

    // 5. Per-provider snippet + managed block upsert.
    for provider in PROVIDERS {
        let snippet_path = providers_dir.join(format!("{}.md", provider.agent));
        let snippet_rel = relative_path(&cwd_path, &snippet_path)
            .unwrap_or_else(|| snippet_path.to_string_lossy().to_string());
        let snippet_content = provider_snippet(provider.agent);

        let snippet_exists = snippet_path.exists();
        if !snippet_exists || force {
            if !dry_run {
                write_file_ensured(&snippet_path, &format!("{}\n", snippet_content))?;
            }
            operations.push(json!({
                "type": "file",
                "path": snippet_path.to_string_lossy(),
                "status": if snippet_exists { "updated" } else { "created" },
                "note": if snippet_exists { "Refreshed provider snippet" } else { "Created provider snippet" },
            }));
        } else {
            operations.push(json!({
                "type": "file",
                "path": snippet_path.to_string_lossy(),
                "status": "unchanged",
                "note": "Provider snippet already exists",
            }));
        }

        let target_path = cwd_path.join(provider.target_file);
        let marker_prefix = format!("agent-chorus:{}", provider.agent);
        let block = make_managed_block(provider.agent, &snippet_rel);
        let upsert = upsert_managed_block(&target_path, &block, &marker_prefix, force, dry_run)?;
        operations.push(json!({
            "type": "integration",
            "path": target_path.to_string_lossy(),
            "status": upsert.status,
            "note": upsert.message,
        }));
    }

    // 6. Optional context pack init + hooks.
    if context_pack {
        let pack_current = cwd_path.join(".agent-context").join("current");
        let hook_path = cwd_path.join(".githooks").join("pre-push");
        if dry_run {
            operations.push(json!({
                "type": "context-pack",
                "path": pack_current.to_string_lossy(),
                "status": "planned",
                "note": "Would init context pack template",
            }));
            operations.push(json!({
                "type": "context-pack",
                "path": hook_path.to_string_lossy(),
                "status": "planned",
                "note": "Would install context-pack pre-push hook",
            }));
        } else {
            // Node captures stdout of each subcommand; the Rust API prints directly
            // and returns Result<()>. We map success/failure to updated/error and
            // substitute a short note instead of the captured stdout.
            match agent_context::init(agent_context::InitOptions {
                pack_dir: None,
                cwd: Some(cwd_path.to_string_lossy().to_string()),
                force,
                follow_symlinks: false,
                tier: agent_context::InitTier::default(),
            }) {
                Ok(_) => operations.push(json!({
                    "type": "context-pack",
                    "path": pack_current.to_string_lossy(),
                    "status": "updated",
                    "note": "Context pack initialized",
                })),
                Err(e) => operations.push(json!({
                    "type": "context-pack",
                    "path": pack_current.to_string_lossy(),
                    "status": "error",
                    "note": format!("Context pack init failed: {}", e),
                })),
            }

            match agent_context::install_hooks(&cwd_path.to_string_lossy(), false) {
                Ok(_) => operations.push(json!({
                    "type": "context-pack",
                    "path": hook_path.to_string_lossy(),
                    "status": "updated",
                    "note": "Installed context-pack pre-push hook",
                })),
                Err(e) => operations.push(json!({
                    "type": "context-pack",
                    "path": hook_path.to_string_lossy(),
                    "status": "error",
                    "note": format!("Install hooks failed: {}", e),
                })),
            }

            println!();
            println!("Next steps:");
            println!("1. Ask your agent to fill the context pack template sections.");
            println!("2. Run `chorus context-pack seal` to finalize the pack.");
        }
    }

    // 7. .gitignore entry.
    let gitignore_path = cwd_path.join(".gitignore");
    let gitignore_entry = ".agent-chorus/";
    let gitignore_exists = gitignore_path.exists();
    let gitignore_content = if gitignore_exists {
        fs::read_to_string(&gitignore_path).unwrap_or_default()
    } else {
        String::new()
    };
    let already_ignored = gitignore_content
        .lines()
        .map(|l| l.trim())
        .any(|t| t == ".agent-chorus/" || t == ".agent-chorus");
    if !already_ignored {
        if !dry_run {
            let sep = if !gitignore_content.is_empty() && !gitignore_content.ends_with('\n') {
                "\n"
            } else {
                ""
            };
            let next = format!("{}{}{}\n", gitignore_content, sep, gitignore_entry);
            fs::write(&gitignore_path, next)?;
        }
        let status = if dry_run {
            "planned"
        } else if gitignore_exists {
            "updated"
        } else {
            "created"
        };
        let note = if dry_run {
            "Would add .agent-chorus/ to .gitignore"
        } else {
            "Added .agent-chorus/ to .gitignore"
        };
        operations.push(json!({
            "type": "gitignore",
            "path": gitignore_path.to_string_lossy(),
            "status": status,
            "note": note,
        }));
    } else {
        operations.push(json!({
            "type": "gitignore",
            "path": gitignore_path.to_string_lossy(),
            "status": "unchanged",
            "note": ".agent-chorus/ already in .gitignore",
        }));
    }

    // 8. Claude Code plugin auto-install when claude CLI is present.
    let package_root = package_root();
    if is_command_available("claude") {
        if claude_plugin_installed() {
            operations.push(json!({
                "type": "plugin",
                "path": "claude plugin",
                "status": "unchanged",
                "note": "agent-chorus Claude Code plugin already installed",
            }));
        } else {
            let (status, note) = install_claude_plugin(&package_root, dry_run);
            operations.push(json!({
                "type": "plugin",
                "path": "claude plugin",
                "status": status,
                "note": note,
            }));
        }
    } else {
        operations.push(json!({
            "type": "plugin",
            "path": "claude plugin",
            "status": "skipped",
            "note": format!(
                "claude CLI not found — install plugin manually: claude plugin marketplace add \"{}\" && claude plugin install agent-chorus",
                package_root.display()
            ),
        }));
    }

    let changed = operations
        .iter()
        .filter(|op| {
            let status = op.get("status").and_then(|s| s.as_str()).unwrap_or("");
            status == "created" || status == "updated"
        })
        .count();

    Ok(SetupResult {
        cwd: cwd_path.to_string_lossy().to_string(),
        dry_run,
        force,
        operations,
        warnings,
        changed,
    })
}

pub fn print_text(result: &SetupResult) {
    let mode = if result.dry_run { "(dry run) " } else { "" };
    println!("Agent Chorus setup {}complete for {}", mode, result.cwd);
    for warning in &result.warnings {
        println!("- [warn] {}", warning);
    }
    for op in &result.operations {
        let status = op.get("status").and_then(|s| s.as_str()).unwrap_or("unknown");
        let path = op.get("path").and_then(|s| s.as_str()).unwrap_or("");
        let note = op.get("note").and_then(|s| s.as_str()).unwrap_or("");
        println!("- [{}] {} ({})", status, path, note);
    }
}

// --- helpers ---

struct UpsertResult {
    status: &'static str,
    message: &'static str,
}

fn write_file_ensured(file_path: &Path, content: &str) -> Result<()> {
    if let Ok(meta) = fs::symlink_metadata(file_path) {
        if meta.file_type().is_symlink() {
            return Err(anyhow!(
                "Refusing to write: target is a symlink: {}",
                file_path.display()
            ));
        }
    }
    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(file_path, content)?;
    Ok(())
}

fn collapse_triple_newlines(s: &str) -> String {
    let mut out = s.to_string();
    while out.contains("\n\n\n") {
        out = out.replace("\n\n\n", "\n\n");
    }
    out
}

fn trim_end(s: &str) -> &str {
    s.trim_end_matches(|c: char| c.is_whitespace())
}

fn trim_start(s: &str) -> &str {
    s.trim_start_matches(|c: char| c.is_whitespace())
}

fn upsert_managed_block(
    file_path: &Path,
    block: &str,
    marker_prefix: &str,
    force: bool,
    dry_run: bool,
) -> Result<UpsertResult> {
    let start_marker = format!("<!-- {}:start -->", marker_prefix);
    let end_marker = format!("<!-- {}:end -->", marker_prefix);

    let existing = if file_path.exists() {
        fs::read_to_string(file_path).unwrap_or_default()
    } else {
        String::new()
    };

    let start_idx = existing.find(&start_marker);
    let end_idx = existing.find(&end_marker);

    // Detect duplicate markers.
    if let Some(si) = start_idx {
        let second_start = existing[si + start_marker.len()..].find(&start_marker);
        let second_end = end_idx.and_then(|ei| existing[ei + end_marker.len()..].find(&end_marker));
        if second_start.is_some() || second_end.is_some() {
            if !force {
                return Ok(UpsertResult {
                    status: "unchanged",
                    message: "Duplicate managed block markers detected (use --force to replace all)",
                });
            }
            // With --force: strip all managed blocks then append once.
            let mut cleaned = existing.clone();
            let mut safety = 0;
            while safety < 10 {
                let s = cleaned.find(&start_marker);
                let e = cleaned.find(&end_marker);
                match (s, e) {
                    (Some(s), Some(e)) if e > s => {
                        let before = trim_end(&cleaned[..s]).to_string();
                        let after = trim_start(&cleaned[e + end_marker.len()..]).to_string();
                        cleaned = format!("{}\n\n{}", before, after);
                        cleaned = collapse_triple_newlines(&cleaned);
                        safety += 1;
                    }
                    _ => break,
                }
            }
            let trimmed = trim_end(&cleaned).to_string();
            let next = if trimmed.is_empty() {
                format!("{}\n", block)
            } else {
                format!("{}\n\n{}\n", trimmed, block)
            };
            if !dry_run {
                write_file_ensured(file_path, &next)?;
            }
            return Ok(UpsertResult {
                status: "updated",
                message: "Replaced duplicate managed blocks",
            });
        }
    }

    let (next, status, message): (String, &'static str, &'static str) =
        match (start_idx, end_idx) {
            (Some(si), Some(ei)) if ei > si => {
                if !force {
                    return Ok(UpsertResult {
                        status: "unchanged",
                        message: "Managed block already present (use --force to refresh)",
                    });
                }
                let before = trim_end(&existing[..si]).to_string();
                let after_raw = &existing[ei + end_marker.len()..];
                let after = trim_start(after_raw).to_string();
                let body = if after.is_empty() {
                    format!("{}\n\n{}\n", before, block)
                } else {
                    format!("{}\n\n{}\n\n{}", before, block, after)
                };
                let body = collapse_triple_newlines(&body);
                (body, "updated", "Managed block written")
            }
            _ if existing.trim().is_empty() => {
                (format!("{}\n", block), "created", "Created file with managed block")
            }
            _ => {
                let trimmed = trim_end(&existing).to_string();
                (
                    format!("{}\n\n{}\n", trimmed, block),
                    "updated",
                    "Managed block written",
                )
            }
        };

    if !dry_run {
        write_file_ensured(file_path, &next)?;
    }

    Ok(UpsertResult { status, message })
}

fn make_managed_block(agent: &str, snippet_rel_path: &str) -> String {
    let marker = format!("agent-chorus:{}", agent);
    [
        format!("<!-- {}:start -->", marker),
        "## Agent Chorus Integration".to_string(),
        String::new(),
        "This project is wired for cross-agent coordination via `chorus`.".to_string(),
        format!("Provider snippet: `{}`", snippet_rel_path),
        String::new(),
        "When a user asks for another agent status (for example \"What is Claude doing?\"),".to_string(),
        "run Agent Chorus commands first and answer with evidence from session output.".to_string(),
        String::new(),
        "Session routing and defaults:".to_string(),
        "1. For status checks like \"What is Claude doing?\", start with `chorus read --agent <target-agent> --cwd <project-path> --include-user --json` (omit `--id` for latest).".to_string(),
        "2. For plain handoff/output checks, use `chorus read --agent <target-agent> --cwd <project-path> --json`.".to_string(),
        "3. \"past session\" means previous session: list 2 and read the second session ID.".to_string(),
        "4. \"past N sessions\" means exclude latest: list N+1 and read the older N session IDs.".to_string(),
        "5. \"last N sessions\" means include latest: list N and read/summarize those sessions.".to_string(),
        "6. Ask for a session ID only after an initial read/list attempt fails or when exact ID is requested.".to_string(),
        String::new(),
        "Support commands:".to_string(),
        "- `chorus list --agent <agent> --cwd <project-path> --json`".to_string(),
        "- `chorus search \"<query>\" --agent <agent> --cwd <project-path> --json`".to_string(),
        "- `chorus compare --source codex --source gemini --source claude --cwd <project-path> --json`".to_string(),
        String::new(),
        "If command syntax is unclear, run `chorus --help`.".to_string(),
        format!("<!-- {}:end -->", marker),
    ]
    .join("\n")
}

fn provider_snippet(agent: &str) -> String {
    [
        format!("# Agent Chorus Provider Snippet ({})", agent),
        String::new(),
        "When the user asks cross-agent questions, run Agent Chorus first.".to_string(),
        String::new(),
        "Primary trigger examples:".to_string(),
        "- \"What is Claude doing?\"".to_string(),
        "- \"What did Gemini say?\"".to_string(),
        "- \"Compare agent outputs\"".to_string(),
        "- \"Show the past 3 sessions from Claude\"".to_string(),
        String::new(),
        "Intent router:".to_string(),
        "- \"What is Claude doing?\" -> `chorus read --agent claude --cwd <project-path> --include-user --json`".to_string(),
        "- \"What did Gemini say?\" -> `chorus read --agent gemini --cwd <project-path> --json`".to_string(),
        "- \"Compare Codex and Claude outputs\" -> `chorus compare --source codex --source claude --cwd <project-path> --json`".to_string(),
        String::new(),
        "Session timing defaults:".to_string(),
        "- No session ID means latest session in scope.".to_string(),
        "- \"past session\" means previous session (exclude latest).".to_string(),
        "- \"past N sessions\" means list N+1 and use older N sessions.".to_string(),
        "- \"last N sessions\" means list N and include latest session.".to_string(),
        "- Ask for session ID only after first fetch fails or exact ID is requested.".to_string(),
        String::new(),
        "Commands:".to_string(),
        "- `chorus read --agent <target-agent> --cwd <project-path> --include-user --json` for live status checks".to_string(),
        "- `chorus read --agent <target-agent> --cwd <project-path> --json` for assistant-only handoff/output reads".to_string(),
        "- `chorus list --agent <agent> --cwd <project-path> --json`".to_string(),
        "- `chorus search \"<query>\" --agent <agent> --cwd <project-path> --json`".to_string(),
        "- `chorus compare --source codex --source gemini --source claude --cwd <project-path> --json`".to_string(),
        String::new(),
        "Use evidence from command output and explicitly report missing session data.".to_string(),
    ]
    .join("\n")
}

fn default_setup_intents() -> String {
    [
        "# Agent Chorus Intents",
        "",
        "Use these triggers consistently across agents and providers:",
        "",
        "- \"What is Claude doing?\"",
        "- \"What did Gemini say?\"",
        "- \"Compare Codex and Claude outputs\"",
        "- \"Read session <id> from Codex\"",
        "",
        "Canonical response behavior:",
        "1. Default to latest session in current project (`--cwd`) when no session is specified.",
        "2. \"past session\" means previous session; \"past N sessions\" excludes latest; \"last N sessions\" includes latest.",
        "3. Fetch evidence with `chorus read` first, then `chorus list/search` only if needed.",
        "4. For multi-source checks use `chorus compare` or `chorus report`.",
        "5. Do not ask for session ID before first fetch unless user requested exact ID.",
        "6. Do not invent missing context; explicitly call out missing sessions.",
        "",
        "Core protocol reference: https://github.com/cote-star/agent-chorus/blob/main/PROTOCOL.md.",
    ]
    .join("\n")
}

fn is_system_directory(dir: &Path) -> bool {
    let s = dir.to_string_lossy();
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

fn relative_path(base: &Path, target: &Path) -> Option<String> {
    let base = base.canonicalize().unwrap_or_else(|_| base.to_path_buf());
    // target may not exist yet — canonicalize only components that do, else fall back.
    let target_buf = target.to_path_buf();
    let target = target_buf
        .canonicalize()
        .unwrap_or_else(|_| target_buf.clone());
    let base_components: Vec<_> = base.components().collect();
    let target_components: Vec<_> = target.components().collect();
    // Find common prefix
    let mut i = 0;
    while i < base_components.len()
        && i < target_components.len()
        && base_components[i] == target_components[i]
    {
        i += 1;
    }
    let ups = base_components.len() - i;
    let mut parts: Vec<String> = std::iter::repeat("..".to_string()).take(ups).collect();
    for comp in &target_components[i..] {
        parts.push(comp.as_os_str().to_string_lossy().to_string());
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

fn is_command_available(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn claude_plugin_installed() -> bool {
    Command::new("claude")
        .args(["plugin", "list"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("agent-chorus"))
        .unwrap_or(false)
}

fn install_claude_plugin(package_root: &Path, dry_run: bool) -> (&'static str, String) {
    if dry_run {
        return ("planned", "Would install agent-chorus Claude Code plugin".to_string());
    }
    let marketplace_ok = Command::new("claude")
        .args(["plugin", "marketplace", "add"])
        .arg(package_root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    let install_ok = marketplace_ok
        && Command::new("claude")
            .args(["plugin", "install", "agent-chorus"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
    if install_ok {
        ("created", "Installed agent-chorus Claude Code plugin".to_string())
    } else {
        let cmd = format!(
            "claude plugin marketplace add \"{}\" && claude plugin install agent-chorus",
            package_root.display()
        );
        (
            "error",
            format!("Plugin install failed — run manually: {}", cmd),
        )
    }
}

/// Package root (equivalent of Node's `path.resolve(__dirname, '..')` → the
/// repo root that contains `scripts/`, `plugin.json`, etc.).
///
/// For a cargo-built binary we walk up from the Cargo manifest dir (`cli/`)
/// to its parent, matching the layout of this repo.
fn package_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or(manifest_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::path::PathBuf;

    fn test_dir(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("chorus_setup_test_{}", name));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create test dir");
        dir
    }

    #[test]
    fn setup_rejects_system_directory() {
        let err = run_setup("/etc/somewhere", true, false, false).unwrap_err();
        assert!(err.to_string().contains("system directory"));
    }

    #[test]
    fn setup_creates_intents_and_snippets_in_tempdir() {
        let dir = test_dir("creates");
        let result = run_setup(&dir.to_string_lossy(), false, false, false).unwrap();

        let intents = dir.join(".agent-chorus").join("INTENTS.md");
        assert!(intents.exists(), "INTENTS.md should exist");
        let content = fs::read_to_string(&intents).unwrap();
        assert!(content.starts_with("# Agent Chorus Intents"));

        for agent in ["codex", "claude", "gemini"] {
            let snippet = dir
                .join(".agent-chorus")
                .join("providers")
                .join(format!("{}.md", agent));
            assert!(snippet.exists(), "snippet for {} should exist", agent);
        }
        for file in ["AGENTS.md", "CLAUDE.md", "GEMINI.md"] {
            let p = dir.join(file);
            assert!(p.exists(), "{} should exist", file);
            let c = fs::read_to_string(&p).unwrap();
            assert!(c.contains("agent-chorus:"), "{} should contain managed marker", file);
        }

        // Because no project markers exist, we expect exactly one warning.
        assert_eq!(result.warnings.len(), 1);
        // Each provider wrote its snippet + integration; plus INTENTS + gitignore + plugin = 9+
        assert!(result.operations.len() >= 9);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn setup_dry_run_writes_nothing() {
        let dir = test_dir("dryrun");
        let result = run_setup(&dir.to_string_lossy(), true, false, false).unwrap();
        assert!(result.dry_run);
        assert!(!dir.join(".agent-chorus").join("INTENTS.md").exists());
        assert!(!dir.join("AGENTS.md").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn setup_idempotent_without_force() {
        let dir = test_dir("idempotent");
        run_setup(&dir.to_string_lossy(), false, false, false).unwrap();
        let second = run_setup(&dir.to_string_lossy(), false, false, false).unwrap();
        let has_unchanged = second
            .operations
            .iter()
            .any(|op| op.get("status").and_then(|s| s.as_str()) == Some("unchanged"));
        assert!(has_unchanged, "second run should mark some operations unchanged");
        let _ = fs::remove_dir_all(&dir);
    }
}
