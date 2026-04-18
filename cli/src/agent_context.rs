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
/// Current manifest schema version understood by this tool.
/// Increment on backward-incompatible manifest changes. See P11 in the
/// agent-context plan for the enforcement contract on verify.
const CURRENT_SCHEMA_VERSION: u64 = 1;
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

/// P1 — semantic baseline fields recorded on the manifest at seal so later
/// verification can detect drift without re-reading the full repo.
///
/// All fields are computed up-front by helpers in this module; `build_manifest`
/// simply serializes them. Helpers degrade to empty values when their source
/// config files are absent so `build_manifest` never has to branch on presence.
#[derive(Default)]
struct SemanticBaseline {
    /// Glob pattern -> number of repo-relative files currently matching.
    /// Sourced from `completeness_contract.json` `required_file_families[]`
    /// and `reporting_rules.json` `groupable_families[]`.
    family_counts: std::collections::BTreeMap<String, usize>,
    /// Numeric claims extracted from prose in `.agent-context/current/*.md`.
    /// Each entry: `{noun, count, file, line}`.
    declared_counts: Vec<Value>,
    /// For every file referenced in `search_scope.json` `verification_shortcuts`,
    /// top-level function signatures keyed `"<file>::<fn_name>"`.
    shortcut_signatures: std::collections::BTreeMap<String, String>,
    /// SHA256 of the dependency-declaring files that exist at the repo root.
    /// Keys: `pyproject`, `cargo`, `npm`. Missing files are simply omitted.
    dependencies_snapshot: std::collections::BTreeMap<String, String>,
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
    /// P3: when true, emit the suggest-patches JSON payload
    /// `{changed_files, pack_sections_to_update, diff_excerpt, baseline_drift}`
    /// on stdout and exit early. Disables normal human-readable output.
    pub suggest_patches: bool,
    /// P6: opt-in CI gate that fails if any commit in the PR range touches
    /// both `.agent-context/**` and non-pack paths. Intended for teams that
    /// want the "pack edits land as their own commit" convention enforced
    /// at merge time. Off by default.
    pub enforce_separate_commits: bool,
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
    /// P3: Pack sections (by filename) affected by the changed files, resolved
    /// through the `zones[]` array in `.agent-context/relevance.json`.
    /// Empty when no zone map is present or none of the changed files match.
    affected_sections: Vec<String>,
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

/// Outcome of the P11 schema-version gate on verify.
///
/// - `Ok` means the manifest is safe to process. A human-readable warning may
///   still be emitted (missing field, or manifest older than the current tool).
/// - `Err` means the manifest is newer than this tool understands; the caller
///   should fail loudly and ask the user to upgrade chorus.
pub(crate) enum SchemaVersionCheck {
    Ok { warning: Option<String> },
    TooNew { message: String },
}

/// Enforce `manifest.schema_version` against [`CURRENT_SCHEMA_VERSION`].
///
/// Contract (see P11 / F34 in the agent-context plan):
/// - missing field -> Ok, back-compat deprecation warning
/// - equal to current -> Ok, no warning
/// - older than current -> Ok, upgrade-recommended warning
/// - newer than current -> TooNew (verify must fail loudly)
pub(crate) fn check_schema_version(manifest: &Value) -> SchemaVersionCheck {
    let raw = manifest.get("schema_version");
    let version = raw.and_then(|v| v.as_u64());
    match (raw, version) {
        (None, _) => SchemaVersionCheck::Ok {
            warning: Some(format!(
                "manifest has no schema_version field; treating as v1 for back-compat. Re-seal to upgrade to v{CURRENT_SCHEMA_VERSION}."
            )),
        },
        (Some(_), None) => SchemaVersionCheck::Ok {
            warning: Some(format!(
                "manifest.schema_version is not a positive integer; treating as v1 for back-compat. Re-seal to upgrade to v{CURRENT_SCHEMA_VERSION}."
            )),
        },
        (Some(_), Some(v)) if v == CURRENT_SCHEMA_VERSION => SchemaVersionCheck::Ok { warning: None },
        (Some(_), Some(v)) if v < CURRENT_SCHEMA_VERSION => SchemaVersionCheck::Ok {
            warning: Some(format!(
                "manifest is schema v{v}, tool is v{CURRENT_SCHEMA_VERSION}. Re-seal to upgrade."
            )),
        },
        (Some(_), Some(v)) => SchemaVersionCheck::TooNew {
            message: format!(
                "manifest schema v{v} is newer than this tool (v{CURRENT_SCHEMA_VERSION}). Upgrade chorus."
            ),
        },
    }
}

/// Compute the SHA256 of the running `chorus` binary, used as
/// `manifest.verifier_sha256`. Returns `None` when the current executable
/// cannot be located or read (rare, but possible on exotic platforms).
fn current_exe_sha256() -> Option<String> {
    let path = std::env::current_exe().ok()?;
    let bytes = fs::read(&path).ok()?;
    Some(sha256_hex(&bytes))
}

fn check_freshness_inner(base: &str, cwd: &Path) -> Result<FreshnessResult> {
    // P3: delegate to the zone-map-aware variant. When no `relevance.json`
    // exists, or it only declares the legacy `include/exclude` shape, the
    // helper silently falls back to the Pass-0 hardcoded behavior.
    check_freshness_with_zones(base, cwd)
}

/// P3: zone-aware freshness check.
///
/// Extends [`check_freshness_inner`]'s original behavior with a zone map. When
/// `.agent-context/relevance.json` declares a `zones[]` array, each changed
/// file is resolved to its affected pack sections via the zone map and the
/// union is returned in `affected_sections`. When the file has no zone map,
/// or the map only declares legacy `include/exclude` rules, this falls back to
/// the Pass-0 `is_context_relevant_with_rules` behavior (the existing
/// hardcoded defaults).
///
/// Pass-0 semantics preserved:
/// - F27 non-git → skipped
/// - F24 shallow clone → skipped
/// - F25 initial commit → skipped
///
/// New P3 behavior:
/// - Zone map validation: any zone whose `paths[]` resolves to zero tracked
///   files is treated as an authoring bug and returned as a `fail` status so
///   `verify` surfaces it loudly.
fn check_freshness_with_zones(base: &str, cwd: &Path) -> Result<FreshnessResult> {
    // F27: non-git directory → explicit skipped status rather than silent empty diff.
    if !is_git_repo(cwd) {
        return Ok(FreshnessResult {
            status: "skipped".to_string(),
            changed_files: Vec::new(),
            pack_updated: false,
            skipped_reason: Some("non-git".to_string()),
            affected_sections: Vec::new(),
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
            affected_sections: Vec::new(),
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
            affected_sections: Vec::new(),
        });
    }

    // Resolve the repo root for zone-map lookup. Fall back to cwd on failure so
    // non-standard layouts still get freshness behavior (they just won't get
    // zone resolution).
    let repo_root = git_repo_root(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let zone_map = load_zone_map(&repo_root);

    // Validate zone map: each zone's paths must resolve to at least one tracked
    // file (per P3 plan). If not, emit a `fail` result so verify fails loudly.
    if let Some(ref zm) = zone_map {
        if let Some(msg) = validate_zone_map(zm, &repo_root)? {
            return Ok(FreshnessResult {
                status: "fail".to_string(),
                changed_files: Vec::new(),
                pack_updated: false,
                skipped_reason: Some(msg),
                affected_sections: Vec::new(),
            });
        }
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
    let mut affected = BTreeSet::new();

    for file_path in changed_files_raw.lines().map(|line| line.trim()).filter(|line| !line.is_empty()) {
        if file_path.starts_with(".agent-context/current/") {
            pack_touched = true;
            continue;
        }
        match &zone_map {
            Some(zm) => {
                let sections = resolve_affected_sections(file_path, zm);
                if !sections.is_empty() {
                    relevant.push(file_path.to_string());
                    for s in sections {
                        affected.insert(s);
                    }
                }
            }
            None => {
                if is_context_relevant(file_path) {
                    relevant.push(file_path.to_string());
                }
            }
        }
    }

    let affected_vec: Vec<String> = affected.into_iter().collect();

    if relevant.is_empty() {
        return Ok(FreshnessResult {
            status: "pass".to_string(),
            changed_files: Vec::new(),
            pack_updated: pack_touched,
            skipped_reason: None,
            affected_sections: affected_vec,
        });
    }

    if pack_touched {
        return Ok(FreshnessResult {
            status: "pass".to_string(),
            changed_files: relevant,
            pack_updated: true,
            skipped_reason: None,
            affected_sections: affected_vec,
        });
    }

    Ok(FreshnessResult {
        status: "warn".to_string(),
        changed_files: relevant,
        pack_updated: false,
        skipped_reason: None,
        affected_sections: affected_vec,
    })
}

/// P6: inspect every commit in `base..HEAD` and return human-readable error
/// lines for commits that touch both `.agent-context/**` and non-pack paths.
/// Empty return means the range passes the separate-commit convention.
///
/// This is intentionally conservative: it only flags commits that *mix*
/// pack and non-pack changes. Pure-pack or pure-code commits are fine, as
/// are ranges where pack and code commits are interleaved but separated.
fn check_separate_commits(base: &str, cwd: &Path) -> Result<Vec<String>> {
    // If git history isn't usable (non-git, shallow clone, initial commit)
    // this check has nothing to say — return empty rather than fail loudly,
    // since the surrounding verify flow already reports those cases.
    if !is_git_repo(cwd) {
        return Ok(Vec::new());
    }
    if is_shallow_repo(cwd).unwrap_or(false) {
        return Ok(Vec::new());
    }

    let range = format!("{base}..HEAD");
    let log = run_git(&["log", "--format=%H", &range], cwd, true)?;
    if log.trim().is_empty() {
        return Ok(Vec::new());
    }

    let mut errors = Vec::new();
    for sha in log.lines().map(str::trim).filter(|s| !s.is_empty()) {
        let files = run_git(
            &["diff-tree", "--name-only", "--no-commit-id", "-r", sha],
            cwd,
            true,
        )?;
        let mut touches_pack = false;
        let mut touches_non_pack = false;
        for path in files.lines().map(str::trim).filter(|s| !s.is_empty()) {
            if path.starts_with(".agent-context/") {
                touches_pack = true;
            } else {
                touches_non_pack = true;
            }
            if touches_pack && touches_non_pack {
                break;
            }
        }
        if touches_pack && touches_non_pack {
            errors.push(format!("commit {sha} mixes pack + non-pack changes"));
        }
    }

    Ok(errors)
}

/// P6: persist the most recent freshness warning to
/// `.agent-context/current/.last_freshness.json` so the pre-push hook can
/// later detect a pack-only follow-up push and report "warning appears
/// addressed". Silent no-op when the pack directory is missing — this runs
/// from check-freshness paths that may fire before init has scaffolded
/// `current/` (e.g. a warn surfaced during a seal dry run).
fn write_last_freshness_state(repo_root: &Path, result: &FreshnessResult) {
    if result.status != "warn" {
        return;
    }
    let pack_root = resolve_pack_root(repo_root, None);
    let current_dir = pack_root.join("current");
    if !current_dir.exists() {
        return;
    }
    let state_path = current_dir.join(".last_freshness.json");
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let payload = json!({
        "changed_files": result.changed_files,
        "affected_sections": result.affected_sections,
        "timestamp": timestamp,
    });
    // Best-effort: serialization failure or write failure must not break
    // freshness reporting. The hook tolerates a missing state file.
    if let Ok(serialized) = serde_json::to_string_pretty(&payload) {
        let _ = fs::write(&state_path, serialized);
    }
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
        // P3: detect a `study/` directory at repo root and tailor the default
        // zone map accordingly so freshness surfaces the right pack sections.
        let has_study = repo_root.join("study").is_dir();
        write_text(
            &relevance_path,
            &default_relevance_json_with_study(has_study),
        )?;
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

    // P1 — compute semantic baseline once, pass into build_manifest. Helpers
    // degrade to empty for repos without the optional structured configs.
    // Resolved here (pre file-hashing) because P5 handlebar expansion consumes
    // `family_counts` before the files are hashed.
    let baseline = collect_semantic_baseline(&repo_root, &current_dir);

    // P5 — expand `{{counts.<slug>}}` handlebars and detect stale prose
    // numeric claims before collect_files_meta hashes the files. The expanded
    // bytes are what get sealed into the manifest, so prose and manifest
    // agree by construction.
    let (slug_counts, noun_counts) = derive_count_maps(&baseline.family_counts);
    let expansion_reports =
        apply_count_templates(&current_dir, &required_files, &slug_counts, &noun_counts);
    let mut all_mismatches: Vec<NumericClaimMismatch> = Vec::new();
    for report in &expansion_reports {
        let abs_path = current_dir.join(&report.file);
        // Write back the expanded content only when it changed — avoids a
        // spurious snapshot bump on repos that don't use handlebars.
        let changed = match fs::read_to_string(&abs_path) {
            Ok(current) => current != report.expanded,
            Err(_) => false,
        };
        if changed {
            write_text_atomic(&abs_path, &report.expanded)?;
        }
        all_mismatches.extend(report.mismatches.iter().cloned());
    }
    if !all_mismatches.is_empty() {
        let mut msg = String::from(
            "[context-pack] seal failed: prose numeric claims disagree with authoritative family_counts:\n",
        );
        for m in &all_mismatches {
            msg.push_str(&format!(
                "  - {}:{}: claimed {} {}, authoritative {}\n",
                m.file, m.line, m.claimed_count, m.noun, m.authoritative_count
            ));
        }
        if options.force {
            // With --force we downgrade the failure to a warning so the
            // author can unstick an unusual seal. verify() still flags the
            // drift on the next run, preserving the audit trail.
            eprintln!("{}", msg.trim_end());
            eprintln!("[context-pack] WARN: --force downgraded count-claim failures to warnings");
        } else {
            msg.push_str(
                "  Fix: update prose to `{{counts.<slug>}}` or surround with `<!-- count-claim: ignore -->` / `<!-- count-claim: end -->`. Use --force to override.\n",
            );
            return Err(anyhow!(msg));
        }
    }

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
        &baseline,
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

        // P12 / F42 — compute the audit trail fields *before* copying the
        // snapshot so the previous snapshot's markdown is still the correct
        // baseline. `prose_diff_sections` records which H2 headings changed
        // vs the most recent snapshot; empty on first-seal.
        let prose_diff_sections = compute_prose_diff_sections(&snapshots_dir, &current_dir);
        let sealed_by = git_committer_identity(&repo_root);

        copy_dir_recursive(&current_dir, &snapshot_dir)?;

        // P12 / F42 — write the audit-trail fields alongside the existing
        // history fields. Older readers keep working (additive schema).
        let history_entry = json!({
            "snapshot_id": snapshot_id,
            "generated_at": generated_at,
            "branch": branch.trim(),
            "head_sha": head_sha,
            "base_sha": options.base,
            "reason": reason,
            "changed_files": Vec::<String>::new(),
            "pack_checksum": manifest.pack_checksum,
            // P12/F42 — audit trail.
            "sealed_by": sealed_by,
            "prose_diff_sections": prose_diff_sections,
            "seal_reason": reason,
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
// P1 — sentinels for the opt-in post-commit-reconcile hook. Kept distinct from
// the pre-push sentinels so installing or uninstalling one never mangles the
// other.
const POST_COMMIT_SENTINEL_START: &str = "# --- agent-chorus:post-commit:start ---";
const POST_COMMIT_SENTINEL_END: &str = "# --- agent-chorus:post-commit:end ---";

pub fn install_hooks(cwd: &str, dry_run: bool) -> Result<()> {
    install_hooks_with_options(cwd, dry_run, false)
}

/// P1 — `install-hooks` extended surface. Callers that want the post-commit
/// reconcile hook pass `enable_post_commit_reconcile: true`. The base
/// pre-push install path is unchanged so existing callers keep working.
pub fn install_hooks_with_options(
    cwd: &str,
    dry_run: bool,
    enable_post_commit_reconcile: bool,
) -> Result<()> {
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

    // P1 — opt-in post-commit reconcile hook. When enabled, we write
    // `.githooks/post-commit` (or the configured hooks dir) that invokes
    // `chorus agent-context post-commit-reconcile` when the commit touched
    // `.agent-context/**`. This keeps the manifest's `post_commit_sha` aligned
    // with HEAD after a pack-bearing commit lands.
    if enable_post_commit_reconcile {
        let post_commit_path = hooks_dir.join("post-commit");
        let post_commit_section = format!(
            "{}\n{}\n{}",
            POST_COMMIT_SENTINEL_START,
            build_post_commit_hook_section(),
            POST_COMMIT_SENTINEL_END
        );
        let final_pc = if post_commit_path.exists() {
            let existing = fs::read_to_string(&post_commit_path).unwrap_or_default();
            if existing.contains(POST_COMMIT_SENTINEL_START)
                && existing.contains(POST_COMMIT_SENTINEL_END)
            {
                let start_idx = existing.find(POST_COMMIT_SENTINEL_START).unwrap();
                let end_idx = existing.find(POST_COMMIT_SENTINEL_END).unwrap()
                    + POST_COMMIT_SENTINEL_END.len();
                let end_idx = if existing.as_bytes().get(end_idx) == Some(&b'\n') {
                    end_idx + 1
                } else {
                    end_idx
                };
                format!(
                    "{}{}\n{}",
                    &existing[..start_idx],
                    post_commit_section,
                    &existing[end_idx..]
                )
            } else {
                let mut content = existing;
                if !content.ends_with('\n') {
                    content.push('\n');
                }
                content.push('\n');
                content.push_str(&post_commit_section);
                content.push('\n');
                content
            }
        } else {
            format!(
                "#!/usr/bin/env bash\nset -euo pipefail\n\n{}\n",
                post_commit_section
            )
        };
        if !dry_run {
            ensure_dir(&hooks_dir)?;
            write_text(&post_commit_path, &final_pc)?;
            #[cfg(unix)]
            {
                let mut perms = fs::metadata(&post_commit_path)?.permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&post_commit_path, perms)?;
            }
        }
        println!(
            "[context-pack] {}: {}",
            if dry_run { "planned" } else { "updated" },
            rel_path(&post_commit_path, &repo_root)
        );
    }

    Ok(())
}

/// P4 — shipped PreToolUse hook template. Kept inline so `install-hooks
/// --install-settings-template` can be used without the shipped
/// `templates/` directory on disk (published binaries, `npx` invocations
/// where the template tree is not unpacked alongside the CLI).
///
/// Keep in sync with `templates/settings.agent-context.json`.
const SETTINGS_TEMPLATE_JSON: &str = include_str!("../../templates/settings.agent-context.json");

/// P4 — merge `templates/settings.agent-context.json` into
/// `<cwd>/.claude/settings.json`, preserving existing keys and existing
/// `hooks.PreToolUse` entries. Missing-file → creates the file. Existing
/// file → deep-merge the template's `hooks.PreToolUse` entries, skipping
/// any entry whose `matcher` already has a command identical to one we
/// would insert. Writes are atomic so a crash mid-write never leaves
/// `settings.json` half-rewritten.
///
/// Idempotency: running twice produces identical content because the
/// matcher+command pair is the de-dup key.
pub fn install_settings_template(cwd: &str, dry_run: bool) -> Result<()> {
    let cwd_path = PathBuf::from(cwd);
    let claude_dir = cwd_path.join(".claude");
    let settings_path = claude_dir.join("settings.json");

    // Parse the shipped template once. A broken template is a programmer
    // bug, not a user error — fail fast and loudly.
    let template: Value = serde_json::from_str(SETTINGS_TEMPLATE_JSON)
        .context("Failed to parse embedded settings.agent-context.json template")?;

    let existing: Value = if settings_path.exists() {
        let raw = fs::read_to_string(&settings_path)
            .with_context(|| format!("Failed to read {}", settings_path.display()))?;
        if raw.trim().is_empty() {
            Value::Object(serde_json::Map::new())
        } else {
            serde_json::from_str(&raw)
                .with_context(|| format!("Failed to parse {}", settings_path.display()))?
        }
    } else {
        Value::Object(serde_json::Map::new())
    };

    let merged = merge_settings_template(&existing, &template);
    let merged_text = format!("{}\n", serde_json::to_string_pretty(&merged)?);

    let existing_text = if settings_path.exists() {
        fs::read_to_string(&settings_path).unwrap_or_default()
    } else {
        String::new()
    };
    let unchanged = existing_text == merged_text;

    if dry_run {
        println!(
            "[agent-context] planned: {} ({})",
            rel_path(&settings_path, &cwd_path),
            if unchanged { "unchanged" } else { "merge PreToolUse hooks" }
        );
        return Ok(());
    }

    if unchanged {
        println!(
            "[agent-context] unchanged: {}",
            rel_path(&settings_path, &cwd_path)
        );
        return Ok(());
    }

    ensure_dir(&claude_dir)?;
    write_text_atomic(&settings_path, &merged_text)?;
    println!(
        "[agent-context] updated: {} (PreToolUse hooks merged)",
        rel_path(&settings_path, &cwd_path)
    );
    Ok(())
}

/// P4 — pure helper so merge behavior is unit-testable without touching
/// the filesystem. Combines two settings.json values with the following
/// rules:
///
/// 1. Non-`hooks` keys from `existing` survive unchanged.
/// 2. For `hooks.PreToolUse`, each template entry is appended unless the
///    existing config already contains an entry with the same `matcher`
///    AND a command string that matches the template's command. This
///    preserves user-authored matchers while keeping the merge idempotent.
/// 3. For any other `hooks.*` array in the template, entries are appended
///    only when their serialized form is not already present.
pub(crate) fn merge_settings_template(existing: &Value, template: &Value) -> Value {
    let mut out = match existing {
        Value::Object(map) => map.clone(),
        _ => serde_json::Map::new(),
    };

    let template_hooks = match template.get("hooks").and_then(|v| v.as_object()) {
        Some(h) => h,
        None => return Value::Object(out),
    };

    let existing_hooks = out
        .get("hooks")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    let mut merged_hooks = existing_hooks.clone();

    for (hook_name, template_entries) in template_hooks {
        let template_arr = match template_entries.as_array() {
            Some(arr) => arr,
            None => continue,
        };
        let mut combined: Vec<Value> = merged_hooks
            .get(hook_name)
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        for entry in template_arr {
            if !hooks_entry_already_present(&combined, entry) {
                combined.push(entry.clone());
            }
        }

        merged_hooks.insert(hook_name.clone(), Value::Array(combined));
    }

    out.insert("hooks".to_string(), Value::Object(merged_hooks));
    Value::Object(out)
}

/// P4 — idempotency check for a single hook entry. An entry is "already
/// present" when some existing entry shares the same `matcher` value AND
/// one of its inner `hooks[].command` strings equals one of the template
/// entry's commands. This lets users add their own commands to a matcher
/// we ship without tripping duplicate detection, while still keeping our
/// own commands from being appended twice.
fn hooks_entry_already_present(existing_arr: &[Value], candidate: &Value) -> bool {
    let candidate_matcher = candidate.get("matcher").and_then(|v| v.as_str());
    let candidate_commands: Vec<&str> = candidate
        .get("hooks")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|h| h.get("command").and_then(|c| c.as_str()))
                .collect()
        })
        .unwrap_or_default();

    for existing in existing_arr {
        let existing_matcher = existing.get("matcher").and_then(|v| v.as_str());
        if existing_matcher != candidate_matcher {
            continue;
        }
        let existing_commands: Vec<&str> = existing
            .get("hooks")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|h| h.get("command").and_then(|c| c.as_str()))
                    .collect()
            })
            .unwrap_or_default();
        for cmd in &candidate_commands {
            if existing_commands.contains(cmd) {
                return true;
            }
        }
    }
    false
}

/// P1 — `chorus agent-context post-commit-reconcile`. Reads the current
/// manifest, stamps `post_commit_sha` with the current git HEAD, and writes
/// the manifest atomically back. No-op (Ok) when the manifest is absent —
/// this lets the post-commit hook run unconditionally without crashing on
/// repos that have not run `init` yet.
///
/// Invariant: never mutates `head_sha_at_seal`, `head_sha`, or any content
/// hashes. Only `post_commit_sha` (and `post_commit_reconciled_at` for
/// operator forensics) are updated.
pub fn post_commit_reconcile(cwd: Option<&str>, pack_dir: Option<&str>) -> Result<()> {
    let cwd_path = match cwd {
        Some(c) => PathBuf::from(c),
        None => env::current_dir().context("Failed to resolve current directory")?,
    };
    if !is_git_repo(&cwd_path) {
        return Err(anyhow!(
            "[context-pack] post-commit-reconcile failed: not a git repository (cwd: {})",
            cwd_path.display()
        ));
    }
    let repo_root = git_repo_root(&cwd_path)?;
    let pack_root = resolve_pack_root(&repo_root, pack_dir);
    let current_dir = pack_root.join("current");
    let manifest_path = current_dir.join("manifest.json");
    if !manifest_path.exists() {
        eprintln!(
            "[context-pack] post-commit-reconcile: manifest not found at {} — skipping",
            rel_path(&manifest_path, &repo_root)
        );
        return Ok(());
    }

    let lock_path = pack_root.join("seal.lock");
    // F29/F30: reconcile rewrites the manifest, so we acquire the same lock
    // seal uses. Verify is lock-free and unaffected.
    let _lock = acquire_lock(&lock_path)?;

    let raw = fs::read_to_string(&manifest_path)
        .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
    let mut manifest: Value = serde_json::from_str(&raw)
        .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;

    let head_sha = run_git(&["rev-parse", "HEAD"], &repo_root, true)?
        .trim()
        .to_string();
    if head_sha.is_empty() {
        return Err(anyhow!(
            "[context-pack] post-commit-reconcile failed: could not resolve git HEAD"
        ));
    }

    if let Some(obj) = manifest.as_object_mut() {
        obj.insert("post_commit_sha".to_string(), Value::String(head_sha.clone()));
        obj.insert(
            "post_commit_reconciled_at".to_string(),
            Value::String(now_stamp()),
        );
    } else {
        return Err(anyhow!(
            "[context-pack] post-commit-reconcile failed: manifest is not a JSON object"
        ));
    }

    write_text_atomic(
        &manifest_path,
        &format!("{}\n", serde_json::to_string_pretty(&manifest)?),
    )?;
    println!(
        "[context-pack] post-commit-reconcile: post_commit_sha={} ({})",
        short_sha(Some(&head_sha)),
        rel_path(&manifest_path, &repo_root)
    );
    Ok(())
}

pub fn verify(options: VerifyOptions) -> Result<()> {
    let cwd_path = PathBuf::from(&options.cwd);
    let repo_root = git_repo_root(&cwd_path).unwrap_or_else(|_| cwd_path.clone());
    let pack_root = resolve_pack_root(&repo_root, options.pack_dir.as_deref());
    let current_dir = pack_root.join("current");
    let manifest_path = current_dir.join("manifest.json");

    // P3: `--suggest-patches` short-circuits verify to emit a structured JSON
    // payload meant for agent consumption. Integrity/freshness checks still
    // feed it via `suggest_patches()` but no human-readable output is printed.
    if options.suggest_patches {
        let base_ref = options.base.as_deref().unwrap_or("origin/main");
        let payload = suggest_patches(base_ref, &cwd_path)?;
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

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

    // P11 / F34: schema_version gate. This is the first check so we refuse
    // fast when the manifest was sealed by a newer chorus than this one.
    match check_schema_version(&manifest) {
        SchemaVersionCheck::Ok { warning } => {
            if let Some(msg) = warning {
                eprintln!("[agent-context] WARN: {msg}");
            }
        }
        SchemaVersionCheck::TooNew { message } => {
            if options.ci {
                let result = json!({
                    "integrity": "fail",
                    "freshness": "skip",
                    "changed_files": [],
                    "pack_updated": false,
                    "exit_code": 1,
                    "schema_error": message,
                });
                println!("{}", serde_json::to_string_pretty(&result)?);
                std::process::exit(1);
            }
            return Err(anyhow!("[agent-context] verify failed: {message}"));
        }
    }

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

    // P5 — drift check: re-resolve family_counts from the current repo, then
    // re-scan each pack markdown for prose numeric claims. Any mismatch is a
    // warning (never a hard fail on verify — verify only reports drift). This
    // catches the window between a successful seal and a subsequent repo
    // change that invalidated the sealed counts.
    let drift_counts = resolve_family_counts(&repo_root, &current_dir);
    let (_drift_slugs, drift_noun_counts) = derive_count_maps(&drift_counts);
    let mut drift_mismatches: Vec<NumericClaimMismatch> = Vec::new();
    if !drift_noun_counts.is_empty() {
        if let Ok(entries) = fs::read_dir(&current_dir) {
            let mut md_paths: Vec<PathBuf> = Vec::new();
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file()
                    && path.extension().and_then(|ext| ext.to_str()) == Some("md")
                {
                    md_paths.push(path);
                }
            }
            md_paths.sort();
            for path in md_paths {
                let label = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                let text = match fs::read_to_string(&path) {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                drift_mismatches
                    .extend(extract_numeric_claims(&text, &drift_noun_counts, &label));
            }
        }
    }
    if !drift_mismatches.is_empty() && !options.ci {
        eprintln!(
            "  WARN  count drift: {} prose claim(s) disagree with current repo:",
            drift_mismatches.len()
        );
        for m in &drift_mismatches {
            eprintln!(
                "    - {}:{}: prose says {} {}, repo currently has {}",
                m.file, m.line, m.claimed_count, m.noun, m.authoritative_count
            );
        }
    }

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
                affected_sections: Vec::new(),
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
                affected_sections: Vec::new(),
            },
        }
    };

    // P2 — structural checks. Non-fatal by default: warnings surface in CI
    // JSON under `structural_warnings[]` and in human output as a summary
    // block, but they do not flip the exit code. This preserves existing
    // verify behavior for already-sealed packs while making drift visible.
    let structural_warnings = run_structural_checks(&manifest, &repo_root, &current_dir);

    if options.ci {
        // P7: compute the diff-since-seal payload so CI can surface acceptance
        // tests whose `invalidated_by` function signatures have drifted. When
        // any are present AND the pack wasn't updated to reconcile, CI must
        // fail so the reconciler can re-validate the tests. Errors from the
        // helper degrade to an empty list — we never want CI to fail because
        // the diff computation itself hit a transient condition.
        let diff_result = diff_since_seal(&cwd_path, options.pack_dir.as_deref())
            .unwrap_or_else(|_| DiffSinceSealResult {
                value: json!({
                    "baseline_sha": Value::Null,
                    "pack_updated": false,
                    "zones": Vec::<Value>::new(),
                    "acceptance_tests_invalidated": Vec::<Value>::new(),
                    "recommended_reconciliation_actions": Vec::<String>::new(),
                }),
                acceptance_tests_invalidated: Vec::new(),
            });
        let pack_updated_in_diff = diff_result
            .value
            .get("pack_updated")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let acceptance_gate_failing =
            !diff_result.acceptance_tests_invalidated.is_empty() && !pack_updated_in_diff;

        // P3: zone-map authoring bugs surface as freshness.status == "fail"
        // and must be treated as a hard failure alongside integrity/warn.
        // P7: gate on acceptance_tests_invalidated when the pack is stale.
        let exit_code = if !integrity_passed
            || freshness.status == "warn"
            || freshness.status == "fail"
            || acceptance_gate_failing
        {
            1
        } else {
            0
        };
        let mut result_obj = serde_json::Map::new();
        result_obj.insert("integrity".to_string(), json!(integrity_status));
        result_obj.insert("freshness".to_string(), json!(freshness.status));
        result_obj.insert("changed_files".to_string(), json!(freshness.changed_files));
        result_obj.insert("pack_updated".to_string(), json!(freshness.pack_updated));
        if let Some(reason) = &freshness.skipped_reason {
            result_obj.insert("skipped_reason".to_string(), json!(reason));
        }
        // P3: emit affected_sections so downstream consumers (CI, agents) can
        // target the right pack files. Always emit (possibly empty) so schema
        // stays stable.
        result_obj.insert(
            "affected_sections".to_string(),
            json!(freshness.affected_sections),
        );
        // P2: emit structural warnings so CI consumers can surface drift
        // without parsing stderr. Shape: [{kind, message, affected_pack_files}].
        result_obj.insert(
            "structural_warnings".to_string(),
            structural_warnings_as_json(&structural_warnings),
        );
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
            if !freshness.affected_sections.is_empty() {
                println!("  Affected pack sections:");
                for s in &freshness.affected_sections {
                    println!("    - {}", s);
                }
            }
            println!("  Consider running: chorus agent-context build");
        }
        "fail" => {
            if let Some(reason) = &freshness.skipped_reason {
                eprintln!("  Freshness: FAIL ({})", reason);
            } else {
                eprintln!("  Freshness: FAIL (zone map invalid)");
            }
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

    // P2: structural warnings summary (non-fatal). Rendered after freshness
    // so the human reader sees integrity → freshness → structural in the
    // same top-down order as the CI JSON keys.
    if !structural_warnings.is_empty() {
        eprintln!(
            "  Structural: {} warning(s)",
            structural_warnings.len()
        );
        for w in &structural_warnings {
            eprintln!("    [{}] {}", w.kind, w.message);
            if !w.affected_pack_files.is_empty() {
                eprintln!(
                    "        affected: {}",
                    w.affected_pack_files.join(", ")
                );
            }
        }
    }

    // P3: zone-map validation failures are hard fails alongside integrity.
    if freshness.status == "fail" {
        let reason = freshness
            .skipped_reason
            .clone()
            .unwrap_or_else(|| "zone map invalid".to_string());
        return Err(anyhow!("[agent-context] verify failed: {}", reason));
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

    // P6: record the warning so a later pack-only push can recognize it.
    if result.status == "warn" {
        let repo_root = git_repo_root(&cwd_path).unwrap_or_else(|_| cwd_path.clone());
        write_last_freshness_state(&repo_root, &result);
    }

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
            // P3: surface affected pack sections so agents know which files to patch.
            if !result.affected_sections.is_empty() {
                println!();
                println!("Affected pack sections:");
                for s in &result.affected_sections {
                    println!("  - {}", s);
                }
            }
            println!();
            println!("Consider running: chorus agent-context build");
        }
        "fail" => {
            let reason = result
                .skipped_reason
                .clone()
                .unwrap_or_else(|| "zone map invalid".to_string());
            return Err(anyhow!(
                "[agent-context] freshness failed: {}",
                reason
            ));
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

// ---- P1: semantic baseline helpers ----------------------------------------
//
// Each helper is self-contained and degrades to empty on absent/malformed
// input so `collect_semantic_baseline` can compose them without branching.

/// P1 — resolve globs from `completeness_contract.json` `required_file_families[]`
/// and `reporting_rules.json` `groupable_families[]`. Keyed by the raw glob;
/// value is the number of repo-relative files currently matching.
///
/// Returns an empty map if both config files are absent or do not define
/// `task_families`. Malformed JSON is treated as absent.
fn resolve_family_counts(
    repo_root: &Path,
    current_dir: &Path,
) -> std::collections::BTreeMap<String, usize> {
    let mut patterns: BTreeSet<String> = BTreeSet::new();

    let completeness_path = current_dir.join("completeness_contract.json");
    if let Ok(Some(completeness)) = read_json(&completeness_path) {
        if let Some(families) = completeness
            .get("task_families")
            .and_then(|v| v.as_object())
        {
            for (_task, entry) in families {
                if let Some(list) = entry
                    .get("required_file_families")
                    .and_then(|v| v.as_array())
                {
                    for item in list {
                        if let Some(p) = item.as_str() {
                            patterns.insert(p.to_string());
                        }
                    }
                }
            }
        }
    }

    let reporting_path = current_dir.join("reporting_rules.json");
    if let Ok(Some(reporting)) = read_json(&reporting_path) {
        if let Some(families) = reporting
            .get("task_families")
            .and_then(|v| v.as_object())
        {
            for (_task, entry) in families {
                if let Some(list) = entry.get("groupable_families").and_then(|v| v.as_array()) {
                    for item in list {
                        if let Some(p) = item.as_str() {
                            patterns.insert(p.to_string());
                        }
                    }
                }
            }
        }
    }

    let mut out = std::collections::BTreeMap::new();
    for pattern in patterns {
        let count = resolve_pattern_matches(repo_root, &pattern)
            .map(|v| v.len())
            .unwrap_or(0);
        out.insert(pattern, count);
    }
    out
}

/// P1 — noun+count pattern for prose claim extraction. Matches occurrences like
/// "6 files", "12 scripts", "study doc" (singular/plural). Kept narrow to avoid
/// false positives on unrelated numeric text.
///
/// The pattern is intentionally lightweight — a single pass returns
/// `(count_str, noun_str)` pairs per line which callers normalize.
fn prose_claim_nouns() -> &'static [&'static str] {
    &[
        "study doc",
        "study docs",
        "script",
        "scripts",
        "test",
        "tests",
        "file",
        "files",
        "API symbol",
        "API symbols",
        "brand",
        "brands",
    ]
}

/// Parse a single markdown file body into `{noun, count, file, line}` Value
/// entries using the prose-claim regex. Lines inside a
/// `<!-- count-claim: ignore -->` region (toggled by any occurrence of the
/// literal comment; region persists until a matching `<!-- count-claim: end -->`
/// or EOF) are skipped to give authors an explicit opt-out.
fn extract_declared_counts_from_text(text: &str, file_label: &str, out: &mut Vec<Value>) {
    let nouns = prose_claim_nouns();
    let mut ignore = false;
    for (idx, raw_line) in text.lines().enumerate() {
        // Region toggles — any explicit end closes the region; otherwise a
        // stray `count-claim: ignore` marker opens one.
        if raw_line.contains("<!-- count-claim: end -->")
            || raw_line.contains("<!-- count-claim: /ignore -->")
        {
            ignore = false;
            continue;
        }
        if raw_line.contains("<!-- count-claim: ignore -->") {
            ignore = true;
            // If the ignore marker is inline with content, still skip this
            // line so callers can annotate a specific sentence without
            // opening a multi-line region.
            continue;
        }
        if ignore {
            continue;
        }

        // Walk the line and find `<number> <noun>` occurrences. We match the
        // longest noun first (e.g. "study docs" before "study doc") to avoid
        // double counting plural forms.
        let mut cursor = 0;
        let bytes = raw_line.as_bytes();
        while cursor < bytes.len() {
            if !bytes[cursor].is_ascii_digit() {
                cursor += 1;
                continue;
            }
            // Capture the leading integer.
            let start = cursor;
            while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                cursor += 1;
            }
            let number_str = &raw_line[start..cursor];
            // Must be followed by exactly one whitespace then a recognized
            // noun phrase. Allow tab/space.
            let after_digits = cursor;
            if after_digits >= bytes.len()
                || !(bytes[after_digits] == b' ' || bytes[after_digits] == b'\t')
            {
                continue;
            }
            let rest = &raw_line[after_digits + 1..];
            let mut matched: Option<&'static str> = None;
            // Try longest noun first by iterating sorted-desc by length.
            let mut ordered: Vec<&&str> = nouns.iter().collect();
            ordered.sort_by_key(|s| std::cmp::Reverse(s.len()));
            for noun in ordered {
                if rest.len() < noun.len() {
                    continue;
                }
                if !rest.is_char_boundary(noun.len()) {
                    continue;
                }
                let candidate = &rest[..noun.len()];
                if candidate.eq_ignore_ascii_case(noun) {
                    // Require a word boundary after the noun (end of line,
                    // whitespace, or punctuation) so "scripts" doesn't match
                    // inside "scriptsdirectory".
                    let next = rest.as_bytes().get(noun.len()).copied();
                    let is_boundary = match next {
                        None => true,
                        Some(b) => !(b.is_ascii_alphanumeric() || b == b'_'),
                    };
                    if is_boundary {
                        matched = Some(noun);
                        break;
                    }
                }
            }
            if let Some(noun) = matched {
                if let Ok(count) = number_str.parse::<u64>() {
                    out.push(json!({
                        "noun": noun,
                        "count": count,
                        "file": file_label,
                        "line": (idx + 1) as u64,
                    }));
                }
                // Advance cursor past the matched noun to avoid re-scanning.
                cursor = after_digits + 1 + noun.len();
            }
        }
    }
}

/// P1 — scan every `.agent-context/current/*.md` file for numeric prose
/// claims. Binary / unreadable files are skipped silently (P8 boundary).
fn extract_declared_counts(current_dir: &Path) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let entries = match fs::read_dir(current_dir) {
        Ok(e) => e,
        Err(_) => return out,
    };
    let mut md_paths: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        md_paths.push(path);
    }
    // Deterministic order so identical repos produce identical manifests.
    md_paths.sort();
    for path in md_paths {
        let file_label = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => continue,
        };
        extract_declared_counts_from_text(&text, &file_label, &mut out);
    }
    out
}

// ---- P5: count SSOT via seal-time template expansion ----------------------
//
// Prose becomes a template; manifest's `family_counts` becomes authoritative.
// Seal expands handlebars like `{{counts.scripts_run}}` before hashing, then
// scans remaining prose for stale numeric claims. Verify re-computes counts
// and re-scans to catch drift between seal and verify.

/// P5 — single occurrence of a stale prose numeric claim produced by
/// `extract_numeric_claims`. The caller formats these into user-facing errors
/// (on seal) or warnings (on verify / `--force`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct NumericClaimMismatch {
    file: String,
    line: u64,
    claimed_count: u64,
    authoritative_count: u64,
    noun: String,
}

/// P5 — normalize a glob pattern into a handlebar slug. Rules:
/// - strip glob wildcards (`*`, `**`, `?`)
/// - replace non-word chars (including `/`, `.`, `-`) with `_`
/// - collapse repeated `_` and trim leading/trailing `_`
///
/// Examples:
/// - `scripts/run_*.py`  → `scripts_run`
/// - `src/brand_lift/*.py` → `src_brand_lift`
/// - `tests/**/*.py` → `tests`
fn slug_for_count_key(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len());
    for ch in pattern.chars() {
        if ch == '*' || ch == '?' {
            // Strip glob wildcards entirely — they are not part of the
            // stable slug identifier.
            continue;
        }
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            // Any other char (`/`, `.`, `-`, whitespace, ...) becomes `_`.
            out.push('_');
        }
    }
    // Collapse runs of `_` and trim edges.
    let mut collapsed = String::with_capacity(out.len());
    let mut prev_underscore = true;
    for ch in out.chars() {
        if ch == '_' {
            if !prev_underscore {
                collapsed.push('_');
            }
            prev_underscore = true;
        } else {
            collapsed.push(ch);
            prev_underscore = false;
        }
    }
    let trimmed = collapsed.trim_matches('_').to_string();
    trimmed
}

/// P5 — expand `{{counts.<slug>}}` handlebars in `content` using the supplied
/// authoritative map. Unknown slugs are left as literal text so an authoring
/// mistake is visible to the numeric-claim scan (the literal handlebar still
/// won't match the prose regex, so this is purely informational).
///
/// Whitespace inside the braces is tolerated: `{{ counts.scripts_run }}` is
/// treated the same as `{{counts.scripts_run}}`.
fn expand_count_handlebars(
    content: &str,
    counts: &std::collections::HashMap<String, usize>,
) -> String {
    let bytes = content.as_bytes();
    let mut out = String::with_capacity(content.len());
    let mut i = 0;
    while i < bytes.len() {
        // Fast path: look for the `{{` opener. Anything else is copied
        // verbatim.
        if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            // Find the matching `}}`.
            if let Some(close) = content[i + 2..].find("}}") {
                let inner = &content[i + 2..i + 2 + close];
                let trimmed = inner.trim();
                if let Some(slug) = trimmed.strip_prefix("counts.") {
                    let slug = slug.trim();
                    if let Some(value) = counts.get(slug) {
                        out.push_str(&value.to_string());
                        i = i + 2 + close + 2;
                        continue;
                    }
                }
                // Not a recognized counts handlebar — leave the literal
                // text as-is so authors see the typo.
                out.push_str(&content[i..i + 2 + close + 2]);
                i = i + 2 + close + 2;
                continue;
            }
        }
        // Push the current char (UTF-8 safe via char_indices).
        let ch_start = i;
        let ch_end = (1..=4)
            .map(|n| ch_start + n)
            .find(|&end| content.is_char_boundary(end))
            .unwrap_or(ch_start + 1);
        out.push_str(&content[ch_start..ch_end]);
        i = ch_end;
    }
    out
}

/// P5 — given already-expanded markdown content and an authoritative
/// noun→count map, return every prose `<n> <noun>` claim whose value
/// disagrees with the authoritative count.
///
/// Contract:
/// - Skips lines inside `<!-- count-claim: ignore -->` .. `<!-- count-claim: end -->`
///   regions (identical semantics to P1's `extract_declared_counts`).
/// - If a noun has no entry in `authoritative`, the claim is left alone — we
///   only flag values that can be checked against a known-good answer.
/// - Claims whose value matches authoritative produce no output.
/// - The `file` label is supplied by the caller since this helper operates on
///   a single file body.
fn extract_numeric_claims(
    content: &str,
    authoritative: &std::collections::HashMap<String, usize>,
    file_label: &str,
) -> Vec<NumericClaimMismatch> {
    let nouns = prose_claim_nouns();
    let mut out: Vec<NumericClaimMismatch> = Vec::new();
    let mut ignore = false;
    for (idx, raw_line) in content.lines().enumerate() {
        if raw_line.contains("<!-- count-claim: end -->")
            || raw_line.contains("<!-- count-claim: /ignore -->")
        {
            ignore = false;
            continue;
        }
        if raw_line.contains("<!-- count-claim: ignore -->") {
            ignore = true;
            continue;
        }
        if ignore {
            continue;
        }

        // Same single-pass number+noun scan as extract_declared_counts_from_text.
        let mut cursor = 0;
        let bytes = raw_line.as_bytes();
        while cursor < bytes.len() {
            if !bytes[cursor].is_ascii_digit() {
                cursor += 1;
                continue;
            }
            let start = cursor;
            while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                cursor += 1;
            }
            let number_str = &raw_line[start..cursor];
            let after_digits = cursor;
            if after_digits >= bytes.len()
                || !(bytes[after_digits] == b' ' || bytes[after_digits] == b'\t')
            {
                continue;
            }
            let rest = &raw_line[after_digits + 1..];
            let mut matched: Option<&'static str> = None;
            let mut ordered: Vec<&&str> = nouns.iter().collect();
            ordered.sort_by_key(|s| std::cmp::Reverse(s.len()));
            for noun in ordered {
                if rest.len() < noun.len() {
                    continue;
                }
                if !rest.is_char_boundary(noun.len()) {
                    continue;
                }
                let candidate = &rest[..noun.len()];
                if candidate.eq_ignore_ascii_case(noun) {
                    let next = rest.as_bytes().get(noun.len()).copied();
                    let is_boundary = match next {
                        None => true,
                        Some(b) => !(b.is_ascii_alphanumeric() || b == b'_'),
                    };
                    if is_boundary {
                        matched = Some(noun);
                        break;
                    }
                }
            }
            if let Some(noun) = matched {
                if let Ok(claimed) = number_str.parse::<u64>() {
                    // Look up authoritative: exact noun first, then
                    // singular form (e.g. "scripts" → "script"), then the
                    // plural form if the claim uses the singular.
                    let singular = noun.trim_end_matches('s');
                    let plural = format!("{}s", noun);
                    let auth = authoritative
                        .get(noun)
                        .or_else(|| authoritative.get(singular))
                        .or_else(|| authoritative.get(plural.as_str()))
                        .copied();
                    if let Some(authoritative_count) = auth {
                        let authoritative_count = authoritative_count as u64;
                        if claimed != authoritative_count {
                            out.push(NumericClaimMismatch {
                                file: file_label.to_string(),
                                line: (idx + 1) as u64,
                                claimed_count: claimed,
                                authoritative_count,
                                noun: noun.to_string(),
                            });
                        }
                    }
                }
                cursor = after_digits + 1 + noun.len();
            }
        }
    }
    out
}

/// P5 — build the two derived maps (by slug for handlebar expansion; by noun
/// for prose-claim authoritative lookup) from P1's family_counts.
///
/// Slug map: every glob key becomes `slug_for_count_key(glob) → count`.
///
/// Noun map: for each registered prose noun, we accumulate the sum of all
/// family_counts whose slug contains the noun as a word token (split on `_`).
/// If no slug matches, the noun is simply absent — the prose scan then leaves
/// any `<n> <noun>` claim alone. This keeps the check conservative: we only
/// flag prose numbers we can cross-check with confidence.
fn derive_count_maps(
    family_counts: &std::collections::BTreeMap<String, usize>,
) -> (
    std::collections::HashMap<String, usize>,
    std::collections::HashMap<String, usize>,
) {
    let mut slug_map: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for (glob, count) in family_counts {
        let slug = slug_for_count_key(glob);
        if !slug.is_empty() {
            // Sum duplicates rather than silently overwrite — two globs that
            // resolve to the same slug (e.g. `a/run_*.py` + `a/run_*.rs`)
            // should aggregate, not mask each other.
            *slug_map.entry(slug).or_insert(0) += *count;
        }
    }

    let mut noun_map: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for noun in prose_claim_nouns() {
        // Split the noun on whitespace so "study docs" matches against the
        // slug tokens `study` or `docs`.
        let noun_lower = noun.to_ascii_lowercase();
        let noun_parts: Vec<&str> = noun_lower.split_whitespace().collect();
        let mut accum = 0usize;
        let mut matched_any = false;
        for (glob, count) in family_counts {
            let slug = slug_for_count_key(glob).to_ascii_lowercase();
            let tokens: Vec<&str> = slug.split('_').filter(|s| !s.is_empty()).collect();
            // A slug matches the noun when any of the noun parts appears as a
            // slug token. Plural/singular forms are both tried.
            let matches = noun_parts.iter().any(|np| {
                let np_single = np.trim_end_matches('s');
                tokens.iter().any(|t| *t == *np || *t == np_single)
            });
            if matches {
                accum += *count;
                matched_any = true;
            }
        }
        if matched_any {
            noun_map.insert((*noun).to_string(), accum);
        }
    }
    (slug_map, noun_map)
}

/// P5 — per-file summary returned to `seal()` so it can either fail the seal
/// or surface warnings. The caller writes the expanded content back to disk
/// before `collect_files_meta` hashes it.
struct CountExpansionReport {
    /// Relative filename (used for diagnostics only).
    file: String,
    /// Content after handlebar expansion. This is what seal writes back.
    expanded: String,
    /// Mismatched prose claims found in `expanded`.
    mismatches: Vec<NumericClaimMismatch>,
}

/// P5 — orchestration helper used by `seal`. Reads every required markdown
/// file in `current_dir`, expands handlebars, scans for stale prose claims,
/// and returns one report per file. Non-markdown required files (the JSON
/// structured layer) are skipped.
fn apply_count_templates(
    current_dir: &Path,
    required_files: &[String],
    slug_counts: &std::collections::HashMap<String, usize>,
    noun_counts: &std::collections::HashMap<String, usize>,
) -> Vec<CountExpansionReport> {
    let mut reports: Vec<CountExpansionReport> = Vec::new();
    for file in required_files {
        if !file.ends_with(".md") {
            continue;
        }
        let path = current_dir.join(file);
        let original = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let expanded = expand_count_handlebars(&original, slug_counts);
        let mismatches = extract_numeric_claims(&expanded, noun_counts, file);
        reports.push(CountExpansionReport {
            file: file.clone(),
            expanded,
            mismatches,
        });
    }
    reports
}

/// P1 — parse Python top-level function signatures via a regex that tolerates
/// decorators, async, and multi-line parameter lists. `ast`-level parsing would
/// be ideal but would introduce a non-stdlib dep; the regex is narrow enough to
/// only trigger on `def name(...)` at column 0.
fn parse_python_signatures(
    source: &str,
    file_label: &str,
    out: &mut std::collections::BTreeMap<String, String>,
) {
    let mut lines = source.lines().peekable();
    let mut accumulated = String::new();
    let mut collecting = false;
    let mut fn_name = String::new();
    while let Some(line) = lines.next() {
        if !collecting {
            // Detect `def NAME(` or `async def NAME(` at column 0 (no leading
            // whitespace) — top-level only per the P1 spec.
            let trimmed_prefix = line.trim_start_matches(|c: char| c == '\t' || c == ' ');
            let is_indented = trimmed_prefix.len() != line.len();
            if is_indented {
                continue;
            }
            let candidate = if let Some(rest) = line.strip_prefix("async def ") {
                Some(rest)
            } else {
                line.strip_prefix("def ")
            };
            let rest = match candidate {
                Some(r) => r,
                None => continue,
            };
            let paren = match rest.find('(') {
                Some(p) => p,
                None => continue,
            };
            let name_part = &rest[..paren];
            if !name_part
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_')
                || name_part.is_empty()
            {
                continue;
            }
            fn_name = name_part.to_string();
            accumulated.clear();
            accumulated.push_str(line.trim_end());
            // Short-circuit: signature fits on one line.
            if line.contains(':') && paren_balanced(line) {
                let sig = one_line_signature(&accumulated);
                out.insert(format!("{}::{}", file_label, fn_name), sig);
                fn_name.clear();
                continue;
            }
            collecting = true;
        } else {
            accumulated.push(' ');
            accumulated.push_str(line.trim());
            if accumulated.contains(':') && paren_balanced(&accumulated) {
                let sig = one_line_signature(&accumulated);
                out.insert(format!("{}::{}", file_label, fn_name), sig);
                fn_name.clear();
                accumulated.clear();
                collecting = false;
            }
        }
    }
}

/// Parens balanced on the accumulated prefix so we know the param list closed.
fn paren_balanced(s: &str) -> bool {
    let mut depth: i32 = 0;
    for ch in s.chars() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

/// Normalize whitespace in a captured signature so equal signatures compare
/// byte-identically across cross-platform line endings and indent variants.
fn one_line_signature(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut prev_space = false;
    for ch in raw.chars() {
        if ch.is_whitespace() {
            if !prev_space && !out.is_empty() {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    // Trim trailing colon-only variants and trailing whitespace.
    out.trim().to_string()
}

/// P1 — Rust top-level `fn NAME(params) -> RetType` or `fn NAME(params)`.
/// Visibility modifiers and generics are preserved in the captured signature.
fn parse_rust_signatures(
    source: &str,
    file_label: &str,
    out: &mut std::collections::BTreeMap<String, String>,
) {
    let mut lines = source.lines().peekable();
    let mut acc = String::new();
    let mut collecting = false;
    let mut fn_name = String::new();
    while let Some(line) = lines.next() {
        if !collecting {
            // Top-level fn: leading whitespace disallowed to skip method impls.
            let is_indented = line.chars().next().map(|c| c.is_whitespace()).unwrap_or(false);
            if is_indented {
                continue;
            }
            // Trim visibility and `async`/`unsafe`/`extern` qualifiers.
            let mut scan = line.trim_start();
            for prefix in ["pub(crate) ", "pub ", "async ", "unsafe ", "extern ", "const "] {
                if let Some(rest) = scan.strip_prefix(prefix) {
                    scan = rest;
                }
            }
            let rest = match scan.strip_prefix("fn ") {
                Some(r) => r,
                None => continue,
            };
            // Extract the function name up to '(' or '<'.
            let stop = rest
                .find(|c: char| c == '(' || c == '<' || c.is_whitespace())
                .unwrap_or(rest.len());
            let name = &rest[..stop];
            if name.is_empty()
                || !name.chars().all(|c| c.is_alphanumeric() || c == '_')
            {
                continue;
            }
            fn_name = name.to_string();
            acc.clear();
            acc.push_str(line.trim_end());
            // Signature ends at `{` or `;` with balanced parens.
            if (acc.contains('{') || acc.contains(';')) && paren_balanced(&acc) {
                let sig = rust_signature_head(&acc);
                out.insert(format!("{}::{}", file_label, fn_name), sig);
                fn_name.clear();
                continue;
            }
            collecting = true;
        } else {
            acc.push(' ');
            acc.push_str(line.trim());
            if (acc.contains('{') || acc.contains(';')) && paren_balanced(&acc) {
                let sig = rust_signature_head(&acc);
                out.insert(format!("{}::{}", file_label, fn_name), sig);
                fn_name.clear();
                acc.clear();
                collecting = false;
            }
        }
    }
}

/// Capture the signature up to (but not including) the body-opening `{` or the
/// trailing `;`. Whitespace is normalized for stable comparison.
fn rust_signature_head(raw: &str) -> String {
    // Prefer the first `{` only if it is after a balanced-paren prefix;
    // otherwise fall back to the first `;`. In practice the simpler "split at
    // first of {/;" is correct for top-level declarations.
    let cut = raw
        .find('{')
        .or_else(|| raw.find(';'))
        .unwrap_or(raw.len());
    one_line_signature(&raw[..cut])
}

/// P1 — TypeScript / JavaScript top-level `function NAME(...)` or
/// `const NAME = (...)` / `export const NAME = (...)` arrow forms. Heuristics
/// are intentionally narrow: class methods, default exports, and IIFEs are
/// skipped so we do not emit false positives.
fn parse_ts_signatures(
    source: &str,
    file_label: &str,
    out: &mut std::collections::BTreeMap<String, String>,
) {
    let lines: Vec<&str> = source.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        // Only top-level (no leading indent).
        let is_indented = line
            .chars()
            .next()
            .map(|c| c.is_whitespace())
            .unwrap_or(false);
        if is_indented {
            i += 1;
            continue;
        }
        let trimmed = line.trim_start();
        // `function NAME(` — accumulate until paren balance.
        let fn_prefix_candidates = [
            "export async function ",
            "export function ",
            "async function ",
            "function ",
        ];
        let mut captured_name: Option<String> = None;
        for prefix in fn_prefix_candidates {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                if let Some(paren) = rest.find('(') {
                    let name = &rest[..paren];
                    if !name.is_empty()
                        && name.chars().all(|c| c.is_alphanumeric() || c == '_')
                    {
                        captured_name = Some(name.to_string());
                    }
                }
                break;
            }
        }
        // `const NAME = (...) =>` or `export const NAME = (...) =>`.
        if captured_name.is_none() {
            for prefix in ["export const ", "const ", "export let ", "let "] {
                if let Some(rest) = trimmed.strip_prefix(prefix) {
                    if let Some(eq) = rest.find('=') {
                        let name = rest[..eq].trim();
                        let after_eq = rest[eq + 1..].trim_start();
                        if !name.is_empty()
                            && name.chars().all(|c| c.is_alphanumeric() || c == '_')
                            && (after_eq.starts_with('(') || after_eq.starts_with("async ("))
                        {
                            captured_name = Some(name.to_string());
                        }
                    }
                    break;
                }
            }
        }
        let name = match captured_name {
            Some(n) => n,
            None => {
                i += 1;
                continue;
            }
        };

        // Accumulate until paren-balanced + contains `=>` or `{`.
        let mut acc = String::new();
        acc.push_str(line.trim_end());
        let mut terminated = false;
        let mut j = i + 1;
        if (acc.contains("=>") || acc.contains('{')) && paren_balanced(&acc) {
            terminated = true;
        }
        while !terminated && j < lines.len() {
            acc.push(' ');
            acc.push_str(lines[j].trim());
            if (acc.contains("=>") || acc.contains('{')) && paren_balanced(&acc) {
                terminated = true;
                break;
            }
            j += 1;
        }
        // Capture everything up to the first `{` (function body) or `=>`.
        let cut = acc
            .find('{')
            .or_else(|| acc.find("=>"))
            .unwrap_or(acc.len());
        let sig = one_line_signature(&acc[..cut]);
        out.insert(format!("{}::{}", file_label, name), sig);
        i = if terminated { j + 1 } else { j };
    }
}

/// P1 — `parse_shortcut_signatures` dispatches by file extension. Languages
/// beyond Python / Rust / TypeScript / JavaScript are stubbed out with an
/// empty map (the manifest simply records no signatures for those files).
fn parse_shortcut_signatures_for_file(
    path: &Path,
    file_label: &str,
) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return out,
    };
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "py" => parse_python_signatures(&source, file_label, &mut out),
        "rs" => parse_rust_signatures(&source, file_label, &mut out),
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" => {
            parse_ts_signatures(&source, file_label, &mut out)
        }
        _ => {
            // Stub — no parser for this language. Future P1 extensions
            // (Go, Java, etc.) would add their branch here.
        }
    }
    out
}

/// P1 — iterate every file referenced in `search_scope.json`
/// `verification_shortcuts[]` and collect top-level function signatures.
///
/// Per the plan: keyed by `"<file>::<fn_name>"` -> signature string.
/// Returns an empty map if `search_scope.json` is absent or malformed so
/// `build_manifest` never has to branch on presence.
fn parse_shortcut_signatures(
    repo_root: &Path,
    current_dir: &Path,
) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    let search_scope_path = current_dir.join("search_scope.json");
    let scope = match read_json(&search_scope_path) {
        Ok(Some(v)) => v,
        _ => return out,
    };
    let families = match scope.get("task_families").and_then(|v| v.as_object()) {
        Some(f) => f,
        None => return out,
    };

    let mut seen_files: BTreeSet<String> = BTreeSet::new();
    for (_task, entry) in families {
        let shortcuts = match entry
            .get("verification_shortcuts")
            .and_then(|v| v.as_object())
        {
            Some(s) => s,
            None => continue,
        };
        for (key, _value) in shortcuts {
            // Keys can take the form "path" or "path:line" — strip the suffix.
            let file_key = key.split(':').next().unwrap_or(key).to_string();
            if file_key.is_empty() {
                continue;
            }
            if !seen_files.insert(file_key.clone()) {
                continue;
            }
            let abs = repo_root.join(&file_key);
            if !abs.exists() {
                continue;
            }
            let sigs = parse_shortcut_signatures_for_file(&abs, &file_key);
            for (k, v) in sigs {
                out.insert(k, v);
            }
        }
    }
    out
}

/// P1 — SHA256 of dependency-declaring file contents. Records one entry per
/// file that exists; absent files are omitted.
fn compute_dependencies_snapshot(
    repo_root: &Path,
) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    let candidates: &[(&str, &str)] = &[
        ("pyproject", "pyproject.toml"),
        ("cargo", "Cargo.toml"),
        ("npm", "package.json"),
    ];
    for (key, filename) in candidates {
        let path = repo_root.join(filename);
        if !path.exists() {
            continue;
        }
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        out.insert((*key).to_string(), sha256_hex(&bytes));
    }
    out
}

/// P1 — compose the full semantic baseline. Safe to call when pack-config
/// files are absent: each helper degrades to empty.
fn collect_semantic_baseline(repo_root: &Path, current_dir: &Path) -> SemanticBaseline {
    SemanticBaseline {
        family_counts: resolve_family_counts(repo_root, current_dir),
        declared_counts: extract_declared_counts(current_dir),
        shortcut_signatures: parse_shortcut_signatures(repo_root, current_dir),
        dependencies_snapshot: compute_dependencies_snapshot(repo_root),
    }
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
    baseline: &SemanticBaseline,
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

    // P11 / F36: forensic tooling-version fields.
    // `chorus_version` pins the sealing tool; `verifier_sha256` is the hash
    // of the binary that sealed it (when available). `skill_version` is
    // reserved for the team_skills track to populate — we leave it null here
    // so a later chorus release or team_skills scaffolder can fill it in.
    let chorus_version = env!("CARGO_PKG_VERSION");
    let verifier_sha256 = match current_exe_sha256() {
        Some(hash) => Value::String(hash),
        None => {
            eprintln!(
                "[agent-context] WARN: could not hash current chorus binary; \
                 manifest.verifier_sha256 will be null"
            );
            Value::Null
        }
    };

    // P1 — semantic baseline serialization. Empty maps/arrays are emitted
    // rather than omitted so consumers can rely on the keys being present.
    let family_counts_value = {
        let mut map = serde_json::Map::new();
        for (k, v) in &baseline.family_counts {
            map.insert(k.clone(), json!(*v));
        }
        Value::Object(map)
    };
    let declared_counts_value = Value::Array(baseline.declared_counts.clone());
    let shortcut_signatures_value = {
        let mut map = serde_json::Map::new();
        for (k, v) in &baseline.shortcut_signatures {
            map.insert(k.clone(), Value::String(v.clone()));
        }
        Value::Object(map)
    };
    let dependencies_snapshot_value = {
        let mut map = serde_json::Map::new();
        for (k, v) in &baseline.dependencies_snapshot {
            map.insert(k.clone(), Value::String(v.clone()));
        }
        Value::Object(map)
    };

    // P1 — `head_sha_at_seal` is the new canonical name for the seal-time HEAD.
    // We keep `head_sha` populated for one release so readers pinned to the
    // older field continue to work. `post_commit_sha` is null at seal; the
    // new `post-commit-reconcile` subcommand populates it after a commit.
    let head_sha_at_seal_value: Value = match head_sha {
        Some(s) => Value::String(s.to_string()),
        None => Value::Null,
    };

    let value = json!({
        "schema_version": CURRENT_SCHEMA_VERSION,
        "chorus_version": chorus_version,
        "skill_version": Value::Null,
        "verifier_sha256": verifier_sha256,
        "generated_at": generated_at,
        "repo_name": repo_name,
        "repo_root": ".",
        "branch": branch_value,
        "detached": detached,
        "head_sha": head_sha,
        "head_sha_at_seal": head_sha_at_seal_value,
        "post_commit_sha": Value::Null,
        "build_reason": reason,
        "base_sha": base_sha,
        "changed_files": changed_files,
        "files_count": files_meta.len(),
        "words_total": words_total,
        "bytes_total": bytes_total,
        "pack_checksum": pack_checksum,
        "stable_checksum": stable_checksum,
        "files": files,
        // P1 — semantic baseline fields.
        "family_counts": family_counts_value,
        "declared_counts": declared_counts_value,
        "shortcut_signatures": shortcut_signatures_value,
        "dependencies_snapshot": dependencies_snapshot_value,
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

/// P12 / F42 — resolve the git committer identity for `history.jsonl`'s
/// `sealed_by` field. Uses `git config user.name` + `user.email` at the repo
/// root so the audit trail records the person running `chorus agent-context
/// seal`, not whatever shell user happens to own the process. Returns
/// `"name <email>"` or an empty string when git isn't configured.
fn git_committer_identity(repo_root: &Path) -> String {
    let name = run_git(&["config", "user.name"], repo_root, true)
        .unwrap_or_default()
        .trim()
        .to_string();
    let email = run_git(&["config", "user.email"], repo_root, true)
        .unwrap_or_default()
        .trim()
        .to_string();
    match (name.is_empty(), email.is_empty()) {
        (true, true) => String::new(),
        (false, true) => name,
        (true, false) => format!("<{email}>"),
        (false, false) => format!("{name} <{email}>"),
    }
}

/// P12 / F42 — compute the H2 section headings whose body changed vs the
/// most recent snapshot's markdown files. Returns heading names prefixed by
/// the file they live in (e.g. `"20_CODE_MAP.md#Contexts"`). Empty list on
/// first seal (no previous snapshot) or when snapshots are unreadable.
fn compute_prose_diff_sections(snapshots_dir: &Path, current_dir: &Path) -> Vec<String> {
    let latest = match most_recent_snapshot_dir(snapshots_dir) {
        Some(p) => p,
        None => return Vec::new(),
    };
    let mut changed: Vec<String> = Vec::new();
    for file_name in REQUIRED_FILES {
        let prev_path = latest.join(file_name);
        let cur_path = current_dir.join(file_name);
        let prev = fs::read_to_string(&prev_path).unwrap_or_default();
        let cur = fs::read_to_string(&cur_path).unwrap_or_default();
        if prev == cur {
            continue;
        }
        let prev_sections = split_markdown_h2_sections(&prev);
        let cur_sections = split_markdown_h2_sections(&cur);
        // Union of headings, stable order via iteration over cur first.
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for (heading, body) in &cur_sections {
            if prev_sections.get(heading) != Some(body) {
                let key = format!("{}#{}", file_name, heading);
                if seen.insert(key.clone()) {
                    changed.push(key);
                }
            }
        }
        for (heading, _body) in &prev_sections {
            if !cur_sections.contains_key(heading) {
                let key = format!("{}#{}", file_name, heading);
                if seen.insert(key.clone()) {
                    changed.push(key);
                }
            }
        }
    }
    changed
}

/// Walk `snapshots_dir` and return the most recent snapshot path by name
/// (snapshot IDs are `<timestamp>_<sha>[-N]` so lexical sort == temporal
/// sort). Returns `None` when the directory is absent or empty.
fn most_recent_snapshot_dir(snapshots_dir: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(snapshots_dir).ok()?;
    let mut ids: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.path())
        .collect();
    if ids.is_empty() {
        return None;
    }
    ids.sort();
    ids.pop()
}

/// Split markdown into `(heading, body)` pairs keyed by H2 heading text.
/// H1 and H3+ headings are ignored. Used by
/// [`compute_prose_diff_sections`] for the history audit trail.
fn split_markdown_h2_sections(text: &str) -> std::collections::BTreeMap<String, String> {
    let mut out: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    let mut current_heading: Option<String> = None;
    let mut body = String::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            if let Some(h) = current_heading.take() {
                out.insert(h, body.clone());
            }
            body.clear();
            current_heading = Some(rest.trim().to_string());
            continue;
        }
        if current_heading.is_some() {
            body.push_str(line);
            body.push('\n');
        }
    }
    if let Some(h) = current_heading.take() {
        out.insert(h, body);
    }
    out
}

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

/// P3: a single zone in the relevance.json zone map.
#[derive(Debug, Clone)]
pub(crate) struct Zone {
    /// Glob patterns (repo-relative) that trigger this zone.
    pub paths: Vec<String>,
    /// Pack section filenames (e.g. `20_CODE_MAP.md`) affected when a file
    /// matching `paths` changes.
    pub affects: Vec<String>,
}

/// P3: load the zone map from `<repo_root>/.agent-context/relevance.json`.
///
/// Returns `None` when:
/// - the file is missing
/// - the JSON is invalid
/// - no `zones` key is present (legacy include/exclude shape — handled
///   separately by `load_relevance_rules` / `is_context_relevant_with_rules`)
///
/// Malformed zone entries (non-object, missing paths/affects, non-string
/// values) are silently skipped — they never abort loading, but they may make
/// the zone empty.
pub(crate) fn load_zone_map(repo_root: &Path) -> Option<Vec<Zone>> {
    let rules_path = repo_root.join(".agent-context").join("relevance.json");
    let raw = fs::read_to_string(&rules_path).ok()?;
    let parsed: Value = serde_json::from_str(&raw).ok()?;
    let zones_val = parsed.get("zones")?.as_array()?;
    let mut zones = Vec::new();
    for z in zones_val {
        let Some(obj) = z.as_object() else { continue };
        let paths: Vec<String> = obj
            .get("paths")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let affects: Vec<String> = obj
            .get("affects")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        if paths.is_empty() && affects.is_empty() {
            continue;
        }
        zones.push(Zone { paths, affects });
    }
    Some(zones)
}

/// P3: resolve a single changed file to the set of pack sections affected,
/// according to the zone map. Returns deduplicated section names in the
/// insertion order of the first matching zone. Empty if no zone matches.
///
/// Matching uses the same globset semantics as Pass-0's
/// `is_context_relevant_with_rules` for consistency with the existing behavior.
pub(crate) fn resolve_affected_sections(file_path: &str, zones: &[Zone]) -> Vec<String> {
    let normalized = file_path.replace('\\', "/");
    let mut out = BTreeSet::new();
    for zone in zones {
        let patterns: Vec<&str> = zone.paths.iter().map(|s| s.as_str()).collect();
        let Some(glob_set) = build_glob_set(&patterns) else {
            continue;
        };
        if glob_set.is_match(&normalized) {
            for section in &zone.affects {
                out.insert(section.clone());
            }
        }
    }
    out.into_iter().collect()
}

/// P3: validate that every zone's `paths[]` resolves to at least one tracked
/// file. Returns `Ok(Some(msg))` with the first offending zone path when the
/// zone map has a pattern resolving to zero tracked files, per the plan's
/// "Verification fails if any zone's `paths` resolve to zero tracked files"
/// rule.
///
/// Implementation uses `git ls-files` once and then applies each zone's
/// globset in-memory. On non-git or empty ls-files output, we conservatively
/// skip validation (the outer freshness check already handles non-git).
fn validate_zone_map(zones: &[Zone], repo_root: &Path) -> Result<Option<String>> {
    if zones.is_empty() {
        return Ok(None);
    }
    let tracked = run_git(&["ls-files"], repo_root, true)?;
    if tracked.is_empty() {
        // Nothing to validate against — treat as a soft pass.
        return Ok(None);
    }
    let tracked_files: Vec<String> = tracked
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    for zone in zones {
        for pattern in &zone.paths {
            let Some(glob_set) = build_glob_set(&[pattern.as_str()]) else {
                // Invalid glob — treat as a user authoring bug and surface it.
                return Ok(Some(format!(
                    "zone path '{pattern}' is not a valid glob pattern"
                )));
            };
            let any_match = tracked_files.iter().any(|f| glob_set.is_match(f));
            if !any_match {
                return Ok(Some(format!(
                    "zone path '{pattern}' resolves to zero tracked files"
                )));
            }
        }
    }
    Ok(None)
}

/// P3: compute the `suggest-patches` JSON payload for agent consumption.
///
/// Shape:
/// ```json
/// {
///   "changed_files": ["..."],
///   "pack_sections_to_update": ["20_CODE_MAP.md", ...],
///   "diff_excerpt": "<git diff excerpt, capped at 2KB>",
///   "baseline_drift": []
/// }
/// ```
///
/// `baseline_drift` is reserved for P2 (baseline manifest + drift detection)
/// and is always emitted as `[]` here so downstream schema consumers can
/// depend on the field existing.
pub(crate) fn suggest_patches(base: &str, cwd: &Path) -> Result<Value> {
    // Run the zone-aware freshness check. Treat failures as "no suggestion"
    // rather than propagating errors — callers still get a structured payload.
    let freshness = check_freshness_with_zones(base, cwd).unwrap_or(FreshnessResult {
        status: "skip".to_string(),
        changed_files: Vec::new(),
        pack_updated: false,
        skipped_reason: None,
        affected_sections: Vec::new(),
    });

    // Collect a bounded diff excerpt (2KB cap). We intentionally use `git diff`
    // content (not just names) so agents can reason about what changed; the
    // cap prevents runaway payloads on massive diffs.
    let diff_excerpt = collect_diff_excerpt(base, cwd).unwrap_or_default();

    Ok(json!({
        "changed_files": freshness.changed_files,
        "pack_sections_to_update": freshness.affected_sections,
        "diff_excerpt": diff_excerpt,
        // Reserved for P2 (baseline + drift check). Always an array for schema
        // stability even when empty.
        "baseline_drift": Vec::<Value>::new(),
    }))
}

/// P4 — evaluate `acceptance_tests.md` in the sealed pack and return one
/// `Value` per test with its `id`, `status`, and (when present) the full
/// `invalidated_by` list plus the subset that matched P2's drift signal.
///
/// Parsing contract: the file is expected to declare tests as YAML-style
/// front-matter blocks or simple markdown bullets with structured hints.
/// To avoid a YAML dependency and to keep this readable when P2 hasn't yet
/// integrated, we parse a lightweight schema:
///
///   ### test: <id>
///   - invalidated_by: fn_a, fn_b
///
/// Any block without a matching `### test:` header is skipped. Missing
/// file, unreadable file, or zero tests → returns an empty Vec.
///
/// NOTE (P2 integration): until P2 lands the `SIGNATURE_DRIFT` detector,
/// `drifted_functions` will always be empty and every test stays `ready`.
/// That's intentional — P4 ships the invalidated_by data pipeline now so
/// downstream agents can rely on the field existing; actual drift-triggered
/// status transitions come online the moment P2 is wired.
fn evaluate_acceptance_tests(
    current_dir: &Path,
    drifted_functions: &std::collections::BTreeSet<String>,
) -> Vec<Value> {
    let path = current_dir.join("acceptance_tests.md");
    let Ok(raw) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let mut out: Vec<Value> = Vec::new();
    let mut current_id: Option<String> = None;
    let mut current_invalidated: Vec<String> = Vec::new();

    let flush = |id: Option<String>,
                 invalidated: Vec<String>,
                 drifted: &std::collections::BTreeSet<String>,
                 out: &mut Vec<Value>| {
        if let Some(id) = id {
            let matched: Vec<String> = invalidated
                .iter()
                .filter(|fn_name| drifted.contains(*fn_name))
                .cloned()
                .collect();
            let status = if matched.is_empty() {
                "ready"
            } else {
                "needs_revalidation"
            };
            out.push(json!({
                "id": id,
                "status": status,
                "invalidated_by": invalidated,
                "invalidated_by_matched": matched,
            }));
        }
    };

    for line in raw.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed
            .strip_prefix("### test:")
            .or_else(|| trimmed.strip_prefix("### Test:"))
            .or_else(|| trimmed.strip_prefix("###test:"))
        {
            flush(
                current_id.take(),
                std::mem::take(&mut current_invalidated),
                drifted_functions,
                &mut out,
            );
            current_id = Some(rest.trim().to_string());
            continue;
        }
        if current_id.is_some() {
            // Accept "- invalidated_by: a, b" or "invalidated_by: [a, b]".
            let lower = trimmed.trim_start_matches(['-', ' ', '*']).to_string();
            if let Some(rest) = lower
                .strip_prefix("invalidated_by:")
                .or_else(|| lower.strip_prefix("invalidated_by :"))
            {
                // Strip optional `[...]` wrapper.
                let cleaned = rest.trim().trim_start_matches('[').trim_end_matches(']');
                for part in cleaned.split(',') {
                    let name = part.trim().trim_matches(|c: char| c == '"' || c == '\'');
                    if !name.is_empty() {
                        current_invalidated.push(name.to_string());
                    }
                }
            }
        }
    }
    flush(
        current_id.take(),
        current_invalidated,
        drifted_functions,
        &mut out,
    );
    out
}

/// P4 — collect the set of function names flagged as signature-drifted by
/// P2. Today we read them from an optional `signature_drift.json` under
/// `.agent-context/current/` because P2 may land after P4 does. When P2 is
/// integrated and stamps drift into the manifest or an alternate file, this
/// helper is the single place to update.
///
/// Graceful-degradation contract:
/// - No file present → empty set (every acceptance test stays `ready`).
/// - Unreadable or unparseable file → empty set.
/// - File present with a `drifted_functions` array → each entry joins the set.
fn collect_signature_drift(current_dir: &Path) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    let path = current_dir.join("signature_drift.json");
    let Ok(raw) = fs::read_to_string(&path) else {
        return out;
    };
    let Ok(value): Result<Value, _> = serde_json::from_str(&raw) else {
        return out;
    };
    if let Some(arr) = value.get("drifted_functions").and_then(|v| v.as_array()) {
        for entry in arr {
            if let Some(name) = entry.as_str() {
                out.insert(name.to_string());
            }
        }
    }
    out
}

/// Capture at most `MAX_DIFF_EXCERPT_BYTES` bytes from `git diff`. Truncates
/// on a UTF-8 boundary; if the diff fails (non-git / invalid base) returns an
/// empty string.
const MAX_DIFF_EXCERPT_BYTES: usize = 2 * 1024;
fn collect_diff_excerpt(base: &str, cwd: &Path) -> Result<String> {
    let raw = run_git(&["diff", &format!("{base}...HEAD")], cwd, true)?;
    let truncated = if raw.len() > MAX_DIFF_EXCERPT_BYTES {
        // Find the last char boundary below the cap so we don't split a
        // multi-byte sequence mid-sequence.
        let mut end = MAX_DIFF_EXCERPT_BYTES;
        while !raw.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        format!("{}\n… [truncated at {} bytes]", &raw[..end], MAX_DIFF_EXCERPT_BYTES)
    } else {
        raw
    };
    Ok(truncated)
}

/// P7 — result of the zone-grouped diff-since-seal computation. Returned by
/// [`diff_since_seal`] for CLI rendering and consumed by `verify --ci` so a
/// non-empty `acceptance_tests_invalidated` can fail CI.
pub(crate) struct DiffSinceSealResult {
    /// Full JSON payload written to stdout by the subcommand.
    pub value: Value,
    /// Mirrors `value["acceptance_tests_invalidated"]` for fast CI inspection.
    pub acceptance_tests_invalidated: Vec<Value>,
}

/// P7 — resolve the seal-time baseline commit. Prefers `post_commit_sha` when
/// the P1 post-commit reconcile step populated it; otherwise falls back to
/// `head_sha_at_seal`. Returns `None` when neither is present as a non-empty
/// string (e.g. an old manifest written before P1, or a pre-seal state).
pub(crate) fn resolve_seal_baseline_sha(manifest: &Value) -> Option<String> {
    let pick = |key: &str| -> Option<String> {
        manifest
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    pick("post_commit_sha").or_else(|| pick("head_sha_at_seal"))
}

/// P7 — parse the acceptance-tests markdown (authored by P4) into a list of
/// `{test_id?, invalidated_by: [function_names]}` entries. When the file is
/// missing, malformed, or carries no `invalidated_by` markers, returns an
/// empty vector so `diff_since_seal` degrades gracefully.
///
/// Expected shape (per P4 plan):
///
/// ```markdown
/// ## Q1: …
/// - invalidated_by: compute_lift_with_ci, resolve_sample
/// ```
///
/// We accept both inline (`invalidated_by: fn_a, fn_b`) and line-prefixed
/// (`- invalidated_by:`) forms. Function names are comma/whitespace-split and
/// trimmed. The `test_id` is taken from the nearest preceding `##` heading
/// when present; otherwise omitted.
pub(crate) fn parse_acceptance_invalidated_by(current_dir: &Path) -> Vec<Value> {
    let path = current_dir.join("acceptance_tests.md");
    let Ok(raw) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut current_heading: Option<String> = None;
    for line in raw.lines() {
        let trimmed = line.trim();
        if let Some(stripped) = trimmed.strip_prefix("## ") {
            current_heading = Some(stripped.trim().to_string());
            continue;
        }
        // Accept `- invalidated_by:`, `invalidated_by:`, and `* invalidated_by:`.
        let body = trimmed
            .trim_start_matches('-')
            .trim_start_matches('*')
            .trim();
        if let Some(rest) = body.strip_prefix("invalidated_by:") {
            let fns: Vec<String> = rest
                .split(|c: char| c == ',' || c.is_whitespace())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if fns.is_empty() {
                continue;
            }
            let mut entry = serde_json::Map::new();
            if let Some(ref h) = current_heading {
                entry.insert("test_id".to_string(), Value::String(h.clone()));
            }
            entry.insert(
                "invalidated_by".to_string(),
                Value::Array(fns.into_iter().map(Value::String).collect()),
            );
            out.push(Value::Object(entry));
        }
    }
    out
}

/// P7 — given a list of signature-drift entries and acceptance-test
/// `invalidated_by` bindings, return the tests that should be marked as
/// needing revalidation. Each drift entry may shape as
/// `{"file": "...", "fn": "name"}` (P2 canonical shape) or simply the
/// function name as a string; both are accepted so the scorer works before
/// P2 lands.
pub(crate) fn match_invalidated_tests(
    drifts: &[Value],
    invalidated_by: &[Value],
) -> Vec<Value> {
    if drifts.is_empty() || invalidated_by.is_empty() {
        return Vec::new();
    }
    let drift_fns: BTreeSet<String> = drifts
        .iter()
        .filter_map(|d| {
            if let Some(s) = d.as_str() {
                return Some(s.to_string());
            }
            d.as_object()
                .and_then(|o| {
                    o.get("fn")
                        .or_else(|| o.get("function"))
                        .or_else(|| o.get("name"))
                })
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .collect();
    let mut matched = Vec::new();
    for entry in invalidated_by {
        let Some(obj) = entry.as_object() else { continue };
        let Some(fns) = obj.get("invalidated_by").and_then(|v| v.as_array()) else {
            continue;
        };
        let hit_fns: Vec<String> = fns
            .iter()
            .filter_map(|v| v.as_str())
            .filter(|name| drift_fns.contains(*name))
            .map(|s| s.to_string())
            .collect();
        if hit_fns.is_empty() {
            continue;
        }
        let mut out_obj = serde_json::Map::new();
        if let Some(id) = obj.get("test_id") {
            out_obj.insert("test_id".to_string(), id.clone());
        }
        out_obj.insert(
            "matched_drifts".to_string(),
            Value::Array(hit_fns.into_iter().map(Value::String).collect()),
        );
        matched.push(Value::Object(out_obj));
    }
    matched
}

/// P7 — compose the reconciler's natural-language bullet list from the diff
/// payload. We keep this as plain strings so downstream agents can paste them
/// into a prompt without further formatting.
fn recommended_actions(
    zones: &[Value],
    acceptance_invalidated: &[Value],
    pack_updated: bool,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for zone in zones {
        let affects: Vec<String> = zone
            .get("affects")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let drifts_n = zone
            .get("signature_drifts")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let count_deltas_n = zone
            .get("count_deltas")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let deletions_n = zone
            .get("deleted_files")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let changed_n = zone
            .get("changed_files")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        if affects.is_empty() || changed_n == 0 {
            continue;
        }
        for section in &affects {
            if drifts_n > 0 {
                out.push(format!(
                    "Update {section}: {drifts_n} signature drift(s)"
                ));
            }
            if count_deltas_n > 0 {
                out.push(format!(
                    "Run expand_counts for {section}: {count_deltas_n} family count delta(s)"
                ));
            }
            if deletions_n > 0 {
                out.push(format!(
                    "Update {section}: {deletions_n} file(s) deleted"
                ));
            }
            if drifts_n == 0 && count_deltas_n == 0 && deletions_n == 0 {
                out.push(format!(
                    "Review {section}: {changed_n} file(s) changed in zone"
                ));
            }
        }
    }
    if !acceptance_invalidated.is_empty() {
        out.push(format!(
            "Revalidate {} acceptance test(s) whose `invalidated_by` function drifted",
            acceptance_invalidated.len()
        ));
    }
    if !pack_updated {
        out.push(
            "Re-seal the pack (`chorus agent-context seal --force`) after patching sections"
                .to_string(),
        );
    }
    out
}

/// P7 — subagent reconciliation diff. Produces a zone-grouped JSON payload
/// comparing the current HEAD against the seal-time baseline recorded on the
/// manifest. Used by the orchestrator after parallel subagents modify code so
/// a single reconciler subagent can patch the right pack sections and re-seal.
///
/// Shape:
///
/// ```json
/// {
///   "baseline_sha": "...",
///   "pack_updated": false,
///   "zones": [{
///     "paths": [...],
///     "affects": [...],
///     "changed_files": [...],
///     "signature_drifts": [],
///     "count_deltas": [],
///     "deleted_files": []
///   }],
///   "acceptance_tests_invalidated": [],
///   "recommended_reconciliation_actions": []
/// }
/// ```
///
/// When P2 drift detection is not yet integrated, `signature_drifts`,
/// `count_deltas`, and `deleted_files` default to empty arrays — see the
/// `TODO(P2-integration)` markers in the body. The schema stays stable so
/// downstream consumers can rely on the keys existing.
pub(crate) fn diff_since_seal(cwd: &Path, pack_dir: Option<&str>) -> Result<DiffSinceSealResult> {
    let repo_root = git_repo_root(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let pack_root = resolve_pack_root(&repo_root, pack_dir);
    let current_dir = pack_root.join("current");
    let manifest_path = current_dir.join("manifest.json");

    // Preserve the PASS-through shape even when the manifest is absent: the
    // subcommand is opportunistic and must emit a usable payload rather than
    // surface a hard error for agents.
    let manifest: Value = match fs::read_to_string(&manifest_path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|_| Value::Null),
        Err(_) => Value::Null,
    };

    let baseline_sha = resolve_seal_baseline_sha(&manifest);

    // When we have no baseline (unsealed pack, or very old pre-P1 manifest)
    // and no git history, return an empty-but-shaped payload.
    let zones_map = load_zone_map(&repo_root);
    let Some(zones) = zones_map else {
        let payload = json!({
            "baseline_sha": baseline_sha,
            "pack_updated": false,
            "zones": Vec::<Value>::new(),
            "acceptance_tests_invalidated": Vec::<Value>::new(),
            "recommended_reconciliation_actions": Vec::<String>::new(),
        });
        return Ok(DiffSinceSealResult {
            value: payload,
            acceptance_tests_invalidated: Vec::new(),
        });
    };

    // Resolve the diff range: prefer <baseline>..HEAD when baseline is known
    // and reachable. Otherwise fall back to HEAD~1..HEAD so the command still
    // produces useful output on fresh repos. Non-git directories produce an
    // empty change list and the zone-level payload stays empty.
    let changed_files_raw = match &baseline_sha {
        Some(sha) => {
            // `rev-parse --verify <sha>^{commit}` returns empty (allow_failure)
            // when the baseline commit isn't reachable — typical in shallow
            // clones or when the seal was done against a deleted branch.
            let verify = run_git(
                &["rev-parse", "--verify", &format!("{sha}^{{commit}}")],
                &repo_root,
                true,
            )?;
            if verify.is_empty() {
                // Baseline unreachable (e.g. shallow clone). Fall back to HEAD~1.
                run_git(&["diff", "--name-only", "HEAD~1..HEAD"], &repo_root, true)?
            } else {
                run_git(
                    &["diff", "--name-only", &format!("{sha}..HEAD")],
                    &repo_root,
                    true,
                )?
            }
        }
        None => run_git(&["diff", "--name-only", "HEAD~1..HEAD"], &repo_root, true)
            .unwrap_or_default(),
    };

    // Partition changed files: pack-internal edits don't belong in zones; they
    // drive `pack_updated`. Non-pack files are zone-grouped.
    let mut pack_updated = false;
    let mut non_pack_changes: Vec<String> = Vec::new();
    for file in changed_files_raw
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
    {
        if file.starts_with(".agent-context/current/") {
            pack_updated = true;
            continue;
        }
        non_pack_changes.push(file.to_string());
    }

    // Zone-group the non-pack changes via P3's resolve_affected_sections.
    // We construct a zone entry per authored zone (paths+affects) and collect
    // the subset of non_pack_changes whose glob matches.
    let mut zone_entries: Vec<Value> = Vec::new();
    for zone in &zones {
        let patterns: Vec<&str> = zone.paths.iter().map(|s| s.as_str()).collect();
        let Some(glob_set) = build_glob_set(&patterns) else {
            continue;
        };
        let matched: Vec<String> = non_pack_changes
            .iter()
            .filter(|f| glob_set.is_match(f.replace('\\', "/").as_str()))
            .cloned()
            .collect();
        if matched.is_empty() {
            continue;
        }

        // TODO(P2-integration): when P2 lands, compute signature_drifts,
        // count_deltas, and deleted_files by diffing the manifest baseline
        // (family_counts, shortcut_signatures) against current repo state
        // for the files in `matched`. Today P2 is not integrated in pass1,
        // so these degrade to [] with the schema preserved.
        let signature_drifts: Vec<Value> = Vec::new();
        let count_deltas: Vec<Value> = Vec::new();
        let deleted_files: Vec<Value> = Vec::new();

        zone_entries.push(json!({
            "paths": zone.paths,
            "affects": zone.affects,
            "changed_files": matched,
            "signature_drifts": signature_drifts,
            "count_deltas": count_deltas,
            "deleted_files": deleted_files,
        }));
    }

    // P4 acceptance-test schema: optional, additive. Collect all drifts across
    // zones so cross-zone drifts can still match an acceptance test's
    // `invalidated_by` list.
    let all_drifts: Vec<Value> = zone_entries
        .iter()
        .flat_map(|z| {
            z.get("signature_drifts")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default()
        })
        .collect();
    let acceptance_bindings = parse_acceptance_invalidated_by(&current_dir);
    let acceptance_invalidated = match_invalidated_tests(&all_drifts, &acceptance_bindings);

    let actions = recommended_actions(&zone_entries, &acceptance_invalidated, pack_updated);

    let payload = json!({
        "baseline_sha": baseline_sha,
        "pack_updated": pack_updated,
        "zones": zone_entries,
        "acceptance_tests_invalidated": acceptance_invalidated.clone(),
        "recommended_reconciliation_actions": actions,
    });

    Ok(DiffSinceSealResult {
        value: payload,
        acceptance_tests_invalidated: acceptance_invalidated,
    })
}

/// P7 — render the `diff --since-seal` payload in a compact human-readable
/// form. Mirrors the JSON contract so operators can eyeball zone grouping +
/// recommended actions without piping through `jq`.
pub fn render_diff_since_seal_text(payload: &Value) {
    let baseline = payload
        .get("baseline_sha")
        .and_then(|v| v.as_str())
        .map(|s| short_sha(Some(s)))
        .unwrap_or_else(|| "(none)".to_string());
    let pack_updated = payload
        .get("pack_updated")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    println!("agent-context diff --since-seal");
    println!("  baseline: {baseline}");
    println!("  pack_updated: {pack_updated}");

    let zones = payload.get("zones").and_then(|v| v.as_array());
    match zones {
        Some(arr) if !arr.is_empty() => {
            println!("  zones ({}):", arr.len());
            for zone in arr {
                let affects: Vec<String> = zone
                    .get("affects")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                let changed: Vec<String> = zone
                    .get("changed_files")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                println!(
                    "    - affects: [{}] ({} file(s) changed)",
                    affects.join(", "),
                    changed.len()
                );
                for f in changed.iter().take(MAX_CHANGED_FILES_DISPLAYED) {
                    println!("        {f}");
                }
                if changed.len() > MAX_CHANGED_FILES_DISPLAYED {
                    println!(
                        "        … and {} more",
                        changed.len() - MAX_CHANGED_FILES_DISPLAYED
                    );
                }
            }
        }
        _ => {
            println!("  zones: (none — no context-relevant code changes)");
        }
    }

    let invalidated = payload
        .get("acceptance_tests_invalidated")
        .and_then(|v| v.as_array());
    if let Some(arr) = invalidated {
        if !arr.is_empty() {
            println!("  acceptance_tests_invalidated:");
            for entry in arr {
                let id = entry
                    .get("test_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("(unknown)");
                println!("    - {id}");
            }
        }
    }

    let actions = payload
        .get("recommended_reconciliation_actions")
        .and_then(|v| v.as_array());
    if let Some(arr) = actions {
        if !arr.is_empty() {
            println!("  recommended_reconciliation_actions:");
            for a in arr {
                if let Some(s) = a.as_str() {
                    println!("    - {s}");
                }
            }
        }
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

/// P3: placeholder form of the default relevance.json, used by tests and any
/// caller that does not want study/ auto-detection. The `init()` flow uses
/// [`default_relevance_json_with_study`] directly.
#[allow(dead_code)]
fn default_relevance_json() -> String {
    // Pass-0 default kept a legacy `include`/`exclude` pair so the existing
    // freshness path stays working without a zone map. P3 adds a default
    // `zones[]` with study/, docs/, and common dependency files so new repos
    // get zone-aware freshness out of the box.
    default_relevance_json_with_study(false)
}

/// P3: assemble the default relevance.json contents, optionally including a
/// `study/**` zone when the repo actually has a `study/` directory. Callers
/// that do not already know whether `study/` exists should use
/// [`default_relevance_json`], which defaults to the placeholder form used by
/// the old behavior.
fn default_relevance_json_with_study(has_study: bool) -> String {
    let study_zone = if has_study {
        r#"    {"paths": ["study/**", "docs/methodology/**"], "affects": ["10_SYSTEM_OVERVIEW.md", "30_BEHAVIORAL_INVARIANTS.md"]},
"#
    } else {
        // Keep a placeholder entry so the default file has the correct shape
        // even when no study/ directory exists yet. Docs alone is a safe
        // default because most repos have a docs/ tree.
        r#"    {"paths": ["docs/**"], "affects": ["10_SYSTEM_OVERVIEW.md", "30_BEHAVIORAL_INVARIANTS.md"]},
"#
    };
    format!(
        r#"{{
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
  ],
  "zones": [
{study_zone}    {{"paths": ["src/**", "cli/src/**"], "affects": ["20_CODE_MAP.md", "30_BEHAVIORAL_INVARIANTS.md"]}},
    {{"paths": ["scripts/run_*.py", "scripts/**"], "affects": ["20_CODE_MAP.md", "40_OPERATIONS_AND_RELEASE.md"]}},
    {{"paths": ["pyproject.toml", "Cargo.toml", "package.json", "cli/Cargo.toml"], "affects": ["40_OPERATIONS_AND_RELEASE.md"]}}
  ]
}}
"#
    )
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
    // P12 / F44 — shell hygiene:
    //   - `set -u` so unset variables fail fast (the `${1:-origin}` default
    //     style is what makes this safe even when git invokes the hook
    //     without both positional args).
    //   - Every `$VAR` interpolation is quoted.
    //   - User-controlled paths pass through a `--` separator to `git diff`
    //     so a path beginning with `-` cannot be interpreted as a flag.
    r#"set -u
remote_name="${1:-origin}"
remote_url="${2:-unknown}"

# P6: when the push range only touches `.agent-context/`, skip the freshness
# cycle entirely. This avoids the noise loop where a code push warns "pack is
# stale", the agent updates the pack and pushes, and the hook re-warns about
# the pack's own commit.
#
# The state file `.agent-context/current/.last_freshness.json` records the
# most recent warning (changed_files + affected_sections + timestamp). On a
# pack-only follow-up push, the hook reads that state and, if the new pack
# paths plausibly cover the affected_sections, prints
# "warning appears addressed: sections [...]".
pack_only_skip() {
  local base="${1:-}"
  local head="${2:-}"

  if [[ -z "$base" || -z "$head" || "$base" == "0000000000000000000000000000000000000000" ]]; then
    return 1
  fi

  # P12/F44 — `--` separator ensures SHA/path values beginning with `-`
  # cannot be parsed as git options.
  local changed
  changed="$(git diff --name-only "${base}..${head}" -- 2>/dev/null || true)"
  if [[ -z "$changed" ]]; then
    return 1
  fi

  # Return non-zero (NOT pack-only) if any path does not start with
  # `.agent-context/`. The inverted grep is important: we must see no
  # non-pack line for this to be a pack-only push.
  if printf '%s\n' "$changed" | grep -vE '^\.agent-context/' >/dev/null 2>&1; then
    return 1
  fi

  echo "[context-pack] pack-only push, skipping freshness check"

  local state_file=".agent-context/current/.last_freshness.json"
  if [[ -f "$state_file" ]]; then
    # Extract affected_sections from the last warning. Prefer jq; fall back to
    # python3 so the check still works on minimal CI images.
    local sections=""
    if command -v jq >/dev/null 2>&1; then
      sections="$(jq -r '.affected_sections[]?' "$state_file" 2>/dev/null | tr '\n' ' ')"
    elif command -v python3 >/dev/null 2>&1; then
      sections="$(python3 -c "import json,sys
try:
    d=json.load(open('$state_file'))
    for s in d.get('affected_sections', []):
        print(s)
except Exception:
    pass" 2>/dev/null | tr '\n' ' ')"
    fi

    if [[ -n "${sections// /}" ]]; then
      # A pack-only push that touches the same section files the prior warning
      # named is treated as "probably addresses the warning". This is
      # best-effort reporting, not a hard guarantee.
      local covered=""
      local missing=""
      for section in $sections; do
        if printf '%s\n' "$changed" | grep -F ".agent-context/current/${section}" >/dev/null 2>&1; then
          covered="${covered}${section} "
        else
          missing="${missing}${section} "
        fi
      done
      if [[ -n "${covered// /}" ]]; then
        echo "[context-pack] warning appears addressed: sections [${covered% }] updated"
      fi
      if [[ -n "${missing// /}" ]]; then
        echo "[context-pack] note: sections still referenced by last warning: [${missing% }]"
      fi
    fi
  fi

  return 0
}

run_context_sync() {
  local local_ref="${1:-}"
  local local_sha="${2:-}"
  local remote_ref="${3:-}"
  local remote_sha="${4:-}"

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
    if pack_only_skip "$remote_sha" "$local_sha"; then
      continue
    fi
    run_context_sync "$local_ref" "$local_sha" "$remote_ref" "$remote_sha"
  fi
done"#
    .to_string()
}

/// P1 — shell body for the post-commit-reconcile hook. Runs
/// `chorus agent-context post-commit-reconcile` only when the just-landed
/// commit touched `.agent-context/**`. Silent no-op when the chorus CLI is
/// unavailable (so the hook never blocks a normal `git commit`).
fn build_post_commit_hook_section() -> String {
    r#"# Only reconcile when the commit touched pack content; checking the full
# `.agent-context/` tree keeps this O(1) for the common non-pack commit.
if git diff --name-only HEAD~1 HEAD 2>/dev/null | grep -q '^\.agent-context/'; then
  if command -v chorus >/dev/null 2>&1; then
    chorus agent-context post-commit-reconcile >/dev/null 2>&1 || true
  fi
fi"#
    .to_string()
}

// ============================================================================
// P2 — Structural verifier
//
// Extends `verify` with structural checks that complement Pass-0's byte-level
// integrity. Ported from team_skills' `scripts/verify_context_pack.py` (the
// machine-checkable pack-author contract) and extended with four P1-baseline
// drift checks:
//
//   1. `family_counts` drift   — resolve globs NOW, compare vs manifest
//   2. `declared_counts` drift — re-extract prose numbers, name stale files
//   3. `shortcut_signatures`   — re-parse signatures, `SIGNATURE_DRIFT` per fn
//   4. `dependencies_snapshot` — re-hash deps files, point at 40_OPERATIONS
//
// Ported from team_skills:
//   a. Template-marker absence in pack JSON ({name}, {domain}, {module}, REPLACE, <!-- AGENT:)
//   b. `completeness_contract.json` glob existence + cardinality >=1
//   c. `search_scope.json` verification_shortcuts `look_for` present in file
//   d. Routing files (CLAUDE.md/GEMINI.md/AGENTS.md) reference canonical paths
//
// Plus `contractually_required_files[]` existence (catches deletions; #5).
//
// Design notes:
// - Warnings are non-fatal by default so existing tests / already-sealed packs
//   don't regress. CI JSON surfaces them under `structural_warnings[]` with a
//   stable shape: `{kind, message, affected_pack_files: []}`.
// - `affected_pack_files` is derived directly from the check category per the
//   P3 zone-map contract: template-marker names the file; family-count drift
//   points at 20_CODE_MAP.md + 40_OPERATIONS_AND_RELEASE.md; signature drift
//   points at 20_CODE_MAP.md + search_scope.json; deps drift points at
//   40_OPERATIONS_AND_RELEASE.md; etc. This is the fixed P3-style affects[]
//   list; a future pass will read zones from `relevance.json` when present.
// - Each helper is intentionally small and pure (manifest JSON + filesystem)
//   so tests can exercise a single check without standing up the full verify.
// ============================================================================

/// P2 — structural warning emitted by `run_structural_checks`. Consumers
/// (human output, CI JSON) render these identically; the `kind` discriminator
/// is the machine-readable category.
#[derive(Clone, Debug)]
struct StructuralWarning {
    /// Upper-snake-case category, e.g. "TEMPLATE_MARKER", "FAMILY_COUNT_DRIFT",
    /// "SIGNATURE_DRIFT", "DECLARED_COUNT_DRIFT", "DEPENDENCIES_DRIFT",
    /// "CONTRACT_GLOB_MISS", "LOOK_FOR_MISSING", "ROUTING_MISSING_REF",
    /// "CONTRACT_REQUIRED_FILE_MISSING".
    kind: String,
    message: String,
    /// Pack filenames (repo-relative under `.agent-context/current/`) whose
    /// prose probably needs updating to clear this warning. Empty is a valid
    /// value (e.g. when the check names a repo file, not a pack file).
    affected_pack_files: Vec<String>,
}

/// P2 — template markers that must never appear in sealed pack JSON. Matches
/// the team_skills reference + the Claude-specific scaffolding tag we emit.
fn template_marker_candidates() -> &'static [&'static str] {
    &["{name}", "{domain}", "{module}", "REPLACE", "<!-- AGENT:"]
}

/// P2 check (a): fail on template markers left in any `*.json` file under the
/// pack's current directory.
fn check_template_markers(current_dir: &Path, out: &mut Vec<StructuralWarning>) {
    let entries = match fs::read_dir(current_dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut paths: Vec<PathBuf> = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        if p.is_file() && p.extension().and_then(|x| x.to_str()) == Some("json") {
            paths.push(p);
        }
    }
    paths.sort();
    for path in paths {
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => continue,
        };
        for marker in template_marker_candidates() {
            if text.contains(marker) {
                let fname = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                out.push(StructuralWarning {
                    kind: "TEMPLATE_MARKER".to_string(),
                    message: format!(
                        "template marker '{}' found in pack JSON: {}",
                        marker, fname
                    ),
                    affected_pack_files: vec![fname.clone()],
                });
            }
        }
    }
}

/// P2 check (b) + #5: `completeness_contract.json` required_file_families[]
/// globs must still resolve to at least one file AND
/// `contractually_required_files[]` entries must still exist on disk.
fn check_contract_files_exist(
    repo_root: &Path,
    current_dir: &Path,
    out: &mut Vec<StructuralWarning>,
) {
    let contract_path = current_dir.join("completeness_contract.json");
    let Ok(Some(contract)) = read_json(&contract_path) else {
        return;
    };
    // Support both the flat `contracts` shape (team_skills reference) and
    // the `task_families` shape (chorus scaffolding). Walk whichever is
    // present so the check stays agnostic.
    let mut entries: Vec<(String, &serde_json::Map<String, Value>)> = Vec::new();
    if let Some(map) = contract.get("contracts").and_then(|v| v.as_object()) {
        for (k, v) in map {
            if let Some(obj) = v.as_object() {
                entries.push((k.clone(), obj));
            }
        }
    }
    if let Some(map) = contract.get("task_families").and_then(|v| v.as_object()) {
        for (k, v) in map {
            if let Some(obj) = v.as_object() {
                entries.push((k.clone(), obj));
            }
        }
    }

    for (name, entry) in entries {
        if let Some(list) = entry
            .get("contractually_required_files")
            .and_then(|v| v.as_array())
        {
            for item in list {
                if let Some(rel) = item.as_str() {
                    if !repo_root.join(rel).exists() {
                        out.push(StructuralWarning {
                            kind: "CONTRACT_REQUIRED_FILE_MISSING".to_string(),
                            message: format!(
                                "completeness_contract '{name}': required file missing on disk: {rel}"
                            ),
                            affected_pack_files: vec![
                                "completeness_contract.json".to_string(),
                                "20_CODE_MAP.md".to_string(),
                            ],
                        });
                    }
                }
            }
        }
        if let Some(list) = entry.get("required_file_families").and_then(|v| v.as_array()) {
            for item in list {
                if let Some(pattern) = item.as_str() {
                    // Template markers in globs are their own failure mode
                    // (redundant with check_template_markers but cheap to
                    // call here for clarity of error message).
                    let has_marker = template_marker_candidates()
                        .iter()
                        .any(|m| pattern.contains(m));
                    if has_marker {
                        out.push(StructuralWarning {
                            kind: "CONTRACT_GLOB_MISS".to_string(),
                            message: format!(
                                "completeness_contract '{name}': template marker in glob: {pattern}"
                            ),
                            affected_pack_files: vec![
                                "completeness_contract.json".to_string(),
                            ],
                        });
                        continue;
                    }
                    let matches =
                        resolve_pattern_matches(repo_root, pattern).unwrap_or_default();
                    if matches.is_empty() {
                        out.push(StructuralWarning {
                            kind: "CONTRACT_GLOB_MISS".to_string(),
                            message: format!(
                                "completeness_contract '{name}': glob matches no files: {pattern}"
                            ),
                            affected_pack_files: vec![
                                "completeness_contract.json".to_string(),
                                "20_CODE_MAP.md".to_string(),
                            ],
                        });
                    }
                }
            }
        }
    }
}

/// P12 / F40 — strip line and block comments from `text` based on the file
/// extension. Called by [`check_verification_shortcuts_look_for`] before
/// matching `look_for` so a string that only appears inside a comment is
/// surfaced as drift rather than a spurious pass.
///
/// Language handling:
/// - `.py`: `#...$` line comments and `"""..."""` / `'''...'''` docstrings
/// - `.rs`: `//...$` line comments and `/*...*/` block comments
/// - `.ts` / `.tsx` / `.js` / `.jsx`: `//...$` line comments and
///   `/*...*/` block comments
/// - Any other (or unknown) extension: returns `text` unchanged. Callers
///   then fall back to the existing substring contract — consumers of older
///   schemas with unusual extensions keep working.
///
/// This is a regex-free implementation: the nested docstring/block-comment
/// contract is easier to reason about as a small state machine than as a
/// multi-line regex, and it keeps us from pulling `regex` into the core
/// structural-check path for a single feature.
fn strip_source_comments(text: &str, extension: &str) -> String {
    match extension {
        "py" => strip_python_comments(text),
        "rs" | "ts" | "tsx" | "js" | "jsx" | "cjs" | "mjs" => strip_c_style_comments(text),
        _ => text.to_string(),
    }
}

/// Python-specific variant: strip `#...$` line comments and `"""..."""` /
/// `'''..'''` docstring-style triple-quoted string blocks. This is a
/// best-effort scanner — it does not understand nested expressions inside
/// f-strings, which is fine for look_for drift detection where we only care
/// about surfacing comment-only matches, not exact lexing.
fn strip_python_comments(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        // Triple-quoted block: consume until the matching closer.
        if i + 2 < bytes.len() && (c == b'"' || c == b'\'')
            && bytes[i + 1] == c
            && bytes[i + 2] == c
        {
            let quote = c;
            i += 3;
            while i + 2 < bytes.len() {
                if bytes[i] == quote && bytes[i + 1] == quote && bytes[i + 2] == quote {
                    i += 3;
                    break;
                }
                // Preserve newlines so line-anchored searches still work.
                if bytes[i] == b'\n' {
                    out.push('\n');
                }
                i += 1;
            }
            // Reached EOF mid-docstring — stop consuming.
            if i + 2 >= bytes.len() {
                break;
            }
            continue;
        }
        // Line comment: drop everything up to the next newline.
        if c == b'#' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        out.push(c as char);
        i += 1;
    }
    out
}

/// C-family variant: strip `//...$` line comments and `/*...*/` block
/// comments. Newlines inside block comments are preserved so line numbers
/// downstream (if any) stay aligned. Does not track strings, which is safe
/// for look_for drift detection since a `//` inside a string literal still
/// represents "this substring appears in real code-ish context" — users who
/// need perfect lexing can fall back to `look_for_regex`.
fn strip_c_style_comments(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'/' && i + 1 < bytes.len() {
            let next = bytes[i + 1];
            if next == b'/' {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            if next == b'*' {
                i += 2;
                while i + 1 < bytes.len() {
                    if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                        i += 2;
                        break;
                    }
                    if bytes[i] == b'\n' {
                        out.push('\n');
                    }
                    i += 1;
                }
                continue;
            }
        }
        out.push(c as char);
        i += 1;
    }
    out
}

/// P12 / F40 — true when `extension` is one of the languages
/// [`strip_source_comments`] actually trims. Used to decide whether a
/// `look_for` failure should report "matches only comments" instead of the
/// generic "string not found" message.
fn extension_supports_comment_strip(extension: &str) -> bool {
    matches!(
        extension,
        "py" | "rs" | "ts" | "tsx" | "js" | "jsx" | "cjs" | "mjs"
    )
}

/// Extract a lowercase file extension (without the `.`) from a path string.
/// Returns empty when no extension is present.
fn path_extension(rel: &str) -> String {
    Path::new(rel)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default()
}

/// P2 check (c) / P12 F40: every `search_scope.json` verification_shortcut
/// with a `look_for` string (or new `look_for_regex`) must match inside the
/// referenced file AFTER comments are stripped for supported languages.
///
/// Handles both shortcut shapes:
/// - team_skills: `verification_shortcuts: [{file, look_for, [look_for_regex]}]`
/// - chorus default scaffold: `verification_shortcuts: { "path": "hint" }`
///   (no `look_for` — we silently skip; there's nothing to verify).
///
/// `look_for_regex` takes precedence over `look_for` when both are present.
/// Invalid regex falls through to a dedicated warning so the author can fix
/// the pattern without a mysterious "no match" error.
fn check_verification_shortcuts_look_for(
    repo_root: &Path,
    current_dir: &Path,
    out: &mut Vec<StructuralWarning>,
) {
    let scope_path = current_dir.join("search_scope.json");
    let Ok(Some(scope)) = read_json(&scope_path) else {
        return;
    };
    let Some(families) = scope.get("task_families").and_then(|v| v.as_object()) else {
        return;
    };
    for (family, data) in families {
        // Array form (team_skills).
        if let Some(arr) = data.get("verification_shortcuts").and_then(|v| v.as_array()) {
            for shortcut in arr {
                let Some(obj) = shortcut.as_object() else { continue };
                let Some(rel) = obj.get("file").and_then(|v| v.as_str()) else {
                    out.push(StructuralWarning {
                        kind: "LOOK_FOR_MISSING".to_string(),
                        message: format!(
                            "search_scope {family}: verification shortcut missing 'file'"
                        ),
                        affected_pack_files: vec!["search_scope.json".to_string()],
                    });
                    continue;
                };
                let look_for = obj
                    .get("look_for")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let look_for_regex = obj
                    .get("look_for_regex")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let path = repo_root.join(rel);
                if !path.exists() {
                    out.push(StructuralWarning {
                        kind: "LOOK_FOR_MISSING".to_string(),
                        message: format!(
                            "search_scope {family}: missing verification file: {rel}"
                        ),
                        affected_pack_files: vec!["search_scope.json".to_string()],
                    });
                    continue;
                }
                if look_for.is_empty() && look_for_regex.is_empty() {
                    continue;
                }
                let raw = fs::read_to_string(&path).unwrap_or_default();
                let extension = path_extension(rel);
                let stripped = strip_source_comments(&raw, &extension);

                // look_for_regex takes precedence when present.
                if !look_for_regex.is_empty() {
                    match regex_lite_match(&stripped, look_for_regex) {
                        Ok(true) => continue,
                        Ok(false) => {
                            out.push(StructuralWarning {
                                kind: "LOOK_FOR_MISSING".to_string(),
                                message: format!(
                                    "search_scope {family}: look_for_regex did not match in {rel}: {look_for_regex}"
                                ),
                                affected_pack_files: vec![
                                    "search_scope.json".to_string(),
                                    "20_CODE_MAP.md".to_string(),
                                ],
                            });
                        }
                        Err(msg) => {
                            out.push(StructuralWarning {
                                kind: "LOOK_FOR_MISSING".to_string(),
                                message: format!(
                                    "search_scope {family}: look_for_regex invalid for {rel}: {msg}"
                                ),
                                affected_pack_files: vec!["search_scope.json".to_string()],
                            });
                        }
                    }
                    continue;
                }

                // Substring path.
                if stripped.contains(look_for) {
                    continue;
                }
                // Distinguish "only matches in comments" from "doesn't match at all"
                // so authors can tell whether the pack pointer is stale or wrong.
                let message = if extension_supports_comment_strip(&extension)
                    && raw.contains(look_for)
                {
                    format!(
                        "search_scope {family}: look_for matches only comments in {rel}: {look_for}"
                    )
                } else {
                    format!(
                        "search_scope {family}: look_for string not found in {rel}: {look_for}"
                    )
                };
                out.push(StructuralWarning {
                    kind: "LOOK_FOR_MISSING".to_string(),
                    message,
                    affected_pack_files: vec![
                        "search_scope.json".to_string(),
                        "20_CODE_MAP.md".to_string(),
                    ],
                });
            }
        }
        // Object form (chorus scaffold) — nothing to verify at `look_for` level,
        // but catching a missing file is still useful. We intentionally do not
        // emit a redundant existence error here because `validate_structured_layer`
        // already runs this check at seal time.
    }
}

/// P12 / F40 — minimalist regex matcher used by `check_verification_shortcuts_look_for`.
/// We intentionally avoid pulling a new `regex` dependency into the CLI crate
/// for the single look_for_regex feature. The subset we need here covers
/// literal characters, character classes `[...]`, escapes `\d \w \s \. \\`,
/// anchors `^$`, and repetition `* + ? {n,m}`. We lean on the already-present
/// `globset`-style semantics: any unsupported construct falls through to
/// `Err` with a human-readable message so authors get a clear diagnostic.
///
/// For the P12 contract (comment-aware look_for match, regex upgrade path)
/// this scope is sufficient — users who need full PCRE can still author
/// the substring form. Returns `Ok(true)` on match, `Ok(false)` on clean
/// non-match, `Err(msg)` when the pattern cannot be compiled.
fn regex_lite_match(text: &str, pattern: &str) -> std::result::Result<bool, String> {
    let compiled = compile_regex_lite(pattern)?;
    Ok(regex_lite_search(&compiled, text))
}

#[derive(Debug, Clone)]
enum RegexLiteTok {
    /// Literal character run.
    Literal(String),
    /// Character class; `invert` flips ASCII range membership.
    Class { chars: Vec<char>, ranges: Vec<(char, char)>, invert: bool },
    /// Any char (.)
    AnyChar,
    /// Digit, word, whitespace shorthand classes.
    Digit,
    Word,
    Space,
    /// ^ / $ anchors.
    LineStart,
    LineEnd,
}

#[derive(Debug, Clone)]
struct RegexLiteAtom {
    tok: RegexLiteTok,
    /// Minimum repetitions.
    min: usize,
    /// Maximum repetitions, `None` for unbounded.
    max: Option<usize>,
}

fn compile_regex_lite(pattern: &str) -> std::result::Result<Vec<RegexLiteAtom>, String> {
    let mut atoms: Vec<RegexLiteAtom> = Vec::new();
    let bytes: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i];
        let tok = match ch {
            '.' => {
                i += 1;
                RegexLiteTok::AnyChar
            }
            '^' => {
                i += 1;
                RegexLiteTok::LineStart
            }
            '$' => {
                i += 1;
                RegexLiteTok::LineEnd
            }
            '(' | ')' | '|' => {
                return Err(format!("unsupported construct `{ch}` (grouping/alternation not implemented)"));
            }
            '[' => {
                // Character class.
                i += 1;
                let mut invert = false;
                if i < bytes.len() && bytes[i] == '^' {
                    invert = true;
                    i += 1;
                }
                let mut chars = Vec::new();
                let mut ranges = Vec::new();
                let mut closed = false;
                while i < bytes.len() {
                    if bytes[i] == ']' {
                        closed = true;
                        i += 1;
                        break;
                    }
                    let c = if bytes[i] == '\\' && i + 1 < bytes.len() {
                        i += 1;
                        bytes[i]
                    } else {
                        bytes[i]
                    };
                    // Range like a-z.
                    if i + 2 < bytes.len() && bytes[i + 1] == '-' && bytes[i + 2] != ']' {
                        let end = if bytes[i + 2] == '\\' && i + 3 < bytes.len() {
                            i += 1;
                            bytes[i + 2]
                        } else {
                            bytes[i + 2]
                        };
                        ranges.push((c, end));
                        i += 3;
                    } else {
                        chars.push(c);
                        i += 1;
                    }
                }
                if !closed {
                    return Err("unterminated character class".to_string());
                }
                RegexLiteTok::Class { chars, ranges, invert }
            }
            '\\' => {
                if i + 1 >= bytes.len() {
                    return Err("trailing backslash".to_string());
                }
                let esc = bytes[i + 1];
                i += 2;
                match esc {
                    'd' => RegexLiteTok::Digit,
                    'w' => RegexLiteTok::Word,
                    's' => RegexLiteTok::Space,
                    '.' | '\\' | '(' | ')' | '[' | ']' | '{' | '}' | '+' | '*' | '?' | '|' | '^' | '$' => {
                        RegexLiteTok::Literal(esc.to_string())
                    }
                    'n' => RegexLiteTok::Literal("\n".to_string()),
                    't' => RegexLiteTok::Literal("\t".to_string()),
                    other => return Err(format!("unsupported escape `\\{other}`")),
                }
            }
            other => {
                i += 1;
                RegexLiteTok::Literal(other.to_string())
            }
        };

        let (min, max, consumed) = parse_regex_quantifier(&bytes, i)?;
        i += consumed;
        atoms.push(RegexLiteAtom { tok, min, max });
    }
    Ok(atoms)
}

fn parse_regex_quantifier(
    bytes: &[char],
    i: usize,
) -> std::result::Result<(usize, Option<usize>, usize), String> {
    if i >= bytes.len() {
        return Ok((1, Some(1), 0));
    }
    match bytes[i] {
        '*' => Ok((0, None, 1)),
        '+' => Ok((1, None, 1)),
        '?' => Ok((0, Some(1), 1)),
        '{' => {
            // Parse {n} or {n,m}.
            let mut j = i + 1;
            let mut n_str = String::new();
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                n_str.push(bytes[j]);
                j += 1;
            }
            if n_str.is_empty() {
                return Err("empty {} quantifier".to_string());
            }
            let n: usize = n_str.parse().map_err(|_| "bad {} quantifier".to_string())?;
            if j < bytes.len() && bytes[j] == '}' {
                return Ok((n, Some(n), j + 1 - i));
            }
            if j < bytes.len() && bytes[j] == ',' {
                j += 1;
                let mut m_str = String::new();
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    m_str.push(bytes[j]);
                    j += 1;
                }
                if j >= bytes.len() || bytes[j] != '}' {
                    return Err("unterminated {} quantifier".to_string());
                }
                let max = if m_str.is_empty() {
                    None
                } else {
                    Some(m_str.parse().map_err(|_| "bad {} quantifier".to_string())?)
                };
                return Ok((n, max, j + 1 - i));
            }
            Err("unterminated {} quantifier".to_string())
        }
        _ => Ok((1, Some(1), 0)),
    }
}

fn regex_lite_search(atoms: &[RegexLiteAtom], text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    for start in 0..=chars.len() {
        if regex_lite_match_at(atoms, 0, &chars, start).is_some() {
            return true;
        }
    }
    false
}

/// Returns `Some(end_pos)` on match, `None` on no-match. Recursive matcher:
/// enumerates greedy-first repetition counts and backtracks only when the
/// rest of the pattern fails. Compact enough to stay readable for the P12
/// scope; a future pass should replace this with the `regex` crate when we
/// want broader pattern support.
fn regex_lite_match_at(
    atoms: &[RegexLiteAtom],
    atom_idx: usize,
    text: &[char],
    pos: usize,
) -> Option<usize> {
    if atom_idx >= atoms.len() {
        return Some(pos);
    }
    let atom = &atoms[atom_idx];
    // Anchors do not consume characters.
    if let RegexLiteTok::LineStart = atom.tok {
        // Match at pos 0 or after a newline.
        if pos == 0 || (pos > 0 && text[pos - 1] == '\n') {
            return regex_lite_match_at(atoms, atom_idx + 1, text, pos);
        }
        return None;
    }
    if let RegexLiteTok::LineEnd = atom.tok {
        if pos == text.len() || text[pos] == '\n' {
            return regex_lite_match_at(atoms, atom_idx + 1, text, pos);
        }
        return None;
    }
    // Greedy: consume up to max, then try each fallback count down to min.
    let mut count = 0;
    let mut cursor = pos;
    while cursor < text.len() && atom.max.map_or(true, |m| count < m)
        && atom_matches_char(&atom.tok, text[cursor])
    {
        cursor += 1;
        count += 1;
    }
    loop {
        if count >= atom.min {
            if let Some(end) = regex_lite_match_at(atoms, atom_idx + 1, text, cursor) {
                return Some(end);
            }
        }
        if count == 0 {
            break;
        }
        count -= 1;
        cursor -= 1;
    }
    None
}

fn atom_matches_char(tok: &RegexLiteTok, c: char) -> bool {
    match tok {
        RegexLiteTok::Literal(s) => s.chars().next().map_or(false, |lit| lit == c),
        RegexLiteTok::AnyChar => c != '\n',
        RegexLiteTok::Digit => c.is_ascii_digit(),
        RegexLiteTok::Word => c.is_ascii_alphanumeric() || c == '_',
        RegexLiteTok::Space => c.is_whitespace(),
        RegexLiteTok::Class { chars, ranges, invert } => {
            let mut hit = chars.contains(&c);
            if !hit {
                for (lo, hi) in ranges {
                    if c >= *lo && c <= *hi {
                        hit = true;
                        break;
                    }
                }
            }
            if *invert { !hit } else { hit }
        }
        _ => false,
    }
}

/// P2 check (d): routing files (CLAUDE.md / GEMINI.md / AGENTS.md) must
/// reference the canonical pack paths. Missing files are not errors (repos
/// may scaffold only a subset of agents); *present* files must be correct.
fn check_routing_files(repo_root: &Path, out: &mut Vec<StructuralWarning>) {
    let claude = repo_root.join("CLAUDE.md");
    let gemini = repo_root.join("GEMINI.md");
    let agents = repo_root.join("AGENTS.md");

    if claude.exists() {
        let text = fs::read_to_string(&claude).unwrap_or_default();
        if !text.contains("00_START_HERE.md") || !text.contains("30_BEHAVIORAL_INVARIANTS.md") {
            out.push(StructuralWarning {
                kind: "ROUTING_MISSING_REF".to_string(),
                message:
                    "CLAUDE.md does not reference canonical pack paths (00_START_HERE.md + 30_BEHAVIORAL_INVARIANTS.md)"
                        .to_string(),
                affected_pack_files: vec!["CLAUDE.md".to_string()],
            });
        }
    }
    if gemini.exists() {
        let text = fs::read_to_string(&gemini).unwrap_or_default();
        if !text.contains("00_START_HERE.md") || !text.contains("30_BEHAVIORAL_INVARIANTS.md") {
            out.push(StructuralWarning {
                kind: "ROUTING_MISSING_REF".to_string(),
                message:
                    "GEMINI.md does not reference canonical pack paths (00_START_HERE.md + 30_BEHAVIORAL_INVARIANTS.md)"
                        .to_string(),
                affected_pack_files: vec!["GEMINI.md".to_string()],
            });
        }
    }
    if agents.exists() {
        let text = fs::read_to_string(&agents).unwrap_or_default();
        // AGENTS.md is the search-and-verify routing: expect routes.json reference.
        if !text.contains("routes.json") {
            out.push(StructuralWarning {
                kind: "ROUTING_MISSING_REF".to_string(),
                message: "AGENTS.md does not reference canonical routes.json".to_string(),
                affected_pack_files: vec!["AGENTS.md".to_string()],
            });
        }
    }
}

/// P2 check 1: compare manifest's stored `family_counts` (from seal) against a
/// live resolution. Emits one `FAMILY_COUNT_DRIFT` warning per pattern whose
/// count changed; names the prose files that most commonly quote code-family
/// counts (20_CODE_MAP.md + 40_OPERATIONS_AND_RELEASE.md).
fn check_family_counts_drift(
    manifest: &Value,
    repo_root: &Path,
    current_dir: &Path,
    out: &mut Vec<StructuralWarning>,
) {
    let stored = manifest
        .get("family_counts")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    if stored.is_empty() {
        return;
    }
    let live = resolve_family_counts(repo_root, current_dir);
    // Iterate in BTreeSet order over the union of keys so output is stable.
    let mut keys: BTreeSet<String> = BTreeSet::new();
    for k in stored.keys() {
        keys.insert(k.clone());
    }
    for k in live.keys() {
        keys.insert(k.clone());
    }
    for k in keys {
        let stored_n = stored.get(&k).and_then(|v| v.as_u64()).unwrap_or(0);
        let live_n = live.get(&k).copied().unwrap_or(0) as u64;
        if stored_n == live_n {
            continue;
        }
        out.push(StructuralWarning {
            kind: "FAMILY_COUNT_DRIFT".to_string(),
            message: format!(
                "family_counts drift for '{k}': manifest says {stored_n}, live resolves to {live_n} (delta {delta})",
                delta = (live_n as i64) - (stored_n as i64)
            ),
            affected_pack_files: vec![
                "20_CODE_MAP.md".to_string(),
                "40_OPERATIONS_AND_RELEASE.md".to_string(),
            ],
        });
    }
}

/// P2 check 2: cross-check pack prose numeric claims against live authoritative
/// counts resolved from globs. Emits one `DECLARED_COUNT_DRIFT` warning per
/// stale (noun, count) tuple, naming every pack file+line that parrots the
/// stale number.
///
/// The authoritative source is live `family_counts` (re-resolved against
/// disk): for each noun we heuristically find the glob whose pattern
/// mentions that noun (e.g. noun "scripts" -> pattern "scripts/run_*.py").
/// When prose says N and the matched glob resolves to M, every prose line
/// saying N is reported as stale.
///
/// Nouns without a matching glob pattern fall back to comparing live prose
/// against the manifest's sealed `declared_counts` for the same noun — if
/// they disagree, we name the stale (older-seal) lines so the reviewer can
/// pick the authoritative answer.
fn check_declared_counts_drift(
    manifest: &Value,
    repo_root: &Path,
    current_dir: &Path,
    out: &mut Vec<StructuralWarning>,
) {
    let live_prose = extract_declared_counts(current_dir);
    if live_prose.is_empty() {
        return;
    }
    let stored_prose = manifest
        .get("declared_counts")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    // Authoritative counts: re-resolve globs NOW. This is what "the repo
    // actually looks like" at verify time. Keyed by the glob pattern.
    let live_families = resolve_family_counts(repo_root, current_dir);

    // Map each noun to an authoritative count when a glob pattern references
    // the noun word. Matching is case-insensitive and checks for the noun or
    // its singular form in the pattern text (e.g. "scripts" -> "scripts/").
    fn noun_matches_pattern(noun: &str, pattern: &str) -> bool {
        let p = pattern.to_ascii_lowercase();
        let n = noun.to_ascii_lowercase();
        if p.contains(&n) {
            return true;
        }
        // Try the singular form (strip trailing "s"/"es").
        let trimmed = n.trim_end_matches('s');
        if trimmed.len() < n.len() && !trimmed.is_empty() && p.contains(trimmed) {
            return true;
        }
        false
    }

    let resolve_auth_count = |noun: &str| -> Option<u64> {
        for (pattern, count) in &live_families {
            if noun_matches_pattern(noun, pattern) {
                return Some(*count as u64);
            }
        }
        None
    };

    // Walk live prose claims: for each (noun, count, file, line), check
    // whether an authoritative answer exists and disagrees.
    use std::collections::BTreeMap;
    // (noun, stale_count, authoritative_count) -> [file:line, ...].
    let mut stale_tuples: BTreeMap<(String, u64, u64), Vec<String>> = BTreeMap::new();

    for v in &live_prose {
        let noun = match v.get("noun").and_then(|x| x.as_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let count = v.get("count").and_then(|x| x.as_u64()).unwrap_or(0);
        let file = v
            .get("file")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let line = v.get("line").and_then(|x| x.as_u64()).unwrap_or(0);

        // Prefer the authoritative answer. Fall back to manifest sealed prose.
        let auth = resolve_auth_count(&noun).or_else(|| {
            stored_prose.iter().find_map(|sv| {
                let sv_noun = sv.get("noun").and_then(|x| x.as_str()).unwrap_or("");
                if sv_noun.eq_ignore_ascii_case(&noun) {
                    sv.get("count").and_then(|x| x.as_u64())
                } else {
                    None
                }
            })
        });
        if let Some(auth_count) = auth {
            if auth_count != count {
                stale_tuples
                    .entry((noun.clone(), count, auth_count))
                    .or_default()
                    .push(format!("{}:{}", file, line));
            }
        }
    }

    for ((noun, stale_count, auth_count), occurrences) in stale_tuples {
        let mut affected: BTreeSet<String> = BTreeSet::new();
        for occ in &occurrences {
            if let Some(f) = occ.split(':').next() {
                affected.insert(f.to_string());
            }
        }
        let affected_vec: Vec<String> = affected.into_iter().collect();
        out.push(StructuralWarning {
            kind: "DECLARED_COUNT_DRIFT".to_string(),
            message: format!(
                "declared_counts drift: pack prose says '{stale_count} {noun}' but authoritative count is {auth_count}; stale at {occurrences}",
                occurrences = occurrences.join(", ")
            ),
            affected_pack_files: affected_vec,
        });
    }
}

/// P2 check 3: re-parse `verification_shortcuts` signatures from the repo and
/// compare to manifest. Emits one `SIGNATURE_DRIFT` warning per function whose
/// signature changed (including renames — the old key disappears, a new key
/// appears; we emit one warning per side so the reviewer can spot renames).
fn check_shortcut_signatures_drift(
    manifest: &Value,
    repo_root: &Path,
    current_dir: &Path,
    out: &mut Vec<StructuralWarning>,
) {
    let stored_map = manifest
        .get("shortcut_signatures")
        .and_then(|v| v.as_object());
    let Some(stored_map) = stored_map else { return };
    let live = parse_shortcut_signatures(repo_root, current_dir);

    let stored_keys: BTreeSet<String> = stored_map.keys().cloned().collect();
    let live_keys: BTreeSet<String> = live.keys().cloned().collect();

    // Changed (same key, different signature).
    for key in stored_keys.intersection(&live_keys) {
        let stored = stored_map.get(key).and_then(|v| v.as_str()).unwrap_or("");
        let live_sig = live.get(key).map(|s| s.as_str()).unwrap_or("");
        if stored != live_sig {
            out.push(StructuralWarning {
                kind: "SIGNATURE_DRIFT".to_string(),
                message: format!(
                    "SIGNATURE_DRIFT: {key}: manifest='{stored}' vs live='{live_sig}'"
                ),
                affected_pack_files: vec![
                    "20_CODE_MAP.md".to_string(),
                    "search_scope.json".to_string(),
                ],
            });
        }
    }
    // Removed (possibly renamed — the old name is gone).
    for key in stored_keys.difference(&live_keys) {
        out.push(StructuralWarning {
            kind: "SIGNATURE_DRIFT".to_string(),
            message: format!(
                "SIGNATURE_DRIFT: {key} present in manifest but missing from source (rename or deletion)"
            ),
            affected_pack_files: vec![
                "20_CODE_MAP.md".to_string(),
                "search_scope.json".to_string(),
            ],
        });
    }
    // Added (possibly renamed — a new name appeared).
    for key in live_keys.difference(&stored_keys) {
        out.push(StructuralWarning {
            kind: "SIGNATURE_DRIFT".to_string(),
            message: format!(
                "SIGNATURE_DRIFT: {key} present in source but not in manifest (new function or rename)"
            ),
            affected_pack_files: vec![
                "20_CODE_MAP.md".to_string(),
                "search_scope.json".to_string(),
            ],
        });
    }
}

/// P2 check 4: re-hash dependency files and compare to manifest. Points the
/// reviewer at `40_OPERATIONS_AND_RELEASE.md` which typically owns the deps
/// narrative.
fn check_dependencies_drift(
    manifest: &Value,
    repo_root: &Path,
    out: &mut Vec<StructuralWarning>,
) {
    let stored = manifest
        .get("dependencies_snapshot")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    if stored.is_empty() {
        return;
    }
    let live = compute_dependencies_snapshot(repo_root);
    let mut keys: BTreeSet<String> = BTreeSet::new();
    for k in stored.keys() {
        keys.insert(k.clone());
    }
    for k in live.keys() {
        keys.insert(k.clone());
    }
    for key in keys {
        let stored_hash = stored.get(&key).and_then(|v| v.as_str()).unwrap_or("");
        let live_hash = live.get(&key).map(|s| s.as_str()).unwrap_or("");
        if stored_hash != live_hash {
            out.push(StructuralWarning {
                kind: "DEPENDENCIES_DRIFT".to_string(),
                message: format!(
                    "dependencies_snapshot drift for '{key}': manifest hash differs from on-disk file; review {file}",
                    file = "40_OPERATIONS_AND_RELEASE.md"
                ),
                affected_pack_files: vec!["40_OPERATIONS_AND_RELEASE.md".to_string()],
            });
        }
    }
}

/// P2 — run every structural check and return their collected warnings. Pure
/// function over manifest JSON + filesystem. Missing pack configs cause
/// individual checks to no-op; the caller treats warnings as non-fatal.
fn run_structural_checks(
    manifest: &Value,
    repo_root: &Path,
    current_dir: &Path,
) -> Vec<StructuralWarning> {
    let mut out: Vec<StructuralWarning> = Vec::new();
    check_template_markers(current_dir, &mut out);
    check_contract_files_exist(repo_root, current_dir, &mut out);
    check_verification_shortcuts_look_for(repo_root, current_dir, &mut out);
    check_routing_files(repo_root, &mut out);
    check_family_counts_drift(manifest, repo_root, current_dir, &mut out);
    check_declared_counts_drift(manifest, repo_root, current_dir, &mut out);
    check_shortcut_signatures_drift(manifest, repo_root, current_dir, &mut out);
    check_dependencies_drift(manifest, repo_root, &mut out);
    check_verified_acceptance_anchors(repo_root, current_dir, &mut out);
    out
}

// ============================================================================
// P12 / F41 — Verified acceptance tests
//
// `acceptance_tests.md` gains an optional `verified: true|false` flag and an
// `anchors: [{file, line, line_contains}]` list per test. When a test is
// `verified: true`, each anchor's `line_contains` must literally appear at the
// named line in the named file (±3 lines tolerance). A mismatch is a hard
// structural warning — the test can only claim "verified" if the pointer
// still resolves.
//
// Schema notes:
// - `anchors` may be specified as `anchors: [file:line: substring]` bullet
//   form (compact, human-friendly) OR as a YAML-lite block:
//       anchors:
//         - file: src/lib.rs
//           line: 42
//           line_contains: "fn compute"
// - The compact form is what the team_skills fixtures use and is preserved
//   across seals; we parse it as `<path>:<line>: <substring>` (first colon
//   ends the path, second colon ends the line number).
// - Fewer than 2 of N tests verified → emit a non-fatal `warning` (warn
//   kind `VERIFIED_COUNT_LOW`) so pack authors see the signal without the
//   pack being blocked from shipping.
// ============================================================================

/// One parsed acceptance-test record. `None` variants mean the block was
/// parsed but the author omitted the field.
#[derive(Debug, Clone)]
struct ParsedAcceptanceTest {
    id: String,
    verified: bool,
    anchors: Vec<ParsedAnchor>,
}

#[derive(Debug, Clone)]
struct ParsedAnchor {
    file: String,
    line: usize,
    line_contains: String,
}

/// Parse `acceptance_tests.md` into a structured list of tests. Skips blocks
/// without an `### test:` header (or alt spellings); gracefully returns an
/// empty vec when the file is missing, empty, or unparseable — callers treat
/// that as "no verified-anchor contract to enforce".
fn parse_acceptance_tests_with_anchors(current_dir: &Path) -> Vec<ParsedAcceptanceTest> {
    let path = current_dir.join("acceptance_tests.md");
    let Ok(raw) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let mut out: Vec<ParsedAcceptanceTest> = Vec::new();
    let mut current: Option<ParsedAcceptanceTest> = None;
    let mut in_anchors_block = false;
    let mut pending_anchor: Option<ParsedAnchor> = None;

    // Inline "flush the pending YAML-lite anchor into the current test" step;
    // written as a direct block rather than a closure because the borrow
    // checker dislikes re-borrowing `current` through a closure while we also
    // hold `t = current.as_mut()` in the same loop iteration.
    macro_rules! flush_pending_anchor {
        () => {{
            if let (Some(t), Some(a)) = (current.as_mut(), pending_anchor.take()) {
                if !a.file.is_empty() && !a.line_contains.is_empty() && a.line > 0 {
                    t.anchors.push(a);
                }
            }
        }};
    }

    for line in raw.lines() {
        let trimmed = line.trim();
        // New test header. Finalize any pending anchor + test first.
        if let Some(rest) = trimmed
            .strip_prefix("### test:")
            .or_else(|| trimmed.strip_prefix("### Test:"))
            .or_else(|| trimmed.strip_prefix("###test:"))
        {
            flush_pending_anchor!();
            if let Some(t) = current.take() {
                out.push(t);
            }
            current = Some(ParsedAcceptanceTest {
                id: rest.trim().to_string(),
                verified: false,
                anchors: Vec::new(),
            });
            in_anchors_block = false;
            continue;
        }

        if current.is_none() {
            continue;
        }

        // `verified: true`. Accept bullet or bare form.
        let body = trimmed.trim_start_matches(['-', ' ', '*']);
        if let Some(rest) = body
            .strip_prefix("verified:")
            .or_else(|| body.strip_prefix("verified :"))
        {
            if let Some(t) = current.as_mut() {
                t.verified = matches!(rest.trim().to_ascii_lowercase().as_str(), "true" | "yes" | "1");
            }
            continue;
        }

        // Anchors block start.
        if body.starts_with("anchors:") {
            flush_pending_anchor!();
            in_anchors_block = true;
            // Inline compact form: `anchors: file.rs:42: fn foo`
            let after = body.trim_start_matches("anchors:").trim();
            if !after.is_empty() {
                if let Some(anchor) = parse_anchor_compact(after) {
                    if let Some(t) = current.as_mut() {
                        t.anchors.push(anchor);
                    }
                }
            }
            continue;
        }

        if in_anchors_block {
            // Bullet list under `anchors:` — supports either compact or YAML-lite.
            if let Some(stripped) = trimmed.strip_prefix('-') {
                flush_pending_anchor!();
                let s = stripped.trim();
                if let Some(anchor) = parse_anchor_compact(s) {
                    if let Some(t) = current.as_mut() {
                        t.anchors.push(anchor);
                    }
                    continue;
                }
                // Treat as start of a YAML-lite block. Expect `file: X` on this line.
                if let Some(rest) = s.strip_prefix("file:") {
                    pending_anchor = Some(ParsedAnchor {
                        file: rest.trim().to_string(),
                        line: 0,
                        line_contains: String::new(),
                    });
                }
                continue;
            }
            // Continuation field for an open pending_anchor.
            if let Some(pending) = pending_anchor.as_mut() {
                if let Some(rest) = body.strip_prefix("line:") {
                    pending.line = rest.trim().parse().unwrap_or(0);
                    continue;
                }
                if let Some(rest) = body.strip_prefix("line_contains:") {
                    pending.line_contains = rest
                        .trim()
                        .trim_matches(|c: char| c == '"' || c == '\'')
                        .to_string();
                    continue;
                }
            }
            // Non-anchor content terminates the block.
            if trimmed.is_empty() || trimmed.starts_with("### ") {
                flush_pending_anchor!();
                in_anchors_block = false;
            }
        }
    }
    flush_pending_anchor!();
    if let Some(t) = current.take() {
        out.push(t);
    }
    out
}

/// Parse the compact anchor form `path:line: substring` into an anchor.
/// Returns `None` when the path, line, or substring portion is missing.
fn parse_anchor_compact(s: &str) -> Option<ParsedAnchor> {
    // First `:` ends the path (no colons in paths are supported here — good
    // enough for source files, which never have colons on Unix).
    let first = s.find(':')?;
    let (path_part, rest1) = s.split_at(first);
    let rest1 = &rest1[1..]; // drop colon
    // Second `:` ends the line number.
    let second = rest1.find(':')?;
    let (line_part, rest2) = rest1.split_at(second);
    let rest2 = &rest2[1..];
    let path = path_part.trim().to_string();
    let line: usize = line_part.trim().parse().ok()?;
    let line_contains = rest2
        .trim()
        .trim_matches(|c: char| c == '"' || c == '\'')
        .to_string();
    if path.is_empty() || line == 0 || line_contains.is_empty() {
        return None;
    }
    Some(ParsedAnchor { file: path, line, line_contains })
}

/// P12 / F41: for every acceptance test with `verified: true`, each anchor's
/// `line_contains` must literally appear at the named line (±3 lines). Emit
/// `VERIFIED_ANCHOR_MISS` when a pointer fails to resolve and
/// `VERIFIED_COUNT_LOW` when fewer than 2 tests claim `verified: true`.
fn check_verified_acceptance_anchors(
    repo_root: &Path,
    current_dir: &Path,
    out: &mut Vec<StructuralWarning>,
) {
    let tests = parse_acceptance_tests_with_anchors(current_dir);
    if tests.is_empty() {
        return;
    }
    let tolerance: isize = 3;
    let mut verified_count = 0;
    for test in &tests {
        if !test.verified {
            continue;
        }
        verified_count += 1;
        for anchor in &test.anchors {
            let path = repo_root.join(&anchor.file);
            let raw = match fs::read_to_string(&path) {
                Ok(s) => s,
                Err(_) => {
                    out.push(StructuralWarning {
                        kind: "VERIFIED_ANCHOR_MISS".to_string(),
                        message: format!(
                            "acceptance_tests.md `{}`: anchor file missing or unreadable: {}",
                            test.id, anchor.file
                        ),
                        affected_pack_files: vec!["acceptance_tests.md".to_string()],
                    });
                    continue;
                }
            };
            let lines: Vec<&str> = raw.lines().collect();
            let mid = anchor.line.saturating_sub(1) as isize;
            let lo = (mid - tolerance).max(0) as usize;
            let hi = ((mid + tolerance) as usize).min(lines.len().saturating_sub(1));
            let mut found = false;
            for idx in lo..=hi {
                if let Some(line) = lines.get(idx) {
                    if line.contains(&anchor.line_contains) {
                        found = true;
                        break;
                    }
                }
            }
            if !found {
                out.push(StructuralWarning {
                    kind: "VERIFIED_ANCHOR_MISS".to_string(),
                    message: format!(
                        "acceptance_tests.md `{}`: anchor `{}` not found within \u{00b1}3 lines of {}:{}",
                        test.id, anchor.line_contains, anchor.file, anchor.line
                    ),
                    affected_pack_files: vec!["acceptance_tests.md".to_string()],
                });
            }
        }
    }

    // Ship-quality bar: at least 2 of 4 (or more) tests should be verified.
    // We warn below that, but only when there are at least 2 tests in the
    // file — otherwise the pack just hasn't fleshed out acceptance tests yet.
    if tests.len() >= 2 && verified_count < 2 {
        out.push(StructuralWarning {
            kind: "VERIFIED_COUNT_LOW".to_string(),
            message: format!(
                "acceptance_tests.md: only {} of {} tests are `verified: true` (ship-quality bar is \u{2265}2)",
                verified_count,
                tests.len()
            ),
            affected_pack_files: vec!["acceptance_tests.md".to_string()],
        });
    }
}

/// Serialize a `StructuralWarning` list into the CI JSON array shape defined
/// by the P2 plan: `[{kind, message, affected_pack_files[]}]`. Always emits an
/// array (possibly empty) so downstream consumers can rely on the key.
fn structural_warnings_as_json(warnings: &[StructuralWarning]) -> Value {
    Value::Array(
        warnings
            .iter()
            .map(|w| {
                json!({
                    "kind": w.kind,
                    "message": w.message,
                    "affected_pack_files": w.affected_pack_files,
                })
            })
            .collect(),
    )
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

    // --- P11 / F34: manifest.schema_version enforcement ---

    #[test]
    fn schema_version_missing_passes_with_deprecation_warning() {
        let manifest = json!({ "files": [] });
        match check_schema_version(&manifest) {
            SchemaVersionCheck::Ok { warning } => {
                let msg = warning.expect("missing schema_version should produce a warning");
                assert!(
                    msg.contains("no schema_version"),
                    "warning should name the missing field, got: {msg}"
                );
            }
            SchemaVersionCheck::TooNew { .. } => {
                panic!("missing schema_version must be treated as v1, not newer-than-tool");
            }
        }
    }

    #[test]
    fn schema_version_matching_passes_clean() {
        let manifest = json!({ "schema_version": CURRENT_SCHEMA_VERSION, "files": [] });
        match check_schema_version(&manifest) {
            SchemaVersionCheck::Ok { warning } => {
                assert!(
                    warning.is_none(),
                    "matching schema_version must not emit a warning, got: {warning:?}"
                );
            }
            SchemaVersionCheck::TooNew { message } => {
                panic!("matching schema_version rejected as too-new: {message}");
            }
        }
    }

    #[test]
    fn schema_version_older_passes_with_upgrade_recommendation() {
        // This test only exercises the "older" branch when the tool has
        // advanced past v1. Until that happens, exercise it by forging a
        // version one step below the tool's current version when the tool
        // is >1; otherwise assert the current-version branch explicitly.
        if CURRENT_SCHEMA_VERSION <= 1 {
            // No "older" version is representable yet; exercise the
            // matching branch as a sanity check and return.
            let manifest = json!({ "schema_version": CURRENT_SCHEMA_VERSION, "files": [] });
            assert!(matches!(
                check_schema_version(&manifest),
                SchemaVersionCheck::Ok { warning: None }
            ));
            return;
        }
        let older = CURRENT_SCHEMA_VERSION - 1;
        let manifest = json!({ "schema_version": older, "files": [] });
        match check_schema_version(&manifest) {
            SchemaVersionCheck::Ok { warning } => {
                let msg = warning.expect("older schema_version should produce a warning");
                assert!(
                    msg.contains("Re-seal"),
                    "warning should recommend re-sealing, got: {msg}"
                );
            }
            SchemaVersionCheck::TooNew { message } => {
                panic!("older schema_version rejected as too-new: {message}");
            }
        }
    }

    #[test]
    fn schema_version_newer_fails_loudly() {
        let forged = CURRENT_SCHEMA_VERSION + 1;
        let manifest = json!({ "schema_version": forged, "files": [] });
        match check_schema_version(&manifest) {
            SchemaVersionCheck::Ok { .. } => {
                panic!(
                    "schema_version {forged} > tool {CURRENT_SCHEMA_VERSION} must fail loudly, not pass"
                );
            }
            SchemaVersionCheck::TooNew { message } => {
                assert!(
                    message.contains("Upgrade chorus"),
                    "error message should direct the user to upgrade, got: {message}"
                );
                assert!(
                    message.contains(&format!("v{forged}")),
                    "error message should name the offending version, got: {message}"
                );
            }
        }
    }

    // --- P11 / F36: seal records current chorus_version in manifest ---

    #[test]
    fn build_manifest_records_current_chorus_version() {
        let files_meta: Vec<FileMeta> = Vec::new();
        let baseline = SemanticBaseline::default();
        let bundle = build_manifest(
            "2026-04-17T00:00:00Z",
            Path::new("/tmp/unused"),
            "fixture-repo",
            "main",
            false,
            Some("abcd1234"),
            "test-seal",
            None,
            &Vec::new(),
            &files_meta,
            &baseline,
        );

        let schema = bundle
            .value
            .get("schema_version")
            .and_then(|v| v.as_u64())
            .expect("schema_version should be present and numeric");
        assert_eq!(
            schema, CURRENT_SCHEMA_VERSION,
            "sealed manifest must record the current schema version"
        );

        let chorus_version = bundle
            .value
            .get("chorus_version")
            .and_then(|v| v.as_str())
            .expect("chorus_version should be a string");
        assert_eq!(
            chorus_version,
            env!("CARGO_PKG_VERSION"),
            "chorus_version must match the package version baked into the binary"
        );

        // skill_version and verifier_sha256 must be present as additive fields.
        assert!(
            bundle.value.get("skill_version").is_some(),
            "skill_version field must be present (null is fine)"
        );
        let verifier = bundle
            .value
            .get("verifier_sha256")
            .expect("verifier_sha256 field must be present");
        assert!(
            verifier.is_string() || verifier.is_null(),
            "verifier_sha256 must be a hex string or null, got: {verifier}"
        );
    }

    // -----------------------------------------------------------------------
    // P3 — Zone map + suggest-patches tests.
    // Pure-helper tests (load_zone_map, resolve_affected_sections) do not need
    // a git repo; freshness + validation tests bootstrap one on the fly.
    // -----------------------------------------------------------------------

    /// Write a relevance.json with the given content under `repo_root`.
    fn write_relevance_json(repo_root: &Path, content: &str) {
        let dir = repo_root.join(".agent-context");
        fs::create_dir_all(&dir).expect("create .agent-context dir");
        fs::write(dir.join("relevance.json"), content).expect("write relevance.json");
    }

    /// Initialize a minimal git repo with one committed file so HEAD~1-style
    /// diffs don't trip the initial-commit guard. Returns the repo root.
    fn init_repo_with_commits(name: &str) -> PathBuf {
        let dir = test_dir(name);
        let run = |args: &[&str]| -> std::process::Output {
            Command::new("git")
                .args(args)
                .current_dir(&dir)
                .output()
                .expect("git command runs")
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "P3 Test"]);
        run(&["config", "commit.gpgsign", "false"]);
        fs::write(dir.join("README.md"), "initial\n").unwrap();
        run(&["add", "README.md"]);
        run(&["commit", "-q", "-m", "initial"]);
        // Second commit so HEAD~1 exists for diff fallback.
        fs::write(dir.join("README.md"), "initial\nv2\n").unwrap();
        run(&["add", "README.md"]);
        run(&["commit", "-q", "-m", "second"]);
        dir
    }

    /// Stage + commit a file under the repo so it becomes tracked for
    /// `git ls-files` and shows up in diffs.
    fn commit_file(repo: &Path, rel: &str, content: &str) {
        let full = repo.join(rel);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&full, content).unwrap();
        Command::new("git")
            .args(["add", rel])
            .current_dir(repo)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-q", "-m", &format!("add {rel}")])
            .current_dir(repo)
            .output()
            .unwrap();
    }

    #[test]
    fn p3_load_zone_map_reads_valid_file() {
        let dir = test_dir("p3_load_zone_map");
        write_relevance_json(
            &dir,
            r#"{
              "zones": [
                {"paths": ["study/**"], "affects": ["10_SYSTEM_OVERVIEW.md"]},
                {"paths": ["src/**"], "affects": ["20_CODE_MAP.md", "30_BEHAVIORAL_INVARIANTS.md"]}
              ]
            }"#,
        );

        let zones = load_zone_map(&dir).expect("zone map should load");
        assert_eq!(zones.len(), 2, "both zones should load");
        assert_eq!(zones[0].paths, vec!["study/**".to_string()]);
        assert_eq!(zones[0].affects, vec!["10_SYSTEM_OVERVIEW.md".to_string()]);
        assert_eq!(zones[1].paths, vec!["src/**".to_string()]);
        assert_eq!(
            zones[1].affects,
            vec![
                "20_CODE_MAP.md".to_string(),
                "30_BEHAVIORAL_INVARIANTS.md".to_string()
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn p3_load_zone_map_missing_returns_none() {
        let dir = test_dir("p3_load_zone_map_missing");
        let zones = load_zone_map(&dir);
        assert!(zones.is_none(), "no file → None");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn p3_load_zone_map_legacy_only_returns_none() {
        let dir = test_dir("p3_load_zone_map_legacy");
        // A legacy file without `zones` should yield None; the caller
        // (check_freshness_with_zones) falls back to hardcoded relevance.
        write_relevance_json(
            &dir,
            r#"{"include": ["**"], "exclude": [".git/**"]}"#,
        );
        assert!(load_zone_map(&dir).is_none(), "no zones key → None");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn p3_resolve_affected_sections_maps_changed_file_to_zone() {
        let zones = vec![
            Zone {
                paths: vec!["study/**".to_string(), "docs/methodology/**".to_string()],
                affects: vec![
                    "10_SYSTEM_OVERVIEW.md".to_string(),
                    "30_BEHAVIORAL_INVARIANTS.md".to_string(),
                ],
            },
            Zone {
                paths: vec!["src/brand_lift/**".to_string()],
                affects: vec!["20_CODE_MAP.md".to_string()],
            },
        ];
        let sections = resolve_affected_sections("study/methodology/intro.md", &zones);
        // The first zone matches; only its affects should appear (sorted/dedup).
        assert_eq!(
            sections,
            vec![
                "10_SYSTEM_OVERVIEW.md".to_string(),
                "30_BEHAVIORAL_INVARIANTS.md".to_string(),
            ]
        );

        let none = resolve_affected_sections("unrelated/thing.txt", &zones);
        assert!(none.is_empty(), "no zone matches → empty");

        let multi = resolve_affected_sections("src/brand_lift/mod.rs", &zones);
        assert_eq!(multi, vec!["20_CODE_MAP.md".to_string()]);
    }

    #[test]
    fn p3_check_freshness_with_zones_returns_affected_sections() {
        // Full end-to-end: a real git repo with a study/ file committed, a
        // zone map referencing study/**, and a subsequent commit that edits
        // the study/ file. The zone-aware freshness check must surface the
        // affected_sections list.
        let repo = init_repo_with_commits("p3_freshness_zones");
        // Commit a file in study/ so the zone path resolves.
        commit_file(&repo, "study/intro.md", "hello\n");
        // Write zone map after the commit so load_zone_map sees it.
        write_relevance_json(
            &repo,
            r#"{
              "zones": [
                {"paths": ["study/**"], "affects": ["10_SYSTEM_OVERVIEW.md"]}
              ]
            }"#,
        );
        // Make a change that touches study/ (relative to HEAD~1).
        commit_file(&repo, "study/intro.md", "hello world\n");

        let result = check_freshness_with_zones("origin/nonexistent", &repo)
            .expect("freshness should succeed");
        assert_eq!(result.status, "warn", "study/ change without pack update → warn");
        assert!(
            result.changed_files.iter().any(|f| f == "study/intro.md"),
            "study/intro.md must appear in changed_files, got {:?}",
            result.changed_files
        );
        assert!(
            result.affected_sections.iter().any(|s| s == "10_SYSTEM_OVERVIEW.md"),
            "affected_sections must include 10_SYSTEM_OVERVIEW.md, got {:?}",
            result.affected_sections
        );
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn p3_zone_referencing_zero_files_fails_verify() {
        // A zone whose `paths` match no tracked file is an authoring bug; the
        // freshness check must surface this as a `fail` so verify fails loudly.
        let repo = init_repo_with_commits("p3_zone_zero_files");
        // Zone points at a dir that does not exist in the repo.
        write_relevance_json(
            &repo,
            r#"{
              "zones": [
                {"paths": ["study/**"], "affects": ["10_SYSTEM_OVERVIEW.md"]}
              ]
            }"#,
        );
        let result = check_freshness_with_zones("origin/nonexistent", &repo)
            .expect("freshness should return a result, not an error");
        assert_eq!(
            result.status, "fail",
            "zone resolving to zero tracked files must fail"
        );
        let reason = result.skipped_reason.unwrap_or_default();
        assert!(
            reason.contains("study/**") && reason.contains("zero tracked files"),
            "fail reason must name the offending zone path, got: {reason}"
        );
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn p3_fallback_when_relevance_json_absent() {
        // No relevance.json → fall through to Pass-0 `is_context_relevant`
        // hardcoded rules; affected_sections remains empty.
        let repo = init_repo_with_commits("p3_fallback_no_relevance");
        // Commit a "context-relevant" file per Pass-0 hardcoded rules
        // (cli/src/** is explicitly included in is_context_relevant).
        commit_file(&repo, "cli/src/foo.rs", "fn a() {}\n");
        // Change it again to produce a HEAD~1 diff hit.
        commit_file(&repo, "cli/src/foo.rs", "fn a() -> i32 { 0 }\n");

        let result = check_freshness_with_zones("origin/nonexistent", &repo)
            .expect("freshness ok");
        assert_eq!(
            result.status, "warn",
            "Pass-0 fallback must treat cli/src changes as context-relevant"
        );
        assert!(
            result.changed_files.iter().any(|f| f == "cli/src/foo.rs"),
            "cli/src/foo.rs must be in changed_files, got {:?}",
            result.changed_files
        );
        assert!(
            result.affected_sections.is_empty(),
            "fallback mode → affected_sections stays empty"
        );
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn p3_suggest_patches_emits_expected_shape() {
        // End-to-end: wire suggest_patches() into a repo with a zone map and
        // a changed file; assert the JSON payload has all four required fields
        // and that baseline_drift is [] (reserved for P2).
        let repo = init_repo_with_commits("p3_suggest_patches_shape");
        commit_file(&repo, "study/intro.md", "hello\n");
        write_relevance_json(
            &repo,
            r#"{
              "zones": [
                {"paths": ["study/**"], "affects": ["10_SYSTEM_OVERVIEW.md"]}
              ]
            }"#,
        );
        commit_file(&repo, "study/intro.md", "hello world\n");

        let payload = suggest_patches("origin/nonexistent", &repo)
            .expect("suggest_patches should succeed");
        let obj = payload.as_object().expect("payload must be an object");
        for key in [
            "changed_files",
            "pack_sections_to_update",
            "diff_excerpt",
            "baseline_drift",
        ] {
            assert!(obj.contains_key(key), "payload missing '{key}'");
        }
        // baseline_drift is reserved for P2 → always empty array.
        assert_eq!(
            obj["baseline_drift"].as_array().unwrap().len(),
            0,
            "baseline_drift must be [] until P2 wires drift detection"
        );
        // pack_sections_to_update must include the zone's affects.
        let sections: Vec<String> = obj["pack_sections_to_update"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        assert!(
            sections.iter().any(|s| s == "10_SYSTEM_OVERVIEW.md"),
            "pack_sections_to_update must contain 10_SYSTEM_OVERVIEW.md, got {:?}",
            sections
        );
        // diff_excerpt must be a string (possibly empty).
        assert!(
            obj["diff_excerpt"].is_string(),
            "diff_excerpt must be a string"
        );
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn p3_default_relevance_json_contains_zones() {
        // Regression guard: the shipped default must carry a zones[] array so
        // new repos get zone-aware freshness out of the box.
        let json_str = default_relevance_json();
        let parsed: Value =
            serde_json::from_str(&json_str).expect("default relevance.json must parse");
        let zones = parsed
            .get("zones")
            .and_then(|v| v.as_array())
            .expect("zones[] must be present");
        assert!(!zones.is_empty(), "default must ship at least one zone");
        // Legacy include/exclude stays for back-compat.
        assert!(parsed.get("include").is_some(), "include[] preserved");
        assert!(parsed.get("exclude").is_some(), "exclude[] preserved");
    }


    // --- P1 tests (restored post-integration) ---

    fn init_git(root: &Path) {
        let _ = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(root)
            .output();
        let _ = std::process::Command::new("git")
            .args(["config", "user.email", "p1@test"])
            .current_dir(root)
            .output();
        let _ = std::process::Command::new("git")
            .args(["config", "user.name", "P1 Tester"])
            .current_dir(root)
            .output();
    }

    #[test]
    fn extract_declared_counts_catches_prose_numbers_and_respects_ignore() {
        let dir = test_dir("p1_declared_counts");
        let current = dir.join(".agent-context").join("current");
        fs::create_dir_all(&current).unwrap();
        let body = "\
# Overview\n\
\n\
The repo ships 6 files and 12 scripts.\n\
\n\
<!-- count-claim: ignore -->\n\
Historic note: we once had 42 tests.\n\
<!-- count-claim: end -->\n\
\n\
After cleanup there are 32 tests today.\n\
";
        fs::write(current.join("10_SYSTEM_OVERVIEW.md"), body).unwrap();

        let out = extract_declared_counts(&current);
        // Collect (noun, count) pairs for easier assertions.
        let pairs: Vec<(String, u64)> = out
            .iter()
            .map(|v| {
                (
                    v.get("noun").and_then(|x| x.as_str()).unwrap().to_string(),
                    v.get("count").and_then(|x| x.as_u64()).unwrap(),
                )
            })
            .collect();
        assert!(
            pairs.contains(&("files".to_string(), 6)),
            "expected '6 files' claim in {pairs:?}"
        );
        assert!(
            pairs.contains(&("scripts".to_string(), 12)),
            "expected '12 scripts' claim in {pairs:?}"
        );
        assert!(
            pairs.contains(&("tests".to_string(), 32)),
            "expected '32 tests' claim in {pairs:?}"
        );
        assert!(
            !pairs.contains(&("tests".to_string(), 42)),
            "'42 tests' inside <!-- count-claim: ignore --> must be skipped, got {pairs:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_family_counts_empty_when_config_absent() {
        let dir = test_dir("p1_family_counts_absent");
        init_git(&dir);
        let current = dir.join(".agent-context").join("current");
        fs::create_dir_all(&current).unwrap();

        let counts = resolve_family_counts(&dir, &current);
        assert!(
            counts.is_empty(),
            "expected empty family_counts when configs absent, got {counts:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn post_commit_reconcile_updates_post_commit_sha() {
        let dir = test_dir("p1_post_commit");
        init_git(&dir);
        // Seed a single commit so HEAD resolves.
        fs::write(dir.join("README.md"), "hi\n").unwrap();
        let _ = std::process::Command::new("git")
            .args(["add", "README.md"])
            .current_dir(&dir)
            .output();
        let _ = std::process::Command::new("git")
            .args(["commit", "-m", "seed", "--quiet"])
            .current_dir(&dir)
            .output();

        // Minimal manifest that we expect the reconcile to stamp.
        let current = dir.join(".agent-context").join("current");
        fs::create_dir_all(&current).unwrap();
        let initial = json!({
            "schema_version": CURRENT_SCHEMA_VERSION,
            "head_sha": "deadbeef",
            "head_sha_at_seal": "deadbeef",
            "post_commit_sha": null,
            "files": [],
        });
        fs::write(
            current.join("manifest.json"),
            format!("{}\n", serde_json::to_string_pretty(&initial).unwrap()),
        )
        .unwrap();

        post_commit_reconcile(Some(dir.to_str().unwrap()), None).expect("reconcile must succeed");

        let raw = fs::read_to_string(current.join("manifest.json")).unwrap();
        let manifest: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let post_sha = manifest
            .get("post_commit_sha")
            .and_then(|v| v.as_str())
            .expect("post_commit_sha must be a string after reconcile");
        // SHA-1 hex = 40 chars; accept any non-empty string from git rev-parse.
        assert!(!post_sha.is_empty());
        // Invariant: seal-time head_sha_at_seal must be preserved.
        assert_eq!(
            manifest.get("head_sha_at_seal").and_then(|v| v.as_str()),
            Some("deadbeef"),
            "reconcile must never mutate head_sha_at_seal"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn compute_dependencies_snapshot_hashes_pyproject_and_npm() {
        let dir = test_dir("p1_deps");
        fs::write(dir.join("pyproject.toml"), "[tool.poetry]\nname=\"demo\"\n").unwrap();
        fs::write(dir.join("package.json"), "{\"name\":\"demo\"}\n").unwrap();

        let snap = compute_dependencies_snapshot(&dir);
        assert!(
            snap.get("pyproject").map(|s| s.len() == 64).unwrap_or(false),
            "pyproject hash should be 64-char hex, got {snap:?}"
        );
        assert!(
            snap.get("npm").map(|s| s.len() == 64).unwrap_or(false),
            "npm hash should be 64-char hex, got {snap:?}"
        );
        assert!(
            !snap.contains_key("cargo"),
            "cargo must be absent when Cargo.toml missing, got {snap:?}"
        );

        // Sanity: value changes when the file changes.
        fs::write(dir.join("pyproject.toml"), "[tool.poetry]\nname=\"demo2\"\n").unwrap();
        let snap2 = compute_dependencies_snapshot(&dir);
        assert_ne!(
            snap.get("pyproject"),
            snap2.get("pyproject"),
            "pyproject hash must change when the file content changes"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_manifest_emits_all_p1_fields() {
        let files_meta: Vec<FileMeta> = Vec::new();
        let mut baseline = SemanticBaseline::default();
        baseline
            .family_counts
            .insert("scripts/run_*.py".to_string(), 12);
        baseline.declared_counts.push(json!({
            "noun": "files",
            "count": 6,
            "file": "10_SYSTEM_OVERVIEW.md",
            "line": 3,
        }));
        baseline
            .shortcut_signatures
            .insert("calc.py::compute_lift".to_string(), "def compute_lift(data)".to_string());
        baseline
            .dependencies_snapshot
            .insert("pyproject".to_string(), "a".repeat(64));

        let bundle = build_manifest(
            "2026-04-17T00:00:00Z",
            Path::new("/tmp/unused"),
            "fixture-repo",
            "main",
            false,
            Some("abcd1234"),
            "test-seal",
            None,
            &Vec::new(),
            &files_meta,
            &baseline,
        );

        // All four semantic fields must be present with the right shape.
        assert_eq!(
            bundle
                .value
                .get("family_counts")
                .and_then(|v| v.get("scripts/run_*.py"))
                .and_then(|v| v.as_u64()),
            Some(12),
            "family_counts missing the expected entry"
        );
        let declared = bundle
            .value
            .get("declared_counts")
            .and_then(|v| v.as_array())
            .expect("declared_counts must be an array");
        assert_eq!(declared.len(), 1, "declared_counts should contain one entry");
        assert!(
            bundle
                .value
                .get("shortcut_signatures")
                .and_then(|v| v.get("calc.py::compute_lift"))
                .and_then(|v| v.as_str())
                .map(|s| s.contains("compute_lift"))
                .unwrap_or(false),
            "shortcut_signatures missing the expected entry"
        );
        assert!(
            bundle
                .value
                .get("dependencies_snapshot")
                .and_then(|v| v.get("pyproject"))
                .and_then(|v| v.as_str())
                .map(|s| s.len() == 64)
                .unwrap_or(false),
            "dependencies_snapshot missing pyproject entry"
        );

        // head_sha_at_seal is the new canonical field; head_sha remains populated.
        assert_eq!(
            bundle
                .value
                .get("head_sha_at_seal")
                .and_then(|v| v.as_str()),
            Some("abcd1234"),
            "head_sha_at_seal should mirror the seal-time HEAD"
        );
        assert_eq!(
            bundle.value.get("head_sha").and_then(|v| v.as_str()),
            Some("abcd1234"),
            "head_sha must stay populated for back-compat"
        );
        assert!(
            bundle.value.get("post_commit_sha").map(|v| v.is_null()).unwrap_or(false),
            "post_commit_sha must be null at seal time"
        );
    }

    #[test]
    fn resolve_family_counts_resolves_run_scripts_glob() {
        let dir = test_dir("p1_family_counts");
        init_git(&dir);
        let scripts = dir.join("scripts");
        fs::create_dir_all(&scripts).unwrap();
        for i in 0..12 {
            fs::write(scripts.join(format!("run_{i}.py")), "# runner\n").unwrap();
        }
        let current = dir.join(".agent-context").join("current");
        fs::create_dir_all(&current).unwrap();
        // Author a minimal completeness_contract.json that declares the glob.
        fs::write(
            current.join("completeness_contract.json"),
            r#"{
              "task_families": {
                "lookup": {"required_file_families": ["scripts/run_*.py"]}
              }
            }"#,
        )
        .unwrap();

        let counts = resolve_family_counts(&dir, &current);
        assert_eq!(
            counts.get("scripts/run_*.py").copied(),
            Some(12),
            "expected 12 scripts under scripts/run_*.py, got {counts:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_shortcut_signatures_for_python_and_rust_and_ts() {
        let dir = test_dir("p1_sigs");
        init_git(&dir);
        // Python
        fs::write(
            dir.join("calc.py"),
            "def compute_lift(data, *, alpha=0.05):\n    return 1\n\n\
             async def fetch(url: str) -> str:\n    return url\n",
        )
        .unwrap();
        // Rust
        fs::write(
            dir.join("lib.rs"),
            "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n\n\
             fn internal() {\n}\n",
        )
        .unwrap();
        // TypeScript
        fs::write(
            dir.join("helpers.ts"),
            "export function shout(msg: string): string {\n  return msg.toUpperCase();\n}\n\n\
             export const greet = (name: string): string => `hi ${name}`;\n",
        )
        .unwrap();

        let py_sigs = parse_shortcut_signatures_for_file(&dir.join("calc.py"), "calc.py");
        assert!(
            py_sigs
                .get("calc.py::compute_lift")
                .map(|s| s.contains("compute_lift"))
                .unwrap_or(false),
            "expected python compute_lift signature, got {py_sigs:?}"
        );
        assert!(
            py_sigs.contains_key("calc.py::fetch"),
            "expected python fetch signature, got {py_sigs:?}"
        );

        let rs_sigs = parse_shortcut_signatures_for_file(&dir.join("lib.rs"), "lib.rs");
        assert!(
            rs_sigs
                .get("lib.rs::add")
                .map(|s| s.contains("-> i32"))
                .unwrap_or(false),
            "expected rust add signature with return type, got {rs_sigs:?}"
        );
        assert!(
            rs_sigs.contains_key("lib.rs::internal"),
            "expected rust internal signature, got {rs_sigs:?}"
        );

        let ts_sigs = parse_shortcut_signatures_for_file(&dir.join("helpers.ts"), "helpers.ts");
        assert!(
            ts_sigs.contains_key("helpers.ts::shout"),
            "expected ts shout signature, got {ts_sigs:?}"
        );
        assert!(
            ts_sigs.contains_key("helpers.ts::greet"),
            "expected ts greet arrow signature, got {ts_sigs:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    // --- P2 structural verifier tests ---

    /// Build a minimal `.agent-context/current/` at `<dir>` and return the
    /// `current_dir` PathBuf. Used by several P2 tests.
    fn p2_init_pack(dir: &Path) -> PathBuf {
        let current = dir.join(".agent-context").join("current");
        fs::create_dir_all(&current).unwrap();
        current
    }

    #[test]
    fn p2_template_marker_in_pack_json_is_flagged() {
        let dir = test_dir("p2_template_markers");
        let current = p2_init_pack(&dir);
        fs::write(
            current.join("routes.json"),
            r#"{"task_routes": {"lookup": {"pack_read_order": ["{name}/00_START_HERE.md"]}}}"#,
        )
        .unwrap();

        let mut out: Vec<StructuralWarning> = Vec::new();
        check_template_markers(&current, &mut out);

        assert!(
            out.iter().any(|w| w.kind == "TEMPLATE_MARKER"
                && w.message.contains("routes.json")
                && w.message.contains("{name}")),
            "expected TEMPLATE_MARKER warning naming routes.json + {{name}}, got {out:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn p2_contract_glob_existence_flags_empty_matches() {
        let dir = test_dir("p2_contract_glob");
        init_git(&dir);
        let current = p2_init_pack(&dir);
        fs::write(
            current.join("completeness_contract.json"),
            r#"{
              "task_families": {
                "lookup": {
                  "contractually_required_files": ["nonexistent/plan.md"],
                  "required_file_families": ["scripts/run_*.py"]
                }
              }
            }"#,
        )
        .unwrap();

        let mut out: Vec<StructuralWarning> = Vec::new();
        check_contract_files_exist(&dir, &current, &mut out);

        // Both checks must fire: missing file AND empty glob.
        assert!(
            out.iter().any(|w| w.kind == "CONTRACT_REQUIRED_FILE_MISSING"
                && w.message.contains("nonexistent/plan.md")),
            "expected CONTRACT_REQUIRED_FILE_MISSING naming nonexistent/plan.md, got {out:?}"
        );
        assert!(
            out.iter().any(|w| w.kind == "CONTRACT_GLOB_MISS"
                && w.message.contains("scripts/run_*.py")),
            "expected CONTRACT_GLOB_MISS naming scripts/run_*.py, got {out:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn p2_look_for_substring_check_flags_missing_string() {
        let dir = test_dir("p2_look_for");
        init_git(&dir);
        let current = p2_init_pack(&dir);
        // Repo file that does NOT contain the look_for string.
        fs::write(
            dir.join("calc.py"),
            "def compute_lift(data):\n    return 1\n",
        )
        .unwrap();
        // team_skills-shape verification_shortcuts (array of {file, look_for}).
        fs::write(
            current.join("search_scope.json"),
            r#"{
              "task_families": {
                "lookup": {
                  "verification_shortcuts": [
                    {"file": "calc.py", "look_for": "MIN_CELL_SIZE = 30"}
                  ]
                }
              }
            }"#,
        )
        .unwrap();

        let mut out: Vec<StructuralWarning> = Vec::new();
        check_verification_shortcuts_look_for(&dir, &current, &mut out);

        assert!(
            out.iter().any(|w| w.kind == "LOOK_FOR_MISSING"
                && w.message.contains("calc.py")
                && w.message.contains("MIN_CELL_SIZE = 30")),
            "expected LOOK_FOR_MISSING naming calc.py + MIN_CELL_SIZE, got {out:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    // --- P12 / F40 — comment-aware look_for tests ---

    /// When the only occurrence of the look_for substring is inside a Python
    /// line comment, the comment-strip pass turns "match" into "no match"
    /// and the warning must explicitly name the comment-only case.
    #[test]
    fn p12_look_for_rejects_comment_only_match_python() {
        let dir = test_dir("p12_look_for_comment_py");
        init_git(&dir);
        let current = p2_init_pack(&dir);
        // Python file where the string appears ONLY in a comment.
        fs::write(
            dir.join("calc.py"),
            "# MIN_CELL_SIZE = 30 (stale doc comment)\n\
             def compute_lift(data):\n    return 1\n",
        )
        .unwrap();
        fs::write(
            current.join("search_scope.json"),
            r#"{
              "task_families": {
                "lookup": {
                  "verification_shortcuts": [
                    {"file": "calc.py", "look_for": "MIN_CELL_SIZE = 30"}
                  ]
                }
              }
            }"#,
        )
        .unwrap();

        let mut out: Vec<StructuralWarning> = Vec::new();
        check_verification_shortcuts_look_for(&dir, &current, &mut out);

        assert!(
            out.iter().any(|w| w.kind == "LOOK_FOR_MISSING"
                && w.message.contains("matches only comments")
                && w.message.contains("calc.py")),
            "expected comment-only LOOK_FOR_MISSING, got {out:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// Rust variant — `//` line comments and `/* */` blocks must be stripped
    /// so a look_for that only appears inside them is flagged, matching the
    /// Python behavior.
    #[test]
    fn p12_look_for_rejects_comment_only_match_rust() {
        let dir = test_dir("p12_look_for_comment_rs");
        init_git(&dir);
        let current = p2_init_pack(&dir);
        fs::write(
            dir.join("lib.rs"),
            "// MIN_CELL_SIZE = 30 is a stale doc hint\n\
             /* block: MIN_CELL_SIZE = 30 */\n\
             pub fn add(a: i32, b: i32) -> i32 { a + b }\n",
        )
        .unwrap();
        fs::write(
            current.join("search_scope.json"),
            r#"{
              "task_families": {
                "lookup": {
                  "verification_shortcuts": [
                    {"file": "lib.rs", "look_for": "MIN_CELL_SIZE = 30"}
                  ]
                }
              }
            }"#,
        )
        .unwrap();

        let mut out: Vec<StructuralWarning> = Vec::new();
        check_verification_shortcuts_look_for(&dir, &current, &mut out);

        assert!(
            out.iter().any(|w| w.kind == "LOOK_FOR_MISSING"
                && w.message.contains("matches only comments")
                && w.message.contains("lib.rs")),
            "expected comment-only LOOK_FOR_MISSING for rust, got {out:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// TypeScript variant using `//` line comments.
    #[test]
    fn p12_look_for_rejects_comment_only_match_ts() {
        let dir = test_dir("p12_look_for_comment_ts");
        init_git(&dir);
        let current = p2_init_pack(&dir);
        fs::write(
            dir.join("svc.ts"),
            "// MIN_CELL_SIZE = 30 (stale JSDoc hint)\n\
             export function svc(): number { return 1; }\n",
        )
        .unwrap();
        fs::write(
            current.join("search_scope.json"),
            r#"{
              "task_families": {
                "lookup": {
                  "verification_shortcuts": [
                    {"file": "svc.ts", "look_for": "MIN_CELL_SIZE = 30"}
                  ]
                }
              }
            }"#,
        )
        .unwrap();

        let mut out: Vec<StructuralWarning> = Vec::new();
        check_verification_shortcuts_look_for(&dir, &current, &mut out);

        assert!(
            out.iter().any(|w| w.kind == "LOOK_FOR_MISSING"
                && w.message.contains("matches only comments")
                && w.message.contains("svc.ts")),
            "expected comment-only LOOK_FOR_MISSING for ts, got {out:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// When the look_for substring is present in real code (not just in a
    /// comment) after comment-stripping, no warning should fire — the
    /// comment-strip must not drop legitimate matches.
    #[test]
    fn p12_look_for_accepts_match_in_real_code() {
        let dir = test_dir("p12_look_for_real_code");
        init_git(&dir);
        let current = p2_init_pack(&dir);
        fs::write(
            dir.join("calc.py"),
            "MIN_CELL_SIZE = 30\n\
             def compute_lift(data):\n    return MIN_CELL_SIZE\n",
        )
        .unwrap();
        fs::write(
            current.join("search_scope.json"),
            r#"{
              "task_families": {
                "lookup": {
                  "verification_shortcuts": [
                    {"file": "calc.py", "look_for": "MIN_CELL_SIZE = 30"}
                  ]
                }
              }
            }"#,
        )
        .unwrap();

        let mut out: Vec<StructuralWarning> = Vec::new();
        check_verification_shortcuts_look_for(&dir, &current, &mut out);

        assert!(
            out.is_empty(),
            "no warning expected when look_for appears in real code, got {out:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// `look_for_regex` takes precedence over `look_for` when both are
    /// present, and uses regex semantics against the comment-stripped source.
    #[test]
    fn p12_look_for_regex_matches_after_comment_strip() {
        let dir = test_dir("p12_look_for_regex");
        init_git(&dir);
        let current = p2_init_pack(&dir);
        fs::write(
            dir.join("calc.py"),
            "# MIN_CELL_SIZE = 30\n\
             MIN_CELL_SIZE = 42\n",
        )
        .unwrap();
        fs::write(
            current.join("search_scope.json"),
            r#"{
              "task_families": {
                "lookup": {
                  "verification_shortcuts": [
                    {"file": "calc.py", "look_for_regex": "MIN_CELL_SIZE = \\d+"}
                  ]
                }
              }
            }"#,
        )
        .unwrap();

        let mut out: Vec<StructuralWarning> = Vec::new();
        check_verification_shortcuts_look_for(&dir, &current, &mut out);

        assert!(
            out.is_empty(),
            "look_for_regex should match MIN_CELL_SIZE = 42 after comment-strip, got {out:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// When the regex does not match, the warning must name the regex field
    /// so authors can tell whether to update the pattern or the pack pointer.
    #[test]
    fn p12_look_for_regex_reports_no_match() {
        let dir = test_dir("p12_look_for_regex_miss");
        init_git(&dir);
        let current = p2_init_pack(&dir);
        fs::write(
            dir.join("calc.py"),
            "def compute_lift(data):\n    return 1\n",
        )
        .unwrap();
        fs::write(
            current.join("search_scope.json"),
            r#"{
              "task_families": {
                "lookup": {
                  "verification_shortcuts": [
                    {"file": "calc.py", "look_for_regex": "MAX_BATCH_SIZE = \\d+"}
                  ]
                }
              }
            }"#,
        )
        .unwrap();

        let mut out: Vec<StructuralWarning> = Vec::new();
        check_verification_shortcuts_look_for(&dir, &current, &mut out);

        assert!(
            out.iter().any(|w| w.kind == "LOOK_FOR_MISSING"
                && w.message.contains("look_for_regex did not match")),
            "expected look_for_regex no-match warning, got {out:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// Direct unit test for the comment-strip helper — covers docstrings
    /// (triple-quoted) and line comments for Python.
    #[test]
    fn p12_strip_python_comments_removes_docstrings() {
        let src = "\"\"\"module docstring with MIN_CELL_SIZE = 30\"\"\"\n\
                   # MIN_CELL_SIZE = 30 also here\n\
                   def f():\n    pass\n";
        let stripped = strip_source_comments(src, "py");
        assert!(
            !stripped.contains("MIN_CELL_SIZE = 30"),
            "docstring and comment must be stripped, got {stripped:?}"
        );
        assert!(stripped.contains("def f()"), "code must remain after strip");
    }

    /// Unit test for the C-style strip covering both `//` and `/* */` cases.
    #[test]
    fn p12_strip_c_style_comments_removes_both_forms() {
        let src = "// comment with TOKEN\n/* block TOKEN */\nfn real() {}\n";
        let stripped = strip_source_comments(src, "rs");
        assert!(
            !stripped.contains("TOKEN"),
            "both comment forms must be stripped, got {stripped:?}"
        );
        assert!(stripped.contains("fn real()"));
    }

    // --- P12 / F41 — verified acceptance tests + anchor tolerance ---

    /// A `verified: true` test whose anchor points at an exact line that
    /// contains the declared substring must pass the check.
    #[test]
    fn p12_verified_acceptance_anchor_passes_when_line_matches() {
        let dir = test_dir("p12_verified_anchor_ok");
        let current = p2_init_pack(&dir);
        fs::write(
            dir.join("lib.rs"),
            "line 1\nline 2\npub fn compute(x: i32) -> i32 { x }\nline 4\n",
        )
        .unwrap();
        fs::write(
            current.join("acceptance_tests.md"),
            "### test: smoke\n\
             - verified: true\n\
             - anchors: lib.rs:3: pub fn compute\n\n\
             ### test: other\n\
             - verified: true\n\
             - anchors: lib.rs:3: pub fn compute\n",
        )
        .unwrap();

        let mut out: Vec<StructuralWarning> = Vec::new();
        check_verified_acceptance_anchors(&dir, &current, &mut out);
        assert!(
            out.is_empty(),
            "anchor at the exact line must pass, got {out:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// Anchor tolerance is ±3 lines — a line_contains string found 2 lines
    /// below the declared line still passes.
    #[test]
    fn p12_verified_acceptance_anchor_tolerates_small_drift() {
        let dir = test_dir("p12_verified_anchor_tolerance");
        let current = p2_init_pack(&dir);
        fs::write(
            dir.join("lib.rs"),
            "l1\nl2\nl3\nl4\npub fn compute(x: i32) -> i32 { x }\nl6\n",
        )
        .unwrap();
        // Declared line is 3, actual is 5 → within ±3 tolerance.
        fs::write(
            current.join("acceptance_tests.md"),
            "### test: smoke\n\
             - verified: true\n\
             - anchors: lib.rs:3: pub fn compute\n\n\
             ### test: other\n\
             - verified: true\n\
             - anchors: lib.rs:3: pub fn compute\n",
        )
        .unwrap();

        let mut out: Vec<StructuralWarning> = Vec::new();
        check_verified_acceptance_anchors(&dir, &current, &mut out);
        assert!(
            out.is_empty(),
            "2-line drift must be tolerated, got {out:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// Anchor outside the ±3 tolerance window must fail with
    /// VERIFIED_ANCHOR_MISS naming the line_contains substring.
    #[test]
    fn p12_verified_acceptance_anchor_fails_when_drift_exceeds_tolerance() {
        let dir = test_dir("p12_verified_anchor_miss");
        let current = p2_init_pack(&dir);
        fs::write(
            dir.join("lib.rs"),
            "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\npub fn compute(x: i32) -> i32 { x }\n",
        )
        .unwrap();
        // Declared at line 2, actual at line 9 → exceeds ±3.
        fs::write(
            current.join("acceptance_tests.md"),
            "### test: smoke\n\
             - verified: true\n\
             - anchors: lib.rs:2: pub fn compute\n\n\
             ### test: other\n\
             - verified: true\n\
             - anchors: lib.rs:2: pub fn compute\n",
        )
        .unwrap();

        let mut out: Vec<StructuralWarning> = Vec::new();
        check_verified_acceptance_anchors(&dir, &current, &mut out);
        assert!(
            out.iter().any(|w| w.kind == "VERIFIED_ANCHOR_MISS"
                && w.message.contains("pub fn compute")),
            "expected VERIFIED_ANCHOR_MISS, got {out:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// Fewer than 2 `verified: true` tests → emit VERIFIED_COUNT_LOW warning
    /// but do NOT emit VERIFIED_ANCHOR_MISS (anchors are still intact on the
    /// verified test that exists). The warning is advisory.
    #[test]
    fn p12_verified_count_low_emits_warning_not_failure() {
        let dir = test_dir("p12_verified_count_low");
        let current = p2_init_pack(&dir);
        fs::write(
            dir.join("lib.rs"),
            "pub fn compute(x: i32) -> i32 { x }\n",
        )
        .unwrap();
        // 4 tests, only 1 is verified.
        fs::write(
            current.join("acceptance_tests.md"),
            "### test: a\n\
             - verified: true\n\
             - anchors: lib.rs:1: pub fn compute\n\n\
             ### test: b\n- verified: false\n\n\
             ### test: c\n- verified: false\n\n\
             ### test: d\n- verified: false\n",
        )
        .unwrap();

        let mut out: Vec<StructuralWarning> = Vec::new();
        check_verified_acceptance_anchors(&dir, &current, &mut out);
        assert!(
            out.iter().any(|w| w.kind == "VERIFIED_COUNT_LOW"
                && w.message.contains("1 of 4")),
            "expected VERIFIED_COUNT_LOW naming 1 of 4, got {out:?}"
        );
        assert!(
            !out.iter().any(|w| w.kind == "VERIFIED_ANCHOR_MISS"),
            "anchor was valid; no MISS expected, got {out:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    // --- P12 / F42 — history.jsonl audit trail tests ---

    /// On first seal (no previous snapshot), the audit helper must return
    /// an empty diff vector rather than panic.
    #[test]
    fn p12_compute_prose_diff_sections_first_seal_is_empty() {
        let dir = test_dir("p12_prose_diff_first");
        let snapshots = dir.join("snapshots");
        fs::create_dir_all(&snapshots).unwrap();
        let current = dir.join("current");
        fs::create_dir_all(&current).unwrap();
        fs::write(current.join("20_CODE_MAP.md"), "## Intro\nhello\n").unwrap();

        let diff = compute_prose_diff_sections(&snapshots, &current);
        assert!(diff.is_empty(), "first seal must have empty diff, got {diff:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// When an H2 section body changes between snapshots, the helper must
    /// report `<file>#<heading>` for that section.
    #[test]
    fn p12_compute_prose_diff_sections_names_changed_h2() {
        let dir = test_dir("p12_prose_diff_h2");
        let snapshots = dir.join("snapshots");
        let prev = snapshots.join("20260101T000000Z_abc");
        fs::create_dir_all(&prev).unwrap();
        fs::write(prev.join("20_CODE_MAP.md"), "## Intro\nold body\n## Other\nunchanged\n")
            .unwrap();
        let current = dir.join("current");
        fs::create_dir_all(&current).unwrap();
        fs::write(
            current.join("20_CODE_MAP.md"),
            "## Intro\nnew body\n## Other\nunchanged\n",
        )
        .unwrap();

        let diff = compute_prose_diff_sections(&snapshots, &current);
        assert!(
            diff.iter().any(|s| s == "20_CODE_MAP.md#Intro"),
            "expected 20_CODE_MAP.md#Intro in diff, got {diff:?}"
        );
        assert!(
            !diff.iter().any(|s| s == "20_CODE_MAP.md#Other"),
            "unchanged section must not appear, got {diff:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// `git_committer_identity` must return "name <email>" when both are
    /// configured. The test sets env locally (not the global user config).
    #[test]
    fn p12_git_committer_identity_formats_name_and_email() {
        let dir = test_dir("p12_committer");
        init_git(&dir);
        // Local git config for this repo only.
        let _ = Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(&dir)
            .output();
        let _ = Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&dir)
            .output();

        let ident = git_committer_identity(&dir);
        assert_eq!(ident, "Test User <test@example.com>");
        let _ = fs::remove_dir_all(&dir);
    }

    /// End-to-end: a full seal writes the new audit-trail fields on
    /// `history.jsonl`. Verifies the JSON structure so readers can rely
    /// on sealed_by/prose_diff_sections/seal_reason.
    #[test]
    fn p12_seal_history_entry_has_audit_trail_fields() {
        let dir = test_dir("p12_seal_audit");
        init_git(&dir);
        // Local git identity so `sealed_by` is populated.
        let _ = Command::new("git")
            .args(["config", "user.name", "Audit Bot"])
            .current_dir(&dir)
            .output();
        let _ = Command::new("git")
            .args(["config", "user.email", "audit@example.com"])
            .current_dir(&dir)
            .output();

        // Minimal pack with all required files.
        let current = dir.join(".agent-context").join("current");
        fs::create_dir_all(&current).unwrap();
        for f in REQUIRED_FILES {
            fs::write(current.join(f), format!("## Intro\n{f} body\n")).unwrap();
        }

        // Commit so HEAD exists.
        let _ = Command::new("git").args(["add", "-A"]).current_dir(&dir).output();
        let _ = Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(&dir)
            .output();

        seal(SealOptions {
            reason: Some("p12-audit-seal".to_string()),
            base: None,
            head: None,
            pack_dir: None,
            cwd: Some(dir.display().to_string()),
            force: false,
            force_snapshot: false,
            follow_symlinks: false,
        })
        .expect("seal should succeed");

        let history = fs::read_to_string(dir.join(".agent-context/history.jsonl"))
            .expect("history.jsonl present after seal");
        let last_line = history.lines().last().expect("history has at least one line");
        let entry: Value = serde_json::from_str(last_line).expect("valid jsonl entry");
        assert_eq!(
            entry.get("seal_reason").and_then(|v| v.as_str()),
            Some("p12-audit-seal"),
            "seal_reason must be populated"
        );
        assert!(
            entry
                .get("sealed_by")
                .and_then(|v| v.as_str())
                .map(|s| s.contains("Audit Bot") && s.contains("audit@example.com"))
                .unwrap_or(false),
            "sealed_by must reflect git identity, got {entry:?}"
        );
        assert!(
            entry.get("prose_diff_sections").and_then(|v| v.as_array()).is_some(),
            "prose_diff_sections must be an array, got {entry:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// Acceptance tests without anchors (or without `verified: true`) must
    /// not produce any warnings — the feature is opt-in.
    #[test]
    fn p12_verified_acceptance_no_verified_tests_is_silent() {
        let dir = test_dir("p12_verified_silent");
        let current = p2_init_pack(&dir);
        fs::write(
            current.join("acceptance_tests.md"),
            "### test: a\n- invalidated_by: compute\n",
        )
        .unwrap();
        let mut out: Vec<StructuralWarning> = Vec::new();
        check_verified_acceptance_anchors(&dir, &current, &mut out);
        assert!(
            out.is_empty(),
            "un-verified tests must not produce warnings, got {out:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn p2_routing_file_check_flags_missing_canonical_refs() {
        let dir = test_dir("p2_routing");
        // CLAUDE.md exists but does not reference the canonical pack paths.
        fs::write(dir.join("CLAUDE.md"), "# empty routing block\n").unwrap();
        // GEMINI.md missing entirely — must not produce a warning.

        let mut out: Vec<StructuralWarning> = Vec::new();
        check_routing_files(&dir, &mut out);

        assert!(
            out.iter().any(|w| w.kind == "ROUTING_MISSING_REF"
                && w.message.contains("CLAUDE.md")),
            "expected ROUTING_MISSING_REF naming CLAUDE.md, got {out:?}"
        );
        assert!(
            !out.iter().any(|w| w.message.contains("GEMINI.md")),
            "missing GEMINI.md must not trigger a warning, got {out:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn p2_family_count_delta_after_adding_a_script() {
        // Fixture: 12 scripts at seal, manifest says 12, add a 13th → warn.
        let dir = test_dir("p2_family_count_delta");
        init_git(&dir);
        let scripts = dir.join("scripts");
        fs::create_dir_all(&scripts).unwrap();
        for i in 0..12 {
            fs::write(scripts.join(format!("run_{i}.py")), "# runner\n").unwrap();
        }
        let current = p2_init_pack(&dir);
        fs::write(
            current.join("completeness_contract.json"),
            r#"{"task_families": {"lookup": {"required_file_families": ["scripts/run_*.py"]}}}"#,
        )
        .unwrap();

        // Manifest records 12 (seal-time count).
        let manifest = json!({
            "family_counts": {"scripts/run_*.py": 12},
        });

        // No drift yet.
        let mut out: Vec<StructuralWarning> = Vec::new();
        check_family_counts_drift(&manifest, &dir, &current, &mut out);
        assert!(
            out.is_empty(),
            "no drift expected before adding the 13th file, got {out:?}"
        );

        // Add a 13th script — live count becomes 13.
        fs::write(scripts.join("run_12.py"), "# 13th\n").unwrap();
        let mut out: Vec<StructuralWarning> = Vec::new();
        check_family_counts_drift(&manifest, &dir, &current, &mut out);
        assert!(
            out.iter().any(|w| w.kind == "FAMILY_COUNT_DRIFT"
                && w.message.contains("scripts/run_*.py")
                && w.message.contains("12")
                && w.message.contains("13")),
            "expected FAMILY_COUNT_DRIFT naming 12 vs 13, got {out:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn p2_declared_count_drift_names_every_stale_file() {
        // Fixture: prose in two pack files says "12 scripts" but the repo
        // actually has 13 matching scripts on disk. The authoritative count
        // comes from the live resolution of the glob pattern that mentions
        // "scripts" in its path.
        let dir = test_dir("p2_declared_count_delta");
        init_git(&dir);
        // Create 13 scripts on disk — the authoritative answer.
        let scripts = dir.join("scripts");
        fs::create_dir_all(&scripts).unwrap();
        for i in 0..13 {
            fs::write(scripts.join(format!("run_{i}.py")), "# runner\n").unwrap();
        }
        let current = p2_init_pack(&dir);
        // Declare the glob so resolve_family_counts returns 13 for it.
        fs::write(
            current.join("completeness_contract.json"),
            r#"{"task_families": {"lookup": {"required_file_families": ["scripts/run_*.py"]}}}"#,
        )
        .unwrap();
        // Two pack files both claim "12 scripts" — both must be reported.
        fs::write(
            current.join("10_SYSTEM_OVERVIEW.md"),
            "# Overview\n\nThe repo has 12 scripts.\n",
        )
        .unwrap();
        fs::write(
            current.join("20_CODE_MAP.md"),
            "# Code Map\n\nWe maintain 12 scripts total.\n",
        )
        .unwrap();

        // Manifest is intentionally the seal-time snapshot; the authoritative
        // signal is the live family_counts (which now resolves to 13).
        let manifest = json!({
            "declared_counts": [
                {"noun": "scripts", "count": 12, "file": "10_SYSTEM_OVERVIEW.md", "line": 3},
                {"noun": "scripts", "count": 12, "file": "20_CODE_MAP.md", "line": 3},
            ],
        });

        let mut out: Vec<StructuralWarning> = Vec::new();
        check_declared_counts_drift(&manifest, &dir, &current, &mut out);

        assert!(
            out.iter().any(|w| w.kind == "DECLARED_COUNT_DRIFT"
                && w.message.contains("12 scripts")
                && w.message.contains("13")),
            "expected DECLARED_COUNT_DRIFT naming '12 scripts' stale vs auth 13, got {out:?}"
        );
        // affected_pack_files should include both stale file names so the
        // reviewer can see every repetition.
        let affected: Vec<String> = out
            .iter()
            .filter(|w| w.kind == "DECLARED_COUNT_DRIFT")
            .flat_map(|w| w.affected_pack_files.clone())
            .collect();
        assert!(
            affected.iter().any(|f| f == "10_SYSTEM_OVERVIEW.md"),
            "expected 10_SYSTEM_OVERVIEW.md in affected_pack_files, got {affected:?}"
        );
        assert!(
            affected.iter().any(|f| f == "20_CODE_MAP.md"),
            "expected 20_CODE_MAP.md in affected_pack_files, got {affected:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn p2_signature_drift_fires_on_rename() {
        // Manifest has "calc.py::compute_lift_with_ci". Rename the function
        // in the repo file → drift check must surface both sides (old missing,
        // new added).
        let dir = test_dir("p2_sig_drift");
        let current = p2_init_pack(&dir);
        fs::write(
            dir.join("calc.py"),
            "def compute_lift(data):\n    return 1\n",
        )
        .unwrap();
        // search_scope points at calc.py so parse_shortcut_signatures picks it up.
        fs::write(
            current.join("search_scope.json"),
            r#"{
              "task_families": {
                "lookup": {
                  "verification_shortcuts": {"calc.py": "compute_lift"}
                }
              }
            }"#,
        )
        .unwrap();

        let manifest = json!({
            "shortcut_signatures": {
                "calc.py::compute_lift_with_ci": "def compute_lift_with_ci(data)"
            }
        });

        let mut out: Vec<StructuralWarning> = Vec::new();
        check_shortcut_signatures_drift(&manifest, &dir, &current, &mut out);

        // Old key gone:
        assert!(
            out.iter().any(|w| w.kind == "SIGNATURE_DRIFT"
                && w.message.contains("compute_lift_with_ci")
                && w.message.contains("rename or deletion")),
            "expected SIGNATURE_DRIFT for renamed-out compute_lift_with_ci, got {out:?}"
        );
        // New key present:
        assert!(
            out.iter().any(|w| w.kind == "SIGNATURE_DRIFT"
                && w.message.contains("calc.py::compute_lift")
                && w.message.contains("new function or rename")),
            "expected SIGNATURE_DRIFT for added compute_lift, got {out:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn p2_dependencies_drift_fires_on_pyproject_change() {
        let dir = test_dir("p2_deps_drift");
        fs::write(dir.join("pyproject.toml"), "[tool.poetry]\nname=\"v1\"\n").unwrap();
        // Seal-time hash is the hash of the *old* file contents; compute it
        // against a different string so drift is guaranteed to fire.
        let manifest = json!({
            "dependencies_snapshot": {"pyproject": "a".repeat(64)}
        });

        let mut out: Vec<StructuralWarning> = Vec::new();
        check_dependencies_drift(&manifest, &dir, &mut out);

        assert!(
            out.iter().any(|w| w.kind == "DEPENDENCIES_DRIFT"
                && w.message.contains("pyproject")
                && w.message.contains("40_OPERATIONS_AND_RELEASE.md")),
            "expected DEPENDENCIES_DRIFT naming pyproject + 40_OPERATIONS_AND_RELEASE.md, got {out:?}"
        );
        // affected_pack_files points at operations md.
        assert!(
            out.iter().any(|w| w
                .affected_pack_files
                .iter()
                .any(|f| f == "40_OPERATIONS_AND_RELEASE.md")),
            "expected affected_pack_files to include 40_OPERATIONS_AND_RELEASE.md, got {out:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn p2_structural_warnings_as_json_shape_is_stable() {
        // CI JSON contract: each warning has {kind, message, affected_pack_files}.
        let warnings = vec![StructuralWarning {
            kind: "TEMPLATE_MARKER".to_string(),
            message: "marker found".to_string(),
            affected_pack_files: vec!["routes.json".to_string()],
        }];
        let v = structural_warnings_as_json(&warnings);
        let arr = v.as_array().expect("must be an array");
        assert_eq!(arr.len(), 1);
        let obj = arr[0].as_object().expect("must be an object");
        assert_eq!(obj.get("kind").and_then(|v| v.as_str()), Some("TEMPLATE_MARKER"));
        assert_eq!(
            obj.get("message").and_then(|v| v.as_str()),
            Some("marker found")
        );
        let affected = obj
            .get("affected_pack_files")
            .and_then(|v| v.as_array())
            .expect("affected_pack_files must be an array");
        assert_eq!(affected.len(), 1);
        assert_eq!(affected[0].as_str(), Some("routes.json"));
    }

    // --- P4 tests (restored post-integration) ---


    #[test]
    fn p4_install_settings_template_is_idempotent() {
        let dir = test_dir("p4_install_idempotent");
        install_settings_template(dir.to_str().unwrap(), false).unwrap();
        let first = fs::read_to_string(dir.join(".claude/settings.json")).unwrap();

        install_settings_template(dir.to_str().unwrap(), false).unwrap();
        let second = fs::read_to_string(dir.join(".claude/settings.json")).unwrap();
        assert_eq!(
            first, second,
            "running install-settings-template twice must produce identical bytes"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn p4_acceptance_tests_graceful_degrade_when_p2_absent() {
        // No signature_drift.json on disk → every test stays `ready`
        // regardless of its invalidated_by entries. This is the graceful
        // degradation contract documented inline on evaluate_acceptance_tests.
        let dir = test_dir("p4_acceptance_no_p2");
        let current = dir.join(".agent-context/current");
        fs::create_dir_all(&current).unwrap();
        fs::write(
            current.join("acceptance_tests.md"),
            "### test: t1\n- invalidated_by: fn_a, fn_b\n",
        )
        .unwrap();

        let drifted = collect_signature_drift(&current);
        assert!(
            drifted.is_empty(),
            "absence of signature_drift.json must yield an empty set, got {drifted:?}"
        );

        let tests = evaluate_acceptance_tests(&current, &drifted);
        assert_eq!(tests.len(), 1);
        assert_eq!(
            tests[0]["status"].as_str(),
            Some("ready"),
            "without P2 drift signal, tests stay ready even when invalidated_by is populated"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn p4_install_settings_template_creates_new_file() {
        let dir = test_dir("p4_install_new");
        install_settings_template(dir.to_str().unwrap(), false)
            .expect("install must succeed when no settings.json exists");

        let path = dir.join(".claude/settings.json");
        assert!(path.exists(), "settings.json must be created");
        let raw = fs::read_to_string(&path).unwrap();
        let parsed: Value = serde_json::from_str(&raw).expect("must be valid JSON");
        let pre_tool_use = parsed
            .get("hooks")
            .and_then(|h| h.get("PreToolUse"))
            .and_then(|v| v.as_array())
            .expect("hooks.PreToolUse must be present");
        assert!(
            pre_tool_use.len() >= 2,
            "shipped template should install both Edit|Write and Bash matchers, got {} entries",
            pre_tool_use.len()
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn p4_install_settings_template_preserves_existing_keys() {
        let dir = test_dir("p4_install_merge");
        let claude_dir = dir.join(".claude");
        fs::create_dir_all(&claude_dir).unwrap();
        let existing = json!({
            "permissions": {"allow": ["Bash(ls:*)"]},
            "env": {"DEBUG": "false"},
            "hooks": {
                "PostToolUse": [
                    {"matcher": "Edit", "hooks": [{"type": "command", "command": "echo hi"}]}
                ]
            }
        });
        fs::write(
            claude_dir.join("settings.json"),
            format!("{}\n", serde_json::to_string_pretty(&existing).unwrap()),
        )
        .unwrap();

        install_settings_template(dir.to_str().unwrap(), false)
            .expect("merge must succeed when settings.json exists");

        let raw = fs::read_to_string(claude_dir.join("settings.json")).unwrap();
        let parsed: Value = serde_json::from_str(&raw).unwrap();

        // Existing non-hooks keys preserved verbatim.
        assert_eq!(
            parsed
                .get("permissions")
                .and_then(|p| p.get("allow"))
                .and_then(|a| a.as_array())
                .map(|a| a.len()),
            Some(1),
            "permissions.allow must be preserved, got {parsed}"
        );
        assert_eq!(
            parsed.get("env").and_then(|e| e.get("DEBUG")).and_then(|v| v.as_str()),
            Some("false"),
            "env.DEBUG must be preserved"
        );
        // Existing hooks.PostToolUse stays intact.
        assert!(
            parsed
                .get("hooks")
                .and_then(|h| h.get("PostToolUse"))
                .and_then(|v| v.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false),
            "existing hooks.PostToolUse must survive the merge"
        );
        // PreToolUse entries added by the template are now present.
        let pre = parsed
            .get("hooks")
            .and_then(|h| h.get("PreToolUse"))
            .and_then(|v| v.as_array())
            .expect("hooks.PreToolUse must be inserted");
        assert!(
            pre.iter().any(|e| e.get("matcher").and_then(|v| v.as_str()) == Some("Edit|Write")),
            "Edit|Write matcher must be present after merge, got {pre:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn p4_merge_settings_template_dedupes_existing_command() {
        // A user who already added the chorus PreToolUse hook (e.g. by hand)
        // should see no duplicate on subsequent install.
        let template: Value = serde_json::from_str(SETTINGS_TEMPLATE_JSON).unwrap();
        let existing = template.clone();
        let merged = merge_settings_template(&existing, &template);
        let pre = merged
            .get("hooks")
            .and_then(|h| h.get("PreToolUse"))
            .and_then(|v| v.as_array())
            .expect("PreToolUse must be present");
        let template_pre_len = template
            .get("hooks")
            .and_then(|h| h.get("PreToolUse"))
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap();
        assert_eq!(
            pre.len(),
            template_pre_len,
            "merging the template with itself must not double entries"
        );
    }

    #[test]
    fn p4_acceptance_tests_missing_file_returns_empty() {
        let dir = test_dir("p4_acceptance_missing");
        let current = dir.join(".agent-context/current");
        fs::create_dir_all(&current).unwrap();
        let drifted = std::collections::BTreeSet::new();
        let tests = evaluate_acceptance_tests(&current, &drifted);
        assert!(tests.is_empty(), "missing file → empty list");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn p4_acceptance_tests_needs_revalidation_on_drift() {
        // acceptance_tests.md declares two tests; one lists `compute_lift`
        // in its invalidated_by field. When signature_drift.json names
        // `compute_lift`, the corresponding test must flip to
        // `needs_revalidation`; the other test must stay `ready`.
        let dir = test_dir("p4_acceptance_drift");
        let current = dir.join(".agent-context/current");
        fs::create_dir_all(&current).unwrap();

        fs::write(
            current.join("acceptance_tests.md"),
            "# Acceptance Tests\n\n\
             ### test: verify_lift_calculation\n\
             - invalidated_by: compute_lift, format_result\n\n\
             ### test: verify_summary_shape\n\
             - invalidated_by: render_summary\n",
        )
        .unwrap();

        // P2 drift signal — only compute_lift drifted.
        fs::write(
            current.join("signature_drift.json"),
            r#"{"drifted_functions": ["compute_lift"]}"#,
        )
        .unwrap();

        let drifted = collect_signature_drift(&current);
        assert!(drifted.contains("compute_lift"));

        let tests = evaluate_acceptance_tests(&current, &drifted);
        assert_eq!(tests.len(), 2, "both tests must be surfaced, got {tests:?}");

        let lift = tests
            .iter()
            .find(|t| t["id"] == "verify_lift_calculation")
            .expect("lift test must be present");
        assert_eq!(
            lift["status"].as_str(),
            Some("needs_revalidation"),
            "drift in compute_lift must flip verify_lift_calculation to needs_revalidation"
        );
        let matched: Vec<&str> = lift["invalidated_by_matched"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(matched, vec!["compute_lift"]);

        let summary = tests
            .iter()
            .find(|t| t["id"] == "verify_summary_shape")
            .expect("summary test must be present");
        assert_eq!(
            summary["status"].as_str(),
            Some("ready"),
            "no overlap with drifted set → stays ready"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    // --- P6 tests (restored post-integration) ---


    /// A PR range with only pure-code and only pure-pack commits passes the
    /// separate-commit gate. A commit that mixes the two is reported with the
    /// `commit <sha> mixes pack + non-pack changes` message.
    #[test]
    fn p6_check_separate_commits_reports_mixed_commit() {
        let repo = init_repo_with_commits("p6_separate_commits_mixed");
        // Capture the pre-range tip — we'll diff against it as `base`.
        let base = run_git(&["rev-parse", "HEAD"], &repo, true)
            .expect("base sha")
            .trim()
            .to_string();

        // Pure-code commit (passes).
        commit_file(&repo, "src/pure_code.rs", "fn a() {}\n");

        // Pure-pack commit (passes).
        fs::create_dir_all(repo.join(".agent-context/current")).unwrap();
        commit_file(
            &repo,
            ".agent-context/current/20_CODE_MAP.md",
            "# pack content\n",
        );

        // Mixed commit: touches both a pack file and a non-pack file in a
        // single commit. This is the case the gate must flag.
        fs::write(repo.join("src/pure_code.rs"), "fn a() -> i32 { 0 }\n").unwrap();
        fs::write(
            repo.join(".agent-context/current/20_CODE_MAP.md"),
            "# pack content v2\n",
        )
        .unwrap();
        let run = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(&repo)
                .output()
                .expect("git")
        };
        run(&["add", "-A"]);
        run(&["commit", "-q", "-m", "mix: pack and code in one commit"]);
        let mixed_sha = run_git(&["rev-parse", "HEAD"], &repo, true)
            .expect("mixed sha")
            .trim()
            .to_string();

        let errors = check_separate_commits(&base, &repo).expect("separate-commit check runs");
        assert_eq!(errors.len(), 1, "only one commit should fail the gate: {errors:?}");
        let msg = &errors[0];
        assert!(
            msg.contains(&mixed_sha),
            "error message must name the offending sha ({mixed_sha}), got {msg:?}"
        );
        assert!(
            msg.contains("mixes pack + non-pack changes"),
            "error message must use the documented phrase, got {msg:?}"
        );

        let _ = fs::remove_dir_all(&repo);
    }

    /// `write_last_freshness_state` creates the state file with the three
    /// documented keys when the pack directory exists, and silently
    /// no-ops when it does not. Covers the "warn → state file" path.
    #[test]
    fn p6_write_last_freshness_state_emits_payload() {
        let dir = test_dir("p6_write_state");
        let current = dir.join(".agent-context").join("current");
        fs::create_dir_all(&current).expect("create pack dir");

        let result = FreshnessResult {
            status: "warn".to_string(),
            changed_files: vec!["src/foo.rs".to_string(), "src/bar.rs".to_string()],
            pack_updated: false,
            skipped_reason: None,
            affected_sections: vec![
                "20_CODE_MAP.md".to_string(),
                "30_BEHAVIORAL_INVARIANTS.md".to_string(),
            ],
        };
        write_last_freshness_state(&dir, &result);

        let state_path = current.join(".last_freshness.json");
        assert!(state_path.exists(), "state file must be created");
        let body = fs::read_to_string(&state_path).expect("read state file");
        let parsed: Value = serde_json::from_str(&body).expect("state file must be valid json");
        let changed = parsed
            .get("changed_files")
            .and_then(|v| v.as_array())
            .expect("changed_files array");
        assert_eq!(changed.len(), 2);
        let sections = parsed
            .get("affected_sections")
            .and_then(|v| v.as_array())
            .expect("affected_sections array");
        assert_eq!(sections.len(), 2);
        assert!(
            parsed.get("timestamp").and_then(|v| v.as_u64()).is_some(),
            "timestamp must be a number"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// The generated pre-push hook must include the P6 pack-only skip
    /// shortcut so users who inspect the hook can see the behavior. We
    /// pin on the sentinel strings the hook's callers expect rather than
    /// the whole body, so surrounding hook content can evolve.
    #[test]
    fn p6_hook_section_contains_pack_only_skip_logic() {
        let hook = build_pre_push_hook_section();
        assert!(
            hook.contains("pack_only_skip"),
            "hook must define pack_only_skip helper, got:\n{hook}"
        );
        assert!(
            hook.contains("pack-only push, skipping freshness check"),
            "hook must announce the skip when triggered, got:\n{hook}"
        );
        assert!(
            hook.contains(".agent-context/current/.last_freshness.json"),
            "hook must read the last-freshness state file, got:\n{hook}"
        );
        assert!(
            hook.contains("warning appears addressed"),
            "hook must report when prior warning is addressed, got:\n{hook}"
        );
    }

    /// P12 / F44 — shell hygiene for the generated pre-push hook.
    /// Verifies `set -u`, quoted env interpolations, and the `--` separator
    /// in the git diff call. Uses `bash -n` to syntax-check the generated
    /// body so authoring regressions land as test failures rather than at
    /// install time.
    #[test]
    fn p12_pre_push_hook_has_set_u_quoted_interpolations_and_dashdash() {
        let hook = build_pre_push_hook_section();
        assert!(
            hook.starts_with("set -u"),
            "hook must start with `set -u` for fail-fast semantics, got:\n{hook}"
        );
        // `$remote_name`/`$remote_url` appear inside a double-quoted echo
        // string, so they are implicitly quoted. Guard against a regression
        // where someone unquotes them by looking for the classic bare-var
        // form outside of quotes.
        assert!(
            !hook.contains(" $remote_name ") && !hook.contains(" $remote_url "),
            "hook must not leave `$remote_name`/`$remote_url` unquoted bare, got:\n{hook}"
        );
        assert!(
            hook.contains("git diff --name-only \"${base}..${head}\" --"),
            "hook must pass `--` to git diff, got:\n{hook}"
        );
        // Also guard against regressions that might drop quotes around
        // `$base` / `$head` (the classic `$var` → injection pitfall).
        assert!(
            !hook.contains("git diff --name-only $base..$head"),
            "hook must not leave `$base..$head` unquoted"
        );

        // Static `bash -n` syntax check — fails loudly on quoting
        // regressions that the substring asserts above would miss.
        // Skip silently if `bash` is not on PATH (e.g. exotic CI).
        if Command::new("bash").arg("--version").output().is_ok() {
            let mut body = String::from("#!/usr/bin/env bash\n");
            body.push_str(&hook);
            body.push('\n');
            let tmp = env::temp_dir().join("chorus_p12_hook_syntax_check.sh");
            fs::write(&tmp, &body).unwrap();
            let out = Command::new("bash")
                .args(["-n"])
                .arg(&tmp)
                .output()
                .expect("bash -n should run");
            assert!(
                out.status.success(),
                "generated hook must parse cleanly under `bash -n`: stderr:\n{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let _ = fs::remove_file(&tmp);
        }
    }

    /// `check_separate_commits` is a no-op when the directory is not a git
    /// repo. The surrounding verify flow already reports that case, so this
    /// check simply must not error out.
    #[test]
    fn p6_check_separate_commits_non_git_is_empty() {
        let dir = test_dir("p6_separate_commits_non_git");
        let errors = check_separate_commits("origin/main", &dir)
            .expect("must not fail on non-git directory");
        assert!(
            errors.is_empty(),
            "non-git directory must return empty error list, got {errors:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// Non-warn results must not overwrite an existing state file; only a
    /// warn is authoritative. The public API reflects this by guarding the
    /// write behind `result.status == "warn"`.
    #[test]
    fn p6_write_last_freshness_state_skips_non_warn() {
        let dir = test_dir("p6_write_state_noop");
        let current = dir.join(".agent-context").join("current");
        fs::create_dir_all(&current).expect("create pack dir");

        let result = FreshnessResult {
            status: "pass".to_string(),
            changed_files: vec![],
            pack_updated: false,
            skipped_reason: None,
            affected_sections: vec![],
        };
        write_last_freshness_state(&dir, &result);

        let state_path = current.join(".last_freshness.json");
        assert!(
            !state_path.exists(),
            "non-warn result must not write the state file"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// When the pack directory does not exist yet, `write_last_freshness_state`
    /// must silently no-op rather than panic. This guards the pre-init case
    /// where a freshness helper fires before the pack has been scaffolded.
    #[test]
    fn p6_write_last_freshness_state_no_pack_dir_is_noop() {
        let dir = test_dir("p6_write_state_no_pack");
        let result = FreshnessResult {
            status: "warn".to_string(),
            changed_files: vec!["src/foo.rs".to_string()],
            pack_updated: false,
            skipped_reason: None,
            affected_sections: vec!["20_CODE_MAP.md".to_string()],
        };
        // Should not panic.
        write_last_freshness_state(&dir, &result);
        assert!(
            !dir.join(".agent-context/current/.last_freshness.json").exists(),
            "state file must not be written when pack dir is absent"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// A range where no commit mixes pack + non-pack returns an empty error
    /// list so the CI gate stays silent.
    #[test]
    fn p6_check_separate_commits_clean_range_passes() {
        let repo = init_repo_with_commits("p6_separate_commits_clean");
        let base = run_git(&["rev-parse", "HEAD"], &repo, true)
            .expect("base sha")
            .trim()
            .to_string();

        // Alternating but separate commits — each is either pure code or
        // pure pack, so the gate must not flag them.
        commit_file(&repo, "src/code_a.rs", "fn a() {}\n");
        fs::create_dir_all(repo.join(".agent-context/current")).unwrap();
        commit_file(
            &repo,
            ".agent-context/current/20_CODE_MAP.md",
            "# pack content\n",
        );
        commit_file(&repo, "src/code_b.rs", "fn b() {}\n");

        let errors = check_separate_commits(&base, &repo).expect("separate-commit check runs");
        assert!(
            errors.is_empty(),
            "clean range must produce no errors, got {errors:?}"
        );

        let _ = fs::remove_dir_all(&repo);
    }

    // --- P5 tests (restored post-integration) ---


    #[test]
    fn extract_numeric_claims_flags_stale_prose() {
        let mut authoritative = std::collections::HashMap::new();
        authoritative.insert("files".to_string(), 7usize);
        let content = "We have 6 files in the repo.";
        let mismatches = extract_numeric_claims(content, &authoritative, "10_SYSTEM_OVERVIEW.md");
        assert_eq!(mismatches.len(), 1, "expected one mismatch, got {mismatches:?}");
        let m = &mismatches[0];
        assert_eq!(m.claimed_count, 6);
        assert_eq!(m.authoritative_count, 7);
        assert_eq!(m.noun, "files");
        assert_eq!(m.line, 1);
    }

    #[test]
    fn slug_for_count_key_normalizes_globs() {
        assert_eq!(slug_for_count_key("scripts/run_*.py"), "scripts_run_py");
        assert_eq!(
            slug_for_count_key("src/brand_lift/*.py"),
            "src_brand_lift_py"
        );
        assert_eq!(slug_for_count_key("tests/**/*.py"), "tests_py");
        assert_eq!(slug_for_count_key("docs/methodology"), "docs_methodology");
    }

    #[test]
    fn extract_numeric_claims_respects_ignore_region() {
        let mut authoritative = std::collections::HashMap::new();
        authoritative.insert("files".to_string(), 7usize);
        let content = "\
<!-- count-claim: ignore -->\n\
Historic note: we once shipped 6 files.\n\
<!-- count-claim: end -->\n\
Current count is {{counts.files_py}}.\n";
        let mismatches = extract_numeric_claims(content, &authoritative, "doc.md");
        assert!(
            mismatches.is_empty(),
            "claim inside ignore region must not be flagged, got {mismatches:?}"
        );
    }

    #[test]
    fn extract_numeric_claims_passes_matching_prose() {
        let mut authoritative = std::collections::HashMap::new();
        authoritative.insert("tests".to_string(), 32usize);
        let content = "Currently 32 tests pass.";
        let mismatches = extract_numeric_claims(content, &authoritative, "doc.md");
        assert!(
            mismatches.is_empty(),
            "prose that matches authoritative must not be flagged, got {mismatches:?}"
        );
    }

    #[test]
    fn seal_fails_on_stale_prose_number_without_force() {
        let dir = test_dir("p5_seal_stale");
        init_git(&dir);
        let scripts = dir.join("scripts");
        fs::create_dir_all(&scripts).unwrap();
        for i in 0..12 {
            fs::write(scripts.join(format!("run_{i}.py")), "# runner\n").unwrap();
        }
        let current = dir.join(".agent-context").join("current");
        fs::create_dir_all(&current).unwrap();
        // Required files (P1 schema — 5 markdown).
        fs::write(
            current.join("00_START_HERE.md"),
            "- Branch at generation: `main`\n- HEAD commit: `unknown`\n- Generated at: `never`\n",
        )
        .unwrap();
        fs::write(
            current.join("10_SYSTEM_OVERVIEW.md"),
            "The repo ships 6 scripts today.\n",
        )
        .unwrap();
        fs::write(current.join("20_CODE_MAP.md"), "map\n").unwrap();
        fs::write(current.join("30_BEHAVIORAL_INVARIANTS.md"), "rules\n").unwrap();
        fs::write(current.join("40_OPERATIONS_AND_RELEASE.md"), "ops\n").unwrap();
        // Completeness contract declares the scripts/run_*.py family so P1
        // family_counts resolves to 12 at seal time.
        fs::write(
            current.join("completeness_contract.json"),
            r#"{
              "task_families": {
                "lookup": {"required_file_families": ["scripts/run_*.py"]}
              }
            }"#,
        )
        .unwrap();

        // Seed a commit so HEAD resolves.
        let _ = std::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(&dir)
            .output();
        let _ = std::process::Command::new("git")
            .args(["commit", "-m", "seed", "--quiet"])
            .current_dir(&dir)
            .output();

        let err = seal(SealOptions {
            reason: None,
            base: None,
            head: None,
            pack_dir: None,
            cwd: Some(dir.to_string_lossy().to_string()),
            force: false,
            force_snapshot: false,
            follow_symlinks: false,
        })
        .expect_err("seal must fail when prose count disagrees with authoritative");
        let msg = format!("{err}");
        assert!(
            msg.contains("disagree")
                || msg.contains("claim")
                || msg.contains("numeric claims"),
            "expected a count-claim failure message, got: {msg}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn expand_count_handlebars_substitutes_known_slugs() {
        let mut counts = std::collections::HashMap::new();
        counts.insert("scripts_run".to_string(), 12usize);
        counts.insert("src_brand_lift".to_string(), 6usize);
        let content = "The repo has {{counts.scripts_run}} scripts and {{counts.src_brand_lift}} brand-lift modules.";
        let expanded = expand_count_handlebars(content, &counts);
        assert_eq!(
            expanded,
            "The repo has 12 scripts and 6 brand-lift modules.",
            "handlebars must expand to authoritative values"
        );
    }

    #[test]
    fn expand_count_handlebars_leaves_unknown_slugs_literal() {
        let counts = std::collections::HashMap::new();
        let content = "No mapping for {{counts.bogus_slug}} here.";
        let expanded = expand_count_handlebars(content, &counts);
        assert_eq!(
            expanded,
            "No mapping for {{counts.bogus_slug}} here.",
            "unknown slugs must remain literal to surface author mistakes"
        );
    }

    #[test]
    fn seal_force_downgrades_stale_prose_to_warning() {
        let dir = test_dir("p5_seal_force");
        init_git(&dir);
        let scripts = dir.join("scripts");
        fs::create_dir_all(&scripts).unwrap();
        for i in 0..12 {
            fs::write(scripts.join(format!("run_{i}.py")), "# runner\n").unwrap();
        }
        let current = dir.join(".agent-context").join("current");
        fs::create_dir_all(&current).unwrap();
        fs::write(
            current.join("00_START_HERE.md"),
            "- Branch at generation: `main`\n- HEAD commit: `unknown`\n- Generated at: `never`\n",
        )
        .unwrap();
        fs::write(
            current.join("10_SYSTEM_OVERVIEW.md"),
            "Legacy prose: 6 scripts ship today.\n",
        )
        .unwrap();
        fs::write(current.join("20_CODE_MAP.md"), "map\n").unwrap();
        fs::write(current.join("30_BEHAVIORAL_INVARIANTS.md"), "rules\n").unwrap();
        fs::write(current.join("40_OPERATIONS_AND_RELEASE.md"), "ops\n").unwrap();
        fs::write(
            current.join("completeness_contract.json"),
            r#"{
              "task_families": {
                "lookup": {"required_file_families": ["scripts/run_*.py"]}
              }
            }"#,
        )
        .unwrap();

        let _ = std::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(&dir)
            .output();
        let _ = std::process::Command::new("git")
            .args(["commit", "-m", "seed", "--quiet"])
            .current_dir(&dir)
            .output();

        // --force must succeed (mismatches downgraded to warnings).
        seal(SealOptions {
            reason: None,
            base: None,
            head: None,
            pack_dir: None,
            cwd: Some(dir.to_string_lossy().to_string()),
            force: true,
            force_snapshot: false,
            follow_symlinks: false,
        })
        .expect("seal must succeed under --force even with stale prose");

        // Manifest should now exist with family_counts populated.
        let manifest: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(current.join("manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            manifest
                .get("family_counts")
                .and_then(|v| v.get("scripts/run_*.py"))
                .and_then(|v| v.as_u64()),
            Some(12),
            "family_counts must be sealed correctly even under --force"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn derive_count_maps_splits_by_slug_and_noun() {
        let mut family = std::collections::BTreeMap::new();
        family.insert("scripts/run_*.py".to_string(), 12usize);
        family.insert("src/brand_lift/*.py".to_string(), 6usize);
        let (slugs, nouns) = derive_count_maps(&family);
        assert_eq!(slugs.get("scripts_run_py").copied(), Some(12));
        assert_eq!(slugs.get("src_brand_lift_py").copied(), Some(6));
        // "scripts" noun sums every slug whose tokens include "scripts"
        // — only scripts/run_*.py does, so authoritative is 12.
        assert_eq!(nouns.get("scripts").copied(), Some(12));
        // "brands" noun would match only `brand` / `brands` slug tokens; the
        // brand_lift slug contains "brand" so it's included via singular.
        assert_eq!(nouns.get("brand").copied(), Some(6));
    }

    #[test]
    fn seal_expands_handlebars_into_sealed_pack() {
        let dir = test_dir("p5_seal_expand");
        init_git(&dir);
        let scripts = dir.join("scripts");
        fs::create_dir_all(&scripts).unwrap();
        for i in 0..12 {
            fs::write(scripts.join(format!("run_{i}.py")), "# runner\n").unwrap();
        }
        let current = dir.join(".agent-context").join("current");
        fs::create_dir_all(&current).unwrap();
        fs::write(
            current.join("00_START_HERE.md"),
            "- Branch at generation: `main`\n- HEAD commit: `unknown`\n- Generated at: `never`\n",
        )
        .unwrap();
        fs::write(
            current.join("10_SYSTEM_OVERVIEW.md"),
            "We ship {{counts.scripts_run_py}} scripts.\n",
        )
        .unwrap();
        fs::write(current.join("20_CODE_MAP.md"), "map\n").unwrap();
        fs::write(current.join("30_BEHAVIORAL_INVARIANTS.md"), "rules\n").unwrap();
        fs::write(current.join("40_OPERATIONS_AND_RELEASE.md"), "ops\n").unwrap();
        fs::write(
            current.join("completeness_contract.json"),
            r#"{
              "task_families": {
                "lookup": {"required_file_families": ["scripts/run_*.py"]}
              }
            }"#,
        )
        .unwrap();

        let _ = std::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(&dir)
            .output();
        let _ = std::process::Command::new("git")
            .args(["commit", "-m", "seed", "--quiet"])
            .current_dir(&dir)
            .output();

        seal(SealOptions {
            reason: None,
            base: None,
            head: None,
            pack_dir: None,
            cwd: Some(dir.to_string_lossy().to_string()),
            force: false,
            force_snapshot: false,
            follow_symlinks: false,
        })
        .expect("seal with expanded handlebars must succeed");

        let expanded_body =
            fs::read_to_string(current.join("10_SYSTEM_OVERVIEW.md")).unwrap();
        assert!(
            expanded_body.contains("We ship 12 scripts."),
            "handlebar must be expanded in the sealed file body, got: {expanded_body:?}"
        );
        assert!(
            !expanded_body.contains("{{counts."),
            "no handlebar should survive seal, got: {expanded_body:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }


    #[test]
    fn extract_numeric_claims_skips_expanded_counts_matches() {
        let mut authoritative = std::collections::HashMap::new();
        authoritative.insert("scripts".to_string(), 12usize);
        let mut slug_counts = std::collections::HashMap::new();
        slug_counts.insert("scripts_run".to_string(), 12usize);
        // Author wrote the handlebar; after expansion the number is the
        // authoritative 12, which must not be flagged as a mismatch.
        let authored = "We ship {{counts.scripts_run}} scripts.";
        let expanded = expand_count_handlebars(authored, &slug_counts);
        assert_eq!(expanded, "We ship 12 scripts.");
        let mismatches = extract_numeric_claims(&expanded, &authoritative, "doc.md");
        assert!(
            mismatches.is_empty(),
            "expanded handlebar value must match authoritative without being flagged, got {mismatches:?}"
        );
    }

    // --- P7 tests (restored post-integration) ---

    fn commit_and_sha(root: &Path, rel: &str, content: &str) -> String {
        commit_file(root, rel, content);
        let out = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(root)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn seed_manifest(root: &Path, head_sha_at_seal: &str) {
        let current = root.join(".agent-context").join("current");
        fs::create_dir_all(&current).unwrap();
        let manifest = json!({
            "schema_version": CURRENT_SCHEMA_VERSION,
            "head_sha": head_sha_at_seal,
            "head_sha_at_seal": head_sha_at_seal,
            "post_commit_sha": null,
            "files": [],
        });
        fs::write(
            current.join("manifest.json"),
            format!("{}\n", serde_json::to_string_pretty(&manifest).unwrap()),
        )
        .unwrap();
    }



    #[test]
    fn diff_since_seal_returns_empty_zones_when_no_changes() {
        let dir = test_dir("p7_no_changes");
        init_git(&dir);
        // Commit an initial seed + a second commit so HEAD~1 resolves; the
        // seal baseline points at the LATEST commit so the diff is empty.
        commit_file(&dir, "seed.txt", "one");
        commit_file(&dir, "second.txt", "two");
        write_relevance_json(
            &dir,
            r#"{"zones":[{"paths":["src/**"],"affects":["20_CODE_MAP.md"]}]}"#,
        );
        // A `src/` file must exist so zone validation can pass when loaded
        // elsewhere; diff_since_seal itself doesn't validate zones here.
        commit_file(&dir, "src/lib.rs", "fn a() {}\n");
        // Re-seal baseline to THIS commit so there's nothing newer to diff.
        let latest = run_git(&["rev-parse", "HEAD"], &dir, true).unwrap();
        seed_manifest(&dir, &latest);

        let out = diff_since_seal(&dir, None).expect("diff_since_seal must succeed");
        let zones = out
            .value
            .get("zones")
            .and_then(|v| v.as_array())
            .expect("zones array");
        assert!(zones.is_empty(), "no changes must yield empty zones, got {zones:?}");
        assert!(
            out.acceptance_tests_invalidated.is_empty(),
            "no drifts -> empty acceptance_tests_invalidated"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_acceptance_invalidated_by_reads_list_markers() {
        let dir = test_dir("p7_parse_invalidated");
        let current = dir.join(".agent-context").join("current");
        fs::create_dir_all(&current).unwrap();
        let body = "\
# Acceptance tests\n\
\n\
## Q1: lookup the release flow\n\
- invalidated_by: publish_release\n\
\n\
## Q2: rename impact\n\
  invalidated_by: compute_lift_with_ci, resolve_sample\n\
\n\
## Q3: unrelated\n\
(no invalidated_by marker)\n\
";
        fs::write(current.join("acceptance_tests.md"), body).unwrap();

        let entries = parse_acceptance_invalidated_by(&current);
        assert_eq!(entries.len(), 2, "expected 2 bindings, got {entries:?}");
        let q1 = &entries[0];
        assert_eq!(
            q1.get("test_id").and_then(|v| v.as_str()),
            Some("Q1: lookup the release flow")
        );
        let q1_fns: Vec<String> = q1
            .get("invalidated_by")
            .and_then(|v| v.as_array())
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        assert_eq!(q1_fns, vec!["publish_release"]);
        let q2 = &entries[1];
        let q2_fns: Vec<String> = q2
            .get("invalidated_by")
            .and_then(|v| v.as_array())
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        assert_eq!(q2_fns, vec!["compute_lift_with_ci", "resolve_sample"]);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn diff_since_seal_groups_changes_by_zone() {
        let dir = test_dir("p7_zone_grouping");
        init_git(&dir);
        // Seed commit so HEAD~1 is available and baseline has a predecessor.
        commit_file(&dir, "seed.txt", "seed");
        // Seed a file in the src/ zone so the zone map validates downstream.
        let baseline_sha = commit_and_sha(&dir, "src/existing.rs", "fn a() {}\n");

        write_relevance_json(
            &dir,
            r#"{"zones":[
              {"paths":["src/**"],"affects":["20_CODE_MAP.md"]},
              {"paths":["docs/**"],"affects":["10_SYSTEM_OVERVIEW.md"]}
            ]}"#,
        );
        seed_manifest(&dir, &baseline_sha);

        // Add a src file and a docs file AFTER seal.
        commit_file(&dir, "src/new_module.rs", "fn b() {}\n");
        commit_file(&dir, "docs/intro.md", "# Intro\n");

        let out = diff_since_seal(&dir, None).expect("diff_since_seal must succeed");
        let zones = out
            .value
            .get("zones")
            .and_then(|v| v.as_array())
            .expect("zones array");
        assert_eq!(zones.len(), 2, "expected 2 populated zones, got {zones:?}");

        // Check each zone carries its own changed_files + affects.
        let find_zone = |affect: &str| -> &Value {
            zones
                .iter()
                .find(|z| {
                    z.get("affects")
                        .and_then(|v| v.as_array())
                        .map(|a| a.iter().any(|s| s.as_str() == Some(affect)))
                        .unwrap_or(false)
                })
                .expect("zone present")
        };
        let src_zone = find_zone("20_CODE_MAP.md");
        let src_changed: Vec<String> = src_zone
            .get("changed_files")
            .and_then(|v| v.as_array())
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        assert!(
            src_changed.iter().any(|f| f == "src/new_module.rs"),
            "src zone must include src/new_module.rs, got {src_changed:?}"
        );
        let docs_zone = find_zone("10_SYSTEM_OVERVIEW.md");
        let docs_changed: Vec<String> = docs_zone
            .get("changed_files")
            .and_then(|v| v.as_array())
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        assert_eq!(docs_changed, vec!["docs/intro.md"]);

        // P2 degradation: empty drift/count/delete arrays present and typed.
        for zone in zones {
            for field in ["signature_drifts", "count_deltas", "deleted_files"] {
                assert!(
                    zone.get(field).and_then(|v| v.as_array()).is_some(),
                    "zone must emit {field} as an array (possibly empty), zone={zone:?}"
                );
            }
        }

        // Recommended actions should include a re-seal reminder since pack
        // wasn't updated in this diff.
        let actions: Vec<String> = out
            .value
            .get("recommended_reconciliation_actions")
            .and_then(|v| v.as_array())
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        assert!(
            actions.iter().any(|s| s.contains("Re-seal")),
            "expected a re-seal reminder in actions, got {actions:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn verify_ci_fails_when_acceptance_test_invalidated_and_pack_stale() {
        // End-to-end: signature drift in a function listed by
        // acceptance_tests.md `invalidated_by` should make `verify --ci`
        // exit non-zero when the pack wasn't updated. We prove this by
        // inspecting the computed diff payload directly — going through the
        // actual `verify` would std::process::exit, which we can't capture
        // in a unit test.
        let dir = test_dir("p7_ci_gate");
        init_git(&dir);
        commit_file(&dir, "seed.txt", "s");
        // Seed a file in the src zone so the initial zone passes validation.
        let baseline_sha = commit_and_sha(&dir, "src/lib.rs", "fn compute_lift_with_ci() {}\n");
        write_relevance_json(
            &dir,
            r#"{"zones":[{"paths":["src/**"],"affects":["20_CODE_MAP.md"]}]}"#,
        );

        // Manually craft a drift payload inside the manifest so we simulate
        // what a P2-integrated pipeline would produce. diff_since_seal doesn't
        // (yet) produce drifts itself (TODO(P2-integration)), so we exercise
        // the matcher + gate directly.
        let bindings = vec![json!({
            "test_id": "Q: rename impact",
            "invalidated_by": ["compute_lift_with_ci"],
        })];
        let drifts = vec![json!({"fn": "compute_lift_with_ci"})];
        let invalidated = match_invalidated_tests(&drifts, &bindings);
        assert_eq!(
            invalidated.len(),
            1,
            "matcher must flag the drifted acceptance test"
        );

        // And confirm diff_since_seal returns pack_updated=false in this
        // scenario (pack wasn't touched after seal).
        seed_manifest(&dir, &baseline_sha);
        commit_file(&dir, "src/new.rs", "fn b() {}\n");
        let out = diff_since_seal(&dir, None).expect("diff_since_seal must succeed");
        assert_eq!(
            out.value.get("pack_updated").and_then(|v| v.as_bool()),
            Some(false),
            "expected pack_updated=false when only code changed"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_seal_baseline_sha_prefers_post_commit_sha() {
        let with_post = json!({
            "head_sha_at_seal": "aaaa",
            "post_commit_sha": "bbbb",
        });
        assert_eq!(resolve_seal_baseline_sha(&with_post).as_deref(), Some("bbbb"));

        let only_seal = json!({
            "head_sha_at_seal": "aaaa",
            "post_commit_sha": null,
        });
        assert_eq!(resolve_seal_baseline_sha(&only_seal).as_deref(), Some("aaaa"));

        let neither = json!({"foo": "bar"});
        assert!(resolve_seal_baseline_sha(&neither).is_none());
    }

    #[test]
    fn match_invalidated_tests_surfaces_drifted_function() {
        // Simulate P4 acceptance-test schema + a P2-style drift payload.
        let bindings = vec![json!({
            "test_id": "Q2: what changes for a rename?",
            "invalidated_by": ["compute_lift_with_ci", "resolve_sample"],
        })];
        // Drift shape 1: {file, fn}
        let drifts_obj = vec![json!({"file": "src/lib.rs", "fn": "compute_lift_with_ci"})];
        let hits = match_invalidated_tests(&drifts_obj, &bindings);
        assert_eq!(hits.len(), 1, "expected one matched test, got {hits:?}");
        let matched: Vec<String> = hits[0]
            .get("matched_drifts")
            .and_then(|v| v.as_array())
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        assert!(matched.contains(&"compute_lift_with_ci".to_string()));

        // Drift shape 2: bare string. Still matches.
        let drifts_str = vec![json!("resolve_sample")];
        let hits2 = match_invalidated_tests(&drifts_str, &bindings);
        assert_eq!(hits2.len(), 1);

        // Non-matching drift yields no invalidation.
        let drifts_none = vec![json!("unrelated_fn")];
        let hits3 = match_invalidated_tests(&drifts_none, &bindings);
        assert!(hits3.is_empty(), "unrelated drift must not match, got {hits3:?}");
    }
}
