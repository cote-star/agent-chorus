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
pub enum BridgeErrorCode {
    NotFound,
    ParseFailed,
    InvalidHandoff,
    UnsupportedAgent,
    UnsupportedMode,
    IoError,
    EmptySession,
}

impl BridgeErrorCode {
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

pub fn classify_error(message: &str) -> BridgeErrorCode {
    let lower = message.to_ascii_lowercase();
    if lower.contains("unsupported agent") || lower.contains("unknown agent") {
        BridgeErrorCode::UnsupportedAgent
    } else if lower.contains("unsupported mode") {
        BridgeErrorCode::UnsupportedMode
    } else if lower.contains("no") && lower.contains("session found") || lower.contains("not found") {
        BridgeErrorCode::NotFound
    } else if lower.contains("failed to parse") || lower.contains("failed to read") {
        BridgeErrorCode::ParseFailed
    } else if lower.contains("missing required") || lower.contains("invalid handoff") || lower.contains("must provide session_id") {
        BridgeErrorCode::InvalidHandoff
    } else if lower.contains("has no messages") || lower.contains("history is empty") {
        BridgeErrorCode::EmptySession
    } else {
        BridgeErrorCode::IoError
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
    let base_dir = codex_base_dir();
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

    let parsed = parse_codex_jsonl(&target_file, last_n)?;
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
    let base_dir = claude_base_dir();
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

    let parsed = parse_claude_jsonl(&target_file, last_n)?;
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
    let dirs = resolve_gemini_chat_dirs(chats_dir, cwd)?;
    if dirs.is_empty() {
        return Err(anyhow!("No Gemini session found. Searched chats directories:"));
    }

    let target_file = if let Some(id_value) = id {
        let mut candidates = Vec::new();
        for dir in &dirs {
            let mut files = collect_matching_files(dir, false, &|file_path| {
                has_extension(file_path, "json") && path_contains(file_path, id_value)
            })?;
            candidates.append(&mut files);
        }
        sort_files_by_mtime_desc(&mut candidates);
        candidates
            .first()
            .map(|f| f.path.clone())
            .context("No Gemini session found.")?
    } else {
        let mut candidates = Vec::new();
        for dir in &dirs {
            let mut files = collect_matching_files(dir, false, &|file_path| {
                has_extension(file_path, "json")
                    && file_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|name| name.starts_with("session-"))
                        .unwrap_or(false)
            })?;
            candidates.append(&mut files);
        }
        sort_files_by_mtime_desc(&mut candidates);
        candidates
            .first()
            .map(|f| f.path.clone())
            .context("No Gemini session found.")?
    };

    let parsed = parse_gemini_json(&target_file, last_n)?;

    Ok(Session {
        agent: "gemini",
        content: parsed.content,
        source: target_file.to_string_lossy().to_string(),
        warnings: parsed.warnings,
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

fn parse_codex_jsonl(path: &Path, last_n: usize) -> Result<ParsedContent> {
    let lines = read_jsonl_lines(path)?;
    let mut messages: Vec<Value> = Vec::new();
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
                    messages.push(json["payload"].clone());
                } else if json["type"] == "event_msg" && json["payload"]["type"] == "agent_message" {
                    let payload = &json["payload"];
                    messages.push(serde_json::json!({
                        "role": "assistant",
                        "content": payload["message"].clone()
                    }));
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

    let message_count = messages.iter().filter(|m| {
        m["role"].as_str().unwrap_or("").eq_ignore_ascii_case("assistant")
    }).count();

    let timestamp = file_modified_iso(path);

    if session_id.is_none() {
        session_id = path.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string());
    }

    let assistant_msgs: Vec<&Value> = messages.iter().filter(|m| {
        m["role"].as_str().unwrap_or("").eq_ignore_ascii_case("assistant")
    }).collect();

    if !messages.is_empty() {
        if last_n > 1 && !assistant_msgs.is_empty() {
            let selected: Vec<&Value> = assistant_msgs.iter().rev().take(last_n).rev().cloned().collect();
            let messages_returned = selected.len();
            let content = selected.iter().map(|m| {
                let text = extract_text(&m["content"]);
                if text.is_empty() { "[No text content]".to_string() } else { text }
            }).collect::<Vec<String>>().join("\n---\n");
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

        let selected = assistant_msgs.last().cloned().or_else(|| messages.last());
        if let Some(message) = selected {
            let text = extract_text(&message["content"]);
            return Ok(ParsedContent {
                content: if text.is_empty() {
                    "[No text content]".to_string()
                } else {
                    redact_sensitive_text(&text)
                },
                warnings,
                session_id,
                cwd: session_cwd,
                timestamp,
                message_count,
                messages_returned: 1,
            });
        }
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

fn parse_claude_jsonl(path: &Path, last_n: usize) -> Result<ParsedContent> {
    let lines = read_jsonl_lines(path)?;
    let mut messages: Vec<String> = Vec::new();
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

                let is_assistant = json["type"] == "assistant"
                    || message["role"]
                        .as_str()
                        .map(|role| role.eq_ignore_ascii_case("assistant"))
                        .unwrap_or(false);

                if !is_assistant {
                    continue;
                }

                let content_field = if message.get("content").is_some() {
                    &message["content"]
                } else {
                    &json["content"]
                };
                let text = extract_claude_text(content_field);
                if !text.is_empty() {
                    messages.push(text);
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
        if last_n > 1 {
            let selected: Vec<&String> = messages.iter().rev().take(last_n).collect::<Vec<_>>().into_iter().rev().collect();
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

fn parse_gemini_json(path: &Path, last_n: usize) -> Result<ParsedContent> {
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
        let assistant_count = messages.iter().filter(|m| {
            m["type"].as_str().map(|t| {
                let lower = t.to_ascii_lowercase();
                lower == "gemini" || lower == "assistant" || lower == "model"
            }).unwrap_or(false)
        }).count();

        let is_assistant_msg = |m: &&Value| {
            m["type"].as_str().map(|t| {
                let lower = t.to_ascii_lowercase();
                lower == "gemini" || lower == "assistant" || lower == "model"
            }).unwrap_or(false)
        };

        let assistant_msgs: Vec<&Value> = messages.iter().filter(is_assistant_msg).collect();

        if last_n > 1 && !assistant_msgs.is_empty() {
            let selected: Vec<&&Value> = assistant_msgs.iter().rev().take(last_n).collect::<Vec<_>>().into_iter().rev().collect();
            let messages_returned = selected.len();
            let content = selected.iter().map(|m| {
                let text = extract_text(&m["content"]);
                if text.is_empty() { "[No text content]".to_string() } else { text }
            }).collect::<Vec<String>>().join("\n---\n");
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

        let selected = messages.iter().rev().find(is_assistant_msg).or_else(|| messages.last());

        if let Some(message) = selected {
            return Ok(ParsedContent {
                content: {
                    let text = extract_text(&message["content"]);
                    if text.is_empty() {
                        "[No text content]".to_string()
                    } else {
                        redact_sensitive_text(&text)
                    }
                },
                warnings: Vec::new(),
                session_id,
                cwd: None,
                timestamp,
                message_count: assistant_count,
                messages_returned: 1,
            });
        }
        return Err(anyhow!("Gemini session has no messages."));
    }

    if let Some(history) = session["history"].as_array() {
        let assistant_count = history.iter().filter(|t| {
            !t["role"].as_str().map(|r| r.eq_ignore_ascii_case("user")).unwrap_or(false)
        }).count();

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

        let is_not_user = |t: &&Value| {
            !t["role"].as_str().map(|role| role.eq_ignore_ascii_case("user")).unwrap_or(false)
        };

        let assistant_turns: Vec<&Value> = history.iter().filter(is_not_user).collect();

        if last_n > 1 && !assistant_turns.is_empty() {
            let selected: Vec<&&Value> = assistant_turns.iter().rev().take(last_n).collect::<Vec<_>>().into_iter().rev().collect();
            let messages_returned = selected.len();
            let content = selected.iter().map(|t| extract_turn_text(t)).collect::<Vec<String>>().join("\n---\n");
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

        let selected = history.iter().rev().find(is_not_user).or_else(|| history.last());
        if let Some(turn) = selected {
            let text = extract_turn_text(turn);
            return Ok(ParsedContent {
                content: redact_sensitive_text(&text),
                warnings: Vec::new(),
                session_id,
                cwd: None,
                timestamp,
                message_count: assistant_count,
                messages_returned: 1,
            });
        }

        return Err(anyhow!("Gemini history is empty."));
    }

    Err(anyhow!(
        "Unknown Gemini session schema. Supported fields: messages, history."
    ))
}

fn extract_text(value: &Value) -> String {
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

fn extract_claude_text(value: &Value) -> String {
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

fn read_jsonl_lines(path: &Path) -> Result<Vec<String>> {
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
    Ok(reader.lines().map_while(Result::ok).collect())
}

fn find_latest_by_cwd(
    files: &[FileEntry],
    expected_cwd: &Path,
    cwd_extractor: fn(&Path) -> Option<PathBuf>,
) -> Option<PathBuf> {
    for file in files {
        if let Some(file_cwd) = cwd_extractor(&file.path) {
            if file_cwd == expected_cwd {
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

fn redact_sensitive_text(input: &str) -> String {
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
            if file_cwd.as_ref() != Some(expected) {
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
            if file_cwd.as_ref() != Some(expected) {
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
            has_extension(p, "json") && p.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with("session-")).unwrap_or(false)
        })?;
        candidates.append(&mut files);
    }
    sort_files_by_mtime_desc(&mut candidates);
    let mut entries = Vec::new();
    for file in candidates.iter().take(limit) {
        let session_id = file.path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string();
        entries.push(serde_json::json!({
            "session_id": session_id,
            "agent": "gemini",
            "cwd": serde_json::Value::Null,
            "modified_at": file_modified_iso(&file.path),
            "file_path": file.path.to_string_lossy().to_string(),
        }));
    }
    Ok(entries)
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
            if file_cwd.as_ref() != Some(expected) {
                continue;
            }
        }

        if fs::metadata(&file.path).map(|m| m.len() > MAX_FILE_SIZE).unwrap_or(false) {
            continue;
        }

        let content = match fs::read_to_string(&file.path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        if content.to_ascii_lowercase().contains(&query_lower) {
            let session_id = file.path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string();
            entries.push(serde_json::json!({
                "session_id": session_id,
                "agent": "codex",
                "cwd": file_cwd.map(|p| p.to_string_lossy().to_string()),
                "modified_at": file_modified_iso(&file.path),
                "file_path": file.path.to_string_lossy().to_string(),
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
            if file_cwd.as_ref() != Some(expected) {
                continue;
            }
        }

        if fs::metadata(&file.path).map(|m| m.len() > MAX_FILE_SIZE).unwrap_or(false) {
            continue;
        }

        let content = match fs::read_to_string(&file.path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        if content.to_ascii_lowercase().contains(&query_lower) {
            let session_id = file.path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string();
            entries.push(serde_json::json!({
                "session_id": session_id,
                "agent": "claude",
                "cwd": file_cwd.map(|p| p.to_string_lossy().to_string()),
                "modified_at": file_modified_iso(&file.path),
                "file_path": file.path.to_string_lossy().to_string(),
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
            has_extension(p, "json") && p.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with("session-")).unwrap_or(false)
        })?;
        candidates.append(&mut files);
    }
    sort_files_by_mtime_desc(&mut candidates);
    
    let query_lower = query.to_ascii_lowercase();
    let mut entries = Vec::new();
    
    for file in candidates {
        if entries.len() >= limit { break; }

        if fs::metadata(&file.path).map(|m| m.len() > MAX_FILE_SIZE).unwrap_or(false) {
            continue;
        }

        let content = match fs::read_to_string(&file.path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        if content.to_ascii_lowercase().contains(&query_lower) {
            let session_id = file.path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string();
            entries.push(serde_json::json!({
                "session_id": session_id,
                "agent": "gemini",
                "cwd": serde_json::Value::Null,
                "modified_at": file_modified_iso(&file.path),
                "file_path": file.path.to_string_lossy().to_string(),
            }));
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

        let content = match fs::read_to_string(&file.path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        if let Some(expected) = expected_cwd_text.as_ref() {
            if !content.to_ascii_lowercase().contains(expected) {
                continue;
            }
        }

        if content.to_ascii_lowercase().contains(&query_lower) {
            let session_id = file.path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string();
            entries.push(serde_json::json!({
                "session_id": session_id,
                "agent": "cursor",
                "cwd": serde_json::Value::Null,
                "modified_at": file_modified_iso(&file.path),
                "file_path": file.path.to_string_lossy().to_string(),
            }));
        }
    }
    Ok(entries)
}

// --- Cursor support ---

fn cursor_base_dir() -> PathBuf {
    std::env::var("BRIDGE_CURSOR_DATA_DIR")
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

pub fn read_cursor_session(id: Option<&str>, _cwd: &str) -> Result<Session> {
    let base_dir = cursor_base_dir();
    if !base_dir.exists() {
        return Err(anyhow!("No Cursor session found. Data directory not found: {}", base_dir.display()));
    }

    let workspaces_dir = base_dir.join("User").join("workspaceStorage");
    if !workspaces_dir.exists() {
        return Err(anyhow!("No Cursor session found. Workspace storage not found: {}", workspaces_dir.display()));
    }

    // Look for composer/chat state files in workspace storage
    let files = collect_matching_files(&workspaces_dir, true, &|p| {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        (name.ends_with(".json") || name.ends_with(".jsonl"))
            && (name.contains("chat") || name.contains("composer") || name.contains("conversation"))
            && id.map(|needle| p.to_string_lossy().contains(needle)).unwrap_or(true)
    })?;

    if files.is_empty() {
        return Err(anyhow!("No Cursor session found."));
    }

    let target_file = files[0].path.clone();

    // Try JSON first, then JSONL
    let content_str = fs::read_to_string(&target_file)?;
    let content = if let Ok(json) = serde_json::from_str::<Value>(&content_str) {
        // Extract text from JSON structure
        if let Some(messages) = json.get("messages").and_then(|m| m.as_array()) {
            let assistant_msgs: Vec<String> = messages.iter()
                .filter(|m| m["role"].as_str().map(|r| r == "assistant").unwrap_or(false))
                .filter_map(|m| m["content"].as_str().map(|s| s.to_string()))
                .collect();
            if let Some(last) = assistant_msgs.last() {
                last.clone()
            } else {
                "[No assistant messages found]".to_string()
            }
        } else if let Some(text) = json.get("content").and_then(|c| c.as_str()) {
            text.to_string()
        } else {
            json.to_string()
        }
    } else {
        // JSONL format
        let mut messages = Vec::new();
        for line in content_str.lines().filter(|l| !l.is_empty()) {
            if let Ok(json) = serde_json::from_str::<Value>(line) {
                if json["role"].as_str().map(|r| r == "assistant").unwrap_or(false) {
                    if let Some(text) = json["content"].as_str() {
                        messages.push(text.to_string());
                    }
                }
            }
        }
        if let Some(last) = messages.last() {
            last.clone()
        } else {
            content_str.lines().rev().take(20).collect::<Vec<&str>>().into_iter().rev().collect::<Vec<&str>>().join("\n")
        }
    };

    let session_id = target_file.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string());
    let timestamp = file_modified_iso(&target_file);

    Ok(Session {
        agent: "cursor",
        content: redact_sensitive_text(&content),
        source: target_file.to_string_lossy().to_string(),
        warnings: Vec::new(),
        session_id,
        cwd: None,
        timestamp,
        message_count: 1,
        messages_returned: 1,
    })
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

fn codex_base_dir() -> PathBuf {
    std::env::var("BRIDGE_CODEX_SESSIONS_DIR")
        .ok()
        .and_then(|value| expand_home(&value))
        .unwrap_or_else(|| expand_home("~/.codex/sessions").unwrap_or_else(|| PathBuf::from("~/.codex/sessions")))
}

fn claude_base_dir() -> PathBuf {
    std::env::var("BRIDGE_CLAUDE_PROJECTS_DIR")
        .ok()
        .and_then(|value| expand_home(&value))
        .unwrap_or_else(|| expand_home("~/.claude/projects").unwrap_or_else(|| PathBuf::from("~/.claude/projects")))
}

fn gemini_tmp_base_dir() -> PathBuf {
    std::env::var("BRIDGE_GEMINI_TMP_DIR")
        .ok()
        .and_then(|value| expand_home(&value))
        .unwrap_or_else(|| expand_home("~/.gemini/tmp").unwrap_or_else(|| PathBuf::from("~/.gemini/tmp")))
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
    use super::redact_sensitive_text;

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
}
