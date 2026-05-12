use std::io::Write;
use std::process::{Command, Stdio};

use mnemo_ports::StorageError;
use serde_json::{Value, json};

use crate::helpers::json_err;
use crate::store::ExtractionProviderConfig;

#[derive(Clone)]
pub(crate) struct WrapupEvent {
    pub position: i64,
    pub event_id: String,
    pub role: Option<String>,
    pub content: String,
    pub occurred_at: Option<String>,
}

#[derive(Clone)]
pub(crate) struct MemoryCandidate {
    pub content: String,
    pub memory_type: String,
    pub importance: String,
    pub conflict_key: Option<String>,
    pub confidence: f64,
}

pub(crate) struct ExtractedMemoryCandidate {
    pub event: WrapupEvent,
    pub candidate: MemoryCandidate,
}

pub(crate) struct ExtractionRun {
    pub provider_name: String,
    pub candidates: Vec<ExtractedMemoryCandidate>,
    pub warning: Option<String>,
}

pub(crate) fn run_extraction(
    provider: &ExtractionProviderConfig,
    events: &[WrapupEvent],
    extraction_instructions: Option<&str>,
) -> Result<ExtractionRun, StorageError> {
    match provider {
        ExtractionProviderConfig::LocalRules => {
            let mut result = run_local_extraction(events);
            if extraction_instructions.is_some() {
                result.warning = Some(
                    "extraction_instructions is ignored by the local_rules provider; \
                     switch to codex_cli to use custom instructions"
                        .to_string(),
                );
            }
            Ok(result)
        }
        ExtractionProviderConfig::CodexCli { command } => {
            let codex = CodexExtractionProvider {
                command: command.clone(),
            };
            let candidates = codex.extract(events, extraction_instructions)?;
            Ok(ExtractionRun {
                provider_name: "codex_cli_v1".to_string(),
                candidates,
                warning: None,
            })
        }
    }
}

fn run_local_extraction(events: &[WrapupEvent]) -> ExtractionRun {
    let provider = LocalRuleExtractionProvider;
    let candidates = events
        .iter()
        .filter_map(|event| {
            provider
                .extract(event)
                .map(|candidate| ExtractedMemoryCandidate {
                    event: event.clone(),
                    candidate,
                })
        })
        .collect();
    ExtractionRun {
        provider_name: "local_rules_v1".to_string(),
        candidates,
        warning: None,
    }
}

struct LocalRuleExtractionProvider;

impl LocalRuleExtractionProvider {
    fn extract(&self, event: &WrapupEvent) -> Option<MemoryCandidate> {
        if event.content.trim().is_empty() {
            return None;
        }
        let lower = event.content.to_lowercase();
        let memory_signal = lower.contains("remember")
            || event.content.contains("记住")
            || lower.contains("prefer")
            || event.content.contains("偏好")
            || event.content.contains("喜欢")
            || lower.contains("always")
            || lower.contains("project")
            || event.content.contains("项目")
            || event.content.contains("决定");
        if !memory_signal {
            return None;
        }
        let memory_type = if lower.contains("prefer")
            || event.content.contains("偏好")
            || event.content.contains("喜欢")
        {
            "preference"
        } else if lower.contains("always")
            || lower.contains("must")
            || event.content.contains("请")
            || event.content.contains("必须")
        {
            "instruction"
        } else if lower.contains("project") || event.content.contains("项目") {
            "project_context"
        } else {
            "fact"
        };
        let conflict_key = infer_conflict_key(&event.content, memory_type);
        let importance = if lower.contains("critical") || event.content.contains("重要") {
            "high"
        } else {
            "normal"
        };
        Some(MemoryCandidate {
            content: normalize_memory_content(&event.content),
            memory_type: memory_type.to_string(),
            importance: importance.to_string(),
            conflict_key,
            confidence: 0.72,
        })
    }
}

pub(crate) struct CodexExtractionProvider {
    pub command: String,
}

impl CodexExtractionProvider {
    pub fn extract(&self, events: &[WrapupEvent], extraction_instructions: Option<&str>) -> Result<Vec<ExtractedMemoryCandidate>, StorageError> {
        if events.is_empty() {
            return Ok(Vec::new());
        }
        let prompt = render_codex_extraction_prompt(events, extraction_instructions)?;
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(&self.command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| StorageError::Storage(format!("spawn codex command: {error}")))?;
        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(prompt.as_bytes())
                .map_err(|error| StorageError::Storage(format!("write codex prompt: {error}")))?;
        }
        let output = child
            .wait_with_output()
            .map_err(|error| StorageError::Storage(format!("wait codex command: {error}")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(StorageError::Storage(format!(
                "codex command exited with {}: {}",
                output.status,
                stderr.trim()
            )));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_codex_extraction_output(&stdout, events)
    }
}

fn render_codex_extraction_prompt(events: &[WrapupEvent], extraction_instructions: Option<&str>) -> Result<String, StorageError> {
    const TEMPLATE: &str = include_str!("prompts/extraction.md");
    let events_json = serde_json::to_string_pretty(
        &events
            .iter()
            .map(|event| {
                json!({
                    "event_id": event.event_id,
                    "role": event.role,
                    "content": event.content,
                    "occurred_at": event.occurred_at
                })
            })
            .collect::<Vec<_>>(),
    )
    .map_err(json_err)?;
    let custom_block = match extraction_instructions {
        Some(instructions) if !instructions.trim().is_empty() => {
            format!("\n<user_instructions>\n{}\n</user_instructions>", instructions.trim())
        }
        _ => String::new(),
    };
    Ok(TEMPLATE
        .replace("{events_json}", &events_json)
        .replace("{custom_instructions}", &custom_block))
}

pub(crate) fn parse_codex_extraction_output(
    output: &str,
    events: &[WrapupEvent],
) -> Result<Vec<ExtractedMemoryCandidate>, StorageError> {
    let json_text = extract_json_object(output).ok_or_else(|| {
        StorageError::Storage("codex output did not contain a JSON object".to_string())
    })?;
    let value = serde_json::from_str::<Value>(json_text).map_err(json_err)?;
    let memories = value
        .get("memories")
        .and_then(Value::as_array)
        .ok_or_else(|| StorageError::Storage("codex output missing memories array".to_string()))?;
    let mut out = Vec::new();
    for item in memories {
        let Some(source_event_id) = item
            .get("source_event_id")
            .or_else(|| item.get("event_id"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let Some(event) = events
            .iter()
            .find(|event| event.event_id == source_event_id)
        else {
            continue;
        };
        let Some(content) = item.get("content").and_then(Value::as_str) else {
            continue;
        };
        let content = content.trim();
        if content.is_empty() {
            continue;
        }
        let memory_type = item
            .get("memory_type")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("fact");
        let importance = item
            .get("importance")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("normal");
        let conflict_key = item
            .get("conflict_key")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(ToString::to_string);
        let confidence = item
            .get("confidence")
            .and_then(Value::as_f64)
            .unwrap_or(0.82)
            .clamp(0.0, 1.0);
        out.push(ExtractedMemoryCandidate {
            event: event.clone(),
            candidate: MemoryCandidate {
                content: normalize_memory_content(content),
                memory_type: memory_type.to_string(),
                importance: importance.to_string(),
                conflict_key,
                confidence,
            },
        });
    }
    Ok(out)
}

fn extract_json_object(output: &str) -> Option<&str> {
    let trimmed = output.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return Some(trimmed);
    }
    let start = output.find('{')?;
    let end = output.rfind('}')?;
    if end <= start {
        return None;
    }
    output.get(start..=end)
}

pub(crate) fn normalize_memory_content(content: &str) -> String {
    let normalized = content.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= 280 {
        return normalized;
    }
    normalized.chars().take(277).collect::<String>() + "..."
}

fn infer_conflict_key(content: &str, memory_type: &str) -> Option<String> {
    let lower = content.to_lowercase();
    if content.contains("中文")
        || lower.contains("chinese")
        || content.contains("英文")
        || lower.contains("english")
    {
        return Some("user.language.preference".to_string());
    }
    if memory_type == "preference" {
        return Some("user.preference.general".to_string());
    }
    if memory_type == "project_context" {
        return Some("project.context.current".to_string());
    }
    None
}

pub(crate) fn continuation_summary(events: &[WrapupEvent]) -> String {
    if events.is_empty() {
        return "No new eligible events to organize.".to_string();
    }
    events
        .iter()
        .rev()
        .take(3)
        .rev()
        .map(|event| {
            format!(
                "{}: {}",
                event.role.as_deref().unwrap_or("event"),
                normalize_memory_content(&event.content)
            )
        })
        .collect::<Vec<_>>()
        .join(" | ")
}
