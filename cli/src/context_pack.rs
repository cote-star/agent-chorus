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
}

pub struct SealOptions {
    pub reason: Option<String>,
    pub base: Option<String>,
    pub head: Option<String>,
    pub pack_dir: Option<String>,
    pub cwd: Option<String>,
    pub force: bool,
    pub force_snapshot: bool,
}

struct FileMeta {
    path: String,
    sha256: String,
    bytes: u64,
    words: usize,
}

struct ManifestBundle {
    value: Value,
    stable_checksum: String,
    pack_checksum: String,
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
    })
}

pub fn init(options: InitOptions) -> Result<()> {
    let cwd = options
        .cwd
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
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
    let routing_block = build_context_pack_routing_block();
    let agent_configs = [
        ("CLAUDE.md", "agent-chorus:context-pack:claude"),
        ("AGENTS.md", "agent-chorus:context-pack:codex"),
        ("GEMINI.md", "agent-chorus:context-pack:gemini"),
    ];
    for (filename, marker) in &agent_configs {
        upsert_context_pack_block(&repo_root.join(filename), &routing_block, marker)?;
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
        "[context-pack] next: ask your agent to fill AGENT sections, then run `chorus context-pack seal`"
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
    let repo_root = git_repo_root(&cwd)?;
    let repo_name = repo_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("repo")
        .to_string();
    let branch = run_git(&["rev-parse", "--abbrev-ref", "HEAD"], &repo_root, true)?
        .trim()
        .to_string();
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

    let required_files = vec![
        "00_START_HERE.md",
        "10_SYSTEM_OVERVIEW.md",
        "20_CODE_MAP.md",
        "30_BEHAVIORAL_INVARIANTS.md",
        "40_OPERATIONS_AND_RELEASE.md",
    ];

    for file in &required_files {
        let path = current_dir.join(file);
        if !path.exists() {
            return Err(anyhow!(
                "[context-pack] seal failed: missing required file {}",
                rel_path(&path, &repo_root)
            ));
        }
        if !options.force {
            let content = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            if content.contains("<!-- AGENT:") {
                return Err(anyhow!(
                    "[context-pack] seal failed: template markers remain in {} (use --force to override)",
                    rel_path(&path, &repo_root)
                ));
            }
        }
    }

    let generated_at = now_stamp();
    let reason = options
        .reason
        .unwrap_or_else(|| "manual-seal".to_string());

    // Update 00_START_HERE.md snapshot metadata BEFORE collecting file checksums
    // so the manifest reflects the updated content.
    update_start_here_snapshot(&current_dir, branch.trim(), head_sha.as_deref(), &generated_at)?;

    let files_meta = collect_files_meta(
        &current_dir,
        &required_files
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
    )?;

    let previous_manifest = read_json(&manifest_path)?;

    let manifest = build_manifest(
        &generated_at,
        &repo_root,
        &repo_name,
        branch.trim(),
        head_sha.as_deref(),
        "unknown",
        "unknown",
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
        return Err(anyhow!("[context-pack] snapshot not found: {}", target_snapshot));
    }

    let source_dir = snapshots_dir.join(&target_snapshot);
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

pub fn verify(pack_dir: Option<&str>, cwd: &str) -> Result<()> {
    let cwd_path = PathBuf::from(cwd);
    let repo_root = git_repo_root(&cwd_path).unwrap_or_else(|_| cwd_path.clone());
    let pack_root = resolve_pack_root(&repo_root, pack_dir);
    let current_dir = pack_root.join("current");
    let manifest_path = current_dir.join("manifest.json");

    if !manifest_path.exists() {
        return Err(anyhow!(
            "[context-pack] verify failed: manifest.json not found at {}",
            manifest_path.display()
        ));
    }

    let manifest_content = fs::read_to_string(&manifest_path)
        .with_context(|| format!("Failed to read manifest at {}", manifest_path.display()))?;
    let manifest: serde_json::Value = serde_json::from_str(&manifest_content)
        .with_context(|| "Failed to parse manifest.json")?;

    let files = manifest.get("files").and_then(|f| f.as_array());
    if files.is_none() {
        return Err(anyhow!("[context-pack] verify failed: manifest has no 'files' array"));
    }

    let mut pass_count = 0usize;
    let mut fail_count = 0usize;

    for file_entry in files.unwrap() {
        let file_path_str = file_entry.get("path").and_then(|p| p.as_str()).unwrap_or("unknown");
        let expected_hash = file_entry.get("sha256").and_then(|h| h.as_str()).unwrap_or("");
        let actual_path = current_dir.join(file_path_str);

        if !actual_path.exists() {
            eprintln!("  FAIL  {}  (file missing)", file_path_str);
            fail_count += 1;
            continue;
        }

        let content = fs::read_to_string(&actual_path)?;
        use sha2::{Digest, Sha256};
        let actual_hash = format!("{:x}", Sha256::digest(content.as_bytes()));

        if actual_hash == expected_hash {
            println!("  PASS  {}", file_path_str);
            pass_count += 1;
        } else {
            eprintln!("  FAIL  {}  (checksum mismatch)", file_path_str);
            fail_count += 1;
        }
    }

    // Verify pack_checksum if present
    if let Some(expected_pack_checksum) = manifest.get("pack_checksum").and_then(|c| c.as_str()) {
        let mut file_entries: Vec<String> = Vec::new();
        if let Some(files_arr) = files {
            for f in files_arr {
                let p = f.get("path").and_then(|v| v.as_str()).unwrap_or("unknown");
                let h = f.get("sha256").and_then(|v| v.as_str()).unwrap_or("");
                file_entries.push(format!("{}:{}", p, h));
            }
        }
        let combined = file_entries.join("\n");
        use sha2::{Digest, Sha256};
        let actual_pack_checksum = format!("{:x}", Sha256::digest(combined.as_bytes()));
        if actual_pack_checksum == expected_pack_checksum {
            println!("  PASS  pack_checksum");
            pass_count += 1;
        } else {
            eprintln!("  FAIL  pack_checksum (mismatch)");
            fail_count += 1;
        }
    }

    let total = pass_count + fail_count;
    println!("\n  Results: {pass_count}/{total} passed");

    if fail_count > 0 {
        Err(anyhow!("[context-pack] verify failed: {} file(s) did not match", fail_count))
    } else {
        println!("  Context pack integrity verified.");
        Ok(())
    }
}

pub fn check_freshness(base: &str, cwd: &str) -> Result<()> {
    let cwd_path = PathBuf::from(cwd);

    let changed_files = {
        let with_base = run_git(&["diff", "--name-only", &format!("{base}...HEAD")], &cwd_path, true)?;
        if with_base.is_empty() {
            run_git(&["diff", "--name-only", "HEAD~1"], &cwd_path, true)?
        } else {
            with_base
        }
    };

    let mut pack_touched = false;
    let mut relevant = Vec::new();

    for file_path in changed_files.lines().map(|line| line.trim()).filter(|line| !line.is_empty()) {
        if file_path.starts_with(".agent-context/current/") {
            pack_touched = true;
            continue;
        }
        if is_context_relevant(file_path) {
            relevant.push(file_path.to_string());
        }
    }

    if relevant.is_empty() {
        println!("PASS context-pack-freshness (no context-relevant files changed)");
        return Ok(());
    }

    if pack_touched {
        println!("PASS context-pack-freshness (context pack was updated)");
        return Ok(());
    }

    println!(
        "WARNING: {} context-relevant file(s) changed but .agent-context/current/ was not updated:",
        relevant.len()
    );
    for file_path in relevant {
        println!("  - {}", file_path);
    }
    println!();
    println!("Consider running: chorus context-pack build");
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

fn write_text_atomic(path: &Path, text: &str) -> Result<()> {
    let parent = path.parent().ok_or_else(|| anyhow!("Missing parent for {}", path.display()))?;
    ensure_dir(parent)?;
    let tmp = parent.join(format!(
        ".{}.tmp",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("context-pack.tmp")
    ));
    fs::write(&tmp, text).with_context(|| format!("Failed to write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("Failed to move {} -> {}", tmp.display(), path.display()))?;
    Ok(())
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

fn collect_files_meta(current_dir: &Path, relative_paths: &[String]) -> Result<Vec<FileMeta>> {
    let mut out = Vec::new();
    for relative_path in relative_paths {
        let absolute_path = current_dir.join(relative_path);
        let content = fs::read_to_string(&absolute_path)
            .with_context(|| format!("Failed to read {}", absolute_path.display()))?;
        let metadata = fs::metadata(&absolute_path)
            .with_context(|| format!("Failed to stat {}", absolute_path.display()))?;
        out.push(FileMeta {
            path: relative_path.clone(),
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
    head_sha: Option<&str>,
    package_version: &str,
    cargo_version: &str,
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
                "sha256": meta.sha256,
                "bytes": meta.bytes,
                "words": meta.words,
            })
        })
        .collect::<Vec<_>>();

    let value = json!({
        "schema_version": 1,
        "generated_at": generated_at,
        "repo_name": repo_name,
        "repo_root": ".",
        "branch": branch,
        "head_sha": head_sha,
        "package_version": package_version,
        "cargo_version": cargo_version,
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

fn append_jsonl(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("Failed to open {}", path.display()))?;
    writeln!(file, "{}", serde_json::to_string(value)?)
        .with_context(|| format!("Failed to append {}", path.display()))?;
    Ok(())
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

struct FileLock {
    path: PathBuf,
}

fn acquire_lock(path: &Path) -> Result<FileLock> {
    acquire_lock_internal(path, true)
}

fn acquire_lock_internal(path: &Path, allow_recovery: bool) -> Result<FileLock> {
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            let pid = std::process::id();
            writeln!(file, "{}", pid)
                .with_context(|| format!("Failed to write lock {}", path.display()))?;
            Ok(FileLock {
                path: path.to_path_buf(),
            })
        }
        Err(error) if allow_recovery && error.kind() == std::io::ErrorKind::AlreadyExists => {
            if let Ok(content) = fs::read_to_string(path) {
                if let Ok(pid) = content.trim().parse::<u32>() {
                    let is_running = Command::new("kill")
                        .arg("-0")
                        .arg(pid.to_string())
                        .status()
                        .map(|s| s.success())
                        .unwrap_or(false);

                    if !is_running {
                        eprintln!("[context-pack] WARNING: cleaned stale lock (pid {} no longer running)", pid);
                        let _ = fs::remove_file(path);
                        return acquire_lock_internal(path, false);
                    }
                }
            }
            Err(anyhow!(
                "[context-pack] another seal is in progress (lock: {}): {}",
                path.display(),
                error
            ))
        }
        Err(error) => Err(anyhow!(
            "[context-pack] another seal is in progress (lock: {}): {}",
            path.display(),
            error
        )),
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

/// Build the routing block content for agent config files.
fn build_context_pack_routing_block() -> String {
    "## Context Pack\n\
     \n\
     When asked to understand this repository:\n\
     \n\
     1. Read `.agent-context/current/00_START_HERE.md` first.\n\
     2. Follow the read order defined in that file.\n\
     3. Only open project files when the context pack identifies a specific target."
        .to_string()
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

## Read Order (Token-Efficient)
1. Read this file.
2. Read `10_SYSTEM_OVERVIEW.md` for architecture and execution paths.
3. Read `30_BEHAVIORAL_INVARIANTS.md` before changing behavior.
4. Use `20_CODE_MAP.md` to deep dive only relevant files.
5. Use `40_OPERATIONS_AND_RELEASE.md` for tests, release, and maintenance.

## Task-Type Routing
**Impact analysis** (list every file that must change): read `30_BEHAVIORAL_INVARIANTS.md` Update Checklist *before* `20_CODE_MAP.md` — the checklist has the full blast radius per change type. CODE_MAP alone is not exhaustive.
**Navigation / lookup** (find a file, find a value): start with `20_CODE_MAP.md` Scope Rule.
**Planning** (add a new feature/module): follow the Extension Recipe in `20_CODE_MAP.md`, then cross-check the BEHAVIORAL_INVARIANTS checklist for that change type.
**Diagnosis** (silent failures, unexpected output): start with `10_SYSTEM_OVERVIEW.md` Silent Failure Modes, then the relevant diagnostic row in `30_BEHAVIORAL_INVARIANTS.md`.

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
| Path | Approach | What | Why It Matters | Risk |
| --- | --- | --- | --- | --- | -->

## Quick Lookup Shortcuts
<!-- AGENT: Add 4-6 common lookup patterns. Map intent to exact file and what to look for.
| I need to find... | Open this file | Look for |
| --- | --- | --- | -->

## Cross-Cutting Tracing Flows
<!-- AGENT: For changes that ripple through multiple layers, document the full chain.
Example: "New parameter through call chain: schema → step → client → wrapper → tests"
List files in dependency order so agents trace the change correctly. -->

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
2. For each `<!-- AGENT: ... -->` block, replace it with repository-derived content.
3. After filling all sections, run `chorus context-pack seal` to finalize (manifest + snapshot).

## Quality Criteria
- Content must be factual and verifiable from the repository.
- Prefer concise bullets over long prose.
- Keep total word count under ~2000 words across all files.
- Do not include secrets or credentials.
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
