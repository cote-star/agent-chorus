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

// Agents enumerated for session-discovery checks. `cursor` is intentionally
// absent: it has two surfaces (CLI JSONL and IDE SQLite) reported by the
// `cursor_session_checks` helper below, not a single combined `sessions_cursor`.
// `hermes` is also absent: it's a provisional adapter whose presence we report
// via the `hermes_surface_check` helper so we can downgrade to `info` when the
// hermes data directory is absent (F12 parity with cursor).
const ALL_AGENTS: &[&str] = &["codex", "gemini", "claude"];

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

    // Setup scaffolding. The integration/snippet/intents checks emit `info`
    // rather than `warn` when the repo has not been initialized via
    // `chorus setup` — un-setup is intentional state, not broken state.
    //
    // Initialization is detected by the presence of either INTENTS.md or
    // the providers/ directory under .agent-chorus/. The bare .agent-chorus/
    // directory alone is *not* a setup signal: the messaging subsystem
    // creates .agent-chorus/messages/ for inbox storage on first `send`,
    // independent of any setup step.
    let setup_root = cwd_path.join(".agent-chorus");
    let setup_initialized = setup_root.join("INTENTS.md").exists()
        || setup_root.join("providers").exists();
    let absent_status = if setup_initialized { "warn" } else { "info" };
    let intents_path = setup_root.join("INTENTS.md");
    push(
        &mut checks,
        "setup_intents",
        if intents_path.exists() { "pass" } else { absent_status },
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
            if snippet_path.exists() { "pass" } else { absent_status },
            &fmt_existence(&snippet_path),
        );

        let target_path = cwd_path.join(provider.target_file);
        if !target_path.exists() {
            push(
                &mut checks,
                &format!("integration_{}", provider.agent),
                absent_status,
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
            if present { "pass" } else { absent_status },
            &if present {
                format!("Managed block present in {}", target_path.display())
            } else {
                format!("Managed block missing in {}", target_path.display())
            },
        );
    }

    // Session discovery per agent.
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

    // Cursor has two on-disk surfaces; report each independently. The
    // surface check answers "is this surface reachable from this host?",
    // not "are there sessions for this specific cwd?" — pass-with-no-cwd
    // matches Node's semantic.
    cursor_surface_checks(&mut checks);
    hermes_surface_check(&mut checks);
    env_override_checks(&mut checks);

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

    // Git hooks path + pre-push.
    //
    // F3: doctor reports the *local* health of this install in this cwd.
    // If the cwd is not a git repository, neither hooks_path nor pre_push
    // checks are meaningful — `git config core.hooksPath` would resolve to
    // a global value (the user's `~/.git-hooks` or similar), and we'd
    // truthfully report a hook as "installed" even though the cwd has no
    // `.git/` at all. That's a local lie. Gate both checks on the cwd
    // actually being a git repo and report `info` otherwise.
    if is_git_repo(cwd_path) {
        let configured = git_hooks_path(cwd_path);
        let (effective_path, source) = match configured.as_deref() {
            Some(hp) => (hp.to_string(), "configured"),
            None => (".git/hooks".to_string(), "default"),
        };
        push(
            &mut checks,
            "context_pack_hooks_path",
            "info",
            &format!("Effective git hooks path: {} ({})", effective_path, source),
        );
        let pre_push = if Path::new(&effective_path).is_absolute() {
            PathBuf::from(&effective_path).join("pre-push")
        } else {
            cwd_path.join(&effective_path).join("pre-push")
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
    } else {
        push(
            &mut checks,
            "context_pack_hooks_path",
            "info",
            "cwd is not a git repository; git hooks checks skipped",
        );
        push(
            &mut checks,
            "context_pack_pre_push",
            "info",
            "cwd is not a git repository; pre-push hook check skipped",
        );
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

fn cursor_surface_checks(checks: &mut Vec<Check>) {
    // F12: cursor-cli and Cursor-IDE surfaces are independently optional.
    // When *neither* surface has sessions AND the surface's data directory
    // doesn't exist, the user simply hasn't installed cursor-agent or the
    // Cursor IDE in any usable way — that's intentional state, not broken
    // state. Report `info` in that case. Report `warn` only when the data
    // directory exists but contains zero sessions (meaning the user has
    // the tool installed but produces no sessions — worth flagging).
    let cli_base = crate::agents::cursor_base_dir_public();
    let app_base = crate::cursor_app::cursor_app_base_dir();

    let (cli_status, cli_detail) = if !cli_base.exists() {
        (
            "info",
            format!(
                "cursor-agent CLI not configured (data directory absent: {})",
                cli_base.display()
            ),
        )
    } else if crate::agents::list_cursor_cli_sessions_count(None, 1) > 0 {
        ("pass", "At least one cursor-agent CLI transcript discovered".to_string())
    } else {
        (
            "warn",
            format!("No cursor-agent CLI transcripts discovered at {}", cli_base.display()),
        )
    };
    push(checks, "sessions_cursor_cli", cli_status, &cli_detail);

    let (app_status, app_detail) = if !app_base.exists() {
        (
            "info",
            format!(
                "Cursor IDE not configured (data directory absent: {})",
                app_base.display()
            ),
        )
    } else if !crate::cursor_app::collect_cursor_app_sessions(&app_base).is_empty() {
        ("pass", "At least one Cursor IDE store.db discovered".to_string())
    } else {
        (
            "warn",
            format!("No Cursor IDE store.db sessions discovered at {}", app_base.display()),
        )
    };
    push(checks, "sessions_cursor_app", app_status, &app_detail);
}

fn hermes_surface_check(checks: &mut Vec<Check>) {
    // F12 parity: hermes is provisional. When its data directory is
    // absent, the user simply hasn't installed hermes — report `info`,
    // not `warn`. `warn` is reserved for "directory exists but no
    // sessions" (installed but quiet).
    let base = crate::agents::hermes_base_dir_public();
    let (status, detail) = if !base.exists() {
        (
            "info",
            format!(
                "Hermes not configured (data directory absent: {})",
                base.display()
            ),
        )
    } else {
        match crate::adapters::get_adapter("hermes")
            .and_then(|a| a.list_sessions(None, 1).ok())
        {
            Some(entries) if !entries.is_empty() => {
                ("pass", "At least one hermes session discovered".to_string())
            }
            _ => (
                "warn",
                format!("No hermes sessions discovered at {}", base.display()),
            ),
        }
    };
    push(checks, "sessions_hermes", status, &detail);
}

/// F2: env-var overrides pointing at non-existent directories produce
/// silent partial coverage that looks identical to a working install.
/// Doctor explicitly flags these as `warn` so users know their env is
/// misconfigured. The override variable name and the dangling path are
/// both included in the detail for easy diagnosis.
fn env_override_checks(checks: &mut Vec<Check>) {
    let overrides = [
        ("CHORUS_CODEX_SESSIONS_DIR", "codex"),
        ("BRIDGE_CODEX_SESSIONS_DIR", "codex (legacy)"),
        ("CHORUS_CLAUDE_PROJECTS_DIR", "claude"),
        ("BRIDGE_CLAUDE_PROJECTS_DIR", "claude (legacy)"),
        ("CHORUS_GEMINI_TMP_DIR", "gemini"),
        ("BRIDGE_GEMINI_TMP_DIR", "gemini (legacy)"),
        ("CHORUS_CURSOR_DATA_DIR", "cursor-agent CLI"),
        ("BRIDGE_CURSOR_DATA_DIR", "cursor-agent CLI (legacy)"),
        ("CHORUS_CURSOR_APP_DATA_DIR", "Cursor IDE"),
        ("BRIDGE_CURSOR_APP_DATA_DIR", "Cursor IDE (legacy)"),
    ];
    for (var, label) in overrides.iter() {
        if let Ok(value) = std::env::var(var) {
            if value.is_empty() {
                continue;
            }
            let expanded = if let Some(stripped) = value.strip_prefix("~/") {
                dirs::home_dir()
                    .map(|h| h.join(stripped))
                    .unwrap_or_else(|| std::path::PathBuf::from(&value))
            } else {
                std::path::PathBuf::from(&value)
            };
            if !expanded.exists() {
                push(
                    checks,
                    "env_override_dangling",
                    "warn",
                    &format!(
                        "{} ({}) points at non-existent directory: {}. Sessions from this adapter will be invisible until the env var is cleared or the directory exists.",
                        var, label, expanded.display()
                    ),
                );
            }
        }
    }
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

/// Whether the given directory (or any ancestor) is a git repository.
/// Used to gate the hooks-path / pre-push checks so we never claim a
/// hook is installed when the cwd has no `.git/` at all.
fn is_git_repo(cwd: &Path) -> bool {
    Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(cwd)
        .output()
        .map(|o| o.status.success())
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
