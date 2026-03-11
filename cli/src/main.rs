mod adapters;
mod agents;
mod context_pack;
#[cfg(feature = "relevance")]
pub mod relevance;
mod report;
mod utils;
pub mod update_check;


use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use serde_json::json;

#[derive(Parser)]
#[command(name = "bridge")]
#[command(about = "Agent Bridge CLI", long_about = None)]
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
    },

    /// Compare sources and return an analyze-mode report
    Compare {
        /// Source spec: <agent> or <agent>:<session-substring>
        #[arg(long = "source", required = true)]
        sources: Vec<String>,

        /// Working directory to scope current-session lookups
        #[arg(long)]
        cwd: Option<String>,

        /// Apply whitespace normalization before comparing
        #[arg(long)]
        normalize: bool,

        /// Emit structured JSON instead of markdown
        #[arg(long)]
        json: bool,
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

    /// Build/sync/install context-pack automation
    #[command(name = "context-pack")]
    ContextPack {
        #[command(subcommand)]
        command: ContextPackCommand,
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

        /// Override pack directory (default: .agent-context or BRIDGE_CONTEXT_PACK_DIR)
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

        /// Override pack directory (default: .agent-context or BRIDGE_CONTEXT_PACK_DIR)
        #[arg(long)]
        pack_dir: Option<String>,
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
        /// Override pack directory (default: .agent-context or BRIDGE_CONTEXT_PACK_DIR)
        #[arg(long)]
        pack_dir: Option<String>,

        /// Working directory (default: current directory)
        #[arg(long)]
        cwd: Option<String>,

        /// Overwrite existing template files
        #[arg(long)]
        force: bool,
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

        /// Override pack directory (default: .agent-context or BRIDGE_CONTEXT_PACK_DIR)
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
                    agents::BridgeErrorCode::UnsupportedAgent
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
        Commands::ContextPack { .. } => false,
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

            if json {
                let report = json!({
                    "agent": session.agent,
                    "source": session.source,
                    "content": session.content,
                    "warnings": session.warnings,
                    "session_id": session.session_id,
                    "cwd": session.cwd,
                    "timestamp": session.timestamp,
                    "message_count": session.message_count,
                    "messages_returned": session.messages_returned,
                });
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                for warning in &session.warnings {
                    eprintln!("{}", utils::sanitize_for_terminal(warning));
                }
                println!("SOURCE: {} Session ({})", format_agent_name(session.agent), utils::sanitize_for_terminal(&session.source));
                println!("---");
                println!("{}", utils::sanitize_for_terminal(&session.content));
            }
        }
        Commands::Compare { sources, cwd, normalize, json } => {
            let effective_cwd = effective_cwd(cwd);
            let source_specs = sources
                .iter()
                .map(|raw| report::parse_source_arg(raw))
                .collect::<Result<Vec<report::SourceSpec>>>()?;

            let request = report::ReportRequest {
                mode: "analyze".to_string(),
                task: "Compare agent outputs".to_string(),
                success_criteria: vec![
                    "Identify agreements and contradictions".to_string(),
                    "Highlight unavailable sources".to_string(),
                ],
                sources: source_specs,
                constraints: Vec::new(),
                normalize,
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
            } else {
                for entry in &entries {
                    println!("{}", serde_json::to_string(entry).unwrap_or_default());
                }
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
            } else {
                for entry in &entries {
                    println!("{}", serde_json::to_string(entry).unwrap_or_default());
                }
            }
        }
        Commands::TrashTalk { cwd } => {
            let effective = effective_cwd(cwd);
            agents::trash_talk(&effective);
        }
        Commands::ContextPack { command } => {
            match command {
                ContextPackCommand::Build {
                    reason,
                    base,
                    head,
                    pack_dir,
                    changed_files,
                    force_snapshot,
                } => {
                    context_pack::build(context_pack::BuildOptions {
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
                    context_pack::sync_main(
                        &local_ref,
                        &local_sha,
                        &remote_ref,
                        &remote_sha,
                    )?;
                }
                ContextPackCommand::InstallHooks { cwd, dry_run } => {
                    let target_cwd = effective_cwd(cwd);
                    context_pack::install_hooks(&target_cwd, dry_run)?;
                }
                ContextPackCommand::Rollback { snapshot, pack_dir } => {
                    context_pack::rollback(snapshot.as_deref(), pack_dir.as_deref())?;
                }
                ContextPackCommand::CheckFreshness { base, cwd } => {
                    let target_cwd = effective_cwd(cwd);
                    context_pack::check_freshness(
                        base.as_deref().unwrap_or("origin/main"),
                        &target_cwd,
                    )?;
                }
                ContextPackCommand::Init {
                    pack_dir,
                    cwd,
                    force,
                } => {
                    context_pack::init(context_pack::InitOptions {
                        pack_dir,
                        cwd,
                        force,
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
                } => {
                    context_pack::seal(context_pack::SealOptions {
                        reason,
                        base,
                        head,
                        pack_dir,
                        cwd,
                        force,
                        force_snapshot,
                    })?;
                }
            }
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
