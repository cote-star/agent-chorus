//! Doctor — diagnostic checks across the agent-chorus install.
//!
//! Mirrors `runDoctor` in `scripts/read_session.cjs:2181`. Pure reads only:
//! probes each agent's config dir, reports setup + context-pack state, and
//! names missing hooks. No filesystem mutations.

use anyhow::Result;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::adapters;
use crate::agents::{claude_base_dir, codex_base_dir, gemini_tmp_base_dir};
use crate::update_check;
use crate::utils;

struct Provider {
    agent: &'static str,
    target_file: &'static str,
}

const PROVIDERS: &[Provider] = &[
    Provider { agent: "codex", target_file: "AGENTS.md" },
    Provider { agent: "claude", target_file: "CLAUDE.md" },
    Provider { agent: "gemini", target_file: "GEMINI.md" },
];

const ALL_AGENTS: &[&str] = &["codex", "gemini", "claude", "cursor"];

#[derive(Debug)]
pub struct Check {
    pub id: String,
    pub status: String,
    pub detail: String,
}

pub struct DoctorResult {
    pub cwd: String,
    pub overall: String,
    pub checks: Vec<Check>,
}

impl DoctorResult {
    pub fn to_json(&self) -> Value {
        let checks: Vec<Value> = self
            .checks
            .iter()
            .map(|c| {
                json!({
                    "id": c.id,
                    "status": c.status,
                    "detail": c.detail,
                })
            })
            .collect();

        json!({
            "cwd": self.cwd,
            "overall": self.overall,
            "checks": checks,
        })
    }
}

pub fn run_doctor(cwd: &str) -> Result<DoctorResult> {
    let cwd_path = Path::new(cwd);
    let cwd_str = cwd_path.to_string_lossy().to_string();
    let mut checks: Vec<Check> = Vec::new();

    let version = env!("CARGO_PKG_VERSION");
    push(&mut checks, "version", "pass", &format!("agent-chorus v{}", version));

    // Agent base dirs
    let codex_base = codex_base_dir();
    push(
        &mut checks,
        "codex_sessions_dir",
        if codex_base.exists() { "pass" } else { "warn" },
        &fmt_existence(&codex_base),
    );
    let claude_base = claude_base_dir();
    push(
        &mut checks,
        "claude_projects_dir",
        if claude_base.exists() { "pass" } else { "warn" },
        &fmt_existence(&claude_base),
    );
    let gemini_base = gemini_tmp_base_dir();
    push(
        &mut checks,
        "gemini_tmp_dir",
        if gemini_base.exists() { "pass" } else { "warn" },
        &fmt_existence(&gemini_base),
    );

    // Setup scaffolding
    let setup_root = cwd_path.join(".agent-chorus");
    let intents_path = setup_root.join("INTENTS.md");
    push(
        &mut checks,
        "setup_intents",
        if intents_path.exists() { "pass" } else { "warn" },
        &fmt_existence(&intents_path),
    );

    // Provider snippets + managed blocks
    for provider in PROVIDERS {
        let snippet_path = setup_root
            .join("providers")
            .join(format!("{}.md", provider.agent));
        push(
            &mut checks,
            &format!("snippet_{}", provider.agent),
            if snippet_path.exists() { "pass" } else { "warn" },
            &fmt_existence(&snippet_path),
        );

        let target_path = cwd_path.join(provider.target_file);
        if !target_path.exists() {
            push(
                &mut checks,
                &format!("integration_{}", provider.agent),
                "warn",
                &format!("Missing provider instruction file: {}", target_path.display()),
            );
            continue;
        }

        let marker = format!("agent-chorus:{}:start", provider.agent);
        let present = std::fs::read_to_string(&target_path)
            .map(|s| s.contains(&marker))
            .unwrap_or(false);
        push(
            &mut checks,
            &format!("integration_{}", provider.agent),
            if present { "pass" } else { "warn" },
            &if present {
                format!("Managed block present in {}", target_path.display())
            } else {
                format!("Managed block missing in {}", target_path.display())
            },
        );
    }

    // Session discovery per agent
    let normalized_cwd = utils::normalize_path(cwd)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| cwd.to_string());
    for agent in ALL_AGENTS {
        match adapters::get_adapter(agent) {
            Some(adapter) => match adapter.list_sessions(Some(&normalized_cwd), 1) {
                Ok(entries) if !entries.is_empty() => push(
                    &mut checks,
                    &format!("sessions_{}", agent),
                    "pass",
                    &format!("At least one {} session discovered", agent),
                ),
                Ok(_) => push(
                    &mut checks,
                    &format!("sessions_{}", agent),
                    "warn",
                    &format!("No {} sessions discovered", agent),
                ),
                Err(e) => push(
                    &mut checks,
                    &format!("sessions_{}", agent),
                    "fail",
                    &e.to_string(),
                ),
            },
            None => push(
                &mut checks,
                &format!("sessions_{}", agent),
                "fail",
                "No adapter available",
            ),
        }
    }

    // Context pack state
    let pack_dir = cwd_path.join(".agent-context").join("current");
    let manifest_path = pack_dir.join("manifest.json");
    let pack_state = if !pack_dir.exists() {
        "UNINITIALIZED"
    } else if has_template_markers(&pack_dir) {
        "TEMPLATE"
    } else if manifest_path.exists() {
        "SEALED_VALID"
    } else {
        "UNINITIALIZED"
    };
    push(
        &mut checks,
        "context_pack_state",
        if pack_state == "UNINITIALIZED" { "warn" } else { "pass" },
        &format!("State: {}", pack_state),
    );
    match pack_state {
        "UNINITIALIZED" => push(
            &mut checks,
            "context_pack_guidance",
            "warn",
            "Run `chorus agent-context init` to start",
        ),
        "TEMPLATE" => push(
            &mut checks,
            "context_pack_guidance",
            "warn",
            "Context pack in template mode. Fill sections then run `chorus agent-context seal`",
        ),
        _ => {}
    }

    // Update check (best-effort; short timeout)
    let update_status = update_check::check_now_for_doctor();
    let detail = if let Some(err) = update_status.error.as_deref() {
        format!("Error: {}", err)
    } else if update_status.up_to_date {
        format!("Up to date ({})", update_status.current)
    } else {
        format!(
            "Update available: {} \u{2192} {}",
            update_status.current,
            update_status.latest.as_deref().unwrap_or("?")
        )
    };
    push(
        &mut checks,
        "update_status",
        if update_status.error.is_some() { "warn" } else { "pass" },
        &detail,
    );

    // Claude plugin check
    if is_command_available("claude") {
        let installed = claude_plugin_installed();
        push(
            &mut checks,
            "claude_plugin",
            if installed { "pass" } else { "warn" },
            if installed {
                "agent-chorus Claude Code plugin installed".to_string()
            } else {
                "Claude Code plugin not installed — run: chorus setup".to_string()
            }
            .as_str(),
        );
    } else {
        push(
            &mut checks,
            "claude_plugin",
            "warn",
            "claude CLI not found — Claude Code plugin status unknown",
        );
    }

    // Git hooks path + pre-push
    let hooks_path = git_hooks_path(cwd_path);
    match hooks_path {
        Some(ref hp) => {
            push(
                &mut checks,
                "context_pack_hooks_path",
                if hp == ".githooks" { "pass" } else { "warn" },
                &if hp == ".githooks" {
                    "Git hooks path set to .githooks".to_string()
                } else {
                    format!(
                        "Git hooks path is {} (expected .githooks for context-pack pre-push automation)",
                        hp
                    )
                },
            );
            let pre_push = if Path::new(hp).is_absolute() {
                PathBuf::from(hp).join("pre-push")
            } else {
                cwd_path.join(hp).join("pre-push")
            };
            push(
                &mut checks,
                "context_pack_pre_push",
                if pre_push.exists() { "pass" } else { "warn" },
                &if pre_push.exists() {
                    format!("Found: {}", pre_push.display())
                } else {
                    format!(
                        "Missing: {} (run: chorus agent-context install-hooks)",
                        pre_push.display()
                    )
                },
            );
        }
        None => push(
            &mut checks,
            "context_pack_hooks_path",
            "warn",
            "Git hooks path not configured",
        ),
    }

    let has_fail = checks.iter().any(|c| c.status == "fail");
    let has_warn = checks.iter().any(|c| c.status == "warn");
    let overall = if has_fail {
        "fail"
    } else if has_warn {
        "warn"
    } else {
        "pass"
    };

    Ok(DoctorResult {
        cwd: cwd_str,
        overall: overall.to_string(),
        checks,
    })
}

fn push(checks: &mut Vec<Check>, id: &str, status: &str, detail: &str) {
    checks.push(Check {
        id: id.to_string(),
        status: status.to_string(),
        detail: detail.to_string(),
    });
}

fn fmt_existence(p: &Path) -> String {
    if p.exists() {
        format!("Found: {}", p.display())
    } else {
        format!("Missing: {}", p.display())
    }
}

fn has_template_markers(pack_dir: &Path) -> bool {
    let entries = match std::fs::read_dir(pack_dir) {
        Ok(it) => it,
        Err(_) => return false,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s == "md")
                .unwrap_or(false)
        {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if content.contains("<!-- AGENT:") {
                    return true;
                }
            }
        }
    }
    false
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

fn git_hooks_path(cwd: &Path) -> Option<String> {
    let out = Command::new("git")
        .args(["config", "--get", "core.hooksPath"])
        .current_dir(cwd)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

pub fn print_text(result: &DoctorResult) {
    println!("Agent Chorus doctor — overall: {}", result.overall);
    println!("CWD: {}", result.cwd);
    for check in &result.checks {
        println!("  [{}] {} — {}", check.status, check.id, check.detail);
    }
}
