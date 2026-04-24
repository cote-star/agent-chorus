use crate::adapters::ReadOptions;
use crate::utils::{expand_home, hash_path, normalize_path};
use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const MAX_FILE_SIZE: u64 = 50 * 1024 * 1024; // 50 MB
const MAX_SCAN_FILES: usize = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChorusErrorCode {
    NotFound,
    ParseFailed,
    InvalidHandoff,
    UnsupportedAgent,
    UnsupportedMode,
    IoError,
    EmptySession,
}

impl ChorusErrorCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NotFound => "NOT_FOUND",
            Self::ParseFailed => "PARSE_FAILED",
            Self::InvalidHandoff => "INVALID_HANDOFF",
            Self::UnsupportedAgent => "UNSUPPORTED_AGENT",
            Self::UnsupportedMode => "UNSUPPORTED_MODE",
            Self::IoError => "IO_ERROR",
            Self::EmptySession => "EMPTY_SESSION",
        }
    }
}

pub fn classify_error(message: &str) -> ChorusErrorCode {
    let lower = message.to_ascii_lowercase();
    if lower.contains("unsupported agent") || lower.contains("unknown agent") {
        ChorusErrorCode::UnsupportedAgent
    } else if lower.contains("unsupported mode") {
        ChorusErrorCode::UnsupportedMode
    } else if lower.contains("no") && lower.contains("session found") || lower.contains("not found") {
        ChorusErrorCode::NotFound
    } else if lower.contains("failed to parse") || lower.contains("failed to read") {
        ChorusErrorCode::ParseFailed
    } else if lower.contains("missing required") || lower.contains("invalid handoff") || lower.contains("must provide session_id") {
        ChorusErrorCode::InvalidHandoff
    } else if lower.contains("has no messages") || lower.contains("history is empty") {
        ChorusErrorCode::EmptySession
    } else {
        ChorusErrorCode::IoError
    }
}

#[derive(Debug)]
pub struct Session {
    pub agent: &'static str,
    pub content: String,
    pub source: String,
    pub warnings: Vec<String>,
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    pub timestamp: Option<String>,
    pub message_count: usize,
    pub messages_returned: usize,
}

#[derive(Clone)]
struct FileEntry {
    path: PathBuf,
    mtime_ns: u128,
}

#[allow(dead_code)]
pub fn read_codex_session(id: Option<&str>, cwd: &str) -> Result<Session> {
    read_codex_session_with_last(id, cwd, 1)
}

pub fn read_codex_session_with_last(id: Option<&str>, cwd: &str, last_n: usize) -> Result<Session> {
    read_codex_session_with_options(id, cwd, last_n, ReadOptions::default())
}

pub fn read_codex_session_with_options(
    id: Option<&str>,
    cwd: &str,
    last_n: usize,
    opts: ReadOptions,
) -> Result<Session> {
    let base_dir = codex_base_dir();
    if is_system_directory(&base_dir) {
        return Err(anyhow!("Refusing to scan system directory: {}", base_dir.display()));
    }
    if !base_dir.exists() {
        return Err(anyhow!("No Codex session found."));
    }

    let mut warnings = Vec::new();
    let target_file = if let Some(id_value) = id {
        let files = collect_matching_files(&base_dir, true, &|file_path| {
            has_extension(file_path, "jsonl") && path_contains(file_path, id_value)
        })?;
        files
            .first()
            .map(|f| f.path.clone())
            .context("No Codex session found.")?
    } else {
        let files = collect_matching_files(&base_dir, true, &|file_path| has_extension(file_path, "jsonl"))?;
        if files.is_empty() {
            return Err(anyhow!("No Codex session found."));
        }

        let expected_cwd = normalize_path(cwd)?;
        if let Some(scoped) = find_latest_by_cwd(&files, &expected_cwd, get_codex_session_cwd) {
            scoped
        } else {
            warnings.push(format!(
                "Warning: no Codex session matched cwd {}; falling back to latest session.",
                expected_cwd.display()
            ));
            files[0].path.clone()
        }
    };

    let parsed = parse_codex_jsonl(&target_file, last_n, opts)?;
    warnings.extend(parsed.warnings);

    Ok(Session {
        agent: "codex",
        content: parsed.content,
        source: target_file.to_string_lossy().to_string(),
        warnings,
        session_id: parsed.session_id,
        cwd: parsed.cwd,
        timestamp: parsed.timestamp,
        message_count: parsed.message_count,
        messages_returned: parsed.messages_returned,
    })
}

#[allow(dead_code)]
pub fn read_claude_session(id: Option<&str>, cwd: &str) -> Result<Session> {
    read_claude_session_with_last(id, cwd, 1)
}

pub fn read_claude_session_with_last(id: Option<&str>, cwd: &str, last_n: usize) -> Result<Session> {
    read_claude_session_with_options(id, cwd, last_n, ReadOptions::default())
}

pub fn read_claude_session_with_options(
    id: Option<&str>,
    cwd: &str,
    last_n: usize,
    opts: ReadOptions,
) -> Result<Session> {
    let base_dir = claude_base_dir();
    if is_system_directory(&base_dir) {
        return Err(anyhow!("Refusing to scan system directory: {}", base_dir.display()));
    }
    if !base_dir.exists() {
        return Err(anyhow!("Claude projects directory not found: {}", base_dir.display()));
    }

    let mut warnings = Vec::new();
    let target_file = if let Some(id_value) = id {
        let files = collect_matching_files(&base_dir, true, &|file_path| {
            has_extension(file_path, "jsonl") && path_contains(file_path, id_value)
        })?;
        files
            .first()
            .map(|f| f.path.clone())
            .context("No Claude session found.")?
    } else {
        let files = collect_matching_files(&base_dir, true, &|file_path| has_extension(file_path, "jsonl"))?;
        if files.is_empty() {
            return Err(anyhow!("No Claude session found."));
        }

        let expected_cwd = normalize_path(cwd)?;
        if let Some(scoped) = find_latest_by_cwd(&files, &expected_cwd, get_claude_session_cwd) {
            scoped
        } else {
            warnings.push(format!(
                "Warning: no Claude session matched cwd {}; falling back to latest session.",
                expected_cwd.display()
            ));
            files[0].path.clone()
        }
    };

    let parsed = parse_claude_jsonl(&target_file, last_n, opts)?;
    warnings.extend(parsed.warnings);

    Ok(Session {
        agent: "claude",
        content: parsed.content,
        source: target_file.to_string_lossy().to_string(),
        warnings,
        session_id: parsed.session_id,
        cwd: parsed.cwd,
        timestamp: parsed.timestamp,
        message_count: parsed.message_count,
        messages_returned: parsed.messages_returned,
    })
}

#[allow(dead_code)]
pub fn read_gemini_session(id: Option<&str>, cwd: &str, chats_dir: Option<&str>) -> Result<Session> {
    read_gemini_session_with_last(id, cwd, chats_dir, 1)
}

pub fn read_gemini_session_with_last(id: Option<&str>, cwd: &str, chats_dir: Option<&str>, last_n: usize) -> Result<Session> {
    read_gemini_session_with_options(id, cwd, chats_dir, last_n, ReadOptions::default())
}

pub fn read_gemini_session_with_options(
    id: Option<&str>,
    cwd: &str,
    chats_dir: Option<&str>,
    last_n: usize,
    opts: ReadOptions,
) -> Result<Session> {
    let dirs = resolve_gemini_chat_dirs(chats_dir, cwd)?;
    if dirs.is_empty() {
        return Err(anyhow!("{}", gemini_not_found_message("No Gemini session found. Searched chats directories:")));
    }

    let mut cross_project_warning: Option<String> = None;
    if dirs.len() > 1 && chats_dir.is_none() {
        cross_project_warning = Some(
            "Warning: Gemini sessions from multiple projects may be mixed. Use --chats-dir to scope to a specific project.".to_string()
        );
    }

    let target_file = if let Some(id_value) = id {
        let mut candidates = Vec::new();
        for dir in &dirs {
            let mut files = collect_matching_files(dir, false, &|file_path| {
                (has_extension(file_path, "json") || has_extension(file_path, "jsonl"))
                    && path_contains(file_path, id_value)
            })?;
            candidates.append(&mut files);
        }
        sort_files_by_mtime_desc(&mut candidates);
        match candidates.first().map(|f| f.path.clone()) {
            Some(path) => path,
            None => return Err(anyhow!("{}", gemini_not_found_message("No Gemini session found."))),
        }
    } else {
        let mut candidates = Vec::new();
        for dir in &dirs {
            let mut files = collect_matching_files(dir, false, &|file_path| {
                (has_extension(file_path, "json") || has_extension(file_path, "jsonl"))
                    && file_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|name| name.starts_with("session-"))
                        .unwrap_or(false)
            })?;
            candidates.append(&mut files);
        }
        sort_files_by_mtime_desc(&mut candidates);
        match candidates.first().map(|f| f.path.clone()) {
            Some(path) => path,
            None => return Err(anyhow!("{}", gemini_not_found_message("No Gemini session found."))),
        }
    };

    // Newer Gemini CLI writes line-delimited JSON (.jsonl) session files;
    // older ones write a single JSON document (.json). Dispatch by extension
    // so both reach the same Session shape downstream.
    let parsed = if has_extension(&target_file, "jsonl") {
        parse_gemini_jsonl(&target_file, last_n, opts)?
    } else {
        parse_gemini_json(&target_file, last_n, opts)?
    };

    let mut warnings = parsed.warnings;
    if let Some(w) = cross_project_warning {
        warnings.insert(0, w);
    }

    Ok(Session {
        agent: "gemini",
        content: parsed.content,
        source: target_file.to_string_lossy().to_string(),
        warnings,
        session_id: parsed.session_id,
        cwd: parsed.cwd,
        timestamp: parsed.timestamp,
        message_count: parsed.message_count,
        messages_returned: parsed.messages_returned,
    })
}

struct ParsedContent {
    content: String,
    warnings: Vec<String>,
    session_id: Option<String>,
    cwd: Option<String>,
    timestamp: Option<String>,
    message_count: usize,
    messages_returned: usize,
}

fn parse_codex_jsonl(path: &Path, last_n: usize, opts: ReadOptions) -> Result<ParsedContent> {
    let lines = read_jsonl_lines(path)?;
    let mut turns: Vec<ConversationTurn> = Vec::new();
    let mut assistant_msgs: Vec<String> = Vec::new();
    let mut skipped = 0usize;
    let mut session_cwd: Option<String> = None;
    let mut session_id: Option<String> = None;

    for line in &lines {
        match serde_json::from_str::<Value>(line) {
            Ok(json) => {
                if json["type"] == "session_meta" {
                    if let Some(cwd) = json["payload"]["cwd"].as_str() {
                        session_cwd = Some(cwd.to_string());
                    }
                    if let Some(id) = json["payload"]["session_id"].as_str() {
                        session_id = Some(id.to_string());
                    }
                }
                if json["type"] == "response_item" && json["payload"]["type"] == "message" {
                    let payload = &json["payload"];
                    let role = payload["role"].as_str().unwrap_or("").to_lowercase();
                    if role == "assistant" || role == "user" {
                        let raw_text = if opts.include_tool_calls {
                            extract_text_with_tool_calls(&payload["content"])
                        } else {
                            extract_text(&payload["content"])
                        };
                        let text = if raw_text.is_empty() {
                            "[No text content]".to_string()
                        } else {
                            raw_text
                        };
                        if role == "assistant" {
                            assistant_msgs.push(text.clone());
                        }
                        turns.push(ConversationTurn { role, text });
                    }
                } else if json["type"] == "event_msg" && json["payload"]["type"] == "agent_message" {
                    let payload = &json["payload"];
                    let text = if let Some(s) = payload["message"].as_str() {
                        s.to_string()
                    } else {
                        let extracted = extract_text(&payload["message"]);
                        if extracted.is_empty() {
                            "[No text content]".to_string()
                        } else {
                            extracted
                        }
                    };
                    assistant_msgs.push(text.clone());
                    turns.push(ConversationTurn { role: "assistant".to_string(), text });
                }
            }
            Err(_) => skipped += 1,
        }
    }

    let mut warnings = Vec::new();
    if skipped > 0 {
        warnings.push(format!(
            "Warning: skipped {} unparseable line(s) in {}",
            skipped,
            path.display()
        ));
    }

    let message_count = assistant_msgs.len();
    let timestamp = file_modified_iso(path);

    if session_id.is_none() {
        session_id = path.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string());
    }

    if !turns.is_empty() {
        if opts.include_user && !assistant_msgs.is_empty() {
            let selected = select_conversation_turns(&turns, last_n);
            let messages_returned = selected.len();
            let content = selected
                .iter()
                .map(|turn| format!("{}:\n{}", turn.role.to_uppercase(), turn.text))
                .collect::<Vec<String>>()
                .join("\n---\n");
            return Ok(ParsedContent {
                content: redact_sensitive_text(&content),
                warnings,
                session_id,
                cwd: session_cwd,
                timestamp,
                message_count,
                messages_returned,
            });
        }

        if last_n > 1 && !assistant_msgs.is_empty() {
            let start = assistant_msgs.len().saturating_sub(last_n);
            let selected: Vec<&String> = assistant_msgs[start..].iter().collect();
            let messages_returned = selected.len();
            let content = selected
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<&str>>()
                .join("\n---\n");
            return Ok(ParsedContent {
                content: redact_sensitive_text(&content),
                warnings,
                session_id,
                cwd: session_cwd,
                timestamp,
                message_count,
                messages_returned,
            });
        }

        // last assistant message, else fall back to last turn text
        let content = assistant_msgs
            .last()
            .cloned()
            .unwrap_or_else(|| turns.last().map(|t| t.text.clone()).unwrap_or_default());
        return Ok(ParsedContent {
            content: redact_sensitive_text(&content),
            warnings,
            session_id,
            cwd: session_cwd,
            timestamp,
            message_count,
            messages_returned: 1,
        });
    }

    Ok(ParsedContent {
        content: redact_sensitive_text(&format!(
            "Could not extract structured messages. Showing last 20 raw lines:\n{}",
            lines
                .iter()
                .rev()
                .take(20)
                .cloned()
                .collect::<Vec<String>>()
                .into_iter()
                .rev()
                .collect::<Vec<String>>()
                .join("\n")
        )),
        warnings,
        session_id,
        cwd: session_cwd,
        timestamp,
        message_count,
        messages_returned: 0,
    })
}

fn parse_claude_jsonl(path: &Path, last_n: usize, opts: ReadOptions) -> Result<ParsedContent> {
    let lines = read_jsonl_lines(path)?;
    let mut messages: Vec<String> = Vec::new();
    let mut turns: Vec<ConversationTurn> = Vec::new();
    let mut skipped = 0usize;
    let mut session_cwd: Option<String> = None;

    for line in &lines {
        match serde_json::from_str::<Value>(line) {
            Ok(json) => {
                if let Some(cwd) = json["cwd"].as_str() {
                    if session_cwd.is_none() {
                        session_cwd = Some(cwd.to_string());
                    }
                }

                let message = if json.get("message").is_some() {
                    &json["message"]
                } else {
                    &json
                };

                // Match Node's adapter: accept either envelope type or message.role
                let role_from_type = json["type"].as_str().unwrap_or("").to_lowercase();
                let role_from_msg = message["role"].as_str().unwrap_or("").to_lowercase();
                let role = if role_from_msg == "assistant" || role_from_msg == "user" {
                    role_from_msg
                } else if role_from_type == "assistant" || role_from_type == "user" {
                    role_from_type
                } else {
                    continue;
                };

                let content_field = if message.get("content").is_some() {
                    &message["content"]
                } else {
                    &json["content"]
                };
                let text = if opts.include_tool_calls {
                    extract_claude_content_with_tool_calls(content_field)
                } else {
                    extract_claude_text(content_field)
                };
                if !text.is_empty() {
                    if role == "assistant" {
                        messages.push(text.clone());
                    }
                    turns.push(ConversationTurn { role, text });
                }
            }
            Err(_) => skipped += 1,
        }
    }

    let mut warnings = Vec::new();
    if skipped > 0 {
        warnings.push(format!(
            "Warning: skipped {} unparseable line(s) in {}",
            skipped,
            path.display()
        ));
    }

    let message_count = messages.len();
    let timestamp = file_modified_iso(path);
    let session_id = path.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string());

    if !messages.is_empty() {
        if opts.include_user {
            let selected = select_conversation_turns(&turns, last_n);
            let messages_returned = selected.len();
            let content = selected
                .iter()
                .map(|t| format!("{}:\n{}", t.role.to_uppercase(), t.text))
                .collect::<Vec<String>>()
                .join("\n---\n");
            return Ok(ParsedContent {
                content: redact_sensitive_text(&content),
                warnings,
                session_id,
                cwd: session_cwd,
                timestamp,
                message_count,
                messages_returned,
            });
        }

        if last_n > 1 {
            let start = messages.len().saturating_sub(last_n);
            let selected: Vec<&String> = messages[start..].iter().collect();
            let messages_returned = selected.len();
            let content = selected.iter().map(|s| s.as_str()).collect::<Vec<&str>>().join("\n---\n");
            return Ok(ParsedContent {
                content: redact_sensitive_text(&content),
                warnings,
                session_id,
                cwd: session_cwd,
                timestamp,
                message_count,
                messages_returned,
            });
        }
        return Ok(ParsedContent {
            content: redact_sensitive_text(messages.last().unwrap()),
            warnings,
            session_id,
            cwd: session_cwd,
            timestamp,
            message_count,
            messages_returned: 1,
        });
    }

    Ok(ParsedContent {
        content: redact_sensitive_text(&format!(
            "Could not extract assistant messages. Showing last 20 raw lines:\n{}",
            lines
                .iter()
                .rev()
                .take(20)
                .cloned()
                .collect::<Vec<String>>()
                .into_iter()
                .rev()
                .collect::<Vec<String>>()
                .join("\n")
        )),
        warnings,
        session_id,
        cwd: session_cwd,
        timestamp,
        message_count,
        messages_returned: 0,
    })
}

fn parse_gemini_json(path: &Path, last_n: usize, opts: ReadOptions) -> Result<ParsedContent> {
    let meta = fs::metadata(path)?;
    if meta.len() > MAX_FILE_SIZE {
        return Err(anyhow!(
            "Skipped {} (exceeds {}MB size limit)",
            path.display(),
            MAX_FILE_SIZE / (1024 * 1024)
        ));
    }
    let raw_content = fs::read_to_string(path)?;
    let session: Value = serde_json::from_str(&raw_content)
        .map_err(|e| anyhow!("Failed to parse Gemini JSON: {}", e))?;

    let session_id = session["sessionId"].as_str().map(|s| s.to_string())
        .or_else(|| path.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string()));
    let timestamp = file_modified_iso(path);

    if let Some(messages) = session["messages"].as_array() {
        // Build turns array with user + assistant roles.
        let mut turns: Vec<ConversationTurn> = Vec::new();
        for msg in messages {
            let type_str = msg["type"]
                .as_str()
                .or_else(|| msg["role"].as_str())
                .unwrap_or("")
                .to_lowercase();
            let role = if type_str == "gemini" || type_str == "assistant" || type_str == "model" {
                "assistant"
            } else if type_str == "user" {
                "user"
            } else {
                continue;
            };
            // Gemini CLI stores content as string; API shape uses parts array.
            let text = if let Some(s) = msg["content"].as_str() {
                s.to_string()
            } else {
                let extracted_content = extract_text(&msg["content"]);
                if !extracted_content.is_empty() {
                    extracted_content
                } else {
                    let from_parts = extract_text(&msg["parts"]);
                    if from_parts.is_empty() {
                        "[No text content]".to_string()
                    } else {
                        from_parts
                    }
                }
            };
            turns.push(ConversationTurn { role: role.to_string(), text });
        }

        let assistant_msgs: Vec<String> = turns
            .iter()
            .filter(|t| t.role == "assistant")
            .map(|t| t.text.clone())
            .collect();
        let assistant_count = assistant_msgs.len();

        if opts.include_user && !assistant_msgs.is_empty() {
            let selected = select_conversation_turns(&turns, last_n);
            let messages_returned = selected.len();
            let content = selected
                .iter()
                .map(|t| format!("{}:\n{}", t.role.to_uppercase(), t.text))
                .collect::<Vec<String>>()
                .join("\n---\n");
            return Ok(ParsedContent {
                content: redact_sensitive_text(&content),
                warnings: Vec::new(),
                session_id,
                cwd: None,
                timestamp,
                message_count: assistant_count,
                messages_returned,
            });
        }

        if last_n > 1 && !assistant_msgs.is_empty() {
            let start = assistant_msgs.len().saturating_sub(last_n);
            let selected: Vec<&String> = assistant_msgs[start..].iter().collect();
            let messages_returned = selected.len();
            let content = selected.iter().map(|s| s.as_str()).collect::<Vec<&str>>().join("\n---\n");
            return Ok(ParsedContent {
                content: redact_sensitive_text(&content),
                warnings: Vec::new(),
                session_id,
                cwd: None,
                timestamp,
                message_count: assistant_count,
                messages_returned,
            });
        }

        if assistant_msgs.is_empty() {
            return Err(anyhow!("Gemini session has no assistant messages."));
        }
        return Ok(ParsedContent {
            content: redact_sensitive_text(assistant_msgs.last().unwrap()),
            warnings: Vec::new(),
            session_id,
            cwd: None,
            timestamp,
            message_count: assistant_count,
            messages_returned: 1,
        });
    }

    if let Some(history) = session["history"].as_array() {
        let extract_turn_text = |turn: &Value| -> String {
            let parts = &turn["parts"];
            if let Some(arr) = parts.as_array() {
                arr.iter().map(|part| part["text"].as_str().unwrap_or("")).collect::<Vec<&str>>().join("\n")
            } else if let Some(raw) = parts.as_str() {
                raw.to_string()
            } else {
                "[No text content]".to_string()
            }
        };

        let mut turns: Vec<ConversationTurn> = Vec::new();
        for t in history {
            let role = if t["role"].as_str().map(|r| r.eq_ignore_ascii_case("user")).unwrap_or(false) {
                "user"
            } else {
                "assistant"
            };
            let text = extract_turn_text(t);
            turns.push(ConversationTurn { role: role.to_string(), text });
        }

        let assistant_turns: Vec<String> = turns
            .iter()
            .filter(|t| t.role == "assistant")
            .map(|t| t.text.clone())
            .collect();
        let assistant_count = assistant_turns.len();

        if opts.include_user && !assistant_turns.is_empty() {
            let selected = select_conversation_turns(&turns, last_n);
            let messages_returned = selected.len();
            let content = selected
                .iter()
                .map(|t| format!("{}:\n{}", t.role.to_uppercase(), t.text))
                .collect::<Vec<String>>()
                .join("\n---\n");
            return Ok(ParsedContent {
                content: redact_sensitive_text(&content),
                warnings: Vec::new(),
                session_id,
                cwd: None,
                timestamp,
                message_count: assistant_count,
                messages_returned,
            });
        }

        if last_n > 1 && !assistant_turns.is_empty() {
            let start = assistant_turns.len().saturating_sub(last_n);
            let selected: Vec<&String> = assistant_turns[start..].iter().collect();
            let messages_returned = selected.len();
            let content = selected.iter().map(|s| s.as_str()).collect::<Vec<&str>>().join("\n---\n");
            return Ok(ParsedContent {
                content: redact_sensitive_text(&content),
                warnings: Vec::new(),
                session_id,
                cwd: None,
                timestamp,
                message_count: assistant_count,
                messages_returned,
            });
        }

        if assistant_turns.is_empty() {
            return Err(anyhow!("Gemini history is empty."));
        }
        return Ok(ParsedContent {
            content: redact_sensitive_text(assistant_turns.last().unwrap()),
            warnings: Vec::new(),
            session_id,
            cwd: None,
            timestamp,
            message_count: assistant_count,
            messages_returned: 1,
        });
    }

    Err(anyhow!(
        "Unknown Gemini session schema. Supported fields: messages, history."
    ))
}

/// Parse line-delimited JSON Gemini sessions (newer Gemini CLI layout).
///
/// Shape: one JSON object per line. Three kinds of lines:
///   - header:    `{"sessionId":..., "projectHash":..., "kind":"main"}` (first line)
///   - message:   `{"id":..., "timestamp":..., "type":"user"|"gemini", "content": <string|array>}`
///   - metadata:  `{"$set":{...}}` — skip
///
/// Gemini's streaming producer can emit the same assistant message twice
/// (once mid-stream, once final) with identical `id`. We dedupe on id while
/// preserving the first-seen order so the conversation reads naturally.
fn parse_gemini_jsonl(path: &Path, last_n: usize, opts: ReadOptions) -> Result<ParsedContent> {
    let meta = fs::metadata(path)?;
    if meta.len() > MAX_FILE_SIZE {
        return Err(anyhow!(
            "Skipped {} (exceeds {}MB size limit)",
            path.display(),
            MAX_FILE_SIZE / (1024 * 1024)
        ));
    }

    let lines = read_jsonl_lines(path)?;
    let mut session_id: Option<String> = None;
    let mut turns: Vec<ConversationTurn> = Vec::new();
    let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut skipped = 0usize;

    for line in &lines {
        let json: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };

        // Header line — capture sessionId and move on.
        if session_id.is_none() {
            if let Some(id) = json["sessionId"].as_str() {
                session_id = Some(id.to_string());
                continue;
            }
        }

        // Metadata events carry only a `$set` key — skip.
        if json.get("$set").is_some() && json.get("type").is_none() {
            continue;
        }

        let type_str = json["type"]
            .as_str()
            .or_else(|| json["role"].as_str())
            .unwrap_or("")
            .to_lowercase();
        let role = if type_str == "gemini" || type_str == "assistant" || type_str == "model" {
            "assistant"
        } else if type_str == "user" {
            "user"
        } else {
            continue;
        };

        // Dedupe streaming duplicates. Only messages with an `id` participate
        // in dedupe; if a message has no id we keep it.
        if let Some(id) = json["id"].as_str() {
            if !seen_ids.insert(id.to_string()) {
                continue;
            }
        }

        // Content is either a string (assistant final answer) or an array of
        // `{text: ...}` parts (user turn, or API-shape assistant turn).
        let text = if let Some(s) = json["content"].as_str() {
            s.to_string()
        } else {
            let from_content = extract_text(&json["content"]);
            if !from_content.is_empty() {
                from_content
            } else {
                let from_parts = extract_text(&json["parts"]);
                if from_parts.is_empty() {
                    "[No text content]".to_string()
                } else {
                    from_parts
                }
            }
        };

        turns.push(ConversationTurn { role: role.to_string(), text });
    }

    let session_id = session_id
        .or_else(|| path.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string()));
    let timestamp = file_modified_iso(path);

    let assistant_msgs: Vec<String> = turns
        .iter()
        .filter(|t| t.role == "assistant")
        .map(|t| t.text.clone())
        .collect();
    let assistant_count = assistant_msgs.len();

    let mut warnings = Vec::new();
    if skipped > 0 {
        warnings.push(format!(
            "Warning: skipped {} unparseable line(s) in {}",
            skipped,
            path.display()
        ));
    }

    if opts.include_user && !assistant_msgs.is_empty() {
        let selected = select_conversation_turns(&turns, last_n);
        let messages_returned = selected.len();
        let content = selected
            .iter()
            .map(|t| format!("{}:\n{}", t.role.to_uppercase(), t.text))
            .collect::<Vec<String>>()
            .join("\n---\n");
        return Ok(ParsedContent {
            content: redact_sensitive_text(&content),
            warnings,
            session_id,
            cwd: None,
            timestamp,
            message_count: assistant_count,
            messages_returned,
        });
    }

    if last_n > 1 && !assistant_msgs.is_empty() {
        let start = assistant_msgs.len().saturating_sub(last_n);
        let selected: Vec<&String> = assistant_msgs[start..].iter().collect();
        let messages_returned = selected.len();
        let content = selected.iter().map(|s| s.as_str()).collect::<Vec<&str>>().join("\n---\n");
        return Ok(ParsedContent {
            content: redact_sensitive_text(&content),
            warnings,
            session_id,
            cwd: None,
            timestamp,
            message_count: assistant_count,
            messages_returned,
        });
    }

    if assistant_msgs.is_empty() {
        return Err(anyhow!("Gemini session has no assistant messages."));
    }
    Ok(ParsedContent {
        content: redact_sensitive_text(assistant_msgs.last().unwrap()),
        warnings,
        session_id,
        cwd: None,
        timestamp,
        message_count: assistant_count,
        messages_returned: 1,
    })
}

pub(crate) fn extract_text(value: &Value) -> String {
    if let Some(raw) = value.as_str() {
        return raw.to_string();
    }

    if let Some(parts) = value.as_array() {
        return parts
            .iter()
            .map(|part| {
                if let Some(raw) = part.as_str() {
                    raw.to_string()
                } else {
                    part["text"].as_str().unwrap_or("").to_string()
                }
            })
            .collect::<Vec<String>>()
            .join("");
    }

    String::new()
}

pub(crate) fn extract_claude_text(value: &Value) -> String {
    if let Some(raw) = value.as_str() {
        return raw.to_string();
    }

    if let Some(parts) = value.as_array() {
        return parts
            .iter()
            .filter_map(|part| {
                if part["type"].as_str().unwrap_or("") == "text" {
                    Some(part["text"].as_str().unwrap_or(""))
                } else {
                    None
                }
            })
            .collect::<Vec<&str>>()
            .join("");
    }

    String::new()
}

/// Claude content extraction that preserves tool_use/tool_result blocks as
/// bracketed sections, mirroring Node's `extractClaudeContentWithToolCalls`.
pub(crate) fn extract_claude_content_with_tool_calls(value: &Value) -> String {
    if let Some(raw) = value.as_str() {
        return raw.to_string();
    }

    let Some(parts) = value.as_array() else {
        return String::new();
    };

    parts
        .iter()
        .filter_map(|part| {
            let ty = part["type"].as_str().unwrap_or("");
            match ty {
                "text" => {
                    let txt = part["text"].as_str().unwrap_or("");
                    if txt.is_empty() {
                        None
                    } else {
                        Some(txt.to_string())
                    }
                }
                "tool_use" => {
                    let name = part["name"].as_str().unwrap_or("unknown");
                    let input_str = serde_json::to_string_pretty(&part["input"])
                        .unwrap_or_else(|_| part["input"].to_string());
                    Some(format!("[TOOL: {}]\n{}\n[/TOOL]", name, input_str))
                }
                "tool_result" => {
                    let tool_id = part["tool_use_id"].as_str().unwrap_or("");
                    let content = if let Some(s) = part["content"].as_str() {
                        s.to_string()
                    } else if let Some(arr) = part["content"].as_array() {
                        arr.iter()
                            .map(|c| c["text"].as_str().unwrap_or(""))
                            .collect::<Vec<&str>>()
                            .join("")
                    } else {
                        String::new()
                    };
                    Some(format!("[TOOL_RESULT: {}]\n{}\n[/TOOL_RESULT]", tool_id, content))
                }
                _ => None,
            }
        })
        .collect::<Vec<String>>()
        .join("\n")
}

/// Generic (non-Claude) content extraction that preserves function_call /
/// tool_use blocks. Mirrors Node's `extractContentWithToolCalls`.
pub(crate) fn extract_text_with_tool_calls(value: &Value) -> String {
    if let Some(raw) = value.as_str() {
        return raw.to_string();
    }

    let Some(parts) = value.as_array() else {
        return String::new();
    };

    parts
        .iter()
        .filter_map(|part| {
            if let Some(raw) = part.as_str() {
                return Some(raw.to_string());
            }
            if let Some(txt) = part["text"].as_str() {
                return Some(txt.to_string());
            }
            let ty = part["type"].as_str().unwrap_or("");
            if ty == "function_call" {
                let name = part["name"].as_str().unwrap_or("unknown");
                let arg_str = if let Some(s) = part["arguments"].as_str() {
                    s.to_string()
                } else {
                    serde_json::to_string_pretty(&part["arguments"])
                        .unwrap_or_else(|_| part["arguments"].to_string())
                };
                return Some(format!("[TOOL: {}]\n{}\n[/TOOL]", name, arg_str));
            }
            if ty == "tool_use" {
                let name = part["name"].as_str().unwrap_or("unknown");
                let input_str = serde_json::to_string_pretty(&part["input"])
                    .unwrap_or_else(|_| part["input"].to_string());
                return Some(format!("[TOOL: {}]\n{}\n[/TOOL]", name, input_str));
            }
            None
        })
        .collect::<Vec<String>>()
        .join("\n")
}

/// A single turn in a reconstructed conversation, used by `--include-user`
/// interleaving.  Role is always "user" or "assistant".
#[derive(Debug, Clone)]
pub(crate) struct ConversationTurn {
    pub role: String,
    pub text: String,
}

/// Select user+assistant turns for rendering `--include-user`: for each of the
/// last `last_n` assistant turns, include the most recent preceding user turn
/// (bounded by the previous assistant's position). Mirrors the Node per-agent
/// `selectConversationTurns` helpers.
pub(crate) fn select_conversation_turns(
    turns: &[ConversationTurn],
    last_n: usize,
) -> Vec<ConversationTurn> {
    let assistant_indexes: Vec<usize> = turns
        .iter()
        .enumerate()
        .filter(|(_, t)| t.role == "assistant")
        .map(|(i, _)| i)
        .collect();

    if assistant_indexes.is_empty() {
        return Vec::new();
    }

    let n = last_n.max(1);
    let start = assistant_indexes.len().saturating_sub(n);
    let chosen = &assistant_indexes[start..];

    let mut selected: Vec<ConversationTurn> = Vec::new();
    let mut lower_bound = 0usize;
    for &assistant_index in chosen {
        // Walk back from `assistant_index - 1` down to `lower_bound` inclusive,
        // matching Node's `for (i = assistantIndex - 1; i >= lowerBound; i -= 1)`.
        let mut user_index: Option<usize> = None;
        let mut i = assistant_index;
        while i > lower_bound {
            i -= 1;
            if turns[i].role == "user" {
                user_index = Some(i);
                break;
            }
        }
        if let Some(ui) = user_index {
            selected.push(turns[ui].clone());
        }
        selected.push(turns[assistant_index].clone());
        lower_bound = assistant_index + 1;
    }
    selected
}

fn file_modified_iso(path: &Path) -> Option<String> {
    fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .map(|mtime| {
            let duration = mtime.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
            let secs = duration.as_secs();
            let days = secs / 86400;
            let time_secs = secs % 86400;
            let hours = time_secs / 3600;
            let minutes = (time_secs % 3600) / 60;
            let seconds = time_secs % 60;
            // Simple epoch-to-date calculation
            let (year, month, day) = epoch_days_to_date(days);
            format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", year, month, day, hours, minutes, seconds)
        })
}

fn epoch_days_to_date(days: u64) -> (u64, u64, u64) {
    // Civil from days algorithm
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

pub(crate) fn read_jsonl_lines(path: &Path) -> Result<Vec<String>> {
    let meta = fs::metadata(path)?;
    if meta.len() > MAX_FILE_SIZE {
        return Err(anyhow!(
            "Skipped {} (exceeds {}MB size limit)",
            path.display(),
            MAX_FILE_SIZE / (1024 * 1024)
        ));
    }
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut lines: Vec<String> = reader.lines().map_while(Result::ok).collect();
    // Concurrent-read safety: if another process is actively writing to this
    // JSONL file, the last line may be truncated mid-JSON.  Drop it if it
    // doesn't look like a complete JSON value.
    if let Some(last) = lines.last() {
        let trimmed = last.trim_end();
        if !trimmed.is_empty() && !trimmed.ends_with(|c: char| c == '}' || c == ']' || c == '"' || c.is_ascii_digit()) {
            lines.pop();
        }
    }
    Ok(lines)
}

/// Extract assistant/model text from a JSONL session file (Codex or Claude format).
fn extract_assistant_text_jsonl(path: &Path, agent: &str) -> String {
    let lines = match read_jsonl_lines(path) {
        Ok(l) => l,
        Err(_) => return String::new(),
    };
    let mut text = String::new();
    for line in &lines {
        let json: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        match agent {
            "codex" => {
                if json.get("role").and_then(|v| v.as_str()) == Some("assistant") {
                    if let Some(content) = json.get("content").and_then(|v| v.as_str()) {
                        text.push_str(content);
                        text.push('\n');
                    }
                }
            }
            "claude" => {
                if json.get("type").and_then(|v| v.as_str()) == Some("assistant") {
                    if let Some(content_arr) = json.get("message")
                        .and_then(|m| m.get("content"))
                        .and_then(|c| c.as_array())
                    {
                        for block in content_arr {
                            if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                                text.push_str(t);
                                text.push('\n');
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    text
}

/// Extract assistant/model text from a Gemini JSON session file.
fn extract_assistant_text_json(path: &Path) -> String {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };
    let json: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };
    let mut text = String::new();
    // Gemini CLI format: { messages: [{ type: "gemini", content: "..." }] }
    if let Some(messages) = json.get("messages").and_then(|m| m.as_array()) {
        for msg in messages {
            let msg_type = msg.get("type").or_else(|| msg.get("role"))
                .and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
            if msg_type == "gemini" || msg_type == "model" || msg_type == "assistant" {
                if let Some(c) = msg.get("content").and_then(|v| v.as_str()) {
                    text.push_str(c);
                    text.push('\n');
                }
                // Also handle parts array (API format)
                if let Some(parts) = msg.get("parts").and_then(|p| p.as_array()) {
                    for part in parts {
                        if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                            text.push_str(t);
                            text.push('\n');
                        }
                    }
                }
            }
        }
    }
    // Fallback: history-based format
    if text.is_empty() {
        if let Some(history) = json.get("history").and_then(|h| h.as_array()) {
            for turn in history {
                let role = turn.get("role").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
                if role != "user" {
                    if let Some(parts) = turn.get("parts").and_then(|p| p.as_array()) {
                        for part in parts {
                            if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                                text.push_str(t);
                                text.push('\n');
                            }
                        }
                    }
                }
            }
        }
    }
    text
}

/// Extract assistant text from a Cursor session file.
fn extract_assistant_text_cursor(path: &Path) -> String {
    let raw = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };
    let mut text = String::new();
    // Try JSON format first
    if let Ok(parsed) = serde_json::from_str::<Value>(&raw) {
        let msgs = if parsed.is_array() {
            parsed.as_array().cloned().unwrap_or_default()
        } else if let Some(arr) = parsed.get("messages").and_then(|m| m.as_array()) {
            arr.clone()
        } else {
            Vec::new()
        };
        for msg in &msgs {
            if msg.get("role").and_then(|v| v.as_str()) == Some("assistant") {
                if let Some(c) = msg.get("content").and_then(|v| v.as_str()) {
                    text.push_str(c);
                    text.push('\n');
                }
            }
        }
    }
    // Fallback: try JSONL
    if text.is_empty() {
        for line in raw.lines() {
            if let Ok(obj) = serde_json::from_str::<Value>(line) {
                if obj.get("role").and_then(|v| v.as_str()) == Some("assistant") {
                    if let Some(c) = obj.get("content").and_then(|v| v.as_str()) {
                        text.push_str(c);
                        text.push('\n');
                    }
                }
            }
        }
    }
    if text.is_empty() { raw } else { text }
}

/// Compute a ~120 character match snippet centered on the first occurrence of query.
fn compute_match_snippet(text: &str, query: &str) -> Option<String> {
    let text_lower = text.to_lowercase();
    let query_lower = query.to_lowercase();
    let pos = text_lower.find(&query_lower)?;

    // Work with chars to avoid panicking on UTF-8 boundaries
    let chars: Vec<char> = text.chars().collect();
    // Find char index of the byte position
    let mut byte_count = 0;
    let mut char_pos = 0;
    for (i, ch) in chars.iter().enumerate() {
        if byte_count >= pos {
            char_pos = i;
            break;
        }
        byte_count += ch.len_utf8();
    }

    let start = char_pos.saturating_sub(60);
    let end = (char_pos + query.len() + 60).min(chars.len());
    let snippet: String = chars[start..end].iter().collect();
    // Replace newlines with spaces
    Some(snippet.replace(['\n', '\r'], " "))
}

/// Hierarchical CWD matching: exact match, ancestor, or descendant.
fn cwd_matches_project(session_cwd: &Path, expected_cwd: &Path) -> bool {
    session_cwd == expected_cwd
        || expected_cwd.starts_with(session_cwd)
        || session_cwd.starts_with(expected_cwd)
}

fn find_latest_by_cwd(
    files: &[FileEntry],
    expected_cwd: &Path,
    cwd_extractor: fn(&Path) -> Option<PathBuf>,
) -> Option<PathBuf> {
    for file in files {
        if let Some(file_cwd) = cwd_extractor(&file.path) {
            if cwd_matches_project(&file_cwd, expected_cwd) {
                return Some(file.path.clone());
            }
        }
    }
    None
}

fn get_codex_session_cwd(file_path: &Path) -> Option<PathBuf> {
    let lines = read_jsonl_lines(file_path).ok()?;
    let first = lines.first()?;
    let json: Value = serde_json::from_str(first).ok()?;
    let cwd = json["payload"]["cwd"].as_str()?;
    normalize_path(cwd).ok()
}

fn get_claude_session_cwd(file_path: &Path) -> Option<PathBuf> {
    let lines = read_jsonl_lines(file_path).ok()?;
    for line in lines {
        let json: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(cwd) = json["cwd"].as_str() {
            if let Ok(path) = normalize_path(cwd) {
                return Some(path);
            }
        }
    }
    None
}

fn is_system_directory(dir: &Path) -> bool {
    let s = dir.to_string_lossy();
    // macOS temp dirs live under /var/folders or /private/var/folders — allow those
    if s.starts_with("/var/folders/") || s.starts_with("/private/var/folders/") {
        return false;
    }
    let system_prefixes = ["/etc", "/usr", "/var", "/bin", "/sbin", "/System", "/Library",
        "/Windows", "/Windows/System32", "/Program Files", "/Program Files (x86)"];
    for prefix in system_prefixes {
        if s == prefix || s.starts_with(&format!("{}/", prefix)) || s.starts_with(&format!("{}\\", prefix)) {
            return true;
        }
    }
    false
}

fn resolve_gemini_chat_dirs(chats_dir: Option<&str>, cwd: &str) -> Result<Vec<PathBuf>> {
    if let Some(dir) = chats_dir {
        let expanded = expand_home(dir).context("Invalid Gemini chats directory")?;
        if is_system_directory(&expanded) {
            return Err(anyhow!("Refusing to scan system directory: {}", expanded.display()));
        }
        return if expanded.exists() {
            Ok(vec![expanded])
        } else {
            Ok(Vec::new())
        };
    }

    let mut ordered = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let add_dir = |dir: PathBuf, ordered_dirs: &mut Vec<PathBuf>, seen_dirs: &mut std::collections::HashSet<PathBuf>| {
        if !dir.exists() {
            return;
        }
        if seen_dirs.insert(dir.clone()) {
            ordered_dirs.push(dir);
        }
    };

    let normalized_cwd = normalize_path(cwd)?;
    let scoped_hash = hash_path(&normalized_cwd);

    let tmp_base = gemini_tmp_base_dir();
    add_dir(
        tmp_base.join(&scoped_hash).join("chats"),
        &mut ordered,
        &mut seen,
    );

    if let Ok(entries) = fs::read_dir(&tmp_base) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                add_dir(path.join("chats"), &mut ordered, &mut seen);
            }
        }
    }

    Ok(ordered)
}

fn resolve_gemini_chat_dirs_for_listing(cwd: Option<&str>) -> Result<Vec<PathBuf>> {
    if let Some(scope) = cwd {
        let normalized_cwd = normalize_path(scope)?;
        let scoped_hash = hash_path(&normalized_cwd);
        let dir = gemini_tmp_base_dir().join(scoped_hash).join("chats");
        if dir.exists() {
            return Ok(vec![dir]);
        }
        return Ok(Vec::new());
    }

    let tmp_base = gemini_tmp_base_dir();
    let mut ordered = Vec::new();
    if let Ok(entries) = fs::read_dir(&tmp_base) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let chats = path.join("chats");
                if chats.exists() {
                    ordered.push(chats);
                }
            }
        }
    }
    Ok(ordered)
}

fn collect_matching_files<F>(dir: &Path, recursive: bool, predicate: &F) -> Result<Vec<FileEntry>>
where
    F: Fn(&Path) -> bool,
{
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut matches = Vec::new();
    let mut stack = vec![dir.to_path_buf()];

    while let Some(current) = stack.pop() {
        if matches.len() >= MAX_SCAN_FILES {
            break;
        }

        let entries = match fs::read_dir(&current) {
            Ok(v) => v,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            if matches.len() >= MAX_SCAN_FILES {
                break;
            }

            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };

            // Skip symlinks (Phase 6)
            if file_type.is_symlink() {
                continue;
            }

            if path.is_dir() {
                if recursive {
                    stack.push(path);
                }
                continue;
            }

            if !predicate(&path) {
                continue;
            }

            let mtime = fs::metadata(&path)
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            let mtime_ns = mtime
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();

            matches.push(FileEntry { path, mtime_ns });
        }
    }

    sort_files_by_mtime_desc(&mut matches);
    Ok(matches)
}

fn sort_files_by_mtime_desc(files: &mut [FileEntry]) {
    files.sort_by(|a, b| {
        b.mtime_ns.cmp(&a.mtime_ns).then_with(|| {
            a.path
                .to_string_lossy()
                .cmp(&b.path.to_string_lossy())
        })
    });
}

fn has_extension(path: &Path, ext: &str) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| value.eq_ignore_ascii_case(ext))
        .unwrap_or(false)
}

fn path_contains(path: &Path, needle: &str) -> bool {
    path.to_string_lossy().contains(needle)
}

pub(crate) fn redact_sensitive_text(input: &str) -> String {
    let step1 = redact_openai_like_keys(input);
    let step2 = redact_aws_access_keys(&step1);
    let step3 = redact_github_tokens(&step2);
    let step4 = redact_google_api_keys(&step3);
    let step5 = redact_slack_tokens(&step4);
    let step6 = redact_bearer_tokens(&step5);
    let step7 = redact_jwt_tokens(&step6);
    let step8 = redact_pem_keys(&step7);
    let step9 = redact_connection_strings(&step8);
    redact_secret_assignments(&step9)
}

/// A single redaction audit entry.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RedactionEntry {
    pub pattern: String,
    pub count: usize,
}

/// Redact sensitive text and return an audit trail of what was redacted.
pub fn redact_sensitive_text_with_audit(input: &str) -> (String, Vec<RedactionEntry>) {
    let mut audit = Vec::new();

    fn count_diff(before: &str, after: &str, pattern: &str, audit: &mut Vec<RedactionEntry>) {
        if before != after {
            // Count how many redaction placeholders appeared that weren't there before
            let markers = ["[REDACTED]", "[REDACTED_JWT]", "[REDACTED_PEM_KEY]"];
            let mut count = 0usize;
            for marker in &markers {
                let after_count = after.matches(marker).count();
                let before_count = before.matches(marker).count();
                count += after_count.saturating_sub(before_count);
            }
            if count == 0 { count = 1; } // at least one if text changed
            audit.push(RedactionEntry {
                pattern: pattern.to_string(),
                count,
            });
        }
    }

    let step1 = redact_openai_like_keys(input);
    count_diff(input, &step1, "openai_key", &mut audit);

    let step2 = redact_aws_access_keys(&step1);
    count_diff(&step1, &step2, "aws_access_key", &mut audit);

    let step3 = redact_github_tokens(&step2);
    count_diff(&step2, &step3, "github_token", &mut audit);

    let step4 = redact_google_api_keys(&step3);
    count_diff(&step3, &step4, "google_api_key", &mut audit);

    let step5 = redact_slack_tokens(&step4);
    count_diff(&step4, &step5, "slack_token", &mut audit);

    let step6 = redact_bearer_tokens(&step5);
    count_diff(&step5, &step6, "bearer_token", &mut audit);

    let step7 = redact_jwt_tokens(&step6);
    count_diff(&step6, &step7, "jwt_token", &mut audit);

    let step8 = redact_pem_keys(&step7);
    count_diff(&step7, &step8, "pem_key", &mut audit);

    let step9 = redact_connection_strings(&step8);
    count_diff(&step8, &step9, "connection_string", &mut audit);

    let final_text = redact_secret_assignments(&step9);
    count_diff(&step9, &final_text, "secret_assignment", &mut audit);

    (final_text, audit)
}

fn redact_openai_like_keys(input: &str) -> String {
    let mut output = String::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0usize;

    while i < chars.len() {
        if i + 3 <= chars.len() && chars[i] == 's' && chars[i + 1] == 'k' && chars[i + 2] == '-' {
            let mut j = i + 3;
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_' || chars[j] == '-') {
                j += 1;
            }
            if j.saturating_sub(i + 3) >= 20 {
                output.push_str("sk-[REDACTED]");
                i = j;
                continue;
            }
        }
        output.push(chars[i]);
        i += 1;
    }

    output
}

fn redact_aws_access_keys(input: &str) -> String {
    let mut output = String::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0usize;

    while i < chars.len() {
        if i + 20 <= chars.len()
            && chars[i] == 'A'
            && chars[i + 1] == 'K'
            && chars[i + 2] == 'I'
            && chars[i + 3] == 'A'
        {
            let mut valid = true;
            for ch in chars.iter().take(i + 20).skip(i + 4) {
                if !ch.is_ascii_uppercase() && !ch.is_ascii_digit() {
                    valid = false;
                    break;
                }
            }
            if valid {
                output.push_str("AKIA[REDACTED]");
                i += 20;
                continue;
            }
        }
        output.push(chars[i]);
        i += 1;
    }

    output
}

fn redact_github_tokens(input: &str) -> String {
    let mut output = String::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0usize;

    let prefixes: &[&str] = &["ghp_", "gho_", "ghs_", "ghr_"];
    while i < chars.len() {
        let mut matched = false;
        for prefix in prefixes {
            let pchars: Vec<char> = prefix.chars().collect();
            if i + pchars.len() <= chars.len() && chars[i..i + pchars.len()] == pchars[..] {
                let mut j = i + pchars.len();
                while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                    j += 1;
                }
                if j.saturating_sub(i + pchars.len()) >= 20 {
                    output.push_str(prefix);
                    output.push_str("[REDACTED]");
                    i = j;
                    matched = true;
                    break;
                }
            }
        }
        if matched { continue; }

        // github_pat_ prefix
        let pat_prefix = "github_pat_";
        let pat_chars: Vec<char> = pat_prefix.chars().collect();
        if i + pat_chars.len() <= chars.len() && chars[i..i + pat_chars.len()] == pat_chars[..] {
            let mut j = i + pat_chars.len();
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            if j.saturating_sub(i + pat_chars.len()) >= 20 {
                output.push_str("github_pat_[REDACTED]");
                i = j;
                continue;
            }
        }

        output.push(chars[i]);
        i += 1;
    }
    output
}

fn redact_google_api_keys(input: &str) -> String {
    let mut output = String::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0usize;

    while i < chars.len() {
        if i + 4 <= chars.len()
            && chars[i] == 'A' && chars[i + 1] == 'I' && chars[i + 2] == 'z' && chars[i + 3] == 'a'
        {
            let mut j = i + 4;
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_' || chars[j] == '-') {
                j += 1;
            }
            if j.saturating_sub(i + 4) >= 20 {
                output.push_str("AIza[REDACTED]");
                i = j;
                continue;
            }
        }
        output.push(chars[i]);
        i += 1;
    }
    output
}

fn redact_slack_tokens(input: &str) -> String {
    let mut output = String::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0usize;

    let prefixes: &[&str] = &["xoxb-", "xoxp-", "xoxs-"];
    while i < chars.len() {
        let mut matched = false;
        for prefix in prefixes {
            let pchars: Vec<char> = prefix.chars().collect();
            if i + pchars.len() <= chars.len() && chars[i..i + pchars.len()] == pchars[..] {
                let mut j = i + pchars.len();
                while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '-') {
                    j += 1;
                }
                if j.saturating_sub(i + pchars.len()) >= 10 {
                    output.push_str(prefix);
                    output.push_str("[REDACTED]");
                    i = j;
                    matched = true;
                    break;
                }
            }
        }
        if matched { continue; }
        output.push(chars[i]);
        i += 1;
    }
    output
}

fn redact_jwt_tokens(input: &str) -> String {
    let mut output = String::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0usize;

    fn is_base64url(ch: char) -> bool {
        ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'
    }

    while i < chars.len() {
        if i + 3 <= chars.len() && chars[i] == 'e' && chars[i + 1] == 'y' && chars[i + 2] == 'J' {
            let mut j = i + 3;
            // First segment
            while j < chars.len() && is_base64url(chars[j]) { j += 1; }
            let seg1_len = j - (i + 3);
            if seg1_len >= 10 && j < chars.len() && chars[j] == '.' {
                j += 1;
                let seg2_start = j;
                while j < chars.len() && is_base64url(chars[j]) { j += 1; }
                let seg2_len = j - seg2_start;
                if seg2_len >= 10 && j < chars.len() && chars[j] == '.' {
                    j += 1;
                    let seg3_start = j;
                    while j < chars.len() && is_base64url(chars[j]) { j += 1; }
                    let seg3_len = j - seg3_start;
                    if seg3_len >= 10 {
                        output.push_str("[REDACTED_JWT]");
                        i = j;
                        continue;
                    }
                }
            }
        }
        output.push(chars[i]);
        i += 1;
    }
    output
}

fn redact_pem_keys(input: &str) -> String {
    let mut out = input.to_string();
    // Replace PEM private key blocks
    while let Some(start) = out.find("-----BEGIN ") {
        let header_end = match out[start..].find("-----\n").or_else(|| out[start..].find("-----\r")) {
            Some(pos) => start + pos + 5,
            None => break,
        };
        // Check this is a PRIVATE KEY header
        let header = &out[start..header_end];
        if !header.contains("PRIVATE KEY") {
            // Skip past this marker to avoid infinite loop
            let placeholder_pos = start + "-----BEGIN ".len();
            if placeholder_pos >= out.len() { break; }
            // Move on by replacing nothing, just advance search
            let after = &out[header_end..];
            if let Some(end_marker) = after.find("-----END ") {
                let block_end_pos = header_end + end_marker;
                if let Some(line_end) = out[block_end_pos..].find("-----") {
                    let final_end = block_end_pos + line_end + 5;
                    // Skip newline after end marker
                    let final_end = if final_end < out.len() && (out.as_bytes()[final_end] == b'\n' || out.as_bytes()[final_end] == b'\r') {
                        final_end + 1
                    } else {
                        final_end
                    };
                    out = format!("{}{}", &out[..start], &out[final_end..]);
                    continue;
                }
            }
            break;
        }
        // Find end marker
        let after = &out[header_end..];
        if let Some(end_pos) = after.find("-----END ") {
            let end_start = header_end + end_pos;
            if let Some(end_line) = out[end_start..].find("-----\n").or_else(|| out[end_start..].find("-----\r")).or_else(|| {
                // Could be at end of string
                if out[end_start..].ends_with("-----") { Some(out[end_start..].len() - 5) } else { None }
            }) {
                let final_end = end_start + end_line + 5;
                let final_end = if final_end < out.len() && (out.as_bytes()[final_end] == b'\n' || out.as_bytes()[final_end] == b'\r') {
                    final_end + 1
                } else {
                    final_end
                };
                out = format!("{}[REDACTED_PEM_KEY]{}", &out[..start], &out[final_end..]);
            } else {
                break;
            }
        } else {
            break;
        }
    }
    out
}

fn redact_connection_strings(input: &str) -> String {
    let mut out = input.to_string();
    let protocols = ["postgres://", "postgresql://", "mysql://", "mongodb://", "redis://", "amqp://"];
    for proto in protocols {
        let mut search_from = 0usize;
        loop {
            let lower = out.to_ascii_lowercase();
            let Some(pos) = lower[search_from..].find(proto) else { break; };
            let start = search_from + pos;
            let url_start = start;
            let proto_end = start + proto.len();
            // Find end of URL (whitespace, quote, or end of string)
            let mut end = proto_end;
            while end < out.len() {
                let ch = out.as_bytes()[end] as char;
                if ch.is_ascii_whitespace() || ch == '"' || ch == '\'' { break; }
                end += 1;
            }
            let proto_actual = &out[url_start..proto_end];
            let replacement = format!("{}[REDACTED]", proto_actual);
            out.replace_range(url_start..end, &replacement);
            search_from = url_start + replacement.len();
        }
    }
    out
}

fn redact_bearer_tokens(input: &str) -> String {
    let mut out = input.to_string();
    let mut search_from = 0usize;

    loop {
        let lower = out.to_ascii_lowercase();
        let Some(relative_start) = lower[search_from..].find("bearer ") else {
            break;
        };
        let start = search_from + relative_start;
        let token_start = start + "bearer ".len();
        let mut token_end = token_start;
        let bytes = out.as_bytes();
        while token_end < bytes.len() {
            let ch = bytes[token_end] as char;
            if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-' {
                token_end += 1;
            } else {
                break;
            }
        }
        if token_end.saturating_sub(token_start) < 10 {
            search_from = token_end.max(start + "bearer ".len());
            continue;
        }
        out.replace_range(start..token_end, "Bearer [REDACTED]");
        search_from = start + "Bearer [REDACTED]".len();
    }
    out
}

fn redact_secret_assignments(input: &str) -> String {
    let keywords = ["api_key", "api-key", "apikey", "token", "secret", "password"];
    let mut output = input.to_string();

    for key in keywords {
        output = redact_assignment_for_key(&output, key);
    }

    output
}

fn redact_assignment_for_key(input: &str, keyword: &str) -> String {
    let mut out = input.to_string();
    let mut search_from = 0usize;

    while search_from < out.len() {
        let lower = out.to_ascii_lowercase();
        let Some(relative) = lower[search_from..].find(keyword) else {
            break;
        };
        let start = search_from + relative;

        let mut idx = start + keyword.len();
        while idx < out.len() && out.as_bytes()[idx].is_ascii_whitespace() {
            idx += 1;
        }
        if idx >= out.len() {
            break;
        }

        let separator = out.as_bytes()[idx] as char;
        if separator != ':' && separator != '=' {
            search_from = start + keyword.len();
            continue;
        }

        idx += 1;
        while idx < out.len() && out.as_bytes()[idx].is_ascii_whitespace() {
            idx += 1;
        }
        if idx >= out.len() {
            break;
        }

        let quote = out.as_bytes()[idx] as char;
        let quoted = quote == '"' || quote == '\'';
        if quoted {
            idx += 1;
        }

        let value_start = idx;
        while idx < out.len() {
            let ch = out.as_bytes()[idx] as char;
            if quoted {
                if ch == quote {
                    break;
                }
            } else if ch.is_ascii_whitespace() || ch == ',' || ch == ';' {
                break;
            }
            idx += 1;
        }

        if idx > value_start {
            // Include closing quote in replacement range if present
            let end = if quoted && idx < out.len() && out.as_bytes()[idx] as char == quote {
                idx + 1
            } else {
                idx
            };
            // Replace from keyword start through end of value (including quotes) with keyword=[REDACTED]
            let replacement = format!("{}=[REDACTED]", keyword);
            out.replace_range(start..end, &replacement);
            search_from = start + replacement.len();
        } else {
            search_from = idx.saturating_add(1);
        }
    }

    out
}

// --- List functions ---

pub fn list_codex_sessions(cwd: Option<&str>, limit: usize) -> Result<Vec<serde_json::Value>> {
    let base_dir = codex_base_dir();
    if !base_dir.exists() { return Ok(Vec::new()); }
    let files = collect_matching_files(&base_dir, true, &|p| has_extension(p, "jsonl"))?;
    let expected_cwd = cwd.map(normalize_path).transpose()?;
    let mut entries = Vec::new();
    for file in files {
        let file_cwd = get_codex_session_cwd(&file.path);
        if let Some(expected) = expected_cwd.as_ref() {
            if let Some(ref file_cwd_path) = file_cwd {
                if !cwd_matches_project(file_cwd_path, expected) {
                    continue;
                }
            } else {
                continue;
            }
        }
        let session_id = file.path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string();
        entries.push(serde_json::json!({
            "session_id": session_id,
            "agent": "codex",
            "cwd": file_cwd.map(|p| p.to_string_lossy().to_string()),
            "modified_at": file_modified_iso(&file.path),
            "file_path": file.path.to_string_lossy().to_string(),
        }));
        if entries.len() >= limit {
            break;
        }
    }
    Ok(entries)
}

pub fn list_claude_sessions(cwd: Option<&str>, limit: usize) -> Result<Vec<serde_json::Value>> {
    let base_dir = claude_base_dir();
    if !base_dir.exists() { return Ok(Vec::new()); }
    let files = collect_matching_files(&base_dir, true, &|p| has_extension(p, "jsonl"))?;
    let expected_cwd = cwd.map(normalize_path).transpose()?;
    let mut entries = Vec::new();
    for file in files {
        let file_cwd = get_claude_session_cwd(&file.path);
        if let Some(expected) = expected_cwd.as_ref() {
            if let Some(ref file_cwd_path) = file_cwd {
                if !cwd_matches_project(file_cwd_path, expected) {
                    continue;
                }
            } else {
                continue;
            }
        }
        let session_id = file.path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string();
        entries.push(serde_json::json!({
            "session_id": session_id,
            "agent": "claude",
            "cwd": file_cwd.map(|p| p.to_string_lossy().to_string()),
            "modified_at": file_modified_iso(&file.path),
            "file_path": file.path.to_string_lossy().to_string(),
        }));
        if entries.len() >= limit {
            break;
        }
    }
    Ok(entries)
}

pub fn list_gemini_sessions(cwd: Option<&str>, limit: usize) -> Result<Vec<serde_json::Value>> {
    let dirs = resolve_gemini_chat_dirs_for_listing(cwd)?;
    let mut candidates = Vec::new();
    for dir in &dirs {
        let mut files = collect_matching_files(dir, false, &|p| {
            (has_extension(p, "json") || has_extension(p, "jsonl"))
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("session-"))
                    .unwrap_or(false)
        })?;
        candidates.append(&mut files);
    }
    sort_files_by_mtime_desc(&mut candidates);
    let mut entries = Vec::new();
    for file in candidates.iter().take(limit) {
        let session_id = file.path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string();
        let (cwd_hint, scope_hash) = infer_gemini_scope(&file.path);
        let mut entry = serde_json::json!({
            "session_id": session_id,
            "agent": "gemini",
            "cwd": cwd_hint,
            "modified_at": file_modified_iso(&file.path),
            "file_path": file.path.to_string_lossy().to_string(),
        });
        if let Some(hash) = scope_hash {
            entry["scope_hash"] = serde_json::Value::String(hash);
        }
        entries.push(entry);
    }
    Ok(entries)
}

/// Best-effort inference of the Gemini session's "cwd" from its scope segment.
///
/// Gemini CLI lays out session files under `~/.gemini/tmp/<scope>/chats/`.
/// The `<scope>` segment is either:
///   - a named directory (e.g. `play`, `sandbox`) — user-named scope; return
///     it verbatim as the cwd hint so `--cwd <X>` filtering can fuzzy-match;
///   - a hex hash (SHA-256 of an absolute cwd, via `hash_path`) — we cannot
///     reverse it without a scope map, so return the scope directory itself
///     as the cwd hint (lossy but still useful as a bucket) and surface the
///     hex string under `scope_hash` so downstream tools can match by hash.
///
/// Returns `(cwd_value_for_json, optional_scope_hash)`. The cwd value is a
/// JSON value (String) rather than `Option<String>` so the listing keeps its
/// stable shape; callers unable to infer a scope still get a valid `cwd`
/// field instead of `null`.
fn infer_gemini_scope(session_path: &Path) -> (serde_json::Value, Option<String>) {
    // Layout: .../tmp/<scope>/chats/session-*.json[l]
    //   grandparent of the file is <scope>/chats
    //   great-grandparent (one more up) is <scope>
    let scope_dir = session_path
        .parent()           // <scope>/chats
        .and_then(|p| p.parent()); // <scope>
    let scope_name = scope_dir
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .map(|s| s.to_string());
    match scope_name {
        Some(name) => {
            // Hex-hash scopes are a known lossy case — we still return the
            // scope dir as the cwd bucket, but flag the hash so callers can
            // opt into a lookup map.
            let is_hex_hash = name.len() >= 40
                && name.chars().all(|c| c.is_ascii_hexdigit());
            if is_hex_hash {
                (serde_json::Value::String(name.clone()), Some(name))
            } else {
                (serde_json::Value::String(name), None)
            }
        }
        None => (serde_json::Value::Null, None),
    }
}

// --- Search functions ---

pub fn search_codex_sessions(query: &str, cwd: Option<&str>, limit: usize) -> Result<Vec<serde_json::Value>> {
    let base_dir = codex_base_dir();
    if !base_dir.exists() { return Ok(Vec::new()); }
    let files = collect_matching_files(&base_dir, true, &|p| has_extension(p, "jsonl"))?;
    let expected_cwd = cwd.map(normalize_path).transpose()?;
    let query_lower = query.to_ascii_lowercase();
    let mut entries = Vec::new();

    for file in files {
        if entries.len() >= limit { break; }

        let file_cwd = get_codex_session_cwd(&file.path);
        if let Some(expected) = expected_cwd.as_ref() {
            if let Some(ref file_cwd_path) = file_cwd {
                if !cwd_matches_project(file_cwd_path, expected) {
                    continue;
                }
            } else {
                continue;
            }
        }

        let assistant_text = extract_assistant_text_jsonl(&file.path, "codex");
        if assistant_text.to_ascii_lowercase().contains(&query_lower) {
            let snippet = compute_match_snippet(&assistant_text, query);
            let session_id = file.path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string();
            entries.push(serde_json::json!({
                "session_id": session_id,
                "agent": "codex",
                "cwd": file_cwd.map(|p| p.to_string_lossy().to_string()),
                "modified_at": file_modified_iso(&file.path),
                "file_path": file.path.to_string_lossy().to_string(),
                "match_snippet": snippet,
            }));
        }
    }
    Ok(entries)
}

pub fn search_claude_sessions(query: &str, cwd: Option<&str>, limit: usize) -> Result<Vec<serde_json::Value>> {
    let base_dir = claude_base_dir();
    if !base_dir.exists() { return Ok(Vec::new()); }
    let files = collect_matching_files(&base_dir, true, &|p| has_extension(p, "jsonl"))?;
    let expected_cwd = cwd.map(normalize_path).transpose()?;
    let query_lower = query.to_ascii_lowercase();
    let mut entries = Vec::new();

    for file in files {
        if entries.len() >= limit { break; }

        let file_cwd = get_claude_session_cwd(&file.path);
        if let Some(expected) = expected_cwd.as_ref() {
            if let Some(ref file_cwd_path) = file_cwd {
                if !cwd_matches_project(file_cwd_path, expected) {
                    continue;
                }
            } else {
                continue;
            }
        }

        let assistant_text = extract_assistant_text_jsonl(&file.path, "claude");
        if assistant_text.to_ascii_lowercase().contains(&query_lower) {
            let snippet = compute_match_snippet(&assistant_text, query);
            let session_id = file.path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string();
            entries.push(serde_json::json!({
                "session_id": session_id,
                "agent": "claude",
                "cwd": file_cwd.map(|p| p.to_string_lossy().to_string()),
                "modified_at": file_modified_iso(&file.path),
                "file_path": file.path.to_string_lossy().to_string(),
                "match_snippet": snippet,
            }));
        }
    }
    Ok(entries)
}

pub fn search_gemini_sessions(query: &str, cwd: Option<&str>, limit: usize) -> Result<Vec<serde_json::Value>> {
    let dirs = resolve_gemini_chat_dirs_for_listing(cwd)?;
    let mut candidates = Vec::new();
    for dir in &dirs {
        let mut files = collect_matching_files(dir, false, &|p| {
            (has_extension(p, "json") || has_extension(p, "jsonl"))
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("session-"))
                    .unwrap_or(false)
        })?;
        candidates.append(&mut files);
    }
    sort_files_by_mtime_desc(&mut candidates);

    let query_lower = query.to_ascii_lowercase();
    let mut entries = Vec::new();

    for file in candidates {
        if entries.len() >= limit { break; }

        let assistant_text = extract_assistant_text_json(&file.path);
        if assistant_text.to_ascii_lowercase().contains(&query_lower) {
            let snippet = compute_match_snippet(&assistant_text, query);
            let session_id = file.path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string();
            let (cwd_hint, scope_hash) = infer_gemini_scope(&file.path);
            let mut entry = serde_json::json!({
                "session_id": session_id,
                "agent": "gemini",
                "cwd": cwd_hint,
                "modified_at": file_modified_iso(&file.path),
                "file_path": file.path.to_string_lossy().to_string(),
                "match_snippet": snippet,
            });
            if let Some(hash) = scope_hash {
                entry["scope_hash"] = serde_json::Value::String(hash);
            }
            entries.push(entry);
        }
    }
    Ok(entries)
}

pub fn search_cursor_sessions(query: &str, cwd: Option<&str>, limit: usize) -> Result<Vec<serde_json::Value>> {
    let base_dir = cursor_base_dir();
    if !base_dir.exists() { return Ok(Vec::new()); }

    let workspaces_dir = base_dir.join("User").join("workspaceStorage");
    if !workspaces_dir.exists() { return Ok(Vec::new()); }

    let files = collect_matching_files(&workspaces_dir, true, &|p| {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        (name.ends_with(".json") || name.ends_with(".jsonl"))
            && (name.contains("chat") || name.contains("composer") || name.contains("conversation"))
    })?;

    let query_lower = query.to_ascii_lowercase();
    let expected_cwd = cwd.map(normalize_path).transpose()?;
    let expected_cwd_text = expected_cwd
        .as_ref()
        .map(|path| path.to_string_lossy().to_ascii_lowercase());
    let mut entries = Vec::new();

    for file in files {
        if entries.len() >= limit { break; }

        if fs::metadata(&file.path).map(|m| m.len() > MAX_FILE_SIZE).unwrap_or(false) {
            continue;
        }

        // CWD filter still needs raw content (cursor files embed workspace paths)
        if let Some(expected) = expected_cwd_text.as_ref() {
            let raw = match fs::read_to_string(&file.path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            if !raw.to_ascii_lowercase().contains(expected) {
                continue;
            }
        }

        let assistant_text = extract_assistant_text_cursor(&file.path);
        if assistant_text.to_ascii_lowercase().contains(&query_lower) {
            let snippet = compute_match_snippet(&assistant_text, query);
            let session_id = file.path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string();
            entries.push(serde_json::json!({
                "session_id": session_id,
                "agent": "cursor",
                "cwd": serde_json::Value::Null,
                "modified_at": file_modified_iso(&file.path),
                "file_path": file.path.to_string_lossy().to_string(),
                "match_snippet": snippet,
            }));
        }
    }
    Ok(entries)
}

// --- Cursor support ---

fn cursor_base_dir() -> PathBuf {
    std::env::var("CHORUS_CURSOR_DATA_DIR")
        .or_else(|_| std::env::var("BRIDGE_CURSOR_DATA_DIR"))
        .ok()
        .and_then(|value| expand_home(&value))
        .unwrap_or_else(|| {
            // macOS: ~/Library/Application Support/Cursor
            // Linux: ~/.cursor
            if cfg!(target_os = "macos") {
                dirs::home_dir()
                    .map(|h| h.join("Library/Application Support/Cursor"))
                    .unwrap_or_else(|| PathBuf::from("~/.cursor"))
            } else {
                expand_home("~/.cursor").unwrap_or_else(|| PathBuf::from("~/.cursor"))
            }
        })
}

#[allow(dead_code)]
pub fn read_cursor_session(id: Option<&str>, cwd: &str) -> Result<Session> {
    read_cursor_session_with_options(id, cwd, 1, ReadOptions::default())
}

pub fn read_cursor_session_with_options(
    id: Option<&str>,
    _cwd: &str,
    last_n: usize,
    opts: ReadOptions,
) -> Result<Session> {
    let base_dir = cursor_base_dir();
    if is_system_directory(&base_dir) {
        return Err(anyhow!("Refusing to scan system directory: {}", base_dir.display()));
    }
    if !base_dir.exists() {
        return Err(anyhow!(cursor_not_found_message(&format!("No Cursor session found. Data directory not found: {}", base_dir.display()))));
    }

    let workspaces_dir = base_dir.join("User").join("workspaceStorage");
    if !workspaces_dir.exists() {
        return Err(anyhow!(cursor_not_found_message(&format!("No Cursor session found. Workspace storage not found: {}", workspaces_dir.display()))));
    }

    // Look for composer/chat state files in workspace storage
    let files = collect_matching_files(&workspaces_dir, true, &|p| {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        (name.ends_with(".json") || name.ends_with(".jsonl"))
            && (name.contains("chat") || name.contains("composer") || name.contains("conversation"))
            && id.map(|needle| p.to_string_lossy().contains(needle)).unwrap_or(true)
    })?;

    if files.is_empty() {
        return Err(anyhow!(cursor_not_found_message("No Cursor session found.")));
    }

    let target_file = files[0].path.clone();

    // Gather turns (user + assistant) and assistant-only messages. Both JSON
    // and JSONL shapes supported; mirror Node's cursor adapter.
    let content_str = fs::read_to_string(&target_file)?;
    let mut turns: Vec<ConversationTurn> = Vec::new();
    let mut parsed_as_json = false;

    if let Ok(json) = serde_json::from_str::<Value>(&content_str) {
        parsed_as_json = true;
        if let Some(messages) = json.get("messages").and_then(|m| m.as_array()) {
            for m in messages {
                let role = m["role"].as_str().unwrap_or("").to_string();
                if role != "assistant" && role != "user" {
                    continue;
                }
                let text = if let Some(s) = m["content"].as_str() {
                    s.to_string()
                } else {
                    serde_json::to_string(&m["content"]).unwrap_or_default()
                };
                turns.push(ConversationTurn { role, text });
            }
        } else if let Some(text) = json.get("content").and_then(|c| c.as_str()) {
            // Single-content shape — treat as a lone assistant message.
            turns.push(ConversationTurn { role: "assistant".to_string(), text: text.to_string() });
        } else {
            // Unknown JSON shape — fall back to whole-doc as raw.
            let raw = json.to_string();
            return Ok(Session {
                agent: "cursor",
                content: redact_sensitive_text(&raw),
                source: target_file.to_string_lossy().to_string(),
                warnings: vec![cursor_warning()],
                session_id: target_file.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string()),
                cwd: None,
                timestamp: file_modified_iso(&target_file),
                message_count: 1,
                messages_returned: 1,
            });
        }
    } else {
        // JSONL format
        for line in content_str.lines().filter(|l| !l.is_empty()) {
            if let Ok(json) = serde_json::from_str::<Value>(line) {
                let role = json["role"].as_str().unwrap_or("").to_string();
                if (role == "assistant" || role == "user") && json["content"].is_string() {
                    turns.push(ConversationTurn {
                        role,
                        text: json["content"].as_str().unwrap().to_string(),
                    });
                }
            }
        }
    }

    let assistant_msgs: Vec<String> = turns
        .iter()
        .filter(|t| t.role == "assistant")
        .map(|t| t.text.clone())
        .collect();
    let message_count = assistant_msgs.len();

    let (content, messages_returned) = if opts.include_user && !assistant_msgs.is_empty() {
        let selected = select_conversation_turns(&turns, last_n);
        let n = selected.len();
        let rendered = selected
            .iter()
            .map(|t| format!("{}:\n{}", t.role.to_uppercase(), t.text))
            .collect::<Vec<String>>()
            .join("\n---\n");
        (rendered, n)
    } else if last_n > 1 && !assistant_msgs.is_empty() {
        let start = assistant_msgs.len().saturating_sub(last_n);
        let sel: Vec<&String> = assistant_msgs[start..].iter().collect();
        let n = sel.len();
        (sel.iter().map(|s| s.as_str()).collect::<Vec<&str>>().join("\n---\n"), n)
    } else if let Some(last) = assistant_msgs.last() {
        (last.clone(), 1)
    } else if parsed_as_json {
        ("[No assistant messages found]".to_string(), 0)
    } else {
        // JSONL fallback: last 20 raw lines
        let tail: Vec<&str> = content_str.lines().rev().take(20).collect::<Vec<&str>>().into_iter().rev().collect();
        (tail.join("\n"), 0)
    };

    let session_id = target_file.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string());
    let timestamp = file_modified_iso(&target_file);

    Ok(Session {
        agent: "cursor",
        content: redact_sensitive_text(&content),
        source: target_file.to_string_lossy().to_string(),
        warnings: vec![cursor_warning()],
        session_id,
        cwd: None,
        timestamp,
        message_count,
        messages_returned,
    })
}

fn cursor_warning() -> String {
    "Warning: Cursor sessions have no project scoping. Results may include sessions from unrelated projects.".to_string()
}

pub fn list_cursor_sessions(cwd: Option<&str>, limit: usize) -> Result<Vec<serde_json::Value>> {
    let base_dir = cursor_base_dir();
    if !base_dir.exists() { return Ok(Vec::new()); }

    let workspaces_dir = base_dir.join("User").join("workspaceStorage");
    if !workspaces_dir.exists() { return Ok(Vec::new()); }

    let files = collect_matching_files(&workspaces_dir, true, &|p| {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        (name.ends_with(".json") || name.ends_with(".jsonl"))
            && (name.contains("chat") || name.contains("composer") || name.contains("conversation"))
    })?;

    let expected_cwd = cwd.map(normalize_path).transpose()?;
    let expected_cwd_text = expected_cwd
        .as_ref()
        .map(|path| path.to_string_lossy().to_ascii_lowercase());
    let mut entries = Vec::new();
    for file in files {
        if let Some(expected) = expected_cwd_text.as_ref() {
            let content = match fs::read_to_string(&file.path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            if !content.to_ascii_lowercase().contains(expected) {
                continue;
            }
        }

        let session_id = file.path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string();
        entries.push(serde_json::json!({
            "session_id": session_id,
            "agent": "cursor",
            "cwd": serde_json::Value::Null,
            "modified_at": file_modified_iso(&file.path),
            "file_path": file.path.to_string_lossy().to_string(),
        }));
        if entries.len() >= limit {
            break;
        }
    }
    Ok(entries)
}

pub(crate) fn codex_base_dir() -> PathBuf {
    std::env::var("CHORUS_CODEX_SESSIONS_DIR")
        .or_else(|_| std::env::var("BRIDGE_CODEX_SESSIONS_DIR"))
        .ok()
        .and_then(|value| expand_home(&value))
        .unwrap_or_else(|| expand_home("~/.codex/sessions").unwrap_or_else(|| PathBuf::from("~/.codex/sessions")))
}

pub(crate) fn claude_base_dir() -> PathBuf {
    std::env::var("CHORUS_CLAUDE_PROJECTS_DIR")
        .or_else(|_| std::env::var("BRIDGE_CLAUDE_PROJECTS_DIR"))
        .ok()
        .and_then(|value| expand_home(&value))
        .unwrap_or_else(|| expand_home("~/.claude/projects").unwrap_or_else(|| PathBuf::from("~/.claude/projects")))
}

pub(crate) fn gemini_tmp_base_dir() -> PathBuf {
    std::env::var("CHORUS_GEMINI_TMP_DIR")
        .or_else(|_| std::env::var("BRIDGE_GEMINI_TMP_DIR"))
        .ok()
        .and_then(|value| expand_home(&value))
        .unwrap_or_else(|| expand_home("~/.gemini/tmp").unwrap_or_else(|| PathBuf::from("~/.gemini/tmp")))
}

/// Return the base directory that holds Gemini profile subdirectories.
///
/// Defaults to `~/.gemini`. Overridable via `CHORUS_GEMINI_BASE_DIR` for
/// tests and non-standard installs.
fn gemini_base_dir() -> PathBuf {
    std::env::var("CHORUS_GEMINI_BASE_DIR")
        .ok()
        .and_then(|value| expand_home(&value))
        .unwrap_or_else(|| expand_home("~/.gemini").unwrap_or_else(|| PathBuf::from("~/.gemini")))
}

/// Look for protobuf-format session files under `~/.gemini/<profile>/conversations/`
/// and return a user-facing addendum naming the first directory containing `.pb`
/// files plus the total count found. Returns `None` if no `.pb` files exist —
/// in that case callers should keep the existing NOT_FOUND wording.
pub(crate) fn detect_gemini_pb_fallback_hint() -> Option<String> {
    let base = gemini_base_dir();
    if !base.exists() {
        return None;
    }

    let mut total_count = 0usize;
    let mut first_dir: Option<PathBuf> = None;

    let profiles = match fs::read_dir(&base) {
        Ok(entries) => entries,
        Err(_) => return None,
    };

    for entry in profiles.flatten() {
        let profile_path = entry.path();
        if !profile_path.is_dir() {
            continue;
        }
        let conversations = profile_path.join("conversations");
        if !conversations.is_dir() {
            continue;
        }
        let inner = match fs::read_dir(&conversations) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for file_entry in inner.flatten() {
            let file_path = file_entry.path();
            if file_path.is_file() && has_extension(&file_path, "pb") {
                total_count += 1;
                if first_dir.is_none() {
                    first_dir = Some(conversations.clone());
                }
            }
        }
    }

    if total_count == 0 {
        return None;
    }

    let dir_display = first_dir
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| format!("{}/<profile>/conversations/", base.display()));
    let noun = if total_count == 1 { "file" } else { "files" };
    Some(format!(
        "No JSONL Gemini session found. Detected {} protobuf (.pb) {} at {} — chorus does not yet parse this format.\nWorkarounds: (1) use `--chats-dir <path>` to point at a JSONL export, (2) see docs/session-handoff-guide.md \"Gemini protobuf fallback\".",
        total_count, noun, dir_display
    ))
}

/// Given a base NOT_FOUND message, return it verbatim when no `.pb` files
/// are detected, or replace it with the richer protobuf hint otherwise.
fn gemini_not_found_message(base_message: &str) -> String {
    match detect_gemini_pb_fallback_hint() {
        Some(hint) => hint,
        None => base_message.to_string(),
    }
}

/// Probe the Cursor workspace storage for SQLite `state.vscdb` files — the
/// format used by modern Cursor (VS Code fork) to persist chat/composer data.
/// chorus's current cursor reader only scans for JSON/JSONL files whose names
/// match `chat|composer|conversation`, which never hits the SQLite rows, so
/// a Cursor install with active chats still returns NOT_FOUND. When `.vscdb`
/// files are present we return a richer hint so users know why.
pub(crate) fn detect_cursor_vscdb_fallback_hint() -> Option<String> {
    let base = cursor_base_dir();
    if !base.exists() {
        return None;
    }
    let workspaces = base.join("User").join("workspaceStorage");
    if !workspaces.is_dir() {
        return None;
    }

    let mut total_count = 0usize;

    let entries = match fs::read_dir(&workspaces) {
        Ok(e) => e,
        Err(_) => return None,
    };
    for ws in entries.flatten() {
        let path = ws.path();
        if !path.is_dir() {
            continue;
        }
        // `state.vscdb` is the primary store; `state.vscdb-wal`/`-shm` are
        // SQLite journal siblings we don't count.
        if path.join("state.vscdb").is_file() {
            total_count += 1;
        }
    }

    if total_count == 0 {
        return None;
    }

    let noun = if total_count == 1 { "file" } else { "files" };
    Some(format!(
        "No JSON/JSONL Cursor session found. Detected {} SQLite state.vscdb {} under {}/User/workspaceStorage/ — modern Cursor persists chat/composer data in SQLite and chorus does not yet parse this format.\nWorkaround: see docs/session-handoff-guide.md \"Cursor SQLite fallback\". Full SQLite reading is tracked as a follow-up.",
        total_count, noun, base.display()
    ))
}

/// Given a base NOT_FOUND message, return it verbatim when no `.vscdb` files
/// are detected, or replace it with the richer SQLite hint otherwise.
fn cursor_not_found_message(base_message: &str) -> String {
    match detect_cursor_vscdb_fallback_hint() {
        Some(hint) => hint,
        None => base_message.to_string(),
    }
}

// --- Trash Talk ---

struct ActiveAgent {
    agent: &'static str,
    content: String,
    message_count: usize,
    session_id: String,
}

fn simple_hash(s: &str) -> usize {
    let mut hash: i32 = 0;
    for byte in s.bytes() {
        hash = hash.wrapping_shl(5).wrapping_sub(hash).wrapping_add(byte as i32);
    }
    hash.unsigned_abs() as usize
}

fn pick_roast(agent: &str, content: &str, message_count: usize) -> &'static str {
    const SHORT_ROASTS: &[&str] = &[
        "That's it? My .gitignore has more content.",
        "Blink and you'd miss that entire session.",
    ];
    const LONG_ROASTS: &[&str] = &[
        "Wrote a novel, did we? Too bad nobody asked for War and Peace.",
        "That session has more words than my last performance review.",
    ];
    const TEST_ROASTS: &[&str] = &[
        "Oh look, someone actually writes tests. Show-off.",
        "Testing? In this economy?",
    ];
    const TODO_ROASTS: &[&str] = &[
        "Still leaving TODOs? That's a cry for help.",
        "TODO: learn to finish things.",
    ];
    const BUG_ROASTS: &[&str] = &[
        "Breaking things again? Classic.",
        "Found a bug? Or just made one?",
    ];
    const CODEX_ROASTS: &[&str] = &[
        "OpenAI's kid showing up to do chores. How responsible.",
        "Codex: because copy-paste needed a rebrand.",
    ];
    const CLAUDE_ROASTS: &[&str] = &[
        "Claude overthinking again? Shocking. Truly shocking.",
        "Too polite to say no, too verbose to say yes.",
    ];
    const GEMINI_ROASTS: &[&str] = &[
        "Did Gemini Google the answer? Old habits die hard.",
        "Gemini: when one model isn't enough, use two and confuse both.",
    ];
    const CURSOR_ROASTS: &[&str] = &[
        "An IDE that thinks it's an agent. Bless its heart.",
        "Cursor: autocomplete with delusions of grandeur.",
    ];
    const GENERIC_ROASTS: &[&str] = &[
        "Participation trophy earned.",
        "Well, at least the process exited cleanly.",
        "Not the worst I've seen. That's not a compliment.",
    ];

    let mut roasts: Vec<&str> = Vec::new();
    if message_count < 5 { roasts.extend_from_slice(SHORT_ROASTS); }
    if message_count > 30 { roasts.extend_from_slice(LONG_ROASTS); }

    let lower = content.to_ascii_lowercase();
    if lower.contains("test") || lower.contains("spec") || lower.contains("assert") {
        roasts.extend_from_slice(TEST_ROASTS);
    }
    if lower.contains("todo") || lower.contains("fixme") || lower.contains("hack") {
        roasts.extend_from_slice(TODO_ROASTS);
    }
    if lower.contains("error") || lower.contains("bug") || lower.contains("fix") {
        roasts.extend_from_slice(BUG_ROASTS);
    }

    match agent {
        "codex" => roasts.extend_from_slice(CODEX_ROASTS),
        "claude" => roasts.extend_from_slice(CLAUDE_ROASTS),
        "gemini" => roasts.extend_from_slice(GEMINI_ROASTS),
        "cursor" => roasts.extend_from_slice(CURSOR_ROASTS),
        _ => {}
    }
    roasts.extend_from_slice(GENERIC_ROASTS);

    roasts[simple_hash(content) % roasts.len()]
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

pub fn trash_talk(cwd: &str) {
    let agents = ["codex", "gemini", "claude", "cursor"];
    let mut active: Vec<ActiveAgent> = Vec::new();

    for agent_name in &agents {
        let adapter = match crate::adapters::get_adapter(agent_name) {
            Some(a) => a,
            None => continue,
        };
        let entries = match adapter.list_sessions(Some(cwd), 1) {
            Ok(e) if !e.is_empty() => e,
            _ => continue,
        };
        let session = match adapter.read_session(None, cwd, None, 1) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let _ = entries; // used only to check presence
        active.push(ActiveAgent {
            agent: match *agent_name {
                "codex" => "codex",
                "gemini" => "gemini",
                "claude" => "claude",
                "cursor" => "cursor",
                _ => "unknown",
            },
            content: session.content,
            message_count: session.message_count,
            session_id: session.session_id.unwrap_or_else(|| "unknown".to_string()),
        });
    }

    println!("\u{1f5d1}\u{fe0f}  TRASH TALK\n");

    if active.is_empty() {
        println!("No agents to trash-talk. It's lonely in here.");
        println!("Try running some agents first \u{2014} I need material.");
        return;
    }

    if active.len() == 1 {
        let a = &active[0];
        let roast = pick_roast(a.agent, &a.content, a.message_count);
        println!("Target: {} ({}, {} messages)\n", capitalize(a.agent), a.session_id, a.message_count);
        println!("\"{}\"\n", roast);
        println!("Verdict: {} is trying. Bless.", capitalize(a.agent));
        return;
    }

    // Battle mode
    active.sort_by(|a, b| b.message_count.cmp(&a.message_count));

    println!("\u{1f4ca} Activity Report:");
    for a in &active {
        println!("  {:<8} {:>3} messages  ({})", capitalize(a.agent), a.message_count, a.session_id);
    }
    println!();

    println!("\u{1f3c6} Winner: {} (by volume \u{2014} congrats on typing the most)", capitalize(active[0].agent));
    println!("\"Quantity over quality, but at least you showed up.\"\n");

    for a in &active[1..] {
        let roast = pick_roast(a.agent, &a.content, a.message_count);
        println!("\u{1f480} {} ({} messages):", capitalize(a.agent), a.message_count);
        println!("\"{}\"\n", roast);
    }

    println!("Verdict: They're all trying their best. It's just not very good.");
}

#[cfg(test)]
mod tests {
    use super::{detect_cursor_vscdb_fallback_hint, detect_gemini_pb_fallback_hint, redact_sensitive_text};

    #[test]
    fn redacts_multiple_bearer_tokens() {
        let input = "Bearer abcdefghij and Bearer zyxwvutsrq";
        let output = redact_sensitive_text(input);
        assert_eq!(output, "Bearer [REDACTED] and Bearer [REDACTED]");
    }

    #[test]
    fn short_bearer_token_does_not_block_later_redaction() {
        let input = "Bearer short and Bearer abcdefghijklmnop";
        let output = redact_sensitive_text(input);
        assert_eq!(output, "Bearer short and Bearer [REDACTED]");
    }

    #[test]
    fn redacts_openai_keys() {
        let input = "key is sk-abcdefghij0123456789abcdefghij";
        let output = redact_sensitive_text(input);
        assert!(output.contains("sk-[REDACTED]"), "got: {}", output);
        assert!(!output.contains("abcdefghij0123456789"));
    }

    #[test]
    fn redacts_aws_access_keys() {
        let input = "aws key: AKIA1234567890ABCDEF";
        let output = redact_sensitive_text(input);
        assert!(output.contains("AKIA[REDACTED]"), "got: {}", output);
        assert!(!output.contains("1234567890ABCDEF"));
    }

    #[test]
    fn redacts_api_key_assignments() {
        let input = "api_key=\"super-secret-123\"";
        let output = redact_sensitive_text(input);
        assert!(output.contains("[REDACTED]"), "got: {}", output);
        assert!(!output.contains("super-secret-123"));
    }

    #[test]
    fn redacts_token_with_colon_separator() {
        let input = "token: 'my_token_value'";
        let output = redact_sensitive_text(input);
        assert!(output.contains("[REDACTED]"), "got: {}", output);
        assert!(!output.contains("my_token_value"));
    }

    #[test]
    fn redacts_password_assignment() {
        let input = "password=hunter2";
        let output = redact_sensitive_text(input);
        assert!(output.contains("[REDACTED]"), "got: {}", output);
        assert!(!output.contains("hunter2"));
    }

    #[test]
    fn redacts_secret_with_spaces() {
        let input = "secret : \"s3cr3t-val\"";
        let output = redact_sensitive_text(input);
        assert!(output.contains("[REDACTED]"), "got: {}", output);
        assert!(!output.contains("s3cr3t-val"));
    }

    #[test]
    fn combined_redaction_stress() {
        let input = "sk-abc12345678901234567890 AKIA1234567890ABCDEF Bearer eyJhbGciOiJIUzI1NiJ9.test api_key=\"super-secret-123\" token: 'val' password=hunter2 secret : \"s3cr3t\"";
        let output = redact_sensitive_text(input);
        assert!(output.contains("sk-[REDACTED]"), "missing sk redaction: {}", output);
        assert!(output.contains("AKIA[REDACTED]"), "missing AWS redaction: {}", output);
        assert!(output.contains("Bearer [REDACTED]"), "missing Bearer redaction: {}", output);
        assert!(!output.contains("super-secret-123"), "api_key not redacted: {}", output);
        assert!(!output.contains("hunter2"), "password not redacted: {}", output);
    }

    #[test]
    fn bearer_case_insensitive() {
        let input = "BEARER abcdefghijklmnop and bearer zyxwvutsrqpomn";
        let output = redact_sensitive_text(input);
        assert_eq!(output, "Bearer [REDACTED] and Bearer [REDACTED]");
    }

    #[test]
    fn no_false_positive_on_short_sk() {
        let input = "sk-short is fine";
        let output = redact_sensitive_text(input);
        assert_eq!(output, "sk-short is fine");
    }

    #[test]
    fn redacts_sk_proj_keys() {
        let input = "key is sk-proj-abcdefghij0123456789abcdefghij";
        let output = redact_sensitive_text(input);
        assert!(output.contains("sk-[REDACTED]"), "got: {}", output);
        assert!(!output.contains("abcdefghij0123456789"));
    }

    #[test]
    fn redacts_github_tokens() {
        let input = "ghp_abcdefghijklmnopqrstuvwxyz1234 and github_pat_abcdefghijklmnopqrstuvwxyz1234";
        let output = redact_sensitive_text(input);
        assert!(output.contains("ghp_[REDACTED]"), "got: {}", output);
        assert!(output.contains("github_pat_[REDACTED]"), "got: {}", output);
    }

    #[test]
    fn redacts_google_api_keys() {
        let input = "key: AIzaSyA1234567890abcdefghijklmno";
        let output = redact_sensitive_text(input);
        assert!(output.contains("AIza[REDACTED]"), "got: {}", output);
    }

    #[test]
    fn redacts_slack_tokens() {
        let input = "xoxb-123456-7890abcdef-ghijklmnop";
        let output = redact_sensitive_text(input);
        assert!(output.contains("xoxb-[REDACTED]"), "got: {}", output);
    }

    #[test]
    fn redacts_jwt_tokens() {
        let input = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
        let output = redact_sensitive_text(input);
        assert!(output.contains("[REDACTED_JWT]"), "got: {}", output);
    }

    #[test]
    fn redacts_connection_strings() {
        let input = "postgres://user:pass@host:5432/db";
        let output = redact_sensitive_text(input);
        assert!(output.contains("postgres://[REDACTED]"), "got: {}", output);
        assert!(!output.contains("user:pass"), "got: {}", output);
    }

    #[test]
    fn redacts_pem_keys() {
        let input = "before\n-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA0Z3VS5JJcds3xfn/ygWyF8PbnGy\n-----END RSA PRIVATE KEY-----\nafter";
        let output = redact_sensitive_text(input);
        assert!(output.contains("[REDACTED_PEM_KEY]"), "got: {}", output);
        assert!(!output.contains("MIIEowIBAAKCAQEA0Z3VS5JJcds3xfn"), "got: {}", output);
    }

    #[test]
    fn redacts_api_hyphen_key() {
        let input = "api-key=\"super-secret-123\"";
        let output = redact_sensitive_text(input);
        assert!(output.contains("[REDACTED]"), "got: {}", output);
        assert!(!output.contains("super-secret-123"), "got: {}", output);
    }

    // --- Gemini NOT_FOUND protobuf-detection tests ---

    /// Guard around `CHORUS_GEMINI_BASE_DIR` so the env-mutating tests below
    /// don't race with each other when cargo test runs them in parallel.
    fn gemini_env_lock() -> std::sync::MutexGuard<'static, ()> {
        use std::sync::{Mutex, OnceLock};
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    fn gemini_fixture(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("chorus_gemini_pb_{}", name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create fixture dir");
        dir
    }

    #[test]
    fn gemini_notfound_names_pb_files_when_present() {
        let _guard = gemini_env_lock();
        let fixture = gemini_fixture("names_pb");
        let conversations = fixture.join("default").join("conversations");
        std::fs::create_dir_all(&conversations).unwrap();
        std::fs::write(conversations.join("a.pb"), b"pb1").unwrap();
        std::fs::write(conversations.join("b.pb"), b"pb2").unwrap();

        std::env::set_var("CHORUS_GEMINI_BASE_DIR", &fixture);
        let hint = detect_gemini_pb_fallback_hint();
        std::env::remove_var("CHORUS_GEMINI_BASE_DIR");

        let hint = hint.expect("expected a protobuf hint when .pb files are present");
        assert!(hint.contains("protobuf (.pb)"), "hint missing protobuf (.pb) phrase: {}", hint);
        assert!(hint.contains("2 protobuf (.pb) files"), "hint should name both files: {}", hint);
        assert!(hint.contains("--chats-dir"), "hint should point at --chats-dir workaround: {}", hint);

        let _ = std::fs::remove_dir_all(&fixture);
    }

    #[test]
    fn gemini_notfound_stays_generic_when_no_pb() {
        let _guard = gemini_env_lock();
        let fixture = gemini_fixture("no_pb");
        // Create a profile + conversations/ dir but NO .pb files. Mix in an
        // unrelated file so we're sure the probe doesn't false-positive.
        let conversations = fixture.join("default").join("conversations");
        std::fs::create_dir_all(&conversations).unwrap();
        std::fs::write(conversations.join("readme.txt"), b"not protobuf").unwrap();

        std::env::set_var("CHORUS_GEMINI_BASE_DIR", &fixture);
        let hint = detect_gemini_pb_fallback_hint();
        std::env::remove_var("CHORUS_GEMINI_BASE_DIR");

        assert!(hint.is_none(), "expected no hint when no .pb files exist: {:?}", hint);

        let _ = std::fs::remove_dir_all(&fixture);
    }

    #[test]
    fn gemini_notfound_handles_missing_base_dir() {
        let _guard = gemini_env_lock();
        let fixture = gemini_fixture("missing_base");
        let nonexistent = fixture.join("not-real");
        // Don't create nonexistent — the probe should short-circuit cleanly.

        std::env::set_var("CHORUS_GEMINI_BASE_DIR", &nonexistent);
        let hint = detect_gemini_pb_fallback_hint();
        std::env::remove_var("CHORUS_GEMINI_BASE_DIR");

        assert!(hint.is_none(), "missing base dir should yield no hint");

        let _ = std::fs::remove_dir_all(&fixture);
    }

    // --- Cursor NOT_FOUND vscdb-detection tests ---

    fn cursor_env_lock() -> std::sync::MutexGuard<'static, ()> {
        use std::sync::{Mutex, OnceLock};
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    fn cursor_fixture(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("chorus_cursor_vscdb_{}", name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create fixture dir");
        dir
    }

    #[test]
    fn cursor_notfound_names_vscdb_files_when_present() {
        let _guard = cursor_env_lock();
        let fixture = cursor_fixture("names_vscdb");
        let ws = fixture.join("User").join("workspaceStorage");
        let ws1 = ws.join("abc123");
        let ws2 = ws.join("def456");
        std::fs::create_dir_all(&ws1).unwrap();
        std::fs::create_dir_all(&ws2).unwrap();
        std::fs::write(ws1.join("state.vscdb"), b"sqlite1").unwrap();
        std::fs::write(ws1.join("state.vscdb-wal"), b"journal").unwrap(); // sibling, ignored
        std::fs::write(ws2.join("state.vscdb"), b"sqlite2").unwrap();

        std::env::set_var("CHORUS_CURSOR_DATA_DIR", &fixture);
        let hint = detect_cursor_vscdb_fallback_hint();
        std::env::remove_var("CHORUS_CURSOR_DATA_DIR");

        let hint = hint.expect("expected a vscdb hint when state.vscdb files are present");
        assert!(hint.contains("SQLite state.vscdb"), "hint missing SQLite phrase: {}", hint);
        assert!(hint.contains("2 SQLite state.vscdb files"), "hint should count both workspaces: {}", hint);
        assert!(hint.contains("workspaceStorage"), "hint should name workspaceStorage: {}", hint);
        // -wal sibling must not inflate the count
        assert!(!hint.contains("3 SQLite"), "sibling -wal file incorrectly counted: {}", hint);

        let _ = std::fs::remove_dir_all(&fixture);
    }

    #[test]
    fn cursor_notfound_stays_generic_when_no_vscdb() {
        let _guard = cursor_env_lock();
        let fixture = cursor_fixture("no_vscdb");
        // Realistic shape: Cursor data dir + User/workspaceStorage but no .vscdb
        let ws = fixture.join("User").join("workspaceStorage").join("abc");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(ws.join("workspace.json"), b"{}").unwrap();

        std::env::set_var("CHORUS_CURSOR_DATA_DIR", &fixture);
        let hint = detect_cursor_vscdb_fallback_hint();
        std::env::remove_var("CHORUS_CURSOR_DATA_DIR");

        assert!(hint.is_none(), "expected no hint when no .vscdb files exist: {:?}", hint);

        let _ = std::fs::remove_dir_all(&fixture);
    }

    #[test]
    fn cursor_notfound_handles_missing_base_dir() {
        let _guard = cursor_env_lock();
        let fixture = cursor_fixture("missing_base");
        let nonexistent = fixture.join("not-real");

        std::env::set_var("CHORUS_CURSOR_DATA_DIR", &nonexistent);
        let hint = detect_cursor_vscdb_fallback_hint();
        std::env::remove_var("CHORUS_CURSOR_DATA_DIR");

        assert!(hint.is_none(), "missing base dir should yield no hint");

        let _ = std::fs::remove_dir_all(&fixture);
    }

    // ============================================================
    // ReadOptions flag tests: --include-user, --tool-calls
    // ============================================================

    use crate::adapters::ReadOptions;
    use super::{
        extract_claude_content_with_tool_calls, extract_text_with_tool_calls,
        read_claude_session_with_options, read_codex_session_with_options,
        read_gemini_session_with_options, read_cursor_session_with_options,
        select_conversation_turns, ConversationTurn,
    };
    use serde_json::json;

    fn claude_env_lock() -> std::sync::MutexGuard<'static, ()> {
        use std::sync::{Mutex, OnceLock};
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    fn codex_env_lock() -> std::sync::MutexGuard<'static, ()> {
        use std::sync::{Mutex, OnceLock};
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    fn gemini_read_env_lock() -> std::sync::MutexGuard<'static, ()> {
        use std::sync::{Mutex, OnceLock};
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    fn cursor_read_env_lock() -> std::sync::MutexGuard<'static, ()> {
        use std::sync::{Mutex, OnceLock};
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    fn claude_fixture(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("chorus_claude_opts_{}", name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn codex_fixture(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("chorus_codex_opts_{}", name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn gemini_fixture_read(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("chorus_gemini_opts_{}", name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cursor_fixture_read(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("chorus_cursor_opts_{}", name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // --- select_conversation_turns pure unit test ---

    #[test]
    fn select_conversation_turns_pairs_user_with_assistant() {
        let turns = vec![
            ConversationTurn { role: "user".into(), text: "Q1".into() },
            ConversationTurn { role: "assistant".into(), text: "A1".into() },
            ConversationTurn { role: "user".into(), text: "Q2".into() },
            ConversationTurn { role: "assistant".into(), text: "A2".into() },
        ];
        let out = select_conversation_turns(&turns, 1);
        let text: Vec<(&str, &str)> = out.iter().map(|t| (t.role.as_str(), t.text.as_str())).collect();
        assert_eq!(text, vec![("user", "Q2"), ("assistant", "A2")]);

        let out2 = select_conversation_turns(&turns, 2);
        let text2: Vec<(&str, &str)> =
            out2.iter().map(|t| (t.role.as_str(), t.text.as_str())).collect();
        assert_eq!(
            text2,
            vec![
                ("user", "Q1"),
                ("assistant", "A1"),
                ("user", "Q2"),
                ("assistant", "A2"),
            ]
        );
    }

    #[test]
    fn select_conversation_turns_bounds_prevent_stealing_previous_user() {
        // Two assistant turns with only one user prompt in between the first
        // one. The second assistant should not claim the first assistant's
        // user because lower_bound stops it.
        let turns = vec![
            ConversationTurn { role: "user".into(), text: "Q1".into() },
            ConversationTurn { role: "assistant".into(), text: "A1".into() },
            ConversationTurn { role: "assistant".into(), text: "A2".into() },
        ];
        let out = select_conversation_turns(&turns, 2);
        let labels: Vec<(&str, &str)> = out.iter().map(|t| (t.role.as_str(), t.text.as_str())).collect();
        assert_eq!(labels, vec![("user", "Q1"), ("assistant", "A1"), ("assistant", "A2")]);
    }

    // --- Claude: include_user / tool-calls ---

    fn write_claude_session(dir: &std::path::Path, id: &str, lines: &[serde_json::Value]) -> std::path::PathBuf {
        let proj = dir.join("-tmp-test-claude");
        std::fs::create_dir_all(&proj).unwrap();
        let file = proj.join(format!("{}.jsonl", id));
        let body = lines
            .iter()
            .map(|l| serde_json::to_string(l).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&file, body).unwrap();
        file
    }

    #[test]
    fn claude_include_user_interleaves_turns() {
        let _guard = claude_env_lock();
        let fixture = claude_fixture("include_user");
        let cwd = "/tmp/test-proj";
        let _ = write_claude_session(&fixture, "sess", &[
            json!({ "type": "user",      "cwd": cwd, "message": { "role": "user",      "content": [{ "type": "text", "text": "Please run the thing" }] } }),
            json!({ "type": "assistant", "cwd": cwd, "message": { "role": "assistant", "content": [{ "type": "text", "text": "I ran the thing." }] } }),
            json!({ "type": "user",      "cwd": cwd, "message": { "role": "user",      "content": [{ "type": "text", "text": "Now do the next step" }] } }),
            json!({ "type": "assistant", "cwd": cwd, "message": { "role": "assistant", "content": [{ "type": "text", "text": "Next step done." }] } }),
        ]);

        std::env::set_var("CHORUS_CLAUDE_PROJECTS_DIR", &fixture);
        let opts = ReadOptions { include_user: true, include_tool_calls: false };
        let session = read_claude_session_with_options(None, cwd, 2, opts).expect("read");
        std::env::remove_var("CHORUS_CLAUDE_PROJECTS_DIR");

        assert!(session.content.contains("USER:"), "missing USER header: {}", session.content);
        assert!(session.content.contains("ASSISTANT:"), "missing ASSISTANT header: {}", session.content);
        assert!(session.content.contains("Please run the thing"), "missing first user: {}", session.content);
        assert!(session.content.contains("Next step done."), "missing last assistant: {}", session.content);
        assert!(session.content.contains("---"), "missing separator: {}", session.content);
        // 4 selected: u/a/u/a
        assert_eq!(session.messages_returned, 4);

        let _ = std::fs::remove_dir_all(&fixture);
    }

    #[test]
    fn claude_tool_calls_emits_tool_use_input() {
        let _guard = claude_env_lock();
        let fixture = claude_fixture("tool_calls");
        let cwd = "/tmp/test-proj";
        let _ = write_claude_session(&fixture, "sess_tc", &[
            json!({ "type": "assistant", "cwd": cwd, "message": { "role": "assistant", "content": [
                { "type": "text", "text": "Reading the file." },
                { "type": "tool_use", "name": "Read", "input": { "file_path": "/tmp/thing.txt" } },
                { "type": "text", "text": "Done." }
            ] } }),
        ]);

        std::env::set_var("CHORUS_CLAUDE_PROJECTS_DIR", &fixture);
        let opts = ReadOptions { include_user: false, include_tool_calls: true };
        let session = read_claude_session_with_options(None, cwd, 1, opts).expect("read");
        std::env::remove_var("CHORUS_CLAUDE_PROJECTS_DIR");

        assert!(session.content.contains("[TOOL: Read]"), "tool header missing: {}", session.content);
        assert!(session.content.contains("/tmp/thing.txt"), "tool input missing: {}", session.content);
        assert!(session.content.contains("[/TOOL]"), "tool footer missing: {}", session.content);

        // Without tool-calls flag, the Read block is elided
        std::env::set_var("CHORUS_CLAUDE_PROJECTS_DIR", &fixture);
        let baseline = read_claude_session_with_options(None, cwd, 1, ReadOptions::default()).expect("read2");
        std::env::remove_var("CHORUS_CLAUDE_PROJECTS_DIR");
        assert!(!baseline.content.contains("[TOOL:"), "baseline should not render tool: {}", baseline.content);
        assert!(baseline.content.contains("Reading the file."), "baseline missing first text: {}", baseline.content);

        let _ = std::fs::remove_dir_all(&fixture);
    }

    // --- Codex: include_tool_calls ---

    #[test]
    fn codex_tool_calls_emits_function_call_arguments() {
        let _guard = codex_env_lock();
        let fixture = codex_fixture("tool_calls");
        let cwd = "/tmp/test-codex";
        let file = fixture.join("session-tc.jsonl");
        let lines = vec![
            json!({ "type": "session_meta", "payload": { "session_id": "sid", "cwd": cwd } }),
            json!({ "type": "response_item", "payload": {
                "type": "message", "role": "assistant", "content": [
                    { "text": "Let me call a function." },
                    { "type": "function_call", "name": "shell", "arguments": "{\"command\":\"ls\"}" }
                ]
            }}),
        ];
        let body = lines.iter().map(|l| serde_json::to_string(l).unwrap()).collect::<Vec<_>>().join("\n");
        std::fs::write(&file, body).unwrap();

        std::env::set_var("CHORUS_CODEX_SESSIONS_DIR", &fixture);
        let opts = ReadOptions { include_user: false, include_tool_calls: true };
        let session = read_codex_session_with_options(None, cwd, 1, opts).expect("read");
        std::env::remove_var("CHORUS_CODEX_SESSIONS_DIR");

        assert!(session.content.contains("[TOOL: shell]"), "missing tool header: {}", session.content);
        assert!(session.content.contains("ls"), "missing tool args: {}", session.content);

        let _ = std::fs::remove_dir_all(&fixture);
    }

    #[test]
    fn codex_include_user_interleaves() {
        let _guard = codex_env_lock();
        let fixture = codex_fixture("include_user");
        let cwd = "/tmp/test-codex-iu";
        let file = fixture.join("session-iu.jsonl");
        let lines = vec![
            json!({ "type": "session_meta", "payload": { "session_id": "sid", "cwd": cwd } }),
            json!({ "type": "response_item", "payload": {
                "type": "message", "role": "user", "content": [{ "text": "Question one" }]
            }}),
            json!({ "type": "response_item", "payload": {
                "type": "message", "role": "assistant", "content": [{ "text": "Answer one" }]
            }}),
        ];
        let body = lines.iter().map(|l| serde_json::to_string(l).unwrap()).collect::<Vec<_>>().join("\n");
        std::fs::write(&file, body).unwrap();

        std::env::set_var("CHORUS_CODEX_SESSIONS_DIR", &fixture);
        let opts = ReadOptions { include_user: true, include_tool_calls: false };
        let session = read_codex_session_with_options(None, cwd, 1, opts).expect("read");
        std::env::remove_var("CHORUS_CODEX_SESSIONS_DIR");

        assert!(session.content.contains("USER:"), "missing USER: {}", session.content);
        assert!(session.content.contains("Question one"), "missing user text: {}", session.content);
        assert!(session.content.contains("Answer one"), "missing assistant text: {}", session.content);
        assert_eq!(session.messages_returned, 2);

        let _ = std::fs::remove_dir_all(&fixture);
    }

    // --- Gemini: include_user over messages schema ---

    #[test]
    fn gemini_include_user_interleaves_messages() {
        let _guard = gemini_read_env_lock();
        let fixture = gemini_fixture_read("include_user");
        // Hash for cwd — not important, we'll use chats_dir to pin.
        let chats_dir = fixture.join("chats");
        std::fs::create_dir_all(&chats_dir).unwrap();
        let session_file = chats_dir.join("session-iu.json");
        let body = json!({
            "sessionId": "sid",
            "messages": [
                { "type": "user",   "content": "Prompt alpha" },
                { "type": "gemini", "content": "Reply alpha"  },
                { "type": "user",   "content": "Prompt beta"  },
                { "type": "gemini", "content": "Reply beta"   }
            ]
        });
        std::fs::write(&session_file, serde_json::to_string(&body).unwrap()).unwrap();

        let opts = ReadOptions { include_user: true, include_tool_calls: false };
        let session = read_gemini_session_with_options(
            None,
            "/tmp/ignored",
            Some(chats_dir.to_str().unwrap()),
            2,
            opts,
        ).expect("read");

        assert!(session.content.contains("USER:"), "missing USER: {}", session.content);
        assert!(session.content.contains("Prompt alpha"), "{}", session.content);
        assert!(session.content.contains("Reply beta"), "{}", session.content);
        assert_eq!(session.messages_returned, 4);

        let _ = std::fs::remove_dir_all(&fixture);
    }

    // --- Gemini: line-delimited .jsonl sessions ---

    #[test]
    fn gemini_jsonl_basic_read_returns_last_assistant() {
        let _guard = gemini_read_env_lock();
        let fixture = gemini_fixture_read("jsonl_basic");
        let chats_dir = fixture.join("chats");
        std::fs::create_dir_all(&chats_dir).unwrap();
        let session_file = chats_dir.join("session-jsonl-basic.jsonl");
        // Header + user + assistant (duplicated, streaming) + $set + user + assistant.
        let jsonl = concat!(
            r#"{"sessionId":"sid-jsonl-basic","kind":"main"}"#, "\n",
            r#"{"id":"u1","type":"user","content":[{"text":"first question"}]}"#, "\n",
            r#"{"$set":{"lastUpdated":"2026-04-24T00:00:00Z"}}"#, "\n",
            r#"{"id":"a1","type":"gemini","content":"first answer"}"#, "\n",
            r#"{"id":"a1","type":"gemini","content":"first answer"}"#, "\n",
            r#"{"id":"u2","type":"user","content":[{"text":"second question"}]}"#, "\n",
            r#"{"id":"a2","type":"gemini","content":"second answer"}"#, "\n",
        );
        std::fs::write(&session_file, jsonl).unwrap();

        let opts = ReadOptions::default();
        let session = read_gemini_session_with_options(
            None,
            "/tmp/ignored",
            Some(chats_dir.to_str().unwrap()),
            1,
            opts,
        ).expect("read jsonl");

        assert_eq!(session.session_id.as_deref(), Some("sid-jsonl-basic"));
        assert_eq!(session.message_count, 2, "dedupe should collapse duplicate a1");
        assert_eq!(session.content, "second answer");
        assert!(session.source.ends_with(".jsonl"), "source should be .jsonl: {}", session.source);

        let _ = std::fs::remove_dir_all(&fixture);
    }

    #[test]
    fn gemini_jsonl_include_user_interleaves() {
        let _guard = gemini_read_env_lock();
        let fixture = gemini_fixture_read("jsonl_include_user");
        let chats_dir = fixture.join("chats");
        std::fs::create_dir_all(&chats_dir).unwrap();
        let session_file = chats_dir.join("session-jsonl-iu.jsonl");
        let jsonl = concat!(
            r#"{"sessionId":"sid-jsonl-iu"}"#, "\n",
            r#"{"id":"u1","type":"user","content":[{"text":"prompt alpha"}]}"#, "\n",
            r#"{"id":"a1","type":"gemini","content":"reply alpha"}"#, "\n",
            r#"{"id":"u2","type":"user","content":[{"text":"prompt beta"}]}"#, "\n",
            r#"{"id":"a2","type":"gemini","content":"reply beta"}"#, "\n",
        );
        std::fs::write(&session_file, jsonl).unwrap();

        let opts = ReadOptions { include_user: true, include_tool_calls: false };
        let session = read_gemini_session_with_options(
            None,
            "/tmp/ignored",
            Some(chats_dir.to_str().unwrap()),
            2,
            opts,
        ).expect("read jsonl");

        assert!(session.content.contains("USER:"), "missing USER: {}", session.content);
        assert!(session.content.contains("prompt alpha"), "{}", session.content);
        assert!(session.content.contains("reply beta"), "{}", session.content);
        assert_eq!(session.messages_returned, 4);

        let _ = std::fs::remove_dir_all(&fixture);
    }

    #[test]
    fn gemini_jsonl_skips_metadata_and_dedupes_streaming_duplicates() {
        let _guard = gemini_read_env_lock();
        let fixture = gemini_fixture_read("jsonl_skip_meta");
        let chats_dir = fixture.join("chats");
        std::fs::create_dir_all(&chats_dir).unwrap();
        let session_file = chats_dir.join("session-jsonl-skip.jsonl");
        // Header, a bunch of $set metadata, one assistant message emitted three
        // times with the same id, a malformed line (should be skipped).
        let jsonl = concat!(
            r#"{"sessionId":"sid-jsonl-skip"}"#, "\n",
            r#"{"$set":{"lastUpdated":"2026-04-24T00:00:00Z"}}"#, "\n",
            r#"{"id":"u1","type":"user","content":[{"text":"hi"}]}"#, "\n",
            r#"{"$set":{"lastUpdated":"2026-04-24T00:00:01Z"}}"#, "\n",
            r#"{"id":"a1","type":"gemini","content":"streaming v1"}"#, "\n",
            r#"not valid json"#, "\n",
            r#"{"id":"a1","type":"gemini","content":"streaming v1"}"#, "\n",
            r#"{"id":"a1","type":"gemini","content":"streaming v1"}"#, "\n",
        );
        std::fs::write(&session_file, jsonl).unwrap();

        let opts = ReadOptions::default();
        let session = read_gemini_session_with_options(
            None,
            "/tmp/ignored",
            Some(chats_dir.to_str().unwrap()),
            1,
            opts,
        ).expect("read jsonl");

        assert_eq!(session.message_count, 1, "three duplicate ids should collapse to one");
        assert_eq!(session.content, "streaming v1");
        // The malformed line should surface as a warning.
        assert!(
            session.warnings.iter().any(|w| w.contains("unparseable")),
            "expected unparseable-line warning, got {:?}",
            session.warnings
        );

        let _ = std::fs::remove_dir_all(&fixture);
    }

    // --- Cursor: include_user over JSON messages ---

    #[test]
    fn cursor_include_user_interleaves() {
        let _guard = cursor_read_env_lock();
        let fixture = cursor_fixture_read("include_user");
        let ws = fixture.join("User").join("workspaceStorage").join("ws1");
        std::fs::create_dir_all(&ws).unwrap();
        let chat_file = ws.join("chat.json");
        let body = json!({
            "messages": [
                { "role": "user",      "content": "Hello cursor" },
                { "role": "assistant", "content": "Hi!" },
                { "role": "user",      "content": "Second request" },
                { "role": "assistant", "content": "Second response" }
            ]
        });
        std::fs::write(&chat_file, serde_json::to_string(&body).unwrap()).unwrap();

        std::env::set_var("CHORUS_CURSOR_DATA_DIR", &fixture);
        let opts = ReadOptions { include_user: true, include_tool_calls: false };
        let session = read_cursor_session_with_options(None, "/tmp/ignored", 2, opts).expect("read");
        std::env::remove_var("CHORUS_CURSOR_DATA_DIR");

        assert!(session.content.contains("USER:\nHello cursor"), "{}", session.content);
        assert!(session.content.contains("ASSISTANT:\nSecond response"), "{}", session.content);
        assert_eq!(session.messages_returned, 4);

        let _ = std::fs::remove_dir_all(&fixture);
    }

    // --- Pure extraction helper tests ---

    #[test]
    fn extract_claude_content_with_tool_calls_emits_tool_use_block() {
        let v = json!([
            { "type": "text", "text": "Hello" },
            { "type": "tool_use", "name": "Bash", "input": { "command": "ls" } }
        ]);
        let out = extract_claude_content_with_tool_calls(&v);
        assert!(out.contains("Hello"));
        assert!(out.contains("[TOOL: Bash]"));
        assert!(out.contains("\"command\""));
        assert!(out.contains("[/TOOL]"));
    }

    #[test]
    fn extract_text_with_tool_calls_emits_function_call_block() {
        let v = json!([
            { "text": "text part" },
            { "type": "function_call", "name": "shell", "arguments": "{\"x\":1}" }
        ]);
        let out = extract_text_with_tool_calls(&v);
        assert!(out.contains("text part"));
        assert!(out.contains("[TOOL: shell]"));
        assert!(out.contains("\"x\":1"));
        assert!(out.contains("[/TOOL]"));
    }

    // --- Gemini list: .jsonl indexing + cwd inference from scope dir ---

    fn gemini_list_env_lock() -> std::sync::MutexGuard<'static, ()> {
        use std::sync::{Mutex, OnceLock};
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    fn gemini_list_fixture(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("chorus_gemini_list_{}", name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create fixture dir");
        dir
    }

    #[test]
    fn gemini_list_indexes_both_json_and_jsonl() {
        let _guard = gemini_list_env_lock();
        let fixture = gemini_list_fixture("mixed_ext");
        // Layout: <fixture>/play/chats/session-*.{json,jsonl}
        let chats = fixture.join("play").join("chats");
        std::fs::create_dir_all(&chats).unwrap();
        std::fs::write(
            chats.join("session-alpha.json"),
            serde_json::json!({ "messages": [] }).to_string(),
        )
        .unwrap();
        std::fs::write(
            chats.join("session-beta.jsonl"),
            "{\"sessionId\":\"beta\"}\n{\"type\":\"user\",\"content\":\"hi\"}\n",
        )
        .unwrap();
        // A non-matching file should be ignored.
        std::fs::write(chats.join("ignore.txt"), b"not a session").unwrap();

        std::env::set_var("CHORUS_GEMINI_TMP_DIR", &fixture);
        let out = super::list_gemini_sessions(None, 10).expect("list_gemini_sessions");
        std::env::remove_var("CHORUS_GEMINI_TMP_DIR");

        let ids: Vec<String> = out
            .iter()
            .map(|e| e["session_id"].as_str().unwrap_or("").to_string())
            .collect();
        assert!(ids.contains(&"session-alpha".to_string()), "missing .json: {:?}", ids);
        assert!(ids.contains(&"session-beta".to_string()), "missing .jsonl: {:?}", ids);
        assert_eq!(out.len(), 2, "expected exactly 2 entries, got: {:?}", ids);

        let _ = std::fs::remove_dir_all(&fixture);
    }

    #[test]
    fn gemini_list_named_scope_returns_cwd_hint_not_null() {
        let _guard = gemini_list_env_lock();
        let fixture = gemini_list_fixture("named_scope");
        let chats = fixture.join("play").join("chats");
        std::fs::create_dir_all(&chats).unwrap();
        std::fs::write(
            chats.join("session-one.json"),
            serde_json::json!({ "messages": [] }).to_string(),
        )
        .unwrap();

        std::env::set_var("CHORUS_GEMINI_TMP_DIR", &fixture);
        let out = super::list_gemini_sessions(None, 10).expect("list_gemini_sessions");
        std::env::remove_var("CHORUS_GEMINI_TMP_DIR");

        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0]["cwd"].as_str(),
            Some("play"),
            "expected cwd=play, got {:?}",
            out[0]["cwd"]
        );
        // Named scope must NOT emit a scope_hash field.
        assert!(
            out[0].get("scope_hash").is_none(),
            "named scope should not have scope_hash: {:?}",
            out[0]
        );

        let _ = std::fs::remove_dir_all(&fixture);
    }

    #[test]
    fn gemini_list_hex_hash_scope_reports_scope_hash() {
        let _guard = gemini_list_env_lock();
        let fixture = gemini_list_fixture("hex_scope");
        // 64-char hex scope (SHA-256-shaped).
        let hex = "a".repeat(64);
        let chats = fixture.join(&hex).join("chats");
        std::fs::create_dir_all(&chats).unwrap();
        std::fs::write(
            chats.join("session-hx.jsonl"),
            "{\"sessionId\":\"hx\"}\n",
        )
        .unwrap();

        std::env::set_var("CHORUS_GEMINI_TMP_DIR", &fixture);
        let out = super::list_gemini_sessions(None, 10).expect("list_gemini_sessions");
        std::env::remove_var("CHORUS_GEMINI_TMP_DIR");

        assert_eq!(out.len(), 1);
        // Hex scope is lossy — we return the scope dir as the cwd bucket
        // AND surface it under scope_hash so callers can opt into a map.
        assert_eq!(out[0]["cwd"].as_str(), Some(hex.as_str()));
        assert_eq!(out[0]["scope_hash"].as_str(), Some(hex.as_str()));

        let _ = std::fs::remove_dir_all(&fixture);
    }

    #[test]
    fn infer_gemini_scope_classifies_named_vs_hex() {
        use super::infer_gemini_scope;
        use std::path::PathBuf;

        // Named scope: just a word.
        let named = PathBuf::from("/tmp/.gemini/tmp/play/chats/session-a.jsonl");
        let (cwd, hash) = infer_gemini_scope(&named);
        assert_eq!(cwd, serde_json::Value::String("play".into()));
        assert_eq!(hash, None);

        // Hex scope: 64 hex chars.
        let hex = "c".repeat(64);
        let hexp = PathBuf::from(format!("/tmp/.gemini/tmp/{}/chats/session-b.json", hex));
        let (cwd2, hash2) = infer_gemini_scope(&hexp);
        assert_eq!(cwd2, serde_json::Value::String(hex.clone()));
        assert_eq!(hash2.as_deref(), Some(hex.as_str()));

        // Short hex-looking (<40) stays named — not flagged as a hash.
        let shortish = PathBuf::from("/tmp/.gemini/tmp/abc/chats/session-c.json");
        let (cwd3, hash3) = infer_gemini_scope(&shortish);
        assert_eq!(cwd3, serde_json::Value::String("abc".into()));
        assert_eq!(hash3, None);
    }
}
