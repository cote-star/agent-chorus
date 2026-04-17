mod adapters;
mod agents;
mod agent_context;
pub mod diff;
pub mod messaging;
pub mod relevance;
mod report;
mod utils;
pub mod update_check;
mod teardown;


use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use serde_json::json;

#[derive(Parser)]
#[command(name = "chorus")]
#[command(about = "Agent Chorus CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Read a session from an agent
    Read {
        /// Agent to read from
        #[arg(long, value_enum)]
        agent: AgentType,

        /// Session ID or UUID (substring match supported)
        #[arg(long)]
        id: Option<String>,

        /// Working directory to scope search (defaults to current directory)
        #[arg(long)]
        cwd: Option<String>,

        /// Explicit path to chats directory (Gemini only)
        #[arg(long)]
        chats_dir: Option<String>,

        /// Number of last assistant messages to return
        #[arg(long, default_value = "1")]
        last: usize,

        /// Emit structured JSON instead of text
        #[arg(long)]
        json: bool,

        /// Return session metadata only (no content)
        #[arg(long)]
        metadata_only: bool,

        /// Include redaction audit trail in output
        #[arg(long)]
        audit_redactions: bool,
    },

    /// Compare sources and return an analyze-mode report
    Compare {
        /// Source spec: <agent> or <agent>:<session-substring>
        #[arg(long = "source", required = true)]
        sources: Vec<String>,

        /// Working directory to scope current-session lookups
        #[arg(long)]
        cwd: Option<String>,

        /// Emit structured JSON instead of markdown
        #[arg(long)]
        json: bool,

        /// Number of last messages to read from each source
        #[arg(long, default_value = "10")]
        last: usize,
    },

    /// Build a report from a handoff packet JSON file
    Report {
        /// Path to handoff JSON file
        #[arg(long)]
        handoff: String,

        /// Working directory fallback for source lookups
        #[arg(long)]
        cwd: Option<String>,

        /// Emit structured JSON instead of markdown
        #[arg(long)]
        json: bool,
    },

    /// List sessions for an agent
    List {
        /// Agent to list sessions for
        #[arg(long, value_enum)]
        agent: AgentType,

        /// Working directory to scope search
        #[arg(long)]
        cwd: Option<String>,

        /// Maximum number of sessions to return
        #[arg(long, default_value = "10")]
        limit: usize,

        /// Emit structured JSON instead of text
        #[arg(long)]
        json: bool,
    },

    /// Search sessions for a keyword
    Search {
        /// Keyword to search for
        #[arg(index = 1)]
        query: String,

        /// Agent to search
        #[arg(long, value_enum)]
        agent: AgentType,

        /// Working directory to scope search
        #[arg(long)]
        cwd: Option<String>,

        /// Maximum number of sessions to return
        #[arg(long, default_value = "10")]
        limit: usize,

        /// Emit structured JSON instead of text
        #[arg(long)]
        json: bool,
    },

    /// Roast agents based on their session content (easter egg)
    #[command(name = "trash-talk")]
    TrashTalk {
        /// Working directory to scope search
        #[arg(long)]
        cwd: Option<String>,
    },

    /// Reverse setup: remove managed blocks, scaffolding, and hooks
    Teardown {
        /// Working directory (default: current directory)
        #[arg(long)]
        cwd: Option<String>,

        /// Preview changes without executing
        #[arg(long)]
        dry_run: bool,

        /// Also remove global cache (~/.cache/agent-chorus/)
        #[arg(long)]
        global: bool,

        /// Emit structured JSON instead of text
        #[arg(long)]
        json: bool,
    },

    /// Build/sync/install agent-context automation
    #[command(name = "agent-context")]
    AgentContext {
        #[command(subcommand)]
        command: ContextPackCommand,
    },

    /// Deprecated alias for agent-context
    #[command(name = "context-pack", hide = true)]
    ContextPack {
        #[command(subcommand)]
        command: ContextPackCommand,
    },

    /// Compare two sessions from the same agent
    Diff {
        /// Agent to diff sessions for
        #[arg(long, value_enum)]
        agent: AgentType,

        /// First session ID (substring match)
        #[arg(long)]
        from: String,

        /// Second session ID (substring match)
        #[arg(long)]
        to: String,

        /// Working directory to scope search
        #[arg(long)]
        cwd: Option<String>,

        /// Number of last assistant messages per session
        #[arg(long, default_value = "1")]
        last: usize,

        /// Emit structured JSON instead of text
        #[arg(long)]
        json: bool,
    },

    /// Inspect relevance patterns for context-pack filtering
    Relevance {
        /// List current include/exclude patterns
        #[arg(long)]
        list: bool,

        /// Test whether a specific file path is relevant
        #[arg(long)]
        test: Option<String>,

        /// Suggest patterns based on detected project conventions
        #[arg(long)]
        suggest: bool,

        /// Working directory (default: current directory)
        #[arg(long)]
        cwd: Option<String>,

        /// Emit structured JSON instead of text
        #[arg(long)]
        json: bool,
    },

    /// Send a message from one agent to another
    Send {
        /// Sending agent
        #[arg(long)]
        from: String,

        /// Target agent
        #[arg(long)]
        to: String,

        /// Message content
        #[arg(long)]
        message: String,

        /// Working directory (default: current directory)
        #[arg(long)]
        cwd: Option<String>,

        /// Emit structured JSON instead of text
        #[arg(long)]
        json: bool,
    },

    /// Read messages for an agent
    Messages {
        /// Agent whose messages to read
        #[arg(long)]
        agent: String,

        /// Working directory (default: current directory)
        #[arg(long)]
        cwd: Option<String>,

        /// Clear messages after reading
        #[arg(long)]
        clear: bool,

        /// Emit structured JSON instead of text
        #[arg(long)]
        json: bool,
    },

    #[cfg(feature = "update-check")]
    #[command(hide = true)]
    UpdateWorker,
}

#[derive(Subcommand)]
enum ContextPackCommand {
    /// Build or refresh context pack files
    Build {
        /// Build reason (metadata only)
        #[arg(long)]
        reason: Option<String>,

        /// Base SHA for changed-file computation
        #[arg(long)]
        base: Option<String>,

        /// Head SHA for changed-file computation
        #[arg(long)]
        head: Option<String>,

        /// Override pack directory (default: .agent-context or CHORUS_CONTEXT_PACK_DIR)
        #[arg(long)]
        pack_dir: Option<String>,

        /// Explicit changed file (repeatable)
        #[arg(long = "changed-file")]
        changed_files: Vec<String>,

        /// Force creating a new snapshot even when unchanged
        #[arg(long)]
        force_snapshot: bool,
    },

    /// Sync context pack during a main-branch push event
    #[command(name = "sync-main")]
    SyncMain {
        #[arg(long)]
        local_ref: String,

        #[arg(long)]
        local_sha: String,

        #[arg(long)]
        remote_ref: String,

        #[arg(long)]
        remote_sha: String,
    },

    /// Install/refresh pre-push hook wiring
    #[command(name = "install-hooks")]
    InstallHooks {
        /// Target directory inside repo (default: current directory)
        #[arg(long)]
        cwd: Option<String>,

        /// Preview changes without writing
        #[arg(long)]
        dry_run: bool,
    },

    /// Restore context pack from snapshot
    Rollback {
        /// Snapshot ID (default: latest)
        #[arg(long)]
        snapshot: Option<String>,

        /// Override pack directory (default: .agent-context or CHORUS_CONTEXT_PACK_DIR)
        #[arg(long)]
        pack_dir: Option<String>,
    },

    /// Verify context pack integrity (checksums)
    Verify {
        /// Override pack directory (default: .agent-context or CHORUS_CONTEXT_PACK_DIR)
        #[arg(long)]
        pack_dir: Option<String>,

        /// Working directory (default: current directory)
        #[arg(long)]
        cwd: Option<String>,

        /// CI mode: output JSON with integrity + freshness results, exit 1 on failure
        #[arg(long)]
        ci: bool,

        /// Base ref for freshness check (default: origin/main)
        #[arg(long)]
        base: Option<String>,

        /// Recover from a corrupt manifest by restoring the most recent intact
        /// snapshot. Prompts for confirmation unless --yes is given.
        #[arg(long)]
        repair: bool,

        /// Skip the interactive confirmation prompt when running --repair.
        #[arg(long = "yes")]
        repair_yes: bool,

        /// P3: emit a JSON payload with changed_files, pack_sections_to_update,
        /// a capped diff excerpt, and a reserved baseline_drift array. Used by
        /// agents to target which pack sections to patch.
        #[arg(long = "suggest-patches")]
        suggest_patches: bool,
    },

    /// Warn when context-relevant files changed without pack update
    #[command(name = "check-freshness")]
    CheckFreshness {
        /// Base ref for diff (default: origin/main)
        #[arg(long)]
        base: Option<String>,

        /// Working directory (default: current directory)
        #[arg(long)]
        cwd: Option<String>,
    },

    /// Initialize context pack templates
    Init {
        /// Override pack directory (default: .agent-context or CHORUS_CONTEXT_PACK_DIR)
        #[arg(long)]
        pack_dir: Option<String>,

        /// Working directory (default: current directory)
        #[arg(long)]
        cwd: Option<String>,

        /// Overwrite existing template files
        #[arg(long)]
        force: bool,

        /// Dereference symlinks whose targets resolve outside the repo root.
        /// Off by default so a rogue symlink cannot exfiltrate content into
        /// the pack. Opt in only for repos that rely on out-of-tree sources.
        #[arg(long)]
        follow_symlinks: bool,
    },

    /// Validate and seal an agent-authored context pack
    Seal {
        /// Seal reason (metadata only)
        #[arg(long)]
        reason: Option<String>,

        /// Base SHA for changed-file computation
        #[arg(long)]
        base: Option<String>,

        /// Head SHA for changed-file computation
        #[arg(long)]
        head: Option<String>,

        /// Override pack directory (default: .agent-context or CHORUS_CONTEXT_PACK_DIR)
        #[arg(long)]
        pack_dir: Option<String>,

        /// Working directory (default: current directory)
        #[arg(long)]
        cwd: Option<String>,

        /// Seal even if template markers remain
        #[arg(long)]
        force: bool,

        /// Force creating a new snapshot even when unchanged
        #[arg(long)]
        force_snapshot: bool,

        /// Dereference symlinks whose targets resolve outside the repo root.
        /// Off by default; turn on only when an out-of-tree source must be
        /// read into the pack. Seal will still warn on skipped files.
        #[arg(long)]
        follow_symlinks: bool,
    },
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum AgentType {
    Codex,
    Gemini,
    Claude,
    Cursor,
}

impl AgentType {
    fn as_str(&self) -> &'static str {
        match self {
            AgentType::Codex => "codex",
            AgentType::Gemini => "gemini",
            AgentType::Claude => "claude",
            AgentType::Cursor => "cursor",
        }
    }
}

fn main() {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            // If --json was passed on the command line, emit structured error
            let raw_args: Vec<String> = std::env::args().collect();
            let has_json = raw_args.iter().any(|a| a == "--json");
            if has_json {
                let msg = e.to_string();
                // Detect unsupported agent from clap's error message
                let code = if msg.contains("invalid value") && msg.contains("--agent") {
                    agents::ChorusErrorCode::UnsupportedAgent
                } else {
                    agents::classify_error(&msg)
                };
                let error_json = serde_json::json!({
                    "error_code": code.as_str(),
                    "message": msg.to_string().lines().next().unwrap_or(""),
                });
                println!("{}", serde_json::to_string_pretty(&error_json).unwrap_or_default());
                std::process::exit(1);
            } else {
                e.exit();
            }
        }
    };
    let json_mode = is_json_mode(&cli.command);

    if let Err(err) = run(cli) {
        if json_mode {
            let msg = format!("{:#}", err);
            let code = agents::classify_error(&msg);
            let error_json = serde_json::json!({
                "error_code": code.as_str(),
                "message": msg,
            });
            println!("{}", serde_json::to_string_pretty(&error_json).unwrap_or_default());
        } else {
            eprintln!("{:#}", err);
        }
        std::process::exit(1);
    }
}

fn is_json_mode(command: &Commands) -> bool {
    match command {
        Commands::Read { json, .. } => *json,
        Commands::Compare { json, .. } => *json,
        Commands::Report { json, .. } => *json,
        Commands::List { json, .. } => *json,
        Commands::Search { json, .. } => *json,
        Commands::TrashTalk { .. } => false,
        Commands::Diff { json, .. } => *json,
        Commands::Relevance { json, .. } => *json,
        Commands::Send { json, .. } => *json,
        Commands::Messages { json, .. } => *json,
        Commands::Teardown { json, .. } => *json,
        Commands::AgentContext { .. } | Commands::ContextPack { .. } => false,
        #[cfg(feature = "update-check")]
        Commands::UpdateWorker => false,
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Read {
            agent,
            id,
            cwd,
            chats_dir,
            last,
            json,
            metadata_only,
            audit_redactions,
        } => {
            let effective_cwd = effective_cwd(cwd);
            let last_n = last.max(1);
            let adapter = adapters::get_adapter(agent.as_str())
                .with_context(|| format!("Unsupported agent: {}", agent.as_str()))?;
            let session = adapter.read_session(
                id.as_deref(),
                &effective_cwd,
                chats_dir.as_deref(),
                last_n,
            )?;

            // If audit mode requested, re-run redaction with audit on the raw content
            let redaction_audit = if audit_redactions {
                let (_, audit) = agents::redact_sensitive_text_with_audit(&session.content);
                Some(audit)
            } else {
                None
            };

            if json {
                let content_value = if metadata_only {
                    serde_json::Value::Null
                } else {
                    serde_json::Value::String(session.content.clone())
                };
                let mut report = json!({
                    "chorus_output_version": 1,
                    "agent": session.agent,
                    "source": session.source,
                    "content": content_value,
                    "warnings": session.warnings,
                    "session_id": session.session_id,
                    "cwd": session.cwd,
                    "timestamp": session.timestamp,
                    "message_count": session.message_count,
                    "messages_returned": session.messages_returned,
                });
                if let Some(ref audit) = redaction_audit {
                    report.as_object_mut().unwrap().insert(
                        "redactions".to_string(),
                        serde_json::to_value(audit)?,
                    );
                }
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                for warning in &session.warnings {
                    eprintln!("{}", utils::sanitize_for_terminal(warning));
                }
                println!("--- BEGIN CHORUS OUTPUT ---");
                println!("SOURCE: {} Session ({})", format_agent_name(session.agent), utils::sanitize_for_terminal(&session.source));
                if !metadata_only {
                    println!("---");
                    println!("{}", utils::sanitize_for_terminal(&session.content));
                }
                if let Some(ref audit) = redaction_audit {
                    if !audit.is_empty() {
                        println!("---");
                        println!("Redaction audit:");
                        for entry in audit {
                            println!("  {} — {} occurrence(s)", entry.pattern, entry.count);
                        }
                    }
                }
                println!("--- END CHORUS OUTPUT ---");
            }
        }
        Commands::Compare { sources, cwd, json, last } => {
            let effective_cwd = effective_cwd(cwd);
            let mut source_specs = sources
                .iter()
                .map(|raw| report::parse_source_arg(raw))
                .collect::<Result<Vec<report::SourceSpec>>>()?;

            for spec in &mut source_specs {
                spec.last_n = Some(last.max(1));
            }

            let request = report::ReportRequest {
                mode: "analyze".to_string(),
                task: "Compare agent outputs".to_string(),
                success_criteria: vec![
                    "Identify agreements and contradictions".to_string(),
                    "Highlight unavailable sources".to_string(),
                ],
                sources: source_specs,
                constraints: Vec::new(),
            };

            let result = report::build_report(&request, &effective_cwd);
            emit_report_output(&result, json)?;
        }
        Commands::Report { handoff, cwd, json } => {
            let effective_cwd = effective_cwd(cwd);
            let request = report::load_handoff(&handoff)
                .with_context(|| format!("Failed to load handoff packet from {}", handoff))?;
            let result = report::build_report(&request, &effective_cwd);
            emit_report_output(&result, json)?;
        }
        Commands::List { agent, cwd, limit, json } => {
            let normalized_cwd = cwd.map(|value| {
                utils::normalize_path(&value)
                    .map(|path| path.to_string_lossy().to_string())
                    .unwrap_or(value)
            });
            let adapter = adapters::get_adapter(agent.as_str())
                .with_context(|| format!("Unsupported agent: {}", agent.as_str()))?;
            let entries = adapter.list_sessions(normalized_cwd.as_deref(), limit)?;

            if json {
                println!("{}", serde_json::to_string_pretty(&entries)?);
            } else if entries.is_empty() {
                println!("No sessions found.");
            } else {
                println!("  {:<8} {:<12}  {:<24} CWD", "AGENT", "SESSION", "TIMESTAMP");
                println!("  {:<8} {:<12}  {:<24} {}", "─".repeat(8), "─".repeat(12), "─".repeat(24), "─".repeat(20));
                for entry in &entries {
                    let agent_name = entry.get("agent").and_then(|v| v.as_str()).unwrap_or("");
                    let sid = entry.get("session_id").and_then(|v| v.as_str()).unwrap_or("");
                    let sid_short = if sid.len() > 12 { &sid[..12] } else { sid };
                    let ts_raw = entry.get("modified_at").and_then(|v| v.as_str()).unwrap_or("unknown");
                    let ts = format_timestamp(ts_raw);
                    let cwd_val = entry.get("cwd").and_then(|v| v.as_str()).unwrap_or("");
                    let cwd_display = if cwd_val.is_empty() { String::new() } else { truncate_cwd(cwd_val) };
                    println!("  {:<8} {:<12}  {:<24} {}", agent_name, sid_short, ts, cwd_display);
                }
                println!("\n  {} session{} found.", entries.len(), if entries.len() == 1 { "" } else { "s" });
            }
        }
        Commands::Search { query, agent, cwd, limit, json } => {
            let normalized_cwd = cwd.map(|value| {
                utils::normalize_path(&value)
                    .map(|path| path.to_string_lossy().to_string())
                    .unwrap_or(value)
            });
            let adapter = adapters::get_adapter(agent.as_str())
                .with_context(|| format!("Unsupported agent: {}", agent.as_str()))?;
            let entries = adapter.search_sessions(&query, normalized_cwd.as_deref(), limit)?;

            if json {
                println!("{}", serde_json::to_string_pretty(&entries)?);
            } else if entries.is_empty() {
                println!("Search for \"{}\": no matching sessions found.", query);
            } else {
                println!("Search for \"{}\": {} result{}\n",
                    query, entries.len(), if entries.len() == 1 { "" } else { "s" });
                for entry in &entries {
                    let agent_name = entry.get("agent").and_then(|v| v.as_str()).unwrap_or("");
                    let sid = entry.get("session_id").and_then(|v| v.as_str()).unwrap_or("");
                    let sid_short = if sid.len() > 12 { &sid[..12] } else { sid };
                    let ts_raw = entry.get("modified_at").and_then(|v| v.as_str()).unwrap_or("unknown");
                    let ts = format_timestamp(ts_raw);
                    let snippet = entry.get("match_snippet")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .map(|s| format!("\n    \"{}\"", s.trim()))
                        .unwrap_or_default();
                    println!("  {:<8} {:<12}  {}{}", agent_name, sid_short, ts, snippet);
                }
            }
        }
        Commands::TrashTalk { cwd } => {
            let effective = effective_cwd(cwd);
            agents::trash_talk(&effective);
        }
        Commands::Diff { agent, from, to, cwd, last, json } => {
            let effective_cwd = effective_cwd(cwd);
            let last_n = last.max(1);
            let result = diff::diff_sessions(agent.as_str(), &from, &to, &effective_cwd, last_n)?;

            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("Diff: {} session {} vs {}", result.agent, result.session_a, result.session_b);
                println!("  +{} added, -{} removed, {} unchanged\n", result.added_lines, result.removed_lines, result.equal_lines);

                // Collapse consecutive equal runs > 5 lines
                const CONTEXT_LINES: usize = 2;
                let mut equal_run: Vec<&str> = Vec::new();

                let flush_equal_run = |run: &[&str]| {
                    if run.len() <= 5 {
                        for line in run {
                            println!("  {}", line);
                        }
                    } else {
                        for line in &run[..CONTEXT_LINES] {
                            println!("  {}", line);
                        }
                        println!("  ... ({} unchanged lines)", run.len() - 2 * CONTEXT_LINES);
                        for line in &run[run.len() - CONTEXT_LINES..] {
                            println!("  {}", line);
                        }
                    }
                };

                for hunk in &result.hunks {
                    match hunk.tag.as_str() {
                        "equal" => {
                            equal_run.push(&hunk.content);
                        }
                        _ => {
                            if !equal_run.is_empty() {
                                flush_equal_run(&equal_run);
                                equal_run.clear();
                            }
                            match hunk.tag.as_str() {
                                "add" => println!("+ {}", hunk.content),
                                "remove" => println!("- {}", hunk.content),
                                _ => println!("  {}", hunk.content),
                            }
                        }
                    }
                }
                // Flush remaining equal lines
                if !equal_run.is_empty() {
                    flush_equal_run(&equal_run);
                }
            }
        }
        Commands::Relevance { list, test, suggest, cwd, json } => {
            let effective = std::path::PathBuf::from(effective_cwd(cwd));

            if let Some(file_path) = test {
                let result = relevance::test_file(&effective, &file_path);
                if json {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                } else {
                    let status = if result.relevant { "RELEVANT" } else { "NOT RELEVANT" };
                    println!("{}: {}", result.path, status);
                    if let Some(ref matched) = result.matched_by {
                        println!("  matched by: {}", matched);
                    }
                }
            } else if suggest {
                let suggestions = relevance::suggest_patterns(&effective);
                if json {
                    println!("{}", serde_json::to_string_pretty(&suggestions)?);
                } else if suggestions.is_empty() {
                    println!("No additional pattern suggestions for this project.");
                } else {
                    for s in &suggestions {
                        println!("[{}] {} — {}", s.suggestion_type, s.pattern, s.reason);
                    }
                }
            } else if list {
                let info = relevance::list_patterns(&effective);
                if json {
                    println!("{}", serde_json::to_string_pretty(&info)?);
                } else {
                    println!("Source: {}", info.source);
                    println!("\nInclude:");
                    for p in &info.include {
                        println!("  {}", p);
                    }
                    println!("\nExclude:");
                    for p in &info.exclude {
                        println!("  {}", p);
                    }
                }
            } else {
                println!("Usage: chorus relevance --list | --test <path> | --suggest");
                println!("  Add --json for structured output");
                println!("  Add --cwd <dir> to specify working directory");
            }
        }
        Commands::Send { from, to, message, cwd, json } => {
            let effective = effective_cwd(cwd);
            let msg = messaging::send_message(&from, &to, &message, &effective)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&msg)?);
            } else {
                println!("Message sent from {} to {} at {}", msg.from, msg.to, msg.timestamp);
            }
        }
        Commands::Messages { agent, cwd, clear, json } => {
            let effective = effective_cwd(cwd);
            let messages = messaging::read_messages(&agent, &effective)?;

            if json {
                println!("{}", serde_json::to_string_pretty(&messages)?);
            } else if messages.is_empty() {
                println!("No messages for {}.", agent);
            } else {
                for msg in &messages {
                    println!("[{}] from={} → to={}: {}", msg.timestamp, msg.from, msg.to, msg.content);
                }
            }

            if clear {
                let count = messaging::clear_messages(&agent, &effective)?;
                if !json {
                    println!("Cleared {} message(s).", count);
                }
            }
        }
        Commands::Teardown { cwd, dry_run, json, global } => {
            let effective_cwd = effective_cwd(cwd);
            let result = teardown::run_teardown(&effective_cwd, dry_run, global)?;

            if json {
                println!("{}", serde_json::to_string_pretty(&json!({
                    "cwd": result.cwd,
                    "dry_run": result.dry_run,
                    "global": result.global,
                    "operations": result.operations,
                    "warnings": result.warnings,
                    "changed": result.changed,
                }))?);
            } else {
                let mode = if result.dry_run { "(dry run) " } else { "" };
                println!("Agent Chorus teardown {}complete for {}", mode, result.cwd);
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
        }
        Commands::ContextPack { command } => {
            eprintln!("Warning: 'context-pack' is deprecated, use 'agent-context' instead.");
            handle_context_pack(command)?;
        }
        Commands::AgentContext { command } => {
            handle_context_pack(command)?;
        }
        #[cfg(feature = "update-check")]
        Commands::UpdateWorker => {
            update_check::run_worker();
        }
    }

    #[cfg(feature = "update-check")]
    update_check::maybe_notify(&cli.command);

    Ok(())
}

fn handle_context_pack(command: ContextPackCommand) -> Result<()> {
    match command {
        ContextPackCommand::Build {
            reason,
            base,
            head,
            pack_dir,
            changed_files,
            force_snapshot,
        } => {
            agent_context::build(agent_context::BuildOptions {
                reason,
                base,
                head,
                pack_dir,
                changed_files,
                force_snapshot,
            })?;
        }
        ContextPackCommand::SyncMain {
            local_ref,
            local_sha,
            remote_ref,
            remote_sha,
        } => {
            agent_context::sync_main(
                &local_ref,
                &local_sha,
                &remote_ref,
                &remote_sha,
            )?;
        }
        ContextPackCommand::InstallHooks { cwd, dry_run } => {
            let target_cwd = effective_cwd(cwd);
            agent_context::install_hooks(&target_cwd, dry_run)?;
        }
        ContextPackCommand::Rollback { snapshot, pack_dir } => {
            agent_context::rollback(snapshot.as_deref(), pack_dir.as_deref())?;
        }
        ContextPackCommand::Verify { pack_dir, cwd, ci, base, repair, repair_yes, suggest_patches } => {
            let target_cwd = effective_cwd(cwd);
            agent_context::verify(agent_context::VerifyOptions {
                pack_dir,
                cwd: target_cwd,
                ci,
                base,
                repair,
                repair_yes,
                suggest_patches,
            })?;
        }
        ContextPackCommand::CheckFreshness { base, cwd } => {
            let target_cwd = effective_cwd(cwd);
            agent_context::check_freshness(
                base.as_deref().unwrap_or("origin/main"),
                &target_cwd,
            )?;
        }
        ContextPackCommand::Init {
            pack_dir,
            cwd,
            force,
            follow_symlinks,
        } => {
            agent_context::init(agent_context::InitOptions {
                pack_dir,
                cwd,
                force,
                follow_symlinks,
            })?;
        }
        ContextPackCommand::Seal {
            reason,
            base,
            head,
            pack_dir,
            cwd,
            force,
            force_snapshot,
            follow_symlinks,
        } => {
            agent_context::seal(agent_context::SealOptions {
                reason,
                base,
                head,
                pack_dir,
                cwd,
                force,
                force_snapshot,
                follow_symlinks,
            })?;
        }
    }
    Ok(())
}

fn emit_report_output(report_value: &serde_json::Value, json_output: bool) -> Result<()> {
    if json_output {
        println!("{}", serde_json::to_string_pretty(report_value)?);
    } else {
        println!("{}", utils::sanitize_for_terminal(&report::report_to_markdown(report_value)));
    }
    Ok(())
}

fn effective_cwd(cwd: Option<String>) -> String {
    cwd.unwrap_or_else(|| {
        std::env::current_dir()
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_else(|_| ".".to_string())
    })
}

fn format_agent_name(agent: &str) -> &'static str {
    match agent {
        "codex" => "Codex",
        "gemini" => "Gemini",
        "claude" => "Claude",
        "cursor" => "Cursor",
        _ => "Unknown",
    }
}

/// Format an ISO 8601 timestamp as a locale-like string (e.g., "3/17/2026, 4:54:38 PM").
fn format_timestamp(iso: &str) -> String {
    // Parse "YYYY-MM-DDThh:mm:ss" prefix
    let parts: Vec<&str> = iso.splitn(2, 'T').collect();
    if parts.len() < 2 {
        return iso.to_string();
    }
    let date_parts: Vec<&str> = parts[0].split('-').collect();
    let time_str = parts[1].trim_end_matches('Z').split('.').next().unwrap_or("");
    let time_parts: Vec<&str> = time_str.split(':').collect();
    if date_parts.len() < 3 || time_parts.len() < 3 {
        return iso.to_string();
    }
    let year: u32 = date_parts[0].parse().unwrap_or(0);
    let month: u32 = date_parts[1].parse().unwrap_or(0);
    let day: u32 = date_parts[2].parse().unwrap_or(0);
    let hour: u32 = time_parts[0].parse().unwrap_or(0);
    let minute: u32 = time_parts[1].parse().unwrap_or(0);
    let second: u32 = time_parts[2].parse().unwrap_or(0);

    let (h12, ampm) = if hour == 0 {
        (12, "AM")
    } else if hour < 12 {
        (hour, "AM")
    } else if hour == 12 {
        (12, "PM")
    } else {
        (hour - 12, "PM")
    };

    format!("{}/{}/{}, {}:{:02}:{:02} {}", month, day, year, h12, minute, second, ampm)
}

/// Truncate a CWD path: if >3 segments, show …/last/two.
fn truncate_cwd(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() <= 3 {
        return path.to_string();
    }
    format!("…/{}", parts[parts.len() - 2..].join("/"))
}
