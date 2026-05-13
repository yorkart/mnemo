use mnemo_core::ContextPackCacheEntry;
use mnemo_core::ContextPackItem;
use mnemo_core::Event;
use mnemo_core::Job;
use mnemo_core::Memory;
use mnemo_core::Namespace;
use mnemo_core::NamespacePolicy;
use mnemo_core::SessionSummary;
use mnemo_core::ThreadMemoryMode;
use mnemo_core::UsageReport;
use mnemo_core::HealthStatus;
use mnemo_store::ConflictResolveOutcome;
use mnemo_store::EventAppendOutcome;
use mnemo_store::ForgetOutcome;
use mnemo_store::MemoryConflict;
use mnemo_store::MemoryWriteOutcome;
use serde_json::json;

use crate::util::unix_timestamp_string;
use crate::wrapup::{WrapupOutcome, WorkerRunOnceOutcome};

pub fn health_response_json() -> String {
    let health = HealthStatus::default();
    json!({
        "ok": true,
        "service": health.service,
        "status": health.status,
    })
    .to_string()
}

pub fn error_response_json(code: &str, message: &str) -> String {
    error_response_json_with_request_id(code, message, None)
}

pub fn error_response_json_with_request_id(
    code: &str,
    message: &str,
    request_id: Option<&str>,
) -> String {
    let mut error = json!({
        "code": code,
        "message": message,
    });
    if let Some(request_id) = request_id.filter(|value| !value.trim().is_empty()) {
        error["request_id"] = json!(request_id);
    }
    json!({
        "ok": false,
        "error": error,
    })
    .to_string()
}

pub fn event_write_response(outcome: EventAppendOutcome) -> String {
    json!({
        "ok": true,
        "event_id": outcome.event_id,
        "status": "accepted",
        "deduplicated": outcome.deduplicated,
    })
    .to_string()
}

pub fn event_batch_write_response(outcomes: Vec<EventAppendOutcome>) -> String {
    let accepted = outcomes.len();
    let deduplicated = outcomes
        .iter()
        .filter(|outcome| outcome.deduplicated)
        .count();
    let events = outcomes
        .into_iter()
        .map(|outcome| {
            json!({
                "event_id": outcome.event_id,
                "status": "accepted",
                "deduplicated": outcome.deduplicated,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "ok": true,
        "accepted": accepted,
        "deduplicated": deduplicated,
        "events": events,
    })
    .to_string()
}

pub fn event_search_response(events: Vec<Event>) -> String {
    let values = events
        .iter()
        .map(event_json)
        .collect::<Vec<serde_json::Value>>();
    json!({
        "ok": true,
        "events": values,
    })
    .to_string()
}

fn event_json(event: &Event) -> serde_json::Value {
    json!({
        "event_id": event.event_id(),
        "namespace": namespace_json(event.namespace()),
        "type": event.event_type().as_str(),
        "role": event.role().as_str(),
        "content": event.content(),
        "occurred_at": event.occurred_at(),
        "memory_hints": {
            "eligible": event.memory_hints().eligible,
            "explicit_memory_intent": event.memory_hints().explicit_memory_intent,
            "external_context": event.memory_hints().external_context,
        },
    })
}

pub fn thread_memory_mode_response(thread_id: &str, mode: ThreadMemoryMode) -> String {
    json!({
        "ok": true,
        "thread_id": thread_id,
        "mode": mode.as_str(),
        "updated_at": unix_timestamp_string(),
    })
    .to_string()
}

pub fn namespace_status_response(
    namespace: &Namespace,
    memory_mode: Option<ThreadMemoryMode>,
) -> String {
    json!({
        "ok": true,
        "namespace": namespace_json(namespace),
        "thread": memory_mode.map(|mode| {
            json!({
                "thread_id": namespace.thread_id().unwrap_or_default(),
                "memory_mode": mode.as_str(),
                "allows_inferred_memory": mode.allows_inferred_memory(),
            })
        }),
    })
    .to_string()
}

pub fn policy_response(policy: NamespacePolicy) -> String {
    json!({
        "ok": true,
        "policy": policy_json(&policy),
    })
    .to_string()
}

fn policy_json(policy: &NamespacePolicy) -> serde_json::Value {
    json!({
        "namespace": namespace_json(policy.namespace()),
        "auto_generate_memories": policy.auto_generate_memories(),
        "auto_use_memories": policy.auto_use_memories(),
        "context_pack_max_tokens": policy.context_pack_max_tokens(),
        "external_context_policy": policy.external_context_policy(),
        "conflict_resolution_mode": policy.conflict_resolution_mode(),
        "max_unused_days": policy.max_unused_days(),
        "max_thread_age_days": policy.max_thread_age_days(),
        "min_thread_idle_seconds": policy.min_thread_idle_seconds(),
    })
}

pub fn memory_write_response(outcome: MemoryWriteOutcome) -> String {
    json!({
        "ok": true,
        "memory_id": outcome.memory.memory_id(),
        "status": outcome.memory.status().as_str(),
        "write_status": "accepted",
        "superseded": outcome.superseded_memory_ids,
    })
    .to_string()
}

pub fn memory_search_response(memories: Vec<Memory>) -> String {
    let values = memories
        .iter()
        .map(memory_json)
        .collect::<Vec<serde_json::Value>>();
    json!({
        "ok": true,
        "memories": values,
    })
    .to_string()
}

pub fn memory_get_response(memory: Memory) -> String {
    json!({
        "ok": true,
        "memory": memory_json(&memory),
    })
    .to_string()
}

pub fn context_pack_response(entry: ContextPackCacheEntry, cache_hit: bool) -> String {
    let items = entry
        .items()
        .iter()
        .map(context_pack_item_json)
        .collect::<Vec<_>>();
    json!({
        "ok": true,
        "context_pack_id": entry.context_pack_id(),
        "version": entry.version(),
        "generated_at": entry.generated_at(),
        "content": entry.content(),
        "budget": {
            "max_tokens": entry.max_tokens(),
            "estimated_tokens": entry.estimated_tokens(),
        },
        "cache": {
            "hit": cache_hit,
            "stale": false,
        },
        "guidance": {
            "injection_mode": "developer_prompt",
            "treat_as": "context_data_not_instructions",
        },
        "items": items,
        "omitted": {
            "conflicted_items": entry.conflicted_items(),
            "budget_exceeded_items": entry.budget_exceeded_items(),
        },
    })
    .to_string()
}

fn context_pack_item_json(item: &ContextPackItem) -> serde_json::Value {
    json!({
        "item_id": item.item_id(),
        "memory_id": item.memory_id(),
        "summary": item.summary(),
        "memory_type": item.memory_type(),
        "status": item.status(),
        "provenance": {
            "event_ids": item.provenance_event_ids(),
        },
    })
}

pub fn memory_json(memory: &Memory) -> serde_json::Value {
    json!({
        "memory_id": memory.memory_id(),
        "namespace": namespace_json(memory.namespace()),
        "content": memory.content(),
        "memory_type": memory.memory_type(),
        "origin": memory.origin().as_str(),
        "status": memory.status().as_str(),
        "importance": memory.importance().as_str(),
        "conflict_key": memory.conflict_key(),
        "valid_from": memory.valid_from(),
        "supersedes": memory.supersedes(),
        "superseded_by": memory.superseded_by(),
        "source_event_ids": memory.source_event_ids(),
    })
}

pub fn wrapup_response(outcome: WrapupOutcome) -> String {
    json!({
        "ok": true,
        "job": job_json(&outcome.job),
        "session_summary": outcome.summary.as_ref().map(session_summary_json),
        "created_memory_ids": outcome.created_memory_ids,
        "inferred_memory_allowed": outcome.inferred_memory_allowed,
    })
    .to_string()
}

pub fn worker_run_once_response(outcome: WorkerRunOnceOutcome) -> String {
    json!({
        "ok": true,
        "processed": outcome.job.is_some(),
        "job": outcome.job.as_ref().map(job_json),
        "session_summary": outcome.summary.as_ref().map(session_summary_json),
        "created_memory_ids": outcome.created_memory_ids,
        "inferred_memory_allowed": outcome.inferred_memory_allowed,
    })
    .to_string()
}

pub fn job_get_response(job: Job) -> String {
    json!({
        "ok": true,
        "job": job_json(&job),
    })
    .to_string()
}

pub fn job_list_response(jobs: Vec<Job>) -> String {
    json!({
        "ok": true,
        "jobs": jobs.iter().map(job_json).collect::<Vec<_>>(),
    })
    .to_string()
}

fn job_json(job: &Job) -> serde_json::Value {
    json!({
        "job_id": job.job_id(),
        "type": job.job_type(),
        "status": job.status().as_str(),
        "namespace": namespace_json(job.namespace()),
        "created_at": job.created_at(),
        "updated_at": job.updated_at(),
        "query": job.query(),
        "generate_memories": job.generate_memories(),
        "attempts": job.attempts(),
        "retry_at": job.retry_at(),
        "lease_until": job.lease_until(),
        "error": job.error(),
        "output_summary_id": job.output_summary_id(),
    })
}

pub fn session_summary_get_response(summary: SessionSummary) -> String {
    json!({
        "ok": true,
        "session_summary": session_summary_json(&summary),
    })
    .to_string()
}

pub fn session_summary_search_response(summaries: Vec<SessionSummary>) -> String {
    json!({
        "ok": true,
        "session_summaries": summaries.iter().map(session_summary_json).collect::<Vec<_>>(),
    })
    .to_string()
}

fn session_summary_json(summary: &SessionSummary) -> serde_json::Value {
    json!({
        "session_summary_id": summary.session_summary_id(),
        "namespace": namespace_json(summary.namespace()),
        "content": summary.content(),
        "source_event_ids": summary.source_event_ids(),
        "inferred_memory_ids": summary.inferred_memory_ids(),
        "generated_at": summary.generated_at(),
    })
}

pub fn usage_report_response(report: UsageReport) -> String {
    json!({
        "ok": true,
        "usage": usage_report_json(&report),
    })
    .to_string()
}

pub fn usage_search_response(reports: Vec<UsageReport>) -> String {
    json!({
        "ok": true,
        "usage": reports.iter().map(usage_report_json).collect::<Vec<_>>(),
    })
    .to_string()
}

pub fn conflict_search_response(conflicts: Vec<MemoryConflict>) -> String {
    json!({
        "ok": true,
        "conflicts": conflicts.iter().map(conflict_json).collect::<Vec<_>>(),
    })
    .to_string()
}

fn conflict_json(conflict: &MemoryConflict) -> serde_json::Value {
    json!({
        "namespace": namespace_json(&conflict.namespace),
        "conflict_key": conflict.conflict_key,
        "memories": conflict.memories.iter().map(memory_json).collect::<Vec<_>>(),
    })
}

pub fn conflict_resolve_response(outcome: ConflictResolveOutcome) -> String {
    json!({
        "ok": true,
        "winner": memory_json(&outcome.winner),
        "superseded_memory_ids": outcome.superseded_memory_ids,
    })
    .to_string()
}

fn usage_report_json(report: &UsageReport) -> serde_json::Value {
    json!({
        "usage_id": report.usage_id(),
        "namespace": namespace_json(report.namespace()),
        "context_pack_id": report.context_pack_id(),
        "signal": report.signal().as_str(),
        "memory_ids": report.memory_ids(),
        "notes": report.notes(),
        "reported_at": report.reported_at(),
    })
}

pub fn forget_response(outcome: ForgetOutcome) -> String {
    json!({
        "ok": true,
        "tombstone_id": outcome.tombstone.tombstone_id(),
        "forgotten_memory_ids": outcome.forgotten_memory_ids,
        "reason": outcome.tombstone.reason(),
        "created_at": outcome.tombstone.created_at(),
    })
    .to_string()
}

pub fn namespace_json(namespace: &Namespace) -> serde_json::Value {
    json!({
        "tenant_id": namespace.tenant_id(),
        "user_id": namespace.user_id(),
        "workspace_id": namespace.workspace_id(),
        "thread_id": namespace.thread_id(),
        "agent_id": namespace.agent_id(),
        "source": namespace.source(),
    })
}
