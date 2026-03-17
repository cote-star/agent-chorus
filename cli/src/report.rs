use crate::adapters;
use crate::agents::Session;
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::collections::HashSet;

#[derive(Clone, Debug)]
pub struct SourceSpec {
    pub agent: String,
    pub session_id: Option<String>,
    pub current_session: bool,
    pub cwd: Option<String>,
    pub chats_dir: Option<String>,
    pub last_n: Option<usize>,
}

#[derive(Debug)]
pub struct ReportRequest {
    pub mode: String,
    pub task: String,
    pub success_criteria: Vec<String>,
    pub sources: Vec<SourceSpec>,
    pub constraints: Vec<String>,
}

pub fn parse_source_arg(raw: &str) -> Result<SourceSpec> {
    let mut parts = raw.splitn(2, ':');
    let agent = parts.next().unwrap_or("").trim().to_ascii_lowercase();
    let session_id = parts.next().map(|v| v.trim().to_string()).filter(|v| !v.is_empty());

    validate_agent(&agent)?;

    Ok(SourceSpec {
        agent,
        session_id: session_id.clone(),
        current_session: session_id.is_none(),
        cwd: None,
        chats_dir: None,
        last_n: None,
    })
}

const MAX_HANDOFF_SIZE: u64 = 1024 * 1024; // 1 MB

pub fn load_handoff(path: &str) -> Result<ReportRequest> {
    let meta = std::fs::metadata(path).with_context(|| format!("Failed to read handoff file: {}", path))?;
    if meta.len() > MAX_HANDOFF_SIZE {
        return Err(anyhow!("Invalid handoff: file exceeds 1MB size limit"));
    }
    let raw = std::fs::read_to_string(path).with_context(|| format!("Failed to read handoff file: {}", path))?;
    let root: Value = serde_json::from_str(&raw).with_context(|| format!("Failed to parse handoff JSON: {}", path))?;

    // Validate no extra fields
    if let Some(obj) = root.as_object() {
        let allowed = ["mode", "task", "success_criteria", "sources", "constraints"];
        let extra: Vec<&String> = obj.keys().filter(|k| !allowed.contains(&k.as_str())).collect();
        if !extra.is_empty() {
            return Err(anyhow!("Invalid handoff: unexpected fields: {}", extra.iter().map(|k| k.as_str()).collect::<Vec<_>>().join(", ")));
        }
    } else {
        return Err(anyhow!("Invalid handoff: must be a JSON object"));
    }

    let mode = root["mode"]
        .as_str()
        .map(|v| v.to_ascii_lowercase())
        .context("Handoff is missing required string field: mode")?;
    validate_mode(&mode)?;

    let task = root["task"]
        .as_str()
        .map(|v| v.to_string())
        .context("Handoff is missing required string field: task")?;

    let success_criteria = root["success_criteria"]
        .as_array()
        .context("Handoff is missing required array field: success_criteria")?
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect::<Vec<String>>();
    if success_criteria.is_empty() {
        return Err(anyhow!("Handoff success_criteria must contain at least one string"));
    }

    let mut sources = Vec::new();
    for source in root["sources"]
        .as_array()
        .context("Handoff is missing required array field: sources")?
    {
        let agent = source["agent"]
            .as_str()
            .map(|v| v.to_ascii_lowercase())
            .context("Each source must include string field: agent")?;
        validate_agent(&agent)?;

        let session_id = source
            .get("session_id")
            .and_then(|v| v.as_str())
            .map(|v| v.to_string());
        let current_session = source
            .get("current_session")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if session_id.is_none() && !current_session {
            return Err(anyhow!(
                "Each source must provide session_id or set current_session=true"
            ));
        }

        let cwd = source
            .get("cwd")
            .and_then(|v| v.as_str())
            .map(|v| v.to_string());

        sources.push(SourceSpec {
            agent,
            session_id,
            current_session,
            cwd,
            chats_dir: None,
            last_n: None,
        });
    }

    let constraints = root
        .get("constraints")
        .and_then(|v| v.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();

    Ok(ReportRequest {
        mode,
        task,
        success_criteria,
        sources,
        constraints,
    })
}

pub fn build_report(request: &ReportRequest, default_cwd: &str) -> Value {
    let mut successful: Vec<(SourceSpec, Session, String)> = Vec::new();
    let mut missing: Vec<(SourceSpec, String, String)> = Vec::new();

    for source in &request.sources {
        let evidence = evidence_tag(source);
        match read_source(source, default_cwd) {
            Ok(session) => successful.push((source.clone(), session, evidence)),
            Err(error) => missing.push((source.clone(), error.to_string(), evidence)),
        }
    }

    let mut findings: Vec<Value> = Vec::new();

    for (source, error, evidence) in &missing {
        findings.push(json!({
            "severity": "P1",
            "summary": format!("Source unavailable: {} ({})", source.agent, error),
            "evidence": [evidence],
            "confidence": 0.9
        }));
    }

    for (_, session, evidence) in &successful {
        for warning in &session.warnings {
            findings.push(json!({
                "severity": "P2",
                "summary": format!("Source warning: {}", warning),
                "evidence": [evidence],
                "confidence": 0.75
            }));
        }
    }

    let unique_count;

    if successful.len() >= 2 {
        let topic_sets: Vec<HashSet<String>> = successful
            .iter()
            .map(|(_, session, _)| extract_topics(&session.content))
            .collect();

        let mut pairs: Vec<(String, String, f64)> = Vec::new();
        for i in 0..topic_sets.len() {
            for j in (i + 1)..topic_sets.len() {
                let sim = jaccard_similarity(&topic_sets[i], &topic_sets[j]);
                pairs.push((
                    successful[i].0.agent.clone(),
                    successful[j].0.agent.clone(),
                    sim,
                ));
            }
        }

        let avg_sim = pairs.iter().map(|(_, _, s)| s).sum::<f64>() / pairs.len() as f64;
        unique_count = if avg_sim > 0.6 { 1 } else { successful.len() };

        let pair_detail = pairs
            .iter()
            .map(|(a, b, s)| format!("{} \u{2194} {}: {:.0}%", a, b, s * 100.0))
            .collect::<Vec<String>>()
            .join(", ");

        if avg_sim > 0.6 {
            findings.push(json!({
                "severity": "P3",
                "summary": format!("Agent outputs are broadly aligned (similarity: {:.0}%)", avg_sim * 100.0),
                "detail": pair_detail,
                "evidence": successful.iter().map(|(_, _, tag)| tag.clone()).collect::<Vec<String>>(),
                "confidence": 0.8
            }));
        } else if avg_sim > 0.3 {
            findings.push(json!({
                "severity": "P2",
                "summary": format!("Agent outputs partially overlap (similarity: {:.0}%)", avg_sim * 100.0),
                "detail": pair_detail,
                "evidence": successful.iter().map(|(_, _, tag)| tag.clone()).collect::<Vec<String>>(),
                "confidence": 0.7
            }));
        } else {
            findings.push(json!({
                "severity": "P1",
                "summary": format!("Divergent agent outputs (similarity: {:.0}%)", avg_sim * 100.0),
                "detail": pair_detail,
                "evidence": successful.iter().map(|(_, _, tag)| tag.clone()).collect::<Vec<String>>(),
                "confidence": 0.75
            }));
        }
    } else {
        unique_count = 1;
        findings.push(json!({
            "severity": "P2",
            "summary": "Insufficient comparable sources",
            "evidence": successful.iter().map(|(_, _, tag)| tag.clone()).collect::<Vec<String>>(),
            "confidence": 0.5
        }));
    }

    let mut recommended_next_actions = Vec::new();
    if !missing.is_empty() {
        recommended_next_actions
            .push("Provide valid session identifiers or cwd values for unavailable sources.".to_string());
    }
    if unique_count > 1 {
        recommended_next_actions
            .push("Inspect full transcripts for diverging sources before final decisions.".to_string());
    }
    if !request.constraints.is_empty() {
        recommended_next_actions.push(format!(
            "Verify recommendations against constraints: {}.",
            request.constraints.join("; ")
        ));
    }
    if recommended_next_actions.is_empty() {
        recommended_next_actions.push("No immediate action required.".to_string());
    }

    let open_questions = missing
        .iter()
        .map(|(source, error, _)| format!("Missing source {}: {}", source.agent, error))
        .collect::<Vec<String>>();

    let verdict = compute_verdict(&request.mode, &missing, unique_count, successful.len());

    json!({
        "mode": request.mode,
        "task": request.task,
        "success_criteria": request.success_criteria,
        "sources_used": successful
            .iter()
            .map(|(_, session, evidence)| format!("{} {}", evidence, session.source))
            .collect::<Vec<String>>(),
        "verdict": verdict,
        "findings": findings,
        "recommended_next_actions": recommended_next_actions,
        "open_questions": open_questions,
    })
}

pub fn report_to_markdown(report: &Value) -> String {
    let mut lines = Vec::new();
    lines.push("### Agent Chorus Coordinator Report".to_string());
    lines.push(String::new());
    lines.push(format!("**Mode:** {}", report["mode"].as_str().unwrap_or("unknown")));
    lines.push(format!("**Task:** {}", report["task"].as_str().unwrap_or("")));
    lines.push("**Success Criteria:**".to_string());

    if let Some(criteria) = report["success_criteria"].as_array() {
        for criterion in criteria {
            lines.push(format!("- {}", criterion.as_str().unwrap_or("")));
        }
    }

    lines.push(String::new());
    lines.push("**Sources Used:**".to_string());
    if let Some(sources) = report["sources_used"].as_array() {
        for source in sources {
            lines.push(format!("- {}", source.as_str().unwrap_or("")));
        }
    }

    lines.push(String::new());
    lines.push(format!("**Verdict:** {}", report["verdict"].as_str().unwrap_or("")));
    lines.push(String::new());
    lines.push("**Findings:**".to_string());

    if let Some(findings) = report["findings"].as_array() {
        for finding in findings {
            let severity = finding["severity"].as_str().unwrap_or("P2");
            let summary = finding["summary"].as_str().unwrap_or("");
            let confidence = finding["confidence"].as_f64().unwrap_or(0.0);
            let evidence = finding["evidence"]
                .as_array()
                .map(|values| {
                    values
                        .iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<&str>>()
                        .join(", ")
                })
                .unwrap_or_default();
            lines.push(format!(
                "- **{}:** {} (evidence: {}; confidence: {:.2})",
                severity, summary, evidence, confidence
            ));
            if let Some(detail) = finding.get("detail").and_then(|d| d.as_str()) {
                lines.push(format!("    Pairs: {}", detail));
            }
        }
    }

    lines.push(String::new());
    lines.push("**Recommended Next Actions:**".to_string());
    if let Some(actions) = report["recommended_next_actions"].as_array() {
        for (index, action) in actions.iter().enumerate() {
            lines.push(format!("{}. {}", index + 1, action.as_str().unwrap_or("")));
        }
    }

    if let Some(open_questions) = report["open_questions"].as_array() {
        if !open_questions.is_empty() {
            lines.push(String::new());
            lines.push("**Open Questions:**".to_string());
            for question in open_questions {
                lines.push(format!("- {}", question.as_str().unwrap_or("")));
            }
        }
    }

    lines.join("\n")
}

fn read_source(source: &SourceSpec, default_cwd: &str) -> Result<Session> {
    let cwd = source.cwd.as_deref().unwrap_or(default_cwd);
    let adapter = adapters::get_adapter(&source.agent)
        .ok_or_else(|| anyhow!("Unsupported agent: {}", source.agent))?;
    adapter.read_session(source.session_id.as_deref(), cwd, source.chats_dir.as_deref(), source.last_n.unwrap_or(10))
}

fn evidence_tag(source: &SourceSpec) -> String {
    let id = source
        .session_id
        .as_ref()
        .map(|value| shorten(value))
        .unwrap_or_else(|| {
            if source.current_session {
                "latest".to_string()
            } else {
                "unspecified".to_string()
            }
        });
    format!("[{}:{}]", source.agent, id)
}

fn shorten(value: &str) -> String {
    value.chars().take(8).collect()
}

fn compute_verdict(mode: &str, missing: &[(SourceSpec, String, String)], unique_contents: usize, success_count: usize) -> &'static str {
    if success_count == 0 {
        return "INCOMPLETE";
    }

    match mode {
        "verify" => {
            if missing.is_empty() && unique_contents <= 1 {
                "PASS"
            } else {
                "FAIL"
            }
        }
        "steer" => "STEERING_PLAN_READY",
        "analyze" => "ANALYSIS_COMPLETE",
        "feedback" => "FEEDBACK_COMPLETE",
        _ => "INCOMPLETE",
    }
}

fn validate_agent(agent: &str) -> Result<()> {
    match agent {
        "codex" | "gemini" | "claude" | "cursor" => Ok(()),
        _ => Err(anyhow!("Unsupported agent: {}", agent)),
    }
}

fn validate_mode(mode: &str) -> Result<()> {
    match mode {
        "verify" | "steer" | "analyze" | "feedback" => Ok(()),
        _ => Err(anyhow!("Unsupported mode: {}", mode)),
    }
}

const STOP_WORDS: &[&str] = &[
    "this", "that", "with", "from", "have", "been", "were", "will", "would", "could",
    "should", "their", "there", "they", "them", "then", "than", "these", "those", "some",
    "what", "when", "which", "where", "while", "about", "into", "also", "your", "more",
    "very", "just", "only", "each", "does", "done", "here", "such", "most",
    "both", "other", "after", "before", "over", "under", "between",
    "being", "make", "made", "like", "well", "back", "even", "still",
    "want", "give", "many", "much", "same", "know", "need", "take",
];

pub fn extract_topics(text: &str) -> HashSet<String> {
    let stop: HashSet<&str> = STOP_WORDS.iter().copied().collect();
    text.to_lowercase()
        .split(|c: char| !c.is_alphabetic())
        .filter(|w| w.len() >= 4)
        .filter(|w| !stop.contains(w))
        .map(|w| w.to_string())
        .collect()
}

pub fn jaccard_similarity(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let intersection = a.intersection(b).count();
    let union = a.union(b).count();
    intersection as f64 / union as f64
}
