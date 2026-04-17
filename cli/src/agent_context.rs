use anyhow::{anyhow, Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const ZERO_SHA: &str = "0000000000000000000000000000000000000000";
const MAX_CHANGED_FILES_DISPLAYED: usize = 12;
const REQUIRED_FILES: &[&str] = &[
    "00_START_HERE.md",
    "10_SYSTEM_OVERVIEW.md",
    "20_CODE_MAP.md",
    "30_BEHAVIORAL_INVARIANTS.md",
    "40_OPERATIONS_AND_RELEASE.md",
];
const STRUCTURED_FILES: &[&str] = &[
    "routes.json",
    "completeness_contract.json",
    "reporting_rules.json",
];
const TASK_FAMILIES: &[&str] = &["lookup", "impact_analysis", "planning", "diagnosis"];

// P8 — hostile input & platform safety bounds.
/// Maximum bytes we will read into a pack file (F23). Files larger than this
/// are skipped with a warning so seal cannot OOM on a rogue asset.
const MAX_PACK_FILE_BYTES: u64 = 5_000_000;
/// Bytes inspected for NUL detection when classifying a file as binary (F19).
const BINARY_SNIFF_BYTES: usize = 8_192;
/// Maximum directory walk depth (F20). Guards against symlink loops and
/// pathological nested-dir layouts when resolving glob patterns.
const MAX_WALK_DEPTH: usize = 20;

pub struct BuildOptions {
    pub reason: Option<String>,
    pub base: Option<String>,
    pub head: Option<String>,
    pub pack_dir: Option<String>,
    /// Reserved: will be used when `build` constructs the start-here template with change summaries.
    #[allow(dead_code)]
    pub changed_files: Vec<String>,
    pub force_snapshot: bool,
}

pub struct InitOptions {
    pub pack_dir: Option<String>,
    pub cwd: Option<String>,
    pub force: bool,
    /// P8/F20: when true, dereference symlinks whose canonical target escapes
    /// the repo root. Default false (safe).
    pub follow_symlinks: bool,
}

pub struct SealOptions {
    pub reason: Option<String>,
    pub base: Option<String>,
    pub head: Option<String>,
    pub pack_dir: Option<String>,
    pub cwd: Option<String>,
    pub force: bool,
    pub force_snapshot: bool,
    /// P8/F20: when true, dereference symlinks whose canonical target escapes
    /// the repo root. Default false (safe).
    pub follow_symlinks: bool,
}

struct FileMeta {
    path: String,
    /// P8/F21: lowercased copy of `path` for case-insensitive FS collision
    /// detection during verify. Written into the manifest alongside `path`.
    path_lower: String,
    sha256: String,
    bytes: u64,
    words: usize,
}

struct ManifestBundle {
    value: Value,
    stable_checksum: String,
    pack_checksum: String,
}

pub struct VerifyOptions {
    pub pack_dir: Option<String>,
    pub cwd: String,
    pub ci: bool,
    pub base: Option<String>,
    /// When true and the manifest is unreadable or fails pack_checksum, attempt
    /// to restore `current/` from the most recent intact snapshot (F32).
    pub repair: bool,
    /// When combined with `repair`, perform the restore without prompting for
    /// interactive confirmation via stdin.
    pub repair_yes: bool,
}

/// Result of running the freshness check as a reusable helper.
struct FreshnessResult {
    /// "pass", "warn", "skip", or "skipped"
    status: String,
    /// Context-relevant files that changed
    changed_files: Vec<String>,
    /// Whether .agent-context/current/ was touched in the diff
    pack_updated: bool,
    /// Reason when status is "skipped" (F24/F25/F27 — shallow-clone, initial-commit, non-git).
    /// None when status is pass/warn/skip.
    skipped_reason: Option<String>,
}

/// Detect whether `cwd` lives inside a shallow git clone. Returns `Ok(true)`
/// if `git rev-parse --is-shallow-repository` prints `true`. Non-zero exit
/// (e.g. non-git directory) returns `Ok(false)` so callers can perform their
/// own non-git detection.
fn is_shallow_repo(cwd: &Path) -> Result<bool> {
    let raw = run_git(&["rev-parse", "--is-shallow-repository"], cwd, true)?;
    Ok(raw.trim() == "true")
}

/// Detect whether `cwd` is inside a git repository. Uses `git rev-parse --git-dir`
/// which succeeds (non-empty stdout) inside any git repo and fails otherwise.
fn is_git_repo(cwd: &Path) -> bool {
    !run_git(&["rev-parse", "--git-dir"], cwd, true)
        .map(|s| s.trim().is_empty())
        .unwrap_or(true)
}

/// Count commits reachable from HEAD. Returns `Ok(None)` when the count
/// cannot be determined (e.g. no HEAD yet).
fn commit_count(cwd: &Path) -> Result<Option<u64>> {
    let raw = run_git(&["rev-list", "--count", "HEAD"], cwd, true)?;
    Ok(raw.trim().parse::<u64>().ok())
}

/// Resolve the current branch for manifest metadata. Returns `(branch, detached)`:
/// - `(Some(name), false)` for normal branches
/// - `(None, true)` when HEAD is detached (either `git symbolic-ref -q HEAD` fails
///   or `rev-parse --abbrev-ref HEAD` prints the literal `HEAD`)
/// - `(None, false)` only if git is absent entirely
fn resolve_branch(cwd: &Path) -> (Option<String>, bool) {
    // symbolic-ref fails on detached HEAD, succeeds otherwise.
    let symbolic = Command::new("git")
        .args(["symbolic-ref", "-q", "HEAD"])
        .current_dir(cwd)
        .output();
    let symbolic_ok = matches!(symbolic, Ok(ref out) if out.status.success());

    let abbrev = run_git(&["rev-parse", "--abbrev-ref", "HEAD"], cwd, true)
        .unwrap_or_default()
        .trim()
        .to_string();

    if !symbolic_ok || abbrev == "HEAD" {
        return (None, true);
    }
    if abbrev.is_empty() {
        return (None, false);
    }
    (Some(abbrev), false)
}

fn check_freshness_inner(base: &str, cwd: &Path) -> Result<FreshnessResult> {
    // F27: non-git directory → explicit skipped status rather than silent empty diff.
    if !is_git_repo(cwd) {
        return Ok(FreshnessResult {
            status: "skipped".to_string(),
            changed_files: Vec::new(),
            pack_updated: false,
            skipped_reason: Some("non-git".to_string()),
        });
    }

    // F24: shallow clone (CI `fetch-depth: 1`) makes `git diff origin/main...HEAD`
    // silently return empty. Surface this as an advisory skip instead of "pass".
    if is_shallow_repo(cwd).unwrap_or(false) {
        return Ok(FreshnessResult {
            status: "skipped".to_string(),
            changed_files: Vec::new(),
            pack_updated: false,
            skipped_reason: Some(
                "shallow-clone: increase fetch-depth to >=20".to_string(),
            ),
        });
    }

    // F25: initial commit → no HEAD~1 to diff against. Return explicit skipped
    // status rather than relying on the fallback diff quietly producing empty output.
    if let Some(1) = commit_count(cwd)? {
        return Ok(FreshnessResult {
            status: "skipped".to_string(),
            changed_files: Vec::new(),
            pack_updated: false,
            skipped_reason: Some("initial-commit".to_string()),
        });
    }

    let changed_files_raw = {
        let with_base = run_git(&["diff", "--name-only", &format!("{base}...HEAD")], cwd, true)?;
        if with_base.is_empty() {
            run_git(&["diff", "--name-only", "HEAD~1"], cwd, true)?
        } else {
            with_base
        }
    };

    let mut pack_touched = false;
    let mut relevant = Vec::new();

    for file_path in changed_files_raw.lines().map(|line| line.trim()).filter(|line| !line.is_empty()) {
        if file_path.starts_with(".agent-context/current/") {
            pack_touched = true;
            continue;
        }
        if is_context_relevant(file_path) {
            relevant.push(file_path.to_string());
        }
    }

    if relevant.is_empty() {
        return Ok(FreshnessResult {
            status: "pass".to_string(),
            changed_files: Vec::new(),
            pack_updated: pack_touched,
            skipped_reason: None,
        });
    }

    if pack_touched {
        return Ok(FreshnessResult {
            status: "pass".to_string(),
            changed_files: relevant,
            pack_updated: true,
            skipped_reason: None,
        });
    }

    Ok(FreshnessResult {
        status: "warn".to_string(),
        changed_files: relevant,
        pack_updated: false,
        skipped_reason: None,
    })
}

pub fn build(options: BuildOptions) -> Result<()> {
    // Wrapper: route to init or seal based on current pack state.
    let cwd = env::current_dir().context("Failed to resolve current directory")?;
    let repo_root = git_repo_root(&cwd)?;
    let pack_root = resolve_pack_root(&repo_root, options.pack_dir.as_deref());
    let current_dir = pack_root.join("current");

    if !current_dir.exists() || is_dir_empty(&current_dir)? {
        // No pack yet: initialize templates.
        return init(InitOptions {
            pack_dir: options.pack_dir,
            cwd: Some(cwd.display().to_string()),
            force: false,
            follow_symlinks: false,
        });
    }

    // Pack exists: seal existing content.
    seal(SealOptions {
        reason: options.reason,
        base: options.base,
        head: options.head,
        pack_dir: options.pack_dir,
        cwd: Some(cwd.display().to_string()),
        force: false,
        force_snapshot: options.force_snapshot,
        follow_symlinks: false,
    })
}

pub fn init(options: InitOptions) -> Result<()> {
    let cwd = options
        .cwd
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    // F27: non-git directory → fail loudly rather than silently producing an
    // ill-formed pack (empty branch / head_sha, no freshness signal).
    if !is_git_repo(&cwd) {
        return Err(anyhow!(
            "[context-pack] init failed: not a git repository (cwd: {})",
            cwd.display()
        ));
    }
    let repo_root = git_repo_root(&cwd)?;
    let repo_name = repo_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("repo")
        .to_string();
    let branch = run_git(&["rev-parse", "--abbrev-ref", "HEAD"], &repo_root, true)?
        .trim()
        .to_string();
    let head_sha = run_git(&["rev-parse", "HEAD"], &repo_root, true)?
        .trim()
        .to_string();

    let pack_root = resolve_pack_root(&repo_root, options.pack_dir.as_deref());
    let current_dir = pack_root.join("current");
    let guide_path = pack_root.join("GUIDE.md");
    let relevance_path = pack_root.join("relevance.json");

    if current_dir.exists() && !options.force {
        let mut has_files = false;
        for entry in fs::read_dir(&current_dir).with_context(|| {
            format!("Failed to read {}", current_dir.display())
        })? {
            if entry.is_ok() {
                has_files = true;
                break;
            }
        }
        if has_files {
            return Err(anyhow!(
                "[context-pack] init aborted: {} is not empty (use --force to overwrite)",
                rel_path(&current_dir, &repo_root)
            ));
        }
    }

    ensure_dir(&current_dir)?;
    ensure_dir(&pack_root)?;

    let generated_at = now_stamp();

    let templates = vec![
        (
            "00_START_HERE.md",
            build_template_start_here(&repo_name, &branch, &head_sha, &generated_at),
        ),
        ("10_SYSTEM_OVERVIEW.md", build_template_system_overview()),
        ("20_CODE_MAP.md", build_template_code_map()),
        ("30_BEHAVIORAL_INVARIANTS.md", build_template_invariants()),
        ("40_OPERATIONS_AND_RELEASE.md", build_template_operations()),
        ("routes.json", build_routes_json()),
        (
            "completeness_contract.json",
            build_completeness_contract_json(),
        ),
        ("reporting_rules.json", build_reporting_rules_json()),
        ("search_scope.json", build_search_scope_json()),
    ];

    for (name, content) in templates {
        write_text(&current_dir.join(name), &content)?;
    }

    if !relevance_path.exists() || options.force {
        write_text(&relevance_path, &default_relevance_json())?;
    }

    if !guide_path.exists() || options.force {
        write_text(&guide_path, &build_guide())?;
    }

    // Wire agent config files with context-pack routing instructions.
    let agent_configs = [
        (
            "CLAUDE.md",
            "agent-chorus:context-pack:claude",
            build_context_pack_routing_block("claude"),
        ),
        (
            "AGENTS.md",
            "agent-chorus:context-pack:codex",
            build_context_pack_routing_block("codex"),
        ),
        (
            "GEMINI.md",
            "agent-chorus:context-pack:gemini",
            build_context_pack_routing_block("gemini"),
        ),
    ];
    for (filename, marker, routing_block) in &agent_configs {
        upsert_context_pack_block(&repo_root.join(filename), routing_block, marker)?;
    }
    println!("[context-pack] agent config files wired (CLAUDE.md, AGENTS.md, GEMINI.md)");

    // Auto-install the pre-push hook so freshness warnings fire on every main push.
    match install_hooks(&repo_root.to_string_lossy(), false) {
        Ok(_) => println!("[context-pack] pre-push hook installed"),
        Err(_) => eprintln!("[context-pack] WARN: could not auto-install pre-push hook — run `chorus context-pack install-hooks` manually"),
    }

    println!(
        "[context-pack] init completed: {}",
        rel_path(&current_dir, &repo_root)
    );
    println!(
        "[context-pack] next: fill markdown + structured files, then run `chorus context-pack seal`"
    );

    Ok(())
}

fn check_content_quality(current_dir: &Path) -> Vec<String> {
    let mut warnings = Vec::new();

    // CODE_MAP: Risk column presence and non-empty values
    let code_map_path = current_dir.join("20_CODE_MAP.md");
    if let Ok(content) = fs::read_to_string(&code_map_path) {
        let has_risk_header = content.lines().any(|l| {
            let lower = l.to_lowercase();
            l.contains('|') && lower.contains("risk")
        });
        if !has_risk_header {
            warnings.push("20_CODE_MAP.md: no Risk column found — add a Risk column to each table row (e.g. \"Silent failure if missed\")".to_string());
        } else {
            let empty_risk_count = content.lines().filter(|l| {
                l.starts_with('|') &&
                !l.contains("---") &&
                !l.to_lowercase().contains("risk") &&
                {
                    let cells: Vec<&str> = l.split('|').map(|c| c.trim()).filter(|c| !c.is_empty()).collect();
                    cells.last().map(|c| c.is_empty()).unwrap_or(false)
                }
            }).count();
            if empty_risk_count > 0 {
                warnings.push(format!("20_CODE_MAP.md: {empty_risk_count} row(s) have an empty Risk column — fill with \"Silent failure if missed\", \"KeyError at runtime\", etc."));
            }
        }
    }

    // BEHAVIORAL_INVARIANTS: checklist has rows with explicit file paths
    let invariants_path = current_dir.join("30_BEHAVIORAL_INVARIANTS.md");
    if let Ok(content) = fs::read_to_string(&invariants_path) {
        let table_rows: Vec<&str> = content.lines().filter(|l| {
            l.starts_with('|') &&
            !l.contains("---") &&
            !l.to_lowercase().contains("change") &&
            !l.to_lowercase().contains("files that must")
        }).collect();
        if table_rows.is_empty() {
            warnings.push("30_BEHAVIORAL_INVARIANTS.md: Update Checklist has no rows — add at least one change-type row with explicit file paths".to_string());
        } else {
            let has_file_path = table_rows.iter().any(|row| {
                // Look for path-like tokens: word chars + slash or dot + word chars
                let re_like = row.contains('/') || row.chars().filter(|c| *c == '.').count() > 0;
                re_like && row.len() > 20
            });
            if !has_file_path {
                warnings.push("30_BEHAVIORAL_INVARIANTS.md: checklist rows do not appear to name explicit file paths — rows should list files by path, not just description".to_string());
            }
        }
    }

    // SYSTEM_OVERVIEW: runtime or silent failure modes section
    let overview_path = current_dir.join("10_SYSTEM_OVERVIEW.md");
    if let Ok(content) = fs::read_to_string(&overview_path) {
        let has_runtime = content.lines().any(|l| {
            l.starts_with("## ") && (l.to_lowercase().contains("runtime") || l.to_lowercase().contains("silent failure"))
        });
        if !has_runtime {
            warnings.push("10_SYSTEM_OVERVIEW.md: no Runtime Architecture or Silent Failure Modes section found — agents need runtime behavior documented to diagnose silent failures".to_string());
        }
    }

    warnings
}

/// Returns `true` when `repo_root/<rel_path>` is tracked as git-ignored.
/// `git check-ignore -q -- <path>` exits 0 when ignored, 1 when not.
/// Anything else (including non-git / git absent) is treated as "not ignored"
/// so this helper is safe to call from the seal path without surfacing noise.
fn is_git_ignored(repo_root: &Path, rel_path: &str) -> bool {
    match Command::new("git")
        .args(["check-ignore", "-q", "--", rel_path])
        .current_dir(repo_root)
        .output()
    {
        Ok(out) => out.status.code() == Some(0),
        Err(_) => false,
    }
}

/// F28: return warning strings for zone paths (search_scope.json `search_directories`
/// and `verification_shortcuts`) whose on-disk file is git-ignored. Silent on
/// missing config, unreadable JSON, or missing files — those cases are either
/// already covered by other seal-time validation or are explicit opt-outs.
fn collect_gitignore_zone_warnings(repo_root: &Path, current_dir: &Path) -> Vec<String> {
    let mut warnings = Vec::new();
    let search_scope_path = current_dir.join("search_scope.json");
    let scope = match read_json(&search_scope_path) {
        Ok(Some(v)) => v,
        _ => return warnings,
    };
    let families = match scope.get("task_families").and_then(|v| v.as_object()) {
        Some(obj) => obj,
        None => return warnings,
    };

    let mut seen_warnings = BTreeSet::new();

    for (_task_name, family_val) in families {
        let entry = match family_val.as_object() {
            Some(obj) => obj,
            None => continue,
        };

        if let Some(dirs) = entry.get("search_directories").and_then(|v| v.as_array()) {
            for dir in dirs.iter().filter_map(|v| v.as_str()) {
                // Resolve dir relative to repo root. Warn per-dir only (don't walk).
                let on_disk = repo_root.join(dir);
                if on_disk.exists() && is_git_ignored(repo_root, dir) {
                    let msg = format!(
                        "zone path '{dir}' matches git-ignored file '{dir}' — update .gitignore or remove the zone"
                    );
                    if seen_warnings.insert(msg.clone()) {
                        warnings.push(msg);
                    }
                }
            }
        }

        if let Some(shortcuts) = entry.get("verification_shortcuts").and_then(|v| v.as_object()) {
            for file_key in shortcuts.keys() {
                // Shortcut keys may use "path:line" form; path comes first.
                let rel = file_key.split(':').next().unwrap_or(file_key.as_str());
                let on_disk = repo_root.join(rel);
                if on_disk.exists() && is_git_ignored(repo_root, rel) {
                    let msg = format!(
                        "zone path '{file_key}' matches git-ignored file '{rel}' — update .gitignore or remove the zone"
                    );
                    if seen_warnings.insert(msg.clone()) {
                        warnings.push(msg);
                    }
                }
            }
        }
    }

    warnings
}

fn is_hook_installed(repo_root: &Path) -> bool {
    let hooks_path = run_git(&["config", "--get", "core.hooksPath"], repo_root, true)
        .unwrap_or_default();
    let hooks_dir = if hooks_path.trim().is_empty() {
        repo_root.join(".githooks")
    } else {
        let p = Path::new(hooks_path.trim());
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            repo_root.join(hooks_path.trim())
        }
    };
    let pre_push = hooks_dir.join("pre-push");
    if !pre_push.exists() {
        return false;
    }
    fs::read_to_string(&pre_push).map(|content| {
        content.contains("# --- agent-chorus:pre-push:start ---") ||
        content.contains("# --- agent-bridge:pre-push:start ---")
    }).unwrap_or(false)
}

pub fn seal(options: SealOptions) -> Result<()> {
    let cwd = options
        .cwd
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    // F27: non-git directory → fail loudly. Previous behavior silently produced a
    // manifest with empty branch/head_sha and skipped freshness with no explanation.
    if !is_git_repo(&cwd) {
        return Err(anyhow!(
            "[context-pack] seal failed: not a git repository (cwd: {})",
            cwd.display()
        ));
    }
    let repo_root = git_repo_root(&cwd)?;
    let repo_name = repo_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("repo")
        .to_string();
    // F26: detect detached HEAD so manifest `branch` is `null`+`detached: true`
    // rather than the literal string "HEAD".
    let (branch_opt, detached) = resolve_branch(&repo_root);
    let branch = branch_opt.clone().unwrap_or_default();
    if detached {
        eprintln!(
            "[context-pack] NOTICE: HEAD is detached — manifest recorded as branch: null, detached: true"
        );
    }
    let head_sha = match options.head.as_ref() {
        Some(sha) if !sha.trim().is_empty() => Some(sha.trim().to_string()),
        _ => {
            let discovered = run_git(&["rev-parse", "HEAD"], &repo_root, true)?;
            if discovered.trim().is_empty() {
                None
            } else {
                Some(discovered.trim().to_string())
            }
        }
    };

    let pack_root = resolve_pack_root(&repo_root, options.pack_dir.as_deref());
    let current_dir = pack_root.join("current");
    let snapshots_dir = pack_root.join("snapshots");
    let history_path = pack_root.join("history.jsonl");
    let manifest_path = current_dir.join("manifest.json");
    let lock_path = pack_root.join("seal.lock");

    if !current_dir.exists() {
        return Err(anyhow!(
            "[context-pack] seal failed: {} does not exist (run init first)",
            rel_path(&current_dir, &repo_root)
        ));
    }

    let _lock = acquire_lock(&lock_path)?;
    ensure_dir(&snapshots_dir)?;

    let required_files = required_files_for_mode(&current_dir);

    for file in &required_files {
        let path = current_dir.join(file);
        if !path.exists() {
            return Err(anyhow!(
                "[context-pack] seal failed: missing required file {}",
                rel_path(&path, &repo_root)
            ));
        }
        if !options.force {
            // P8 — read via helper so a binary/non-UTF-8 required file raises
            // a clear error instead of panicking read_to_string.
            let content = read_file_for_pack(&path, &repo_root, options.follow_symlinks)
                .map_err(|e| anyhow!("[context-pack] seal failed: cannot read {}: {}", rel_path(&path, &repo_root), e))?;
            if content.contains("<!-- AGENT:") {
                return Err(anyhow!(
                    "[context-pack] seal failed: template markers remain in {} (use --force to override)",
                    rel_path(&path, &repo_root)
                ));
            }
        }
    }

    validate_structured_layer(&repo_root, &current_dir)?;

    let generated_at = now_stamp();
    let reason = options
        .reason
        .unwrap_or_else(|| "manual-seal".to_string());

    // Update 00_START_HERE.md snapshot metadata BEFORE collecting file checksums
    // so the manifest reflects the updated content.
    update_start_here_snapshot(&current_dir, branch.trim(), head_sha.as_deref(), &generated_at)?;

    let files_meta = collect_files_meta(
        &current_dir,
        &repo_root,
        &required_files,
        options.follow_symlinks,
    )?;

    let previous_manifest = read_json(&manifest_path)?;

    let manifest = build_manifest(
        &generated_at,
        &repo_root,
        &repo_name,
        branch.trim(),
        detached,
        head_sha.as_deref(),
        &reason,
        options.base.as_deref(),
        &Vec::new(),
        &files_meta,
    );

    write_text_atomic(
        &manifest_path,
        &format!("{}\n", serde_json::to_string_pretty(&manifest.value)?),
    )?;
    let previous_stable = previous_manifest
        .as_ref()
        .and_then(|value| value.get("stable_checksum"))
        .and_then(|value| value.as_str())
        .map(|value| value.to_string());
    let previous_head = previous_manifest
        .as_ref()
        .and_then(|value| value.get("head_sha"))
        .and_then(|value| value.as_str())
        .map(|value| value.to_string());

    let changed = options.force_snapshot
        || previous_manifest.is_none()
        || previous_stable.as_deref() != Some(manifest.stable_checksum.as_str())
        || previous_head != head_sha;

    let quality_warnings = check_content_quality(&current_dir);
    for w in &quality_warnings {
        eprintln!("[context-pack] WARN: {w}");
    }

    // F28: warn if any zone path (search_scope.json search_directories / verification_shortcuts)
    // resolves to a file that is git-ignored — a strong signal the pack and .gitignore disagree.
    for w in collect_gitignore_zone_warnings(&repo_root, &current_dir) {
        eprintln!("[context-pack] WARN: {w}");
    }

    if !is_hook_installed(&repo_root) {
        eprintln!("[context-pack] WARN: pre-push hook is not installed — run `chorus context-pack install-hooks` to enable staleness detection on main pushes");
    }

    if changed {
        let mut snapshot_id = format!(
            "{}_{}",
            compact_timestamp(&generated_at),
            short_sha(head_sha.as_deref())
        );
        let mut snapshot_dir = snapshots_dir.join(&snapshot_id);
        let mut counter = 1;
        while snapshot_dir.exists() {
            snapshot_id = format!(
                "{}_{}-{}",
                compact_timestamp(&generated_at),
                short_sha(head_sha.as_deref()),
                counter
            );
            snapshot_dir = snapshots_dir.join(&snapshot_id);
            counter += 1;
        }

        copy_dir_recursive(&current_dir, &snapshot_dir)?;

        let history_entry = json!({
            "snapshot_id": snapshot_id,
            "generated_at": generated_at,
            "branch": branch.trim(),
            "head_sha": head_sha,
            "base_sha": options.base,
            "reason": reason,
            "changed_files": Vec::<String>::new(),
            "pack_checksum": manifest.pack_checksum,
        });
        append_jsonl(&history_path, &history_entry)?;

        println!(
            "[context-pack] sealed: {} (snapshot {})",
            rel_path(&pack_root, &repo_root),
            history_entry.get("snapshot_id").and_then(|v| v.as_str()).unwrap_or("unknown")
        );
    } else {
        println!("[context-pack] unchanged; no new snapshot created");
    }

    Ok(())
}

pub fn sync_main(
    local_ref: &str,
    local_sha: &str,
    remote_ref: &str,
    remote_sha: &str,
) -> Result<()> {
    let cwd = env::current_dir().context("Failed to resolve current directory")?;
    let repo_root = git_repo_root(&cwd)?;

    if !is_main_push(local_ref, remote_ref) {
        println!("[context-pack] skipped (push is not targeting main)");
        return Ok(());
    }

    if local_sha.trim().is_empty() || is_zero_sha(local_sha) {
        println!("[context-pack] skipped (main deletion or empty local sha)");
        return Ok(());
    }

    let changed_files = compute_changed_files(&repo_root, Some(remote_sha), local_sha)?;
    let rules = load_relevance_rules(&repo_root);
    let relevant: Vec<&String> = changed_files
        .iter()
        .filter(|path| is_context_relevant_with_rules(path, &rules))
        .collect();

    if relevant.is_empty() {
        println!("[context-pack] skipped (no context-relevant file changes)");
        return Ok(());
    }

    // Advisory-only: warn but never block the push or auto-build
    eprintln!(
        "[context-pack] ADVISORY: context-relevant files changed on main push. \
         Update pack content with your agent, then run 'chorus context-pack seal'."
    );

    Ok(())
}

pub fn rollback(snapshot: Option<&str>, pack_dir: Option<&str>) -> Result<()> {
    let cwd = env::current_dir().context("Failed to resolve current directory")?;
    let repo_root = git_repo_root(&cwd)?;
    let pack_root = resolve_pack_root(&repo_root, pack_dir);
    let current_dir = pack_root.join("current");
    let snapshots_dir = pack_root.join("snapshots");
    let lock_path = pack_root.join("seal.lock");

    // Rollback mutates .agent-context/current/ — serialize against seal (F29).
    let _lock = acquire_lock(&lock_path)?;

    let mut snapshot_ids = fs::read_dir(&snapshots_dir)
        .with_context(|| format!("Failed to list snapshots at {}", snapshots_dir.display()))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().to_str().map(|s| s.to_string()))
        .collect::<Vec<_>>();
    snapshot_ids.sort();

    if snapshot_ids.is_empty() {
        return Err(anyhow!(
            "[context-pack] no snapshots found in {}",
            rel_path(&snapshots_dir, &repo_root)
        ));
    }

    let target_snapshot = snapshot
        .map(|value| value.to_string())
        .unwrap_or_else(|| snapshot_ids.last().cloned().unwrap_or_default());

    if !snapshot_ids.iter().any(|id| id == &target_snapshot) {
        // F55: if not found in the active snapshots directory, check the rotated
        // history index so rollback can still locate snapshots referenced in
        // archived history files.
        if !snapshot_known_to_history_index(&pack_root, &target_snapshot) {
            return Err(anyhow!(
                "[context-pack] snapshot not found: {}",
                target_snapshot
            ));
        }
    }

    let source_dir = snapshots_dir.join(&target_snapshot);
    if !source_dir.exists() {
        return Err(anyhow!(
            "[context-pack] snapshot directory missing: {} (tracked in history_index but files were pruned)",
            rel_path(&source_dir, &repo_root)
        ));
    }
    if current_dir.exists() {
        fs::remove_dir_all(&current_dir)
            .with_context(|| format!("Failed to clear {}", current_dir.display()))?;
    }
    ensure_dir(&current_dir)?;
    copy_dir_recursive(&source_dir, &current_dir)?;

    println!(
        "[context-pack] restored snapshot {} -> {}",
        target_snapshot,
        rel_path(&current_dir, &repo_root)
    );
    Ok(())
}

/// Check whether the requested snapshot ID appears in any rotated history file
/// recorded in `history_index.json`. Allows rollback to surface a useful error
/// when the snapshots directory and the history index have diverged (F55).
fn snapshot_known_to_history_index(pack_root: &Path, snapshot_id: &str) -> bool {
    let index_path = pack_root.join("history_index.json");
    let index = match read_json(&index_path) {
        Ok(Some(v)) => v,
        _ => return false,
    };
    let files = match index.get("files").and_then(|f| f.as_array()) {
        Some(a) => a,
        None => return false,
    };
    for entry in files {
        let name = match entry.get("name").and_then(|n| n.as_str()) {
            Some(n) => n,
            None => continue,
        };
        let rotated_path = pack_root.join(name);
        if let Ok(raw) = fs::read_to_string(&rotated_path) {
            for line in raw.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
                    if val.get("snapshot_id").and_then(|s| s.as_str()) == Some(snapshot_id) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

const HOOK_SENTINEL_START: &str = "# --- agent-chorus:pre-push:start ---";
const HOOK_SENTINEL_END: &str = "# --- agent-chorus:pre-push:end ---";
// Legacy sentinels for backward compatibility during migration
const LEGACY_HOOK_SENTINEL_START: &str = "# --- agent-bridge:pre-push:start ---";
const LEGACY_HOOK_SENTINEL_END: &str = "# --- agent-bridge:pre-push:end ---";

pub fn install_hooks(cwd: &str, dry_run: bool) -> Result<()> {
    let cwd_path = PathBuf::from(cwd);
    let repo_root = git_repo_root(&cwd_path)?;

    let existing_hooks_path = run_git(&["config", "--get", "core.hooksPath"], &repo_root, true)?;

    // Determine hooks directory — prefer existing if set, otherwise use .githooks
    let hooks_dir = if !existing_hooks_path.is_empty() {
        if existing_hooks_path != ".githooks" {
            println!(
                "[context-pack] NOTE: core.hooksPath is '{}'; appending chorus hook there.",
                existing_hooks_path
            );
        }
        repo_root.join(&existing_hooks_path)
    } else {
        repo_root.join(".githooks")
    };

    let pre_push_path = hooks_dir.join("pre-push");
    let chorus_section = format!(
        "{}\n{}\n{}",
        HOOK_SENTINEL_START,
        build_pre_push_hook_section(),
        HOOK_SENTINEL_END
    );

    let final_content = if pre_push_path.exists() {
        let existing = fs::read_to_string(&pre_push_path).unwrap_or_default();
        // Detect new or legacy sentinels
        let (has_sentinel, sentinel_start, sentinel_end_str) =
            if existing.contains(HOOK_SENTINEL_START) && existing.contains(HOOK_SENTINEL_END) {
                (true, HOOK_SENTINEL_START, HOOK_SENTINEL_END)
            } else if existing.contains(LEGACY_HOOK_SENTINEL_START) && existing.contains(LEGACY_HOOK_SENTINEL_END) {
                (true, LEGACY_HOOK_SENTINEL_START, LEGACY_HOOK_SENTINEL_END)
            } else {
                (false, "", "")
            };
        if has_sentinel {
            // Replace existing chorus section
            let start_idx = existing.find(sentinel_start).unwrap();
            let end_idx = existing.find(sentinel_end_str).unwrap() + sentinel_end_str.len();
            // Trim trailing newline after end sentinel if present
            let end_idx = if existing.as_bytes().get(end_idx) == Some(&b'\n') {
                end_idx + 1
            } else {
                end_idx
            };
            format!("{}{}\n{}", &existing[..start_idx], chorus_section, &existing[end_idx..])
        } else {
            // Append chorus section to existing hook
            let mut content = existing;
            if !content.ends_with('\n') {
                content.push('\n');
            }
            content.push('\n');
            content.push_str(&chorus_section);
            content.push('\n');
            content
        }
    } else {
        // Create new hook file with shebang
        format!("#!/usr/bin/env bash\nset -euo pipefail\n\n{}\n", chorus_section)
    };

    let content_unchanged = if pre_push_path.exists() {
        fs::read_to_string(&pre_push_path).unwrap_or_default() == final_content
    } else {
        false
    };

    if !dry_run {
        ensure_dir(&hooks_dir)?;
        write_text(&pre_push_path, &final_content)?;
        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&pre_push_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&pre_push_path, perms)?;
        }
        // Only set core.hooksPath if it wasn't already configured
        if existing_hooks_path.is_empty() {
            run_git(&["config", "core.hooksPath", ".githooks"], &repo_root, false)?;
        }
    }

    let status = if dry_run {
        "planned"
    } else if content_unchanged {
        "unchanged"
    } else {
        "updated"
    };
    println!(
        "[context-pack] {}: {}",
        status,
        rel_path(&pre_push_path, &repo_root)
    );
    if !dry_run {
        println!("[context-pack] pre-push hook is active");
    }

    Ok(())
}

pub fn verify(options: VerifyOptions) -> Result<()> {
    let cwd_path = PathBuf::from(&options.cwd);
    let repo_root = git_repo_root(&cwd_path).unwrap_or_else(|_| cwd_path.clone());
    let pack_root = resolve_pack_root(&repo_root, options.pack_dir.as_deref());
    let current_dir = pack_root.join("current");
    let manifest_path = current_dir.join("manifest.json");

    // Verify is intentionally lock-free: it only reads. Writes are serialized
    // through seal/rollback's `acquire_lock` (F30). If a future `--watch` mode
    // is added, it should take a shared read lock; single-shot verify does not.

    if !manifest_path.exists() {
        if options.repair {
            return run_repair(&repo_root, &pack_root, options.repair_yes);
        }
        if options.ci {
            let result = json!({
                "integrity": "fail",
                "freshness": "skip",
                "changed_files": [],
                "pack_updated": false,
                "exit_code": 1
            });
            println!("{}", serde_json::to_string_pretty(&result)?);
            std::process::exit(1);
        }
        return Err(anyhow!(
            "[agent-context] verify failed: manifest.json not found at {}",
            manifest_path.display()
        ));
    }

    let manifest_content = match fs::read_to_string(&manifest_path) {
        Ok(s) => s,
        Err(err) => {
            if options.repair {
                eprintln!(
                    "[agent-context] manifest unreadable ({}); attempting repair",
                    err
                );
                return run_repair(&repo_root, &pack_root, options.repair_yes);
            }
            return Err(err).with_context(|| {
                format!("Failed to read manifest at {}", manifest_path.display())
            });
        }
    };
    let manifest: serde_json::Value = match serde_json::from_str(&manifest_content) {
        Ok(m) => m,
        Err(err) => {
            if options.repair {
                eprintln!(
                    "[agent-context] manifest.json is malformed ({}); attempting repair",
                    err
                );
                return run_repair(&repo_root, &pack_root, options.repair_yes);
            }
            return Err(anyhow!("Failed to parse manifest.json: {}", err));
        }
    };

    let files = manifest.get("files").and_then(|f| f.as_array());
    if files.is_none() {
        if options.repair {
            eprintln!(
                "[agent-context] manifest.json missing 'files' array; attempting repair"
            );
            return run_repair(&repo_root, &pack_root, options.repair_yes);
        }
        if options.ci {
            let result = json!({
                "integrity": "fail",
                "freshness": "skip",
                "changed_files": [],
                "pack_updated": false,
                "exit_code": 1
            });
            println!("{}", serde_json::to_string_pretty(&result)?);
            std::process::exit(1);
        }
        return Err(anyhow!("[agent-context] verify failed: manifest has no 'files' array"));
    }

    // F31 TOCTOU mitigation: snapshot every file's bytes at one instant, then
    // hash and compare. If any file changes between snapshot and compare, we
    // re-hash once and warn.
    let files_arr = files.unwrap();
    let mut pass_count = 0usize;
    let mut fail_count = 0usize;

    for file_entry in files_arr {
        let file_path_str = file_entry
            .get("path")
            .and_then(|p| p.as_str())
            .unwrap_or("unknown");
        let expected_hash = file_entry
            .get("sha256")
            .and_then(|h| h.as_str())
            .unwrap_or("");
        let actual_path = current_dir.join(file_path_str);

        if !actual_path.exists() {
            if !options.ci {
                eprintln!("  FAIL  {}  (file missing)", file_path_str);
            }
            fail_count += 1;
            continue;
        }

        let actual_hash = match hash_file_stable(&actual_path) {
            Ok(h) => h,
            Err(err) => {
                if !options.ci {
                    eprintln!("  FAIL  {}  (read error: {})", file_path_str, err);
                }
                fail_count += 1;
                continue;
            }
        };

        if actual_hash == expected_hash {
            if !options.ci {
                println!("  PASS  {}", file_path_str);
            }
            pass_count += 1;
        } else {
            if !options.ci {
                eprintln!("  FAIL  {}  (checksum mismatch)", file_path_str);
            }
            fail_count += 1;
        }
    }

    // Verify pack_checksum if present
    if let Some(expected_pack_checksum) = manifest.get("pack_checksum").and_then(|c| c.as_str()) {
        let mut file_entries: Vec<String> = Vec::new();
        for f in files_arr {
            let p = f.get("path").and_then(|v| v.as_str()).unwrap_or("unknown");
            let h = f.get("sha256").and_then(|v| v.as_str()).unwrap_or("");
            file_entries.push(format!("{}:{}", p, h));
        }
        let combined = file_entries.join("\n");
        let actual_pack_checksum = sha256_hex(combined.as_bytes());
        if actual_pack_checksum == expected_pack_checksum {
            if !options.ci {
                println!("  PASS  pack_checksum");
            }
            pass_count += 1;
        } else {
            if options.repair {
                eprintln!(
                    "[agent-context] pack_checksum mismatch; attempting repair"
                );
                return run_repair(&repo_root, &pack_root, options.repair_yes);
            }
            if !options.ci {
                eprintln!("  FAIL  pack_checksum (mismatch)");
            }
            fail_count += 1;
        }
    }

    let integrity_passed = fail_count == 0;
    let integrity_status = if integrity_passed { "pass" } else { "fail" };

    // Run freshness check
    let base_ref = options.base.as_deref().unwrap_or("origin/main");
    let freshness = if options.ci || options.base.is_some() {
        match check_freshness_inner(base_ref, &cwd_path) {
            Ok(result) => result,
            Err(_) => FreshnessResult {
                status: "skip".to_string(),
                changed_files: Vec::new(),
                pack_updated: false,
                skipped_reason: None,
            },
        }
    } else {
        // When not in CI and no base specified, attempt freshness but treat errors as skip
        match check_freshness_inner(base_ref, &cwd_path) {
            Ok(result) => result,
            Err(_) => FreshnessResult {
                status: "skip".to_string(),
                changed_files: Vec::new(),
                pack_updated: false,
                skipped_reason: None,
            },
        }
    };

    if options.ci {
        let exit_code = if !integrity_passed || freshness.status == "warn" { 1 } else { 0 };
        let mut result_obj = serde_json::Map::new();
        result_obj.insert("integrity".to_string(), json!(integrity_status));
        result_obj.insert("freshness".to_string(), json!(freshness.status));
        result_obj.insert("changed_files".to_string(), json!(freshness.changed_files));
        result_obj.insert("pack_updated".to_string(), json!(freshness.pack_updated));
        if let Some(reason) = &freshness.skipped_reason {
            result_obj.insert("skipped_reason".to_string(), json!(reason));
        }
        result_obj.insert("exit_code".to_string(), json!(exit_code));
        println!("{}", serde_json::to_string_pretty(&Value::Object(result_obj))?);
        if exit_code != 0 {
            std::process::exit(exit_code);
        }
        return Ok(());
    }

    // Human-readable output
    let total = pass_count + fail_count;
    println!("\n  Results: {pass_count}/{total} passed");

    if !integrity_passed {
        eprintln!("[agent-context] verify: {} file(s) did not match", fail_count);
    } else {
        println!("  Context pack integrity verified.");
    }

    // Show freshness info in human-readable mode
    match freshness.status.as_str() {
        "pass" => {
            if freshness.changed_files.is_empty() {
                println!("  Freshness: PASS (no context-relevant files changed)");
            } else {
                println!("  Freshness: PASS (agent-context was updated)");
            }
        }
        "warn" => {
            println!(
                "  Freshness: WARNING — {} context-relevant file(s) changed but .agent-context/current/ was not updated:",
                freshness.changed_files.len()
            );
            for f in &freshness.changed_files {
                println!("    - {}", f);
            }
            println!("  Consider running: chorus agent-context build");
        }
        "skipped" => {
            if let Some(reason) = &freshness.skipped_reason {
                println!("  Freshness: skipped ({})", reason);
            } else {
                println!("  Freshness: skipped");
            }
        }
        _ => {
            println!("  Freshness: skipped (no git history available)");
        }
    }

    if !integrity_passed {
        Err(anyhow!("[agent-context] verify failed: {} file(s) did not match", fail_count))
    } else {
        Ok(())
    }
}

/// Hash a file, re-hashing once if the bytes change mid-read (F31 TOCTOU mitigation).
///
/// Reads the file bytes twice with a quick second attempt. If the hash is
/// stable on both reads we return it; otherwise we emit a warning and return
/// the second (later) hash so the verify comparison reflects the most recent
/// observable state.
fn hash_file_stable(path: &Path) -> Result<String> {
    let first = fs::read(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let first_hash = sha256_hex(&first);
    // Fast re-read to detect a racing writer. We read at most twice.
    let second = match fs::read(path) {
        Ok(b) => b,
        Err(_) => return Ok(first_hash),
    };
    let second_hash = sha256_hex(&second);
    if first_hash == second_hash {
        Ok(first_hash)
    } else {
        eprintln!(
            "[agent-context] WARN: {} changed during verify; using re-hashed value",
            path.display()
        );
        Ok(second_hash)
    }
}

/// Restore `current/` from the most recent intact snapshot (F32).
///
/// Scans `.agent-context/snapshots/` for directories containing a parseable
/// `manifest.json`. The most recent (lexicographic) one wins. Without `--yes`
/// the plan is printed and stdin confirmation is required; exit code 2 means
/// the user declined.
fn run_repair(repo_root: &Path, pack_root: &Path, yes: bool) -> Result<()> {
    let current_dir = pack_root.join("current");
    let snapshots_dir = pack_root.join("snapshots");

    if !snapshots_dir.exists() {
        return Err(anyhow!(
            "[agent-context] repair failed: no snapshots directory at {} (no recovery snapshot found)",
            rel_path(&snapshots_dir, repo_root)
        ));
    }

    let mut candidates: Vec<String> = fs::read_dir(&snapshots_dir)
        .with_context(|| format!("Failed to list snapshots at {}", snapshots_dir.display()))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().to_str().map(|s| s.to_string()))
        .collect();
    candidates.sort();
    candidates.reverse();

    let mut selected: Option<String> = None;
    for candidate in &candidates {
        let manifest = snapshots_dir.join(candidate).join("manifest.json");
        if manifest.exists() {
            if let Ok(raw) = fs::read_to_string(&manifest) {
                if serde_json::from_str::<Value>(&raw).is_ok() {
                    selected = Some(candidate.clone());
                    break;
                }
            }
        }
    }

    let snapshot_id = selected.ok_or_else(|| {
        anyhow!(
            "[agent-context] repair failed: no intact snapshot found in {} (no recovery snapshot found)",
            rel_path(&snapshots_dir, repo_root)
        )
    })?;
    let source_dir = snapshots_dir.join(&snapshot_id);

    println!(
        "[agent-context] repair plan: restore {} -> {}",
        rel_path(&source_dir, repo_root),
        rel_path(&current_dir, repo_root)
    );
    println!("  - clears current contents of the pack directory");
    println!(
        "  - copies files from snapshot {} into place (manifest.json included)",
        snapshot_id
    );

    if !yes {
        eprint!("Proceed with repair? [y/N] ");
        std::io::stderr().flush().ok();
        let mut buf = String::new();
        if std::io::stdin().read_line(&mut buf).is_err() {
            return Err(anyhow!(
                "[agent-context] repair aborted: could not read confirmation"
            ));
        }
        let answer = buf.trim().to_lowercase();
        if answer != "y" && answer != "yes" {
            eprintln!("[agent-context] repair declined by user");
            std::process::exit(2);
        }
    }

    // Serialize with seal/rollback (F29) so a concurrent seal can't race us.
    let lock_path = pack_root.join("seal.lock");
    let _lock = acquire_lock(&lock_path)?;

    if current_dir.exists() {
        fs::remove_dir_all(&current_dir)
            .with_context(|| format!("Failed to clear {}", current_dir.display()))?;
    }
    ensure_dir(&current_dir)?;
    copy_dir_recursive(&source_dir, &current_dir)?;

    println!(
        "[agent-context] repair completed: restored snapshot {} to {}",
        snapshot_id,
        rel_path(&current_dir, repo_root)
    );
    Ok(())
}

pub fn check_freshness(base: &str, cwd: &str) -> Result<()> {
    let cwd_path = PathBuf::from(cwd);
    let result = check_freshness_inner(base, &cwd_path)?;

    match result.status.as_str() {
        "pass" => {
            if result.changed_files.is_empty() {
                println!("PASS agent-context-freshness (no context-relevant files changed)");
            } else {
                println!("PASS agent-context-freshness (agent-context was updated)");
            }
        }
        "warn" => {
            println!(
                "WARNING: {} context-relevant file(s) changed but .agent-context/current/ was not updated:",
                result.changed_files.len()
            );
            for file_path in &result.changed_files {
                println!("  - {}", file_path);
            }
            println!();
            println!("Consider running: chorus agent-context build");
        }
        "skipped" => {
            if let Some(reason) = &result.skipped_reason {
                println!("SKIPPED agent-context-freshness ({reason})");
            } else {
                println!("SKIPPED agent-context-freshness");
            }
        }
        _ => {}
    }

    Ok(())
}

fn git_repo_root(cwd: &Path) -> Result<PathBuf> {
    let root = run_git(&["rev-parse", "--show-toplevel"], cwd, true)?;
    if root.trim().is_empty() {
        Ok(cwd.to_path_buf())
    } else {
        Ok(PathBuf::from(root.trim()))
    }
}

fn run_git(args: &[&str], cwd: &Path, allow_failure: bool) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .with_context(|| format!("Failed to run git {}", args.join(" ")))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else if allow_failure {
        Ok(String::new())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Err(anyhow!(
            "git {} failed: {}{}{}",
            args.join(" "),
            stderr,
            if !stderr.is_empty() && !stdout.is_empty() { "\n" } else { "" },
            stdout
        ))
    }
}

fn resolve_pack_root(repo_root: &Path, pack_dir: Option<&str>) -> PathBuf {
    let dir = pack_dir
        .map(|value| value.to_string())
        .or_else(|| env::var("CHORUS_CONTEXT_PACK_DIR").ok())
        .or_else(|| env::var("BRIDGE_CONTEXT_PACK_DIR").ok())
        .unwrap_or_else(|| ".agent-context".to_string());
    let dir_path = PathBuf::from(dir);
    if dir_path.is_absolute() {
        dir_path
    } else {
        repo_root.join(dir_path)
    }
}

fn ensure_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)
        .with_context(|| format!("Failed to create directory {}", path.display()))?;
    Ok(())
}

fn write_text(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    fs::write(path, text).with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

/// Atomically write `bytes` to `path`.
///
/// Writes to a sibling `*.tmp` file, fsyncs the contents, then renames into place.
/// On POSIX the rename is atomic, so either the old file is fully intact or the
/// new contents are visible — a partial write is never observable (F33).
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("Missing parent for {}", path.display()))?;
    ensure_dir(parent)?;
    let tmp_name = format!(
        ".{}.tmp.{}",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("context-pack.tmp"),
        std::process::id()
    );
    let tmp = parent.join(tmp_name);
    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)
            .with_context(|| format!("Failed to open {}", tmp.display()))?;
        file.write_all(bytes)
            .with_context(|| format!("Failed to write {}", tmp.display()))?;
        file.sync_all()
            .with_context(|| format!("Failed to fsync {}", tmp.display()))?;
    }
    fs::rename(&tmp, path)
        .with_context(|| format!("Failed to move {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}

fn write_text_atomic(path: &Path, text: &str) -> Result<()> {
    atomic_write(path, text.as_bytes())
}

/// Reserved: content generator for auto-fill build mode.
/// Will be wired when `build --auto-fill` is implemented.
#[allow(dead_code)]
fn read_package_version(path: &Path) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    value.get("version").and_then(|v| v.as_str()).map(|v| v.to_string())
}

/// Reserved: content generator for auto-fill build mode.
/// Will be wired when `build --auto-fill` is implemented.
#[allow(dead_code)]
fn parse_cargo_version(raw: &str) -> Option<String> {
    for line in raw.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("version") {
            let value = rest.trim();
            if let Some(eq_rest) = value.strip_prefix('=') {
                let candidate = eq_rest.trim().trim_matches('"').to_string();
                if !candidate.is_empty() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

fn compute_changed_files(repo_root: &Path, base: Option<&str>, head: &str) -> Result<Vec<String>> {
    if head.trim().is_empty() {
        return Ok(Vec::new());
    }

    let output = if base.map(|value| value.trim().is_empty() || is_zero_sha(value)).unwrap_or(true) {
        run_git(&["show", "--pretty=format:", "--name-only", head], repo_root, true)?
    } else {
        let range = format!("{}..{}", base.unwrap_or(""), head);
        run_git(&["diff", "--name-only", &range], repo_root, true)?
    };

    Ok(output
        .lines()
        .map(|line| line.trim().replace('\\', "/"))
        .filter(|line| !line.is_empty())
        .collect())
}

/// Reserved: content generator for auto-fill build mode.
/// Will be wired when `build --auto-fill` is implemented.
#[allow(dead_code)]
fn normalize_changed_files(files: &[String]) -> Vec<String> {
    let mut set = BTreeSet::new();
    for file in files {
        let normalized = file.trim().replace('\\', "/");
        if !normalized.is_empty() {
            set.insert(normalized);
        }
    }
    set.into_iter().collect()
}

/// Reserved: content generator for auto-fill build mode.
/// Will be wired when `build --auto-fill` is implemented.
#[allow(dead_code)]
fn summarize_path_counts(paths: &[String]) -> Vec<(String, usize)> {
    let mut buckets = vec![
        ("scripts/".to_string(), "scripts".to_string(), 0usize),
        ("cli/src/".to_string(), "cli/src".to_string(), 0usize),
        ("schemas/".to_string(), "schemas".to_string(), 0usize),
        ("fixtures/".to_string(), "fixtures".to_string(), 0usize),
        (".github/workflows/".to_string(), ".github/workflows".to_string(), 0usize),
        ("docs/".to_string(), "docs".to_string(), 0usize),
    ];

    for file in paths {
        for (prefix, _name, count) in &mut buckets {
            if file.starts_with(prefix.as_str()) {
                *count += 1;
                break;
            }
        }
    }

    buckets
        .into_iter()
        .filter(|(_, _, count)| *count > 0)
        .map(|(_, name, count)| (name, count))
        .collect()
}

/// Failure modes for [`read_file_for_pack`] that callers can downgrade to
/// warnings (skip file) instead of aborting seal. See P8 / F19, F20, F23.
#[derive(Debug)]
enum PackReadError {
    /// File contains NUL bytes in the first [`BINARY_SNIFF_BYTES`] bytes — we
    /// treat it as a binary blob and refuse to hash it (F19).
    LikelyBinary(PathBuf),
    /// File larger than [`MAX_PACK_FILE_BYTES`] — refuse to read to avoid
    /// OOM / slow hashing on pack-adjacent logs or assets (F23).
    TooLarge(PathBuf, u64),
    /// File is a symlink whose canonical target escapes the repo root (F20).
    /// Only raised when `follow_symlinks` is false.
    SymlinkEscape(PathBuf, PathBuf),
    /// Anything else filesystem-level (missing file, permissions).
    IoError(PathBuf, std::io::Error),
}

impl std::fmt::Display for PackReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackReadError::LikelyBinary(p) => {
                write!(f, "binary content (NUL bytes detected) at {}", p.display())
            }
            PackReadError::TooLarge(p, n) => {
                write!(f, "file too large ({} bytes, limit {}) at {}", n, MAX_PACK_FILE_BYTES, p.display())
            }
            PackReadError::SymlinkEscape(p, target) => {
                write!(f, "symlink {} escapes repo root (target: {})", p.display(), target.display())
            }
            PackReadError::IoError(p, e) => {
                write!(f, "io error reading {}: {}", p.display(), e)
            }
        }
    }
}

/// P8 — read a file that is destined for the pack (hashed into the manifest)
/// in a way that cannot panic on hostile input.
///
/// Behaviour:
/// - Refuses files whose canonical target is outside `repo_root`, unless
///   `follow_symlinks` is true (F20).
/// - Refuses files whose size exceeds [`MAX_PACK_FILE_BYTES`] (F23).
/// - Refuses files whose first [`BINARY_SNIFF_BYTES`] bytes contain NUL (F19).
/// - Otherwise reads as bytes and decodes with [`String::from_utf8_lossy`],
///   replacing invalid sequences with U+FFFD (F19 — never panic).
fn read_file_for_pack(
    path: &Path,
    repo_root: &Path,
    follow_symlinks: bool,
) -> std::result::Result<String, PackReadError> {
    // F20 — check symlink status before committing to the read.
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            let target = match fs::canonicalize(path) {
                Ok(t) => t,
                Err(e) => return Err(PackReadError::IoError(path.to_path_buf(), e)),
            };
            let root_canonical = fs::canonicalize(repo_root).unwrap_or_else(|_| repo_root.to_path_buf());
            if !target.starts_with(&root_canonical) && !follow_symlinks {
                return Err(PackReadError::SymlinkEscape(path.to_path_buf(), target));
            }
        }
        Ok(_) => {}
        Err(e) => return Err(PackReadError::IoError(path.to_path_buf(), e)),
    }

    // F23 — size guard.
    let metadata = fs::metadata(path)
        .map_err(|e| PackReadError::IoError(path.to_path_buf(), e))?;
    if metadata.len() > MAX_PACK_FILE_BYTES {
        return Err(PackReadError::TooLarge(path.to_path_buf(), metadata.len()));
    }

    // F19 — read as bytes, detect binary, decode lossily.
    let bytes = fs::read(path)
        .map_err(|e| PackReadError::IoError(path.to_path_buf(), e))?;
    let sniff_len = bytes.len().min(BINARY_SNIFF_BYTES);
    if bytes[..sniff_len].contains(&0u8) {
        return Err(PackReadError::LikelyBinary(path.to_path_buf()));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Check whether `target` resolves inside `repo_root`. Used by symlink policy
/// and glob-pattern sanitization. Best-effort canonicalization — if either
/// path cannot be canonicalized (e.g. the target does not exist yet), we
/// fall back to the as-given value.
#[allow(dead_code)]
fn is_within_repo_root(repo_root: &Path, target: &Path) -> bool {
    let root = fs::canonicalize(repo_root).unwrap_or_else(|_| repo_root.to_path_buf());
    let canonical = fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf());
    canonical.starts_with(&root)
}

/// P8/F22 — reject glob patterns that could resolve outside the repo root.
///
/// Conservative rules:
/// - Absolute paths (`/foo/**`, `C:\…`) are rejected.
/// - Patterns containing `..` are rejected; callers should not need to walk
///   out of the repo to describe pack scope.
/// - Other patterns are passed through — the glob library is responsible for
///   matching safety at resolution time.
fn validate_pack_glob(pattern: &str, _repo_root: &Path) -> Result<()> {
    let normalized = pattern.replace('\\', "/");
    if normalized.is_empty() {
        return Err(anyhow!(
            "[context-pack] invalid glob pattern: empty string"
        ));
    }
    if normalized.starts_with('/') {
        return Err(anyhow!(
            "[context-pack] invalid glob pattern {:?}: absolute paths are not allowed; use a repo-relative pattern",
            pattern
        ));
    }
    // Windows absolute path heuristic: "C:" style drive letter.
    let bytes = normalized.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && (bytes[0] as char).is_ascii_alphabetic() {
        return Err(anyhow!(
            "[context-pack] invalid glob pattern {:?}: absolute paths are not allowed; use a repo-relative pattern",
            pattern
        ));
    }
    // Reject any `..` segment — either leading or embedded, the pattern
    // could escape the repo root once resolved.
    for segment in normalized.split('/') {
        if segment == ".." {
            return Err(anyhow!(
                "[context-pack] invalid glob pattern {:?}: `..` path traversal is not allowed",
                pattern
            ));
        }
    }
    Ok(())
}

fn collect_files_meta(
    current_dir: &Path,
    repo_root: &Path,
    relative_paths: &[String],
    follow_symlinks: bool,
) -> Result<Vec<FileMeta>> {
    let mut out = Vec::new();
    for relative_path in relative_paths {
        let absolute_path = current_dir.join(relative_path);
        let content = match read_file_for_pack(&absolute_path, repo_root, follow_symlinks) {
            Ok(c) => c,
            Err(err) => {
                // P8 — skip hostile files with a clear warning rather than
                // panicking the entire seal.
                eprintln!("[context-pack] WARN: skipping pack file: {}", err);
                continue;
            }
        };
        let metadata = fs::metadata(&absolute_path)
            .with_context(|| format!("Failed to stat {}", absolute_path.display()))?;
        out.push(FileMeta {
            path: relative_path.clone(),
            path_lower: relative_path.to_lowercase(),
            sha256: sha256_hex(content.as_bytes()),
            bytes: metadata.len(),
            words: content.split_whitespace().count(),
        });
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn build_manifest(
    generated_at: &str,
    _repo_root: &Path,
    repo_name: &str,
    branch: &str,
    detached: bool,
    head_sha: Option<&str>,
    reason: &str,
    base_sha: Option<&str>,
    changed_files: &[String],
    files_meta: &[FileMeta],
) -> ManifestBundle {
    let pack_checksum_input = files_meta
        .iter()
        .map(|meta| format!("{}:{}", meta.path, meta.sha256))
        .collect::<Vec<_>>()
        .join("\n");
    let pack_checksum = sha256_hex(pack_checksum_input.as_bytes());

    let stable_input = files_meta
        .iter()
        .filter(|meta| meta.path != "00_START_HERE.md")
        .map(|meta| format!("{}:{}", meta.path, meta.sha256))
        .collect::<Vec<_>>()
        .join("\n");
    let stable_checksum = sha256_hex(stable_input.as_bytes());

    let words_total: usize = files_meta.iter().map(|meta| meta.words).sum();
    let bytes_total: u64 = files_meta.iter().map(|meta| meta.bytes).sum();

    let files = files_meta
        .iter()
        .map(|meta| {
            json!({
                "path": meta.path,
                // P8/F21: lowercased path for case-insensitive FS collision
                // detection on verify. Additive field; keeps existing shape.
                "path_lower": meta.path_lower,
                "sha256": meta.sha256,
                "bytes": meta.bytes,
                "words": meta.words,
            })
        })
        .collect::<Vec<_>>();

    // F26: detached HEAD → branch is null + detached: true rather than the literal "HEAD".
    let branch_value: Value = if detached || branch.is_empty() || branch == "HEAD" {
        Value::Null
    } else {
        Value::String(branch.to_string())
    };

    let value = json!({
        "schema_version": 1,
        "generated_at": generated_at,
        "repo_name": repo_name,
        "repo_root": ".",
        "branch": branch_value,
        "detached": detached,
        "head_sha": head_sha,
        "build_reason": reason,
        "base_sha": base_sha,
        "changed_files": changed_files,
        "files_count": files_meta.len(),
        "words_total": words_total,
        "bytes_total": bytes_total,
        "pack_checksum": pack_checksum,
        "stable_checksum": stable_checksum,
        "files": files,
    });

    ManifestBundle {
        value,
        stable_checksum,
        pack_checksum,
    }
}

// History rotation thresholds (F55).
const HISTORY_ROTATE_MAX_BYTES: u64 = 5 * 1024 * 1024;
const HISTORY_ROTATE_MAX_ENTRIES: usize = 1000;

fn append_jsonl(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    rotate_history_if_needed(path)?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("Failed to open {}", path.display()))?;
    writeln!(file, "{}", serde_json::to_string(value)?)
        .with_context(|| format!("Failed to append {}", path.display()))?;
    file.sync_all().ok();
    Ok(())
}

/// Rotate `history.jsonl` when it exceeds 5MB or 1000 lines (F55).
///
/// The active file is renamed to `history.jsonl.{N}` (next integer), a fresh
/// empty `history.jsonl` is created, and `history_index.json` is rewritten with
/// an entry describing the rotated file. Rollback consults the index to locate
/// snapshot IDs that live in historical files.
fn rotate_history_if_needed(history_path: &Path) -> Result<()> {
    if !history_path.exists() {
        return Ok(());
    }
    let metadata = match fs::metadata(history_path) {
        Ok(m) => m,
        Err(_) => return Ok(()),
    };
    let size = metadata.len();
    let line_count = if size > 0 {
        fs::read_to_string(history_path)
            .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count())
            .unwrap_or(0)
    } else {
        0
    };

    if size < HISTORY_ROTATE_MAX_BYTES && line_count < HISTORY_ROTATE_MAX_ENTRIES {
        return Ok(());
    }

    let parent = history_path
        .parent()
        .ok_or_else(|| anyhow!("Missing parent for {}", history_path.display()))?;
    let base_name = history_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("history.jsonl");

    // Find next rotation index.
    let mut next_index: u32 = 1;
    if let Ok(entries) = fs::read_dir(parent) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if let Some(rest) = name.strip_prefix(&format!("{}.", base_name)) {
                    if let Ok(n) = rest.parse::<u32>() {
                        if n >= next_index {
                            next_index = n + 1;
                        }
                    }
                }
            }
        }
    }

    let rotated_name = format!("{}.{}", base_name, next_index);
    let rotated_path = parent.join(&rotated_name);

    // Gather first/last snapshot IDs from the current file to record in index.
    let (first_id, last_id, entries) = summarize_history_file(history_path);

    fs::rename(history_path, &rotated_path).with_context(|| {
        format!(
            "Failed to rotate {} -> {}",
            history_path.display(),
            rotated_path.display()
        )
    })?;

    // Start a fresh empty file so subsequent appends land cleanly.
    atomic_write(history_path, b"")?;

    // Rewrite the history index atomically.
    let index_path = parent.join("history_index.json");
    let mut files_arr: Vec<Value> = match read_json(&index_path)? {
        Some(v) => v
            .get("files")
            .and_then(|f| f.as_array())
            .cloned()
            .unwrap_or_default(),
        None => Vec::new(),
    };
    files_arr.push(json!({
        "name": rotated_name,
        "first_id": first_id,
        "last_id": last_id,
        "entries": entries,
    }));
    let index_value = json!({
        "schema_version": 1,
        "active": base_name,
        "files": files_arr,
    });
    atomic_write(
        &index_path,
        format!("{}\n", serde_json::to_string_pretty(&index_value)?).as_bytes(),
    )?;

    eprintln!(
        "[context-pack] rotated history: {} -> {} ({} entries, {} bytes)",
        base_name, rotated_name, entries, size
    );

    Ok(())
}

fn summarize_history_file(path: &Path) -> (Option<String>, Option<String>, usize) {
    let raw = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return (None, None, 0),
    };
    let mut first_id: Option<String> = None;
    let mut last_id: Option<String> = None;
    let mut count = 0usize;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        count += 1;
        if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
            let id = v
                .get("snapshot_id")
                .and_then(|s| s.as_str())
                .map(|s| s.to_string());
            if first_id.is_none() {
                first_id = id.clone();
            }
            if id.is_some() {
                last_id = id;
            }
        }
    }
    (first_id, last_id, count)
}

fn read_json(path: &Path) -> Result<Option<Value>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let value = serde_json::from_str::<Value>(&raw)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(Some(value))
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<()> {
    ensure_dir(destination)?;
    for entry in fs::read_dir(source)
        .with_context(|| format!("Failed to read {}", source.display()))?
    {
        let entry = entry.with_context(|| format!("Failed to read entry in {}", source.display()))?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_dir_recursive(&source_path, &destination_path)?;
        } else {
            if let Some(parent) = destination_path.parent() {
                ensure_dir(parent)?;
            }
            fs::copy(&source_path, &destination_path).with_context(|| {
                format!(
                    "Failed to copy {} -> {}",
                    source_path.display(),
                    destination_path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn rel_path(path: &Path, base: &Path) -> String {
    path.strip_prefix(base)
        .map(|relative| relative.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

fn is_dir_empty(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(true);
    }
    let mut entries = fs::read_dir(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    Ok(entries.next().is_none())
}

#[derive(Debug)]
struct FileLock {
    path: PathBuf,
}

/// Maximum time (seconds) to wait for a lock before giving up (F29).
const LOCK_WAIT_SECS: u64 = 10;
/// Initial backoff interval (ms) when retrying lock acquisition.
const LOCK_BACKOFF_MS: u64 = 50;
/// Maximum backoff interval (ms).
const LOCK_BACKOFF_MAX_MS: u64 = 500;

fn acquire_lock(path: &Path) -> Result<FileLock> {
    acquire_lock_with_timeout(path, LOCK_WAIT_SECS)
}

/// Acquire the seal lock with bounded wait.
///
/// The lock covers the entire `read-manifest → write-files → write-history`
/// transaction (F29). If another process holds the lock but its PID is dead,
/// the stale lock is reclaimed with a warning. Live holders cause the caller
/// to wait with exponential backoff up to `timeout_secs`, then fail with a
/// clear message.
fn acquire_lock_with_timeout(path: &Path, timeout_secs: u64) -> Result<FileLock> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    let start = SystemTime::now();
    let mut backoff_ms = LOCK_BACKOFF_MS;
    loop {
        match try_create_lock(path) {
            Ok(lock) => return Ok(lock),
            Err(LockAttempt::HeldByDeadPid(pid)) => {
                eprintln!(
                    "[context-pack] WARNING: cleaned stale lock (pid {} no longer running)",
                    pid
                );
                let _ = fs::remove_file(path);
                // Loop will retry immediately.
            }
            Err(LockAttempt::Held) => {
                let elapsed = SystemTime::now()
                    .duration_since(start)
                    .unwrap_or_default()
                    .as_secs();
                if elapsed >= timeout_secs {
                    return Err(anyhow!(
                        "[context-pack] another seal is in progress (lock: {}); waited {}s",
                        path.display(),
                        timeout_secs
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(backoff_ms));
                backoff_ms = (backoff_ms * 2).min(LOCK_BACKOFF_MAX_MS);
            }
            Err(LockAttempt::Io(err)) => {
                return Err(anyhow!(
                    "[context-pack] failed to acquire lock ({}): {}",
                    path.display(),
                    err
                ));
            }
        }
    }
}

enum LockAttempt {
    HeldByDeadPid(u32),
    Held,
    Io(std::io::Error),
}

fn try_create_lock(path: &Path) -> std::result::Result<FileLock, LockAttempt> {
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            let pid = std::process::id();
            if let Err(err) = writeln!(file, "{}", pid) {
                let _ = fs::remove_file(path);
                return Err(LockAttempt::Io(err));
            }
            let _ = file.sync_all();
            Ok(FileLock {
                path: path.to_path_buf(),
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if let Ok(content) = fs::read_to_string(path) {
                if let Ok(pid) = content.trim().parse::<u32>() {
                    let is_running = Command::new("kill")
                        .arg("-0")
                        .arg(pid.to_string())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status()
                        .map(|s| s.success())
                        .unwrap_or(false);
                    if !is_running {
                        return Err(LockAttempt::HeldByDeadPid(pid));
                    }
                }
            }
            Err(LockAttempt::Held)
        }
        Err(error) => Err(LockAttempt::Io(error)),
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn sha256_hex(input: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input);
    format!("{:x}", hasher.finalize())
}

fn is_zero_sha(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty() && trimmed.chars().all(|ch| ch == '0') && trimmed.len() == ZERO_SHA.len()
}

fn short_sha(sha: Option<&str>) -> String {
    match sha {
        Some(value) if !value.trim().is_empty() && !is_zero_sha(value) => value.chars().take(12).collect(),
        _ => "none".to_string(),
    }
}

fn compact_timestamp(iso: &str) -> String {
    let mut compact = iso.replace(['-', ':'], "");
    if let Some(dot_idx) = compact.find('.') {
        if let Some(z_rel) = compact[dot_idx..].find('Z') {
            let end = dot_idx + z_rel + 1;
            compact.replace_range(dot_idx..end, "Z");
        }
    }
    compact
}

fn now_stamp() -> String {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();

    // Days since epoch calculation
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Civil date from days since epoch (algorithm from Howard Hinnant)
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m, d, hours, minutes, seconds)
}

fn is_main_push(local_ref: &str, remote_ref: &str) -> bool {
    local_ref == "refs/heads/main" || remote_ref == "refs/heads/main"
}

fn is_context_relevant(file_path: &str) -> bool {
    let normalized = file_path.replace('\\', "/");
    if normalized.starts_with("blog/")
        || normalized.starts_with("notes/")
        || normalized.starts_with("drafts/")
        || normalized.starts_with("scratch/")
        || normalized.starts_with("tmp/")
        || normalized.starts_with(".agent-context/")
        || normalized.starts_with("docs/demo-")
    {
        return false;
    }

    if matches!(
        normalized.as_str(),
        "README.md"
            | "PROTOCOL.md"
            | "CONTRIBUTING.md"
            | "SKILL.md"
            | "CLAUDE.md"
            | "AGENTS.md"
            | "package.json"
            | "package-lock.json"
            | "cli/Cargo.toml"
            | "cli/Cargo.lock"
            | "docs/architecture.svg"
            | "docs/silo-tax-before-after.webp"
    ) {
        return true;
    }

    normalized.starts_with("scripts/")
        || normalized.starts_with("cli/src/")
        || normalized.starts_with("schemas/")
        || normalized.starts_with("fixtures/golden/")
        || normalized.starts_with("fixtures/session-store/")
        || normalized.starts_with(".github/workflows/")
}

/// Load relevance rules from `.agent-context/relevance.json` if it exists.
/// Returns None if the file is missing or contains invalid JSON.
/// Expected format: { "include": ["pattern", ...], "exclude": ["pattern", ...] }
fn load_relevance_rules(repo_root: &Path) -> Option<Value> {
    let rules_path = repo_root.join(".agent-context").join("relevance.json");
    let raw = fs::read_to_string(&rules_path).ok()?;
    let rules: Value = serde_json::from_str(&raw).ok()?;
    if rules.is_object()
        && (rules.get("include").and_then(|v| v.as_array()).is_some()
            || rules.get("exclude").and_then(|v| v.as_array()).is_some())
    {
        Some(rules)
    } else {
        None
    }
}

fn build_glob_set(patterns: &[&str]) -> Option<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        if let Ok(glob) = Glob::new(pattern) {
            builder.add(glob);
        }
    }
    builder.build().ok()
}

/// Determine if a file is context-relevant using loaded rules or hardcoded defaults.
fn is_context_relevant_with_rules(file_path: &str, rules: &Option<Value>) -> bool {
    let normalized = file_path.replace('\\', "/");

    if let Some(rules) = rules {
        if let Some(excludes_array) = rules.get("exclude").and_then(|v| v.as_array()) {
            let patterns: Vec<&str> = excludes_array.iter().filter_map(|v| v.as_str()).collect();
            if let Some(glob_set) = build_glob_set(&patterns) {
                if glob_set.is_match(&normalized) {
                    return false;
                }
            }
        }
        if let Some(includes_array) = rules.get("include").and_then(|v| v.as_array()) {
            let patterns: Vec<&str> = includes_array.iter().filter_map(|v| v.as_str()).collect();
            if let Some(glob_set) = build_glob_set(&patterns) {
                if glob_set.is_match(&normalized) {
                    return true;
                }
            }
        }
        return false;
    }

    // Fall back to hardcoded defaults
    is_context_relevant(file_path)
}

/// Reserved: content generator for auto-fill build mode.
/// Will be wired when `build --auto-fill` is implemented.
#[allow(dead_code)]
fn build_start_here(
    repo_name: &str,
    branch: &str,
    head_sha: &str,
    package_version: &str,
    cargo_version: &str,
    generated_at: &str,
    changed_files: &[String],
) -> String {
    let changed_summary = if changed_files.is_empty() {
        "- No explicit change range provided (manual build).".to_string()
    } else {
        changed_files
            .iter()
            .take(MAX_CHANGED_FILES_DISPLAYED)
            .map(|path| format!("- {}", path))
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        "# Context Pack: Start Here\n\nThis context pack is the first-stop index for agent work in this repository.\n\n## Snapshot\n- Repo: `{repo_name}`\n- Branch at generation: `{branch}`\n- HEAD commit: `{head_sha}`\n- Node package version: `{package_version}`\n- Rust crate version: `{cargo_version}`\n- Generated at: `{generated_at}`\n\n## Read Order (Token-Efficient)\n1. Read this file.\n2. Read `10_SYSTEM_OVERVIEW.md` for architecture and execution paths.\n3. Read `30_BEHAVIORAL_INVARIANTS.md` before changing behavior.\n4. Use `20_CODE_MAP.md` to deep dive only relevant files.\n5. Use `40_OPERATIONS_AND_RELEASE.md` for tests, release, and maintenance.\n\n## Fast Facts\n- Product: Local-first cross-agent session chorus CLI.\n- Implementations: Node (`scripts/read_session.cjs`) and Rust (`cli/src/main.rs`).\n- Quality gate: Node/Rust parity + schema validation + edge-case checks.\n- Core risk: behavior drift between Node and Rust command/output contracts.\n\n## Last Change Range Input\n{changed_summary}\n\n## Scope Rule\nFor \"understand this repo end-to-end\" requests:\n- Start with this pack only.\n- Open source files only after this pack identifies a precise target.\n- Treat this pack as the source of navigation and invariants.\n"
    )
}

/// Reserved: content generator for auto-fill build mode.
/// Will be wired when `build --auto-fill` is implemented.
#[allow(dead_code)]
fn build_system_overview(
    package_version: &str,
    cargo_version: &str,
    tracked_file_count: usize,
    path_counts: &[(String, usize)],
    command_surface: &[(&str, &str, Vec<&str>)],
) -> String {
    let command_rows = command_surface
        .iter()
        .map(|(command, intent, paths)| {
            format!(
                "| `{}` | {} | {} |",
                command,
                intent,
                paths
                    .iter()
                    .map(|path| format!("`{}`", path))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let path_lines = if path_counts.is_empty() {
        "- No tracked path counts available.".to_string()
    } else {
        path_counts
            .iter()
            .map(|(name, count)| format!("- {}: {} tracked files", name, count))
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        "# System Overview\n\n## Product Shape\n- Package version: `{package_version}`\n- Crate version: `{cargo_version}`\n- Tracked files: `{tracked_file_count}`\n- Delivery: npm package (`chorus`) + Rust binary (`chorus`).\n\n## Runtime Architecture\n1. User asks a provider agent for cross-agent status.\n2. Agent invokes chorus command (`read`, `list`, `search`, `compare`, `report`, `diff`, `relevance`, `send`, `messages`, `setup`, `doctor`, `trash-talk`, `context-pack`).\n3. Chorus resolves session stores (Codex/Claude/Gemini/Cursor), applies redaction, and returns terminal text or JSON.\n4. Agent answers user with evidence from chorus output.\n\n## Dual-Implementation Contract\n- Node path: `scripts/read_session.cjs` + `scripts/adapters/*.cjs`.\n- Rust path: `cli/src/main.rs`, `cli/src/agents.rs`, `cli/src/report.rs`, `cli/src/adapters/*.rs`.\n- Protocol authority: `PROTOCOL.md` and `schemas/*.json`.\n- Parity guard: `scripts/conformance.sh`.\n\n## Command Surface\n| Command | Intent | Primary Paths |\n| --- | --- | --- |\n{command_rows}\n\n## Tracked Path Density\n{path_lines}\n"
    )
}

/// Reserved: content generator for auto-fill build mode.
/// Will be wired when `build --auto-fill` is implemented.
#[allow(dead_code)]
fn build_code_map() -> String {
    r#"# Code Map

## High-Impact Paths
| Path | What | Why It Matters | Change Risk |
| --- | --- | --- | --- |
| `scripts/read_session.cjs` | Node CLI command parser + execution engine | Defines behavior for all user-facing commands in Node distribution | High |
| `scripts/adapters/*.cjs` | Node agent-specific session adapters | Session discovery, parsing, and metadata quality for each provider | High |
| `cli/src/main.rs` | Rust CLI command/arg handling | Entry-point parity with Node and release binary behavior | High |
| `cli/src/agents.rs` | Rust session parsing + redaction + read/list/search | Largest behavioral surface and most error-code paths | High |
| `cli/src/report.rs` | Rust compare/report logic | Cross-agent divergence logic and report markdown/json structure | High |
| `schemas/*.json` | JSON contract definitions | External compatibility for `--json` users and tests | High |
| `PROTOCOL.md` | Versioned protocol contract | Human contract that aligns Node, Rust, and tests | High |
| `README.md` | Public command docs and examples | User expectations and documentation-driven behavior | Medium |
| `scripts/conformance.sh` | Parity checks across implementations | Prevents silent behavior drift before release | High |
| `scripts/test_edge_cases.sh` | Edge and error-path checks | Guards hard-to-debug regressions in parse/error handling | High |
| `.github/workflows/ci.yml` | Mandatory validation workflow | Ensures checks run on push/PR | Medium |
| `.github/workflows/release.yml` | Release pipeline | Controls publish safety and artifact generation | Medium |

## Extension Recipe (New Agent)
1. Implement adapter in Rust: `cli/src/adapters/<agent>.rs` and register in `cli/src/adapters/mod.rs`.
2. Implement adapter in Node: `scripts/adapters/<agent>.cjs` and register in `scripts/adapters/registry.cjs`.
3. Add schema enum coverage in `schemas/*.json`.
4. Add fixtures and golden expectations under `fixtures/`.
5. Validate parity and edge cases through test scripts.
"#
    .to_string()
}

/// Reserved: content generator for auto-fill build mode.
/// Will be wired when `build --auto-fill` is implemented.
#[allow(dead_code)]
fn build_invariants() -> String {
    r#"# Behavioral Invariants

These constraints are contract-level and must be preserved unless intentionally versioned.

## Core Protocol Invariants
1. `read`, `list`, `search`, `compare`, and `report` must align with `PROTOCOL.md`.
2. Node and Rust outputs must remain behaviorally equivalent for shared commands.
3. `--json` failures must emit structured error JSON with stable error codes.
4. Secret-like tokens must be redacted before output.
5. Session selection defaults: latest scoped session first, then documented fallback behavior.

## Session Semantics
1. "current/latest" means newest session.
2. "past/previous session" means one session before newest.
3. `read --last N` returns last N assistant messages joined by `\n---\n`.
4. CWD scoping applies where provider data supports it.

## Update Checklist Before Merging Behavior Changes
1. Update code in both Node and Rust paths when command semantics change.
2. Update schema files for JSON shape changes.
3. Update `README.md` and `PROTOCOL.md` for public contract changes.
4. Update fixtures/golden outputs.
5. Run conformance, edge-case, and schema validation scripts.
"#
    .to_string()
}

/// Reserved: content generator for auto-fill build mode.
/// Will be wired when `build --auto-fill` is implemented.
#[allow(dead_code)]
fn build_operations() -> String {
    r#"# Operations And Release

## Standard Validation
```bash
npm run check
cargo test --manifest-path cli/Cargo.toml
```

## Main CI Checks
- `scripts/conformance.sh`
- `scripts/test_edge_cases.sh`
- `scripts/check_readme_examples.sh`
- `scripts/check_package_contents.sh`
- `scripts/validate_schemas.sh`

## Release Flow
1. Push tag `v*` to trigger `.github/workflows/release.yml`.
2. Verify phase runs conformance/docs/schema/version checks.
3. Package/publish Node artifact.
4. Build/upload Rust binaries and publish crate when tokens are configured.

## Context Pack Maintenance Contract
1. Build pack manually: `chorus context-pack build`.
2. Install branch-aware pre-push hook: `chorus context-pack install-hooks`.
3. On `main` push, hook runs `context-pack:sync-main`.
4. Sync updates the pack only when changed files are context-relevant.
5. Snapshots are saved under `.agent-context/snapshots/` for rollback/recovery.

## Rollback/Recovery
- Restore latest snapshot: `chorus context-pack rollback`
- Restore named snapshot: `chorus context-pack rollback --snapshot <snapshot_id>`
"#
    .to_string()
}

/// Update the Snapshot metadata lines in 00_START_HERE.md so they stay in sync
/// with manifest.json.  Only touches Branch, HEAD commit, and Generated at.
fn update_start_here_snapshot(
    current_dir: &Path,
    branch: &str,
    head_sha: Option<&str>,
    generated_at: &str,
) -> Result<()> {
    let start_here = current_dir.join("00_START_HERE.md");
    if !start_here.exists() {
        return Ok(());
    }
    let content = fs::read_to_string(&start_here)
        .with_context(|| format!("Failed to read {}", start_here.display()))?;

    let mut result = String::with_capacity(content.len());
    for line in content.lines() {
        if line.starts_with("- Branch at generation: `") {
            result.push_str(&format!("- Branch at generation: `{}`", branch));
        } else if line.starts_with("- HEAD commit: `") {
            result.push_str(&format!(
                "- HEAD commit: `{}`",
                head_sha.unwrap_or("unknown")
            ));
        } else if line.starts_with("- Generated at: `") {
            result.push_str(&format!("- Generated at: `{}`", generated_at));
        } else {
            result.push_str(line);
        }
        result.push('\n');
    }
    // Preserve trailing newline behavior of original
    if !content.ends_with('\n') {
        result.pop();
    }

    write_text_atomic(&start_here, &result)?;
    Ok(())
}

fn has_structured_layer(current_dir: &Path) -> bool {
    current_dir.join("routes.json").exists()
}

fn required_files_for_mode(current_dir: &Path) -> Vec<String> {
    let mut files = REQUIRED_FILES
        .iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>();
    if has_structured_layer(current_dir) {
        files.extend(STRUCTURED_FILES.iter().map(|value| value.to_string()));
    }
    files
}

fn walk_files(root_dir: &Path, current_dir: &Path, out: &mut Vec<String>) -> Result<()> {
    walk_files_bounded(root_dir, current_dir, out, 0)
}

/// P8/F20 — bounded directory walk. `depth` is the distance from the walk's
/// initial root. We stop descending past [`MAX_WALK_DEPTH`] to prevent
/// symlink-loop hangs and runaway recursion on pathological layouts.
fn walk_files_bounded(
    root_dir: &Path,
    current_dir: &Path,
    out: &mut Vec<String>,
    depth: usize,
) -> Result<()> {
    if depth >= MAX_WALK_DEPTH {
        eprintln!(
            "[context-pack] WARN: walk depth limit ({}) reached at {} — skipping deeper entries",
            MAX_WALK_DEPTH,
            current_dir.display()
        );
        return Ok(());
    }
    for entry in fs::read_dir(current_dir)
        .with_context(|| format!("Failed to read {}", current_dir.display()))?
    {
        let entry =
            entry.with_context(|| format!("Failed to read entry in {}", current_dir.display()))?;
        let entry_path = entry.path();
        if entry.file_name() == ".git" {
            continue;
        }
        // F20 — do NOT descend into symlinked directories; they can loop.
        // Pack snapshotting still follows symlinks in `copy_dir_recursive`
        // via fs::copy (which dereferences) so any in-repo symlink still
        // copies correctly, but walking is bounded here.
        let is_symlink = fs::symlink_metadata(&entry_path)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false);
        if is_symlink {
            continue;
        }
        if entry_path.is_dir() {
            walk_files_bounded(root_dir, &entry_path, out, depth + 1)?;
        } else if entry_path.is_file() {
            out.push(rel_path(&entry_path, root_dir).replace('\\', "/"));
        }
    }
    Ok(())
}

fn resolve_pattern_matches(repo_root: &Path, pattern: &str) -> Result<Vec<String>> {
    let normalized = pattern.replace('\\', "/");
    if !normalized.contains('*') && !normalized.contains('?') && !normalized.contains('[') {
        let target = repo_root.join(&normalized);
        if target.exists() {
            return Ok(vec![normalized]);
        }
        return Ok(Vec::new());
    }

    let glob = Glob::new(&normalized)
        .with_context(|| format!("Invalid glob pattern in structured pack: {}", pattern))?;
    let mut builder = GlobSetBuilder::new();
    builder.add(glob);
    let matcher = builder
        .build()
        .with_context(|| format!("Failed to compile glob pattern {}", pattern))?;

    let mut files = Vec::new();
    walk_files(repo_root, repo_root, &mut files)?;
    Ok(files
        .into_iter()
        .filter(|file_path| matcher.is_match(file_path))
        .collect())
}

fn validate_pattern_matches(repo_root: &Path, pattern: &str, label: &str) -> Result<()> {
    if resolve_pattern_matches(repo_root, pattern)?.is_empty() {
        return Err(anyhow!(
            "[context-pack] seal failed: {} did not match any files: {}",
            label,
            pattern
        ));
    }
    Ok(())
}

fn validate_structured_layer(repo_root: &Path, current_dir: &Path) -> Result<()> {
    let routes_path = current_dir.join("routes.json");
    if !routes_path.exists() {
        return Ok(());
    }

    let completeness_path = current_dir.join("completeness_contract.json");
    let reporting_path = current_dir.join("reporting_rules.json");

    for required_path in [&completeness_path, &reporting_path] {
        if !required_path.exists() {
            return Err(anyhow!(
                "[context-pack] seal failed: structured mode requires {}",
                rel_path(required_path, repo_root)
            ));
        }
    }

    let routes = read_json(&routes_path)?
        .ok_or_else(|| anyhow!("[context-pack] seal failed: routes.json is missing"))?;
    let completeness = read_json(&completeness_path)?.ok_or_else(|| {
        anyhow!("[context-pack] seal failed: completeness_contract.json is missing")
    })?;
    let reporting = read_json(&reporting_path)?
        .ok_or_else(|| anyhow!("[context-pack] seal failed: reporting_rules.json is missing"))?;

    let routes_map = routes
        .get("task_routes")
        .and_then(|value| value.as_object())
        .ok_or_else(|| anyhow!("[context-pack] seal failed: routes.json must define task_routes"))?;
    let completeness_map = completeness
        .get("task_families")
        .and_then(|value| value.as_object())
        .ok_or_else(|| {
            anyhow!(
                "[context-pack] seal failed: completeness_contract.json must define task_families"
            )
        })?;
    let reporting_map = reporting
        .get("task_families")
        .and_then(|value| value.as_object())
        .ok_or_else(|| {
            anyhow!(
                "[context-pack] seal failed: reporting_rules.json must define task_families"
            )
        })?;

    for task in TASK_FAMILIES {
        let route = routes_map
            .get(*task)
            .and_then(|value| value.as_object())
            .ok_or_else(|| {
                anyhow!(
                    "[context-pack] seal failed: routes.json is missing task_routes.{}",
                    task
                )
            })?;
        let completeness_entry = completeness_map
            .get(*task)
            .and_then(|value| value.as_object())
            .ok_or_else(|| {
                anyhow!(
                    "[context-pack] seal failed: completeness_contract.json is missing task_families.{}",
                    task
                )
            })?;
        let reporting_entry = reporting_map
            .get(*task)
            .and_then(|value| value.as_object())
            .ok_or_else(|| {
                anyhow!(
                    "[context-pack] seal failed: reporting_rules.json is missing task_families.{}",
                    task
                )
            })?;

        let completeness_ref = route
            .get("completeness_ref")
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                anyhow!(
                    "[context-pack] seal failed: routes.json task_routes.{} must define completeness_ref",
                    task
                )
            })?;
        if completeness_ref != *task {
            return Err(anyhow!(
                "[context-pack] seal failed: routes.json completeness_ref for {} must equal {}",
                task,
                task
            ));
        }
        let reporting_ref = route
            .get("reporting_ref")
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                anyhow!(
                    "[context-pack] seal failed: routes.json task_routes.{} must define reporting_ref",
                    task
                )
            })?;
        if reporting_ref != *task {
            return Err(anyhow!(
                "[context-pack] seal failed: routes.json reporting_ref for {} must equal {}",
                task,
                task
            ));
        }

        for key in ["pack_read_order", "fallback_files"] {
            if let Some(entries) = route.get(key).and_then(|value| value.as_array()) {
                for entry in entries {
                    let entry = entry.as_str().ok_or_else(|| {
                        anyhow!(
                            "[context-pack] seal failed: routes.json {} entries must be strings",
                            key
                        )
                    })?;
                    let target_path = current_dir.join(entry);
                    if !target_path.exists() {
                        return Err(anyhow!(
                            "[context-pack] seal failed: routes.json references missing pack file {}",
                            entry
                        ));
                    }
                }
            }
        }

        for key in [
            "contractually_required_files",
            "required_file_families",
            "required_chain_members",
        ] {
            if let Some(entries) = completeness_entry.get(key).and_then(|value| value.as_array()) {
                for entry in entries {
                    let pattern = entry.as_str().ok_or_else(|| {
                        anyhow!(
                            "[context-pack] seal failed: completeness_contract.json {} entries must be strings",
                            key
                        )
                    })?;
                    validate_pattern_matches(repo_root, pattern, &format!("completeness_contract.json {}", key))?;
                }
            }
        }

        let optional_budget = reporting_entry
            .get("optional_verify_budget")
            .and_then(|value| value.as_i64())
            .ok_or_else(|| {
                anyhow!(
                    "[context-pack] seal failed: reporting_rules.json optional_verify_budget must be an integer for {}",
                    task
                )
            })?;
        if optional_budget < 0 {
            return Err(anyhow!(
                "[context-pack] seal failed: reporting_rules.json optional_verify_budget must be a non-negative integer for {}",
                task
            ));
        }

        for key in ["groupable_families", "never_enumerate_individually"] {
            if let Some(entries) = reporting_entry.get(key).and_then(|value| value.as_array()) {
                for entry in entries {
                    let pattern = entry.as_str().ok_or_else(|| {
                        anyhow!(
                            "[context-pack] seal failed: reporting_rules.json {} entries must be strings",
                            key
                        )
                    })?;
                    validate_pattern_matches(repo_root, pattern, &format!("reporting_rules.json {}", key))?;
                }
            }
        }
    }

    // Validate search_scope.json if present (not required — backward compat)
    let search_scope_path = current_dir.join("search_scope.json");
    if search_scope_path.exists() {
        let scope = read_json(&search_scope_path)?
            .ok_or_else(|| anyhow!("[context-pack] seal failed: search_scope.json is empty"))?;
        if let Some(scope_families) = scope.get("task_families").and_then(|v| v.as_object()) {
            for task in TASK_FAMILIES {
                if let Some(entry) = scope_families.get(*task).and_then(|v| v.as_object()) {
                    // Validate search_directories exist on disk
                    if let Some(dirs) = entry.get("search_directories").and_then(|v| v.as_array())
                    {
                        for dir in dirs {
                            if let Some(dir_str) = dir.as_str() {
                                // P8/F22 — reject path-traversal / absolute
                                // patterns before doing any filesystem lookup.
                                validate_pack_glob(dir_str, repo_root).map_err(|e| {
                                    anyhow!(
                                        "[context-pack] seal failed: search_scope.json ({}) {}",
                                        task, e
                                    )
                                })?;
                                let dir_path = repo_root.join(dir_str);
                                if !dir_path.exists() {
                                    return Err(anyhow!(
                                        "[context-pack] seal failed: search_scope.json references missing directory {}",
                                        dir_str
                                    ));
                                }
                            }
                        }
                    }
                    // Validate verification_shortcuts reference real files
                    if let Some(shortcuts) =
                        entry.get("verification_shortcuts").and_then(|v| v.as_object())
                    {
                        for (file_path, _) in shortcuts {
                            // P8/F22 — reject path-traversal / absolute
                            // shortcut keys before touching disk.
                            let shortcut_path = file_path.split(':').next().unwrap_or(file_path);
                            validate_pack_glob(shortcut_path, repo_root).map_err(|e| {
                                anyhow!(
                                    "[context-pack] seal failed: search_scope.json ({}) verification_shortcuts {}",
                                    task, e
                                )
                            })?;
                            let file_on_disk = repo_root.join(shortcut_path);
                            if !file_on_disk.exists() {
                                return Err(anyhow!(
                                    "[context-pack] seal failed: search_scope.json verification_shortcuts references missing file {}",
                                    file_path
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some(entries) = reporting
        .get("global_rules")
        .and_then(|value| value.get("authoritative_vs_derived_paths"))
        .and_then(|value| value.as_array())
    {
        for entry in entries {
            let entry = entry.as_object().ok_or_else(|| {
                anyhow!(
                    "[context-pack] seal failed: reporting_rules.json authoritative_vs_derived_paths entries must be objects"
                )
            })?;
            let pattern = entry
                .get("pattern")
                .and_then(|value| value.as_str())
                .ok_or_else(|| {
                    anyhow!(
                        "[context-pack] seal failed: reporting_rules.json authoritative_vs_derived_paths entries must contain pattern"
                    )
                })?;
            let role = entry
                .get("role")
                .and_then(|value| value.as_str())
                .ok_or_else(|| {
                    anyhow!(
                        "[context-pack] seal failed: reporting_rules.json authoritative_vs_derived_paths entries must contain role"
                    )
                })?;
            validate_pattern_matches(
                repo_root,
                pattern,
                "reporting_rules.json authoritative_vs_derived_paths",
            )?;
            if role == "authoritative" && pattern.contains("_generated/") {
                return Err(anyhow!(
                    "[context-pack] seal failed: generated files cannot be marked as authoritative edit targets"
                ));
            }
        }
    }

    Ok(())
}

/// Build the routing block content for agent config files.
fn build_context_pack_routing_block(agent_kind: &str) -> String {
    if agent_kind == "codex" {
        "## Context Pack\n\
         \n\
         When asked to understand this repository:\n\
         \n\
         1. Read `.agent-context/current/00_START_HERE.md`.\n\
         2. Read `.agent-context/current/routes.json`.\n\
         3. Identify the active task type in `routes.json`.\n\
         4. Read the matching entries in `completeness_contract.json`, `reporting_rules.json`, and `search_scope.json`.\n\
         5. Search ONLY within the directories listed in `search_scope.json` for your task type.\n\
         6. Use `verification_shortcuts` to check specific line ranges instead of reading full files.\n\
         7. Do not enumerate files in directories marked `exclude_from_search`.\n\
         8. Do not open repo files before those steps unless a referenced structured file is missing.\n\
         \n\
         If `.agent-context/current/routes.json` is missing, fall back to the markdown pack only."
            .to_string()
    } else {
        "## Context Pack\n\
         \n\
         **BEFORE starting any task**, read the context pack in this order:\n\
         \n\
         1. `.agent-context/current/00_START_HERE.md` — entrypoint, routing, stop rules\n\
         2. `.agent-context/current/30_BEHAVIORAL_INVARIANTS.md` — change checklists, file families, what NOT to do\n\
         3. `.agent-context/current/20_CODE_MAP.md` — navigation index, tracing flows\n\
         \n\
         Read these three files BEFORE opening any repo source files. Then open only the files the pack identifies as relevant.\n\
         \n\
         For architecture questions, also read `10_SYSTEM_OVERVIEW.md`. For test/deploy questions, also read `40_OPERATIONS_AND_RELEASE.md`."
            .to_string()
    }
}

fn build_routes_json() -> String {
    serde_json::to_string_pretty(&json!({
        "schema_version": 1,
        "task_routes": {
            "lookup": {
                "description": "Find a value, threshold, URL, or authoritative file.",
                "pack_read_order": ["00_START_HERE.md", "20_CODE_MAP.md", "reporting_rules.json"],
                "fallback_files": ["30_BEHAVIORAL_INVARIANTS.md"],
                "completeness_ref": "lookup",
                "reporting_ref": "lookup"
            },
            "impact_analysis": {
                "description": "List every file or file family that must change.",
                "pack_read_order": [
                    "00_START_HERE.md",
                    "30_BEHAVIORAL_INVARIANTS.md",
                    "completeness_contract.json",
                    "reporting_rules.json",
                    "20_CODE_MAP.md"
                ],
                "fallback_files": ["10_SYSTEM_OVERVIEW.md"],
                "completeness_ref": "impact_analysis",
                "reporting_ref": "impact_analysis"
            },
            "planning": {
                "description": "Write an implementation plan with files, commands, and validation.",
                "pack_read_order": [
                    "00_START_HERE.md",
                    "20_CODE_MAP.md",
                    "30_BEHAVIORAL_INVARIANTS.md",
                    "completeness_contract.json",
                    "reporting_rules.json"
                ],
                "fallback_files": ["40_OPERATIONS_AND_RELEASE.md"],
                "completeness_ref": "planning",
                "reporting_ref": "planning"
            },
            "diagnosis": {
                "description": "Rank likely root causes and cite the runtime path.",
                "pack_read_order": [
                    "00_START_HERE.md",
                    "10_SYSTEM_OVERVIEW.md",
                    "30_BEHAVIORAL_INVARIANTS.md",
                    "completeness_contract.json",
                    "reporting_rules.json"
                ],
                "fallback_files": ["20_CODE_MAP.md"],
                "completeness_ref": "diagnosis",
                "reporting_ref": "diagnosis"
            }
        }
    })).unwrap_or_else(|_| "{}".to_string()) + "\n"
}

fn build_completeness_contract_json() -> String {
    serde_json::to_string_pretty(&json!({
        "schema_version": 1,
        "task_families": {
            "lookup": {
                "minimum_sufficient_evidence": [
                    "exact answer",
                    "authoritative source path",
                    "one supporting chain only if the task asks for authority"
                ],
                "required_chain_members": [],
                "contractually_required_files": [],
                "required_file_families": []
            },
            "impact_analysis": {
                "minimum_sufficient_evidence": [
                    "complete blast radius",
                    "required file families",
                    "contractually required pass-through layers"
                ],
                "required_chain_members": [],
                "contractually_required_files": [],
                "required_file_families": []
            },
            "planning": {
                "minimum_sufficient_evidence": [
                    "files to create or modify",
                    "commands in order",
                    "validation criteria"
                ],
                "required_chain_members": [],
                "contractually_required_files": [],
                "required_file_families": []
            },
            "diagnosis": {
                "minimum_sufficient_evidence": [
                    "ranked root causes",
                    "runtime path or failure chain",
                    "confirmation method for each cause"
                ],
                "required_chain_members": [],
                "contractually_required_files": [],
                "required_file_families": []
            }
        }
    })).unwrap_or_else(|_| "{}".to_string()) + "\n"
}

fn build_reporting_rules_json() -> String {
    serde_json::to_string_pretty(&json!({
        "schema_version": 1,
        "global_rules": {
            "grouped_reporting_default": true,
            "authoritative_vs_derived_paths": []
        },
        "task_families": {
            "lookup": {
                "optional_verify_budget": 1,
                "stop_after": "Stop after the authoritative source and one optional supporting check.",
                "stop_unless": [
                    "a structured artifact references a missing file",
                    "markdown and structured artifacts disagree",
                    "code contradicts the structured contract",
                    "the task explicitly asks for concrete instances rather than grouped families"
                ],
                "groupable_families": [],
                "never_enumerate_individually": []
            },
            "impact_analysis": {
                "optional_verify_budget": 2,
                "stop_after": "Stop after the blast radius is complete and required families are grouped correctly.",
                "stop_unless": [
                    "a structured artifact references a missing file",
                    "markdown and structured artifacts disagree",
                    "code contradicts the structured contract",
                    "the task explicitly asks for concrete instances rather than grouped families"
                ],
                "groupable_families": [],
                "never_enumerate_individually": []
            },
            "planning": {
                "optional_verify_budget": 2,
                "stop_after": "Stop after the plan is executable without further repo browsing.",
                "stop_unless": [
                    "a structured artifact references a missing file",
                    "markdown and structured artifacts disagree",
                    "code contradicts the structured contract",
                    "the task explicitly asks for concrete instances rather than grouped families"
                ],
                "groupable_families": [],
                "never_enumerate_individually": []
            },
            "diagnosis": {
                "optional_verify_budget": 3,
                "stop_after": "Stop after the ranked runtime chain is established and each cause has a confirmation method.",
                "stop_unless": [
                    "a structured artifact references a missing file",
                    "markdown and structured artifacts disagree",
                    "code contradicts the structured contract",
                    "the task explicitly asks for concrete instances rather than grouped families"
                ],
                "groupable_families": [],
                "never_enumerate_individually": []
            }
        }
    })).unwrap_or_else(|_| "{}".to_string()) + "\n"
}

fn build_search_scope_json() -> String {
    serde_json::to_string_pretty(&json!({
        "schema_version": 1,
        "description": "Search scope boundaries for search-and-verify agents (e.g. Codex). Bounds WHERE to search, not WHEN to stop.",
        "task_families": {
            "lookup": {
                "search_directories": [],
                "exclude_from_search": [],
                "verification_shortcuts": {}
            },
            "impact_analysis": {
                "search_directories": [],
                "exclude_from_search": [],
                "verification_shortcuts": {},
                "derived_file_policy": "Do not list generated/compiled/bundled output files as change targets. They are produced by a build/generate step."
            },
            "planning": {
                "search_directories": [],
                "exclude_from_search": [],
                "verification_shortcuts": {}
            },
            "diagnosis": {
                "search_directories": [],
                "exclude_from_search": [],
                "verification_shortcuts": {}
            }
        }
    }))
    .unwrap_or_else(|_| "{}".to_string())
        + "\n"
}

/// Upsert a managed block into a file (prepend if new, replace if exists).
/// Block is delimited by HTML comment markers.
/// Idempotent — running twice produces the same result.
fn upsert_context_pack_block(
    file_path: &Path,
    block: &str,
    marker_prefix: &str,
) -> Result<()> {
    let start_marker = format!("<!-- {}:start -->", marker_prefix);
    let end_marker = format!("<!-- {}:end -->", marker_prefix);
    let managed_block = format!("{}\n{}\n{}", start_marker, block, end_marker);

    if file_path.exists() {
        let content = fs::read_to_string(file_path)
            .with_context(|| format!("Failed to read {}", file_path.display()))?;
        let new_content = if let (Some(start_idx), Some(end_idx)) =
            (content.find(&start_marker), content.find(&end_marker))
        {
            // Replace existing managed block in place
            format!(
                "{}{}{}",
                &content[..start_idx],
                managed_block,
                &content[end_idx + end_marker.len()..]
            )
        } else {
            // Prepend before existing content
            format!("{}\n\n{}", managed_block, content)
        };
        write_text(file_path, &new_content)?;
    } else {
        write_text(file_path, &format!("{}\n", managed_block))?;
    }
    Ok(())
}

fn default_relevance_json() -> String {
    r#"{
  "include": ["**"],
  "exclude": [
    ".agent-context/**",
    ".git/**",
    "node_modules/**",
    "target/**",
    "dist/**",
    "build/**",
    "vendor/**",
    "tmp/**"
  ]
}
"#
    .to_string()
}

fn build_template_start_here(
    repo_name: &str,
    branch: &str,
    head_sha: &str,
    generated_at: &str,
) -> String {
    format!(
        r#"# Context Pack: Start Here

## Snapshot
- Repo: `{repo_name}`
- Branch at generation: `{branch}`
- HEAD commit: `{head_sha}`
- Generated at: `{generated_at}`

## Read Order — MANDATORY before starting work
1. Read this file completely.
2. Read `30_BEHAVIORAL_INVARIANTS.md` — change checklists, file families, negative guidance.
3. Read `20_CODE_MAP.md` — navigation index, tracing flows, extension recipe.

Do NOT open repo source files until you have read steps 1-3. These three files give you enough context to avoid common mistakes (wrong patterns, missing files, deprecated approaches).

Read on demand:
- `10_SYSTEM_OVERVIEW.md` — for architecture or diagnosis tasks.
- `40_OPERATIONS_AND_RELEASE.md` — for test, CI, or deploy tasks.

## Task-Type Routing
**Impact analysis** (list every file that must change): read `30_BEHAVIORAL_INVARIANTS.md` Update Checklist *before* `20_CODE_MAP.md` — the checklist has the full blast radius per change type. CODE_MAP alone is not exhaustive.
**Navigation / lookup** (find a file, find a value): start with `20_CODE_MAP.md` Scope Rule.
**Planning** (add a new feature/module): follow the Extension Recipe in `20_CODE_MAP.md`, then cross-check the BEHAVIORAL_INVARIANTS checklist for that change type.
**Diagnosis** (silent failures, unexpected output): start with `10_SYSTEM_OVERVIEW.md` Silent Failure Modes, then the relevant diagnostic row in `30_BEHAVIORAL_INVARIANTS.md`.

## Structured Routing
- Read `routes.json` after this file to identify the task family before opening repo files.
- Read the matching entries in `completeness_contract.json` and `reporting_rules.json`.
- Use `search_scope.json` for search directory boundaries and verification shortcuts.
- Treat the structured files as authoritative for stop conditions, grouped reporting, and contractual completeness.

## Stop Rules
- Stop when the minimum sufficient evidence for the active task is satisfied.
- Use optional verification sparingly and stay within the task-family verify budget.
- Continue exploring only when a structured file is missing, the pack disagrees with the repo, or the task explicitly asks for concrete instances instead of grouped families.

## Fast Facts
<!-- AGENT: Replace with 3-5 bullets covering product, languages/entry points, quality gate, core risk. -->

## Scope Rule
<!-- AGENT: Provide navigation rules — what to open first for each area of the codebase, what to skip. -->
"#
    )
}

fn build_template_system_overview() -> String {
    r#"# System Overview

<!-- AGENT: Fill by introspecting the repository. -->

## Product Shape
<!-- AGENT: Add package version(s), tracked file count, delivery mechanism(s). -->

## Runtime Architecture
<!-- AGENT: Describe primary execution flow in 3-5 numbered steps. -->

## Silent Failure Modes
<!-- AGENT: List any code paths where a failure produces no error — null return, silent drop, unchecked default.
These are the hardest things to find by reading code and the most valuable to have written down.
Example: "If selector has no match in prompts.yml, resolver returns null — Spark UDF propagates as null row with no error logged."
If none are known, write "None identified." -->

## Command/API Surface
<!-- AGENT: Table | Command/Endpoint | Intent | Primary Source Files | -->

## Tracked Path Density
<!-- AGENT: Summarize top-level directory distribution (git ls-files). -->
"#
    .to_string()
}

fn build_template_code_map() -> String {
    r#"# Code Map

## High-Impact Paths

> **This table is a navigation index, not a complete blast-radius list.** For impact analysis tasks,
> read `30_BEHAVIORAL_INVARIANTS.md` Update Checklist first — it has the full file set per change type.
> Use this table to navigate to those files once you know which are relevant. Verify coverage with grep.

<!-- AGENT: Identify 8-15 key paths. Use [Approach 1], [Approach 2], or [Both] in the Approach column
if the repo has coexisting architectural patterns — omit the column if there is only one approach.
Risk must be filled: use "Silent failure if missed", "KeyError at runtime", "Build drift", etc.
Authority must be filled: "authoritative" (edit this file), "derived" (generated/compiled — do not edit directly), or "reference" (read-only context).
| Path | Approach | What | Why It Matters | Risk | Authority |
| --- | --- | --- | --- | --- | --- | -->

## Quick Lookup Shortcuts
<!-- AGENT: Add 4-6 common lookup patterns. Map intent to exact file and what to look for.
| I need to find... | Open this file | Look for |
| --- | --- | --- | -->

## Cross-Cutting Tracing Flows
<!-- AGENT: For changes that ripple through multiple layers, document the full chain.
Example: "New parameter through call chain: schema → step → client → wrapper → tests"
List files in dependency order so agents trace the change correctly. -->

## Minimum Sufficient Evidence
<!-- AGENT: For each common task family, define the minimum file set needed before an answer is complete.
Keep this short. Example:
- Lookup: authoritative source file + one support check.
- Impact analysis: invariant checklist row + grouped file families.
- Planning: target files + commands + validation.
- Diagnosis: runtime path + likely failure point + confirmation method. -->

## Extension Recipe
<!-- AGENT: Describe how to add a new module/adapter/plugin. List all files that must change together. -->
"#
    .to_string()
}

fn build_template_invariants() -> String {
    r#"# Behavioral Invariants

<!-- AGENT: List contract-level constraints to preserve. -->

## Core Invariants
<!-- AGENT: 3-8 numbered items. Each must be a testable statement, not a description.
Good: "Every selector in a spec must match an entry in prompts.yml — missing match raises ValueError at sync time."
Bad: "Prompts must be valid." -->

## Update Checklist Before Merging Behavior Changes
<!-- AGENT: One row per common change type. The "Files that must change together" column must list
explicit file paths — not descriptions, not directory names. Agents will use these rows as a checklist.
If a missed file causes a silent production failure, say so explicitly in the row.
| Change type | Files that must change together |
| --- | --- | -->

## File Families
<!-- AGENT: List homogeneous file families where all members change the same way.
For each family, state: the glob pattern, how many members, and whether to report as a family
or enumerate individually. Agents should inspect one representative unless divergence is suspected.
Example: "models/assets_gen/_specs/*.prompt.yml (20 files) — report as family, do not enumerate individually."
Example: "models/assets_gen/_generated/*.yml (17 files) — derived, never list as change targets." -->

## Often Reviewed But Not Always Required
<!-- AGENT: List files that are commonly inspected during debugging or planning but are not contractually
required for every change. Separate these from the must-change checklist so agents do not over-read. -->

## Negative Guidance
<!-- AGENT: List patterns that agents commonly over-explore. Be explicit about what NOT to do.
Example: "Do not enumerate _generated/ files individually for impact analysis — they are regenerated by a build step."
Example: "Do not inspect both sync and async wrappers unless the parameter is known to diverge between them."
Example: "Do not open test files to determine blast radius — tests are updated after source, not before." -->
"#
    .to_string()
}

fn build_template_operations() -> String {
    r#"# Operations And Release

## Standard Validation
<!-- AGENT: Add local validation commands (tests, linters, etc.). -->

## CI Checks
<!-- AGENT: List CI workflows/steps that gate merges. -->

## Release Flow
<!-- AGENT: Describe how releases are triggered and what they produce. -->

## Context Pack Maintenance
1. Initialize scaffolding: `chorus context-pack init` (pre-push hook installed automatically)
2. Have your agent fill in the template sections.
3. Seal the pack: `chorus context-pack seal`
4. When freshness warnings appear on push, update content then run `chorus context-pack seal`

## Rollback/Recovery
- Restore latest snapshot: `chorus context-pack rollback`
- Restore named snapshot: `chorus context-pack rollback --snapshot <snapshot_id>`
"#
    .to_string()
}

fn build_guide() -> String {
    r#"# Context Pack Generation Guide

This guide tells AI agents how to fill in the context pack templates.

## Process
1. Read each file in `.agent-context/current/` in numeric order.
2. Fill the markdown templates with repository-derived content.
3. Fill the structured files (`routes.json`, `completeness_contract.json`, `reporting_rules.json`) with deterministic repo-specific rules.
4. After filling all sections, run `chorus context-pack seal` to finalize (manifest + snapshot).

## Quality Criteria
- Content must be factual and verifiable from the repository.
- Prefer concise bullets over long prose.
- Keep total word count under ~2000 words across all files.
- Do not include secrets or credentials.
- Keep structured artifacts explicit and deterministic; do not auto-generate them from freeform prose.
- If unsure, note `TBD` rather than inventing details.

## When to Update
- After significant architectural or contract changes.
- After adding new commands/APIs/features.
- When `chorus context-pack check-freshness` reports stale content.
"#
    .to_string()
}

fn build_pre_push_hook_section() -> String {
    r#"remote_name="${1:-origin}"
remote_url="${2:-unknown}"

run_context_sync() {
  local local_ref="$1"
  local local_sha="$2"
  local remote_ref="$3"
  local remote_sha="$4"

  if command -v chorus >/dev/null 2>&1; then
    chorus context-pack sync-main \
      --local-ref "$local_ref" \
      --local-sha "$local_sha" \
      --remote-ref "$remote_ref" \
      --remote-sha "$remote_sha"
    return
  fi

  if [[ -f scripts/read_session.cjs ]]; then
    node scripts/read_session.cjs context-pack sync-main \
      --local-ref "$local_ref" \
      --local-sha "$local_sha" \
      --remote-ref "$remote_ref" \
      --remote-sha "$remote_sha"
    return
  fi

  echo "[context-pack] WARN: chorus command not found; skipping context-pack sync"
}

while read -r local_ref local_sha remote_ref remote_sha; do
  if [[ "$local_ref" == "refs/heads/main" || "$remote_ref" == "refs/heads/main" ]]; then
    echo "[context-pack] validating main push for ${remote_name} (${remote_url})"
    run_context_sync "$local_ref" "$local_sha" "$remote_ref" "$remote_sha"
  fi
done"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::path::PathBuf;

    fn test_dir(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("chorus_cp_test_{}", name));
        // Clean up from any previous run (idempotent)
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create test dir");
        dir
    }

    // --- upsert_context_pack_block tests ---

    #[test]
    fn upsert_creates_file_when_missing() {
        let dir = test_dir("upsert_creates");
        let file = dir.join("CLAUDE.md");
        let block = "## Context Pack\n\nRead the pack.";
        let marker = "agent-chorus:context-pack:claude";

        upsert_context_pack_block(&file, block, marker).unwrap();

        let content = fs::read_to_string(&file).unwrap();
        assert!(content.contains("<!-- agent-chorus:context-pack:claude:start -->"));
        assert!(content.contains("## Context Pack"));
        assert!(content.contains("<!-- agent-chorus:context-pack:claude:end -->"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn upsert_prepends_when_no_markers() {
        let dir = test_dir("upsert_prepends");
        let file = dir.join("CLAUDE.md");
        fs::write(&file, "# Existing Content\n\nSome instructions.\n").unwrap();

        let block = "## Context Pack\n\nRead the pack.";
        let marker = "agent-chorus:context-pack:claude";

        upsert_context_pack_block(&file, block, marker).unwrap();

        let content = fs::read_to_string(&file).unwrap();
        // Managed block should come before existing content
        let block_pos = content.find("<!-- agent-chorus:context-pack:claude:start -->").unwrap();
        let existing_pos = content.find("# Existing Content").unwrap();
        assert!(block_pos < existing_pos, "managed block should be prepended");
        // Existing content preserved
        assert!(content.contains("Some instructions."));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn upsert_replaces_existing_block() {
        let dir = test_dir("upsert_replaces");
        let file = dir.join("CLAUDE.md");
        let marker = "agent-chorus:context-pack:claude";

        // Write initial content with a managed block in the middle
        let initial = "# Header\n\n\
            <!-- agent-chorus:context-pack:claude:start -->\n\
            ## Old Block\n\
            <!-- agent-chorus:context-pack:claude:end -->\n\n\
            # Footer\n";
        fs::write(&file, initial).unwrap();

        let new_block = "## New Block\n\nUpdated instructions.";
        upsert_context_pack_block(&file, new_block, marker).unwrap();

        let content = fs::read_to_string(&file).unwrap();
        assert!(!content.contains("Old Block"), "old block should be replaced");
        assert!(content.contains("New Block"), "new block should be present");
        assert!(content.contains("Updated instructions."));
        // Surrounding content preserved
        assert!(content.contains("# Header"));
        assert!(content.contains("# Footer"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn upsert_is_idempotent() {
        let dir = test_dir("upsert_idempotent");
        let file = dir.join("CLAUDE.md");
        let block = "## Context Pack\n\nRead the pack.";
        let marker = "agent-chorus:context-pack:claude";

        upsert_context_pack_block(&file, block, marker).unwrap();
        let first = fs::read_to_string(&file).unwrap();

        upsert_context_pack_block(&file, block, marker).unwrap();
        let second = fs::read_to_string(&file).unwrap();

        assert_eq!(first, second, "running upsert twice should produce identical content");

        let _ = fs::remove_dir_all(&dir);
    }

    // --- update_start_here_snapshot tests ---

    #[test]
    fn snapshot_replaces_three_metadata_lines() {
        let dir = test_dir("snapshot_metadata");
        let start_here = dir.join("00_START_HERE.md");

        let initial = "# Context Pack: Start Here\n\n\
            ## Snapshot\n\
            - Repo: `my-repo`\n\
            - Branch at generation: `main`\n\
            - HEAD commit: `abc1234`\n\
            - Generated at: `2026-01-01T00:00:00Z`\n\n\
            ## Read Order\n\
            1. 10_SYSTEM_OVERVIEW.md\n";
        fs::write(&start_here, initial).unwrap();

        update_start_here_snapshot(&dir, "feature-x", Some("def5678"), "2026-03-23T12:00:00Z")
            .unwrap();

        let content = fs::read_to_string(&start_here).unwrap();
        assert!(content.contains("- Branch at generation: `feature-x`"));
        assert!(content.contains("- HEAD commit: `def5678`"));
        assert!(content.contains("- Generated at: `2026-03-23T12:00:00Z`"));
        // Repo line and rest of file preserved
        assert!(content.contains("- Repo: `my-repo`"));
        assert!(content.contains("## Read Order"));
        assert!(content.contains("1. 10_SYSTEM_OVERVIEW.md"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_noop_when_file_missing() {
        let dir = test_dir("snapshot_noop");
        // Don't create 00_START_HERE.md — it should be a no-op
        let result =
            update_start_here_snapshot(&dir, "main", Some("abc1234"), "2026-03-23T12:00:00Z");
        assert!(result.is_ok(), "should return Ok when file is missing");
        assert!(
            !dir.join("00_START_HERE.md").exists(),
            "should not create the file"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    // --- P10: atomic writes (F33) ---

    #[test]
    fn atomic_write_replaces_atomically_and_no_partial_file() {
        let dir = test_dir("atomic_write_basic");
        let target = dir.join("manifest.json");

        atomic_write(&target, b"{\"v\":1}").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "{\"v\":1}");

        // Subsequent write replaces cleanly; tmp sibling is not left behind.
        atomic_write(&target, b"{\"v\":2}").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "{\"v\":2}");
        for entry in fs::read_dir(&dir).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            assert!(
                !name.contains(".tmp."),
                "tmp sibling should be renamed away: {}",
                name
            );
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_write_orphan_tmp_leaves_original_intact() {
        // Simulate a crashed partial write: the tmp file exists with partial bytes,
        // but the rename never happened. The real file must remain intact.
        let dir = test_dir("atomic_write_partial");
        let target = dir.join("manifest.json");
        atomic_write(&target, b"{\"v\":1}").unwrap();

        // Emulate the first phase of atomic_write without the rename step.
        let tmp = dir.join(format!(".manifest.json.tmp.{}", std::process::id() + 1));
        fs::write(&tmp, b"{\"v\":2, partial").unwrap();

        // The real manifest is unchanged.
        assert_eq!(fs::read_to_string(&target).unwrap(), "{\"v\":1}");

        // A subsequent successful atomic_write still works and doesn't trip on
        // the orphan tmp file from a prior crash.
        atomic_write(&target, b"{\"v\":3}").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "{\"v\":3}");

        let _ = fs::remove_dir_all(&dir);
    }

    // --- P10: lock handling (F29) ---

    #[test]
    fn lock_steals_from_dead_pid_with_warning() {
        let dir = test_dir("lock_dead_pid");
        let lock_path = dir.join("seal.lock");

        // PID 1 on macOS/Linux (launchd/init) is always running; we want a PID
        // that is not. A PID larger than any valid one works on every OS.
        fs::write(&lock_path, "4294967294\n").unwrap();
        assert!(lock_path.exists());

        // With a dead PID we should steal (not wait). If the PID ever happens
        // to exist the test would wait up to the timeout — use 2s to keep the
        // suite fast.
        let lock = acquire_lock_with_timeout(&lock_path, 2).unwrap();
        drop(lock);
        assert!(!lock_path.exists(), "lock dropped should remove the file");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn lock_times_out_when_live_holder_does_not_release() {
        let dir = test_dir("lock_live_holder");
        let lock_path = dir.join("seal.lock");

        // Write our own PID (definitely running): the acquire path should not
        // steal it, and should time out.
        fs::write(&lock_path, format!("{}\n", std::process::id())).unwrap();

        let start = std::time::Instant::now();
        let err = acquire_lock_with_timeout(&lock_path, 1).unwrap_err();
        let elapsed = start.elapsed().as_millis();
        assert!(
            elapsed >= 900,
            "should have waited roughly the timeout, got {}ms",
            elapsed
        );
        let msg = format!("{}", err);
        assert!(msg.contains("another seal is in progress"), "msg: {}", msg);
        assert!(msg.contains("waited 1s"), "msg: {}", msg);

        let _ = fs::remove_dir_all(&dir);
    }

    // --- P10: history rotation (F55) ---

    #[test]
    fn history_rotates_when_entry_threshold_exceeded() {
        let dir = test_dir("history_rotation");
        let pack_root = dir.clone();
        let history = pack_root.join("history.jsonl");

        // Write 1001 entries to cross the F55 threshold.
        {
            let mut file = fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&history)
                .unwrap();
            for i in 0..1001 {
                writeln!(
                    file,
                    "{{\"snapshot_id\":\"s{:04}\",\"generated_at\":\"2026-01-01T00:00:00Z\"}}",
                    i
                )
                .unwrap();
            }
        }

        // Trigger rotation by appending one more entry via append_jsonl.
        append_jsonl(&history, &json!({ "snapshot_id": "s1002" })).unwrap();

        // Rotated file should exist with the first 1001 entries; active file
        // should have exactly the 1002-nd.
        let rotated = pack_root.join("history.jsonl.1");
        assert!(rotated.exists(), "history.jsonl.1 should exist");
        let rotated_lines = fs::read_to_string(&rotated)
            .unwrap()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count();
        assert_eq!(rotated_lines, 1001);

        let active = fs::read_to_string(&history).unwrap();
        let active_lines: Vec<&str> =
            active.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(active_lines.len(), 1, "active file should have the new entry");

        // history_index.json should be readable + reference the rotated file.
        let index_path = pack_root.join("history_index.json");
        assert!(index_path.exists(), "history_index.json should be written");
        let idx: Value =
            serde_json::from_str(&fs::read_to_string(&index_path).unwrap()).unwrap();
        let files = idx.get("files").and_then(|f| f.as_array()).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(
            files[0].get("name").and_then(|n| n.as_str()).unwrap(),
            "history.jsonl.1"
        );
        assert_eq!(files[0].get("entries").and_then(|e| e.as_u64()).unwrap(), 1001);
        assert_eq!(
            files[0].get("first_id").and_then(|s| s.as_str()).unwrap(),
            "s0000"
        );
        assert_eq!(
            files[0].get("last_id").and_then(|s| s.as_str()).unwrap(),
            "s1000"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    // --- P10: verify --repair (F32) ---

    #[test]
    fn repair_restores_from_latest_intact_snapshot() {
        let dir = test_dir("repair_restore");
        let pack_root = dir.join(".agent-context");
        let current = pack_root.join("current");
        let snapshots = pack_root.join("snapshots");
        fs::create_dir_all(&current).unwrap();
        fs::create_dir_all(&snapshots).unwrap();

        // Good snapshot with a valid manifest.json.
        let snap_a = snapshots.join("20260101T000000Z_aaaaaaaaaaaa");
        fs::create_dir_all(&snap_a).unwrap();
        fs::write(snap_a.join("manifest.json"), "{\"files\":[]}").unwrap();
        fs::write(snap_a.join("00_START_HERE.md"), "snap-a").unwrap();

        // Newer snapshot with a corrupt manifest: repair must skip it.
        let snap_b = snapshots.join("20260201T000000Z_bbbbbbbbbbbb");
        fs::create_dir_all(&snap_b).unwrap();
        fs::write(snap_b.join("manifest.json"), "not json!").unwrap();
        fs::write(snap_b.join("00_START_HERE.md"), "snap-b").unwrap();

        // Newest-good snapshot.
        let snap_c = snapshots.join("20260301T000000Z_cccccccccccc");
        fs::create_dir_all(&snap_c).unwrap();
        fs::write(snap_c.join("manifest.json"), "{\"files\":[]}").unwrap();
        fs::write(snap_c.join("00_START_HERE.md"), "snap-c").unwrap();

        // Current dir has a corrupt manifest we want to repair past.
        fs::write(current.join("manifest.json"), "{{ broken").unwrap();

        // Run repair with yes=true and the repo_root set to the test dir so
        // rel_path stays clean.
        run_repair(&dir, &pack_root, true).unwrap();

        let restored =
            fs::read_to_string(current.join("00_START_HERE.md")).unwrap();
        assert_eq!(restored, "snap-c", "should restore from newest intact snapshot");
        // Manifest should be valid JSON again.
        let manifest_raw =
            fs::read_to_string(current.join("manifest.json")).unwrap();
        serde_json::from_str::<Value>(&manifest_raw).unwrap();

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn repair_errors_when_no_intact_snapshot_exists() {
        let dir = test_dir("repair_none");
        let pack_root = dir.join(".agent-context");
        fs::create_dir_all(pack_root.join("current")).unwrap();
        fs::create_dir_all(pack_root.join("snapshots")).unwrap();

        let err = run_repair(&dir, &pack_root, true).unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("no recovery snapshot found"),
            "unexpected error: {}",
            msg
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
