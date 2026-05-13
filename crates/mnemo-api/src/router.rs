use mnemo_core::JobStatus;
use mnemo_core::MnemoError;
use mnemo_core::MnemoResult;
use mnemo_core::ThreadMemoryMode;

use crate::http::{HttpRequest, HttpResponse};
use crate::requests::*;
use crate::responses::*;
use crate::server::ApiState;
use crate::util::unix_timestamp_string;
use crate::wrapup::{
    apply_policy_update, build_context_pack_cache_entry, context_pack_cache_key, run_wrapup,
    run_wrapup_for_namespace, run_worker_once,
};

pub fn handle_request(state: &ApiState, request: &HttpRequest) -> HttpResponse {
    match route(state, request) {
        Ok(response) => response,
        Err(error) => HttpResponse::from_error_for_request(error, request),
    }
}

fn route(state: &ApiState, request: &HttpRequest) -> MnemoResult<HttpResponse> {
    if request.method == "GET" {
        if let Some(memory_id) = path_suffix(&request.path, "/v1/memories/") {
            state.authorize(request)?;
            let memory = state
                .store()
                .find_memory(memory_id)?
                .ok_or_else(|| MnemoError::NotFound("memory not found".to_string()))?;
            return Ok(HttpResponse::ok(memory_get_response(memory)));
        }
        if let Some(job_id) = path_suffix(&request.path, "/v1/jobs/") {
            state.authorize(request)?;
            let job = state
                .store()
                .get_job(job_id)?
                .ok_or_else(|| MnemoError::NotFound("job not found".to_string()))?;
            return Ok(HttpResponse::ok(job_get_response(job)));
        }
        if let Some(summary_id) = path_suffix(&request.path, "/v1/session-summaries/") {
            state.authorize(request)?;
            let summary = state
                .store()
                .get_session_summary(summary_id)?
                .ok_or_else(|| MnemoError::NotFound("session summary not found".to_string()))?;
            return Ok(HttpResponse::ok(session_summary_get_response(summary)));
        }
    }
    if request.method == "PATCH" {
        if let Some(memory_id) = path_suffix(&request.path, "/v1/memories/") {
            state.authorize(request)?;
            let payload: MemoryPatchRequest = request.json_body()?;
            let namespace = payload.namespace.clone().into_namespace()?;
            let memory = state
                .store()
                .patch_memory(&namespace, memory_id, payload.into_patch()?)?;
            return Ok(HttpResponse::ok(memory_get_response(memory)));
        }
    }

    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/v1/health") | ("GET", "/health") => Ok(HttpResponse::ok(health_response_json())),
        ("GET", "/v1/jobs") => {
            state.authorize(request)?;
            Ok(HttpResponse::ok(job_list_response(
                state.store().list_jobs()?,
            )))
        }
        ("POST", "/v1/jobs/retry") => {
            state.authorize(request)?;
            let payload: JobRetryRequest = request.json_body()?;
            let job = state
                .store()
                .get_job(payload.job_id.as_str())?
                .ok_or_else(|| MnemoError::NotFound("job not found".to_string()))?;
            if job.status() != JobStatus::Failed {
                return Err(MnemoError::InvalidRequest(
                    "only failed jobs can be retried".to_string(),
                ));
            }
            let outcome = run_wrapup_for_namespace(
                state.store(),
                state.wrapup_provider(),
                job.namespace().clone(),
                payload.q.as_deref(),
                payload.generate_memories,
                None,
            )?;
            Ok(HttpResponse::ok(wrapup_response(outcome)))
        }
        ("POST", "/v1/worker/run-once") => {
            state.authorize(request)?;
            let payload = request
                .json_body::<WorkerRunOnceRequest>()
                .unwrap_or_default();
            let outcome = run_worker_once(state.store(), state.wrapup_provider(), payload.lease_seconds)?;
            Ok(HttpResponse::ok(worker_run_once_response(outcome)))
        }
        ("GET", "/v1/namespaces/policy") => {
            state.authorize(request)?;
            let payload: NamespacePolicyGetRequest = request.json_body()?;
            let policy = state
                .store()
                .get_policy(&payload.namespace.into_namespace()?)?;
            Ok(HttpResponse::ok(policy_response(policy)))
        }
        ("PUT", "/v1/namespaces/policy") => {
            state.authorize(request)?;
            let payload: NamespacePolicySetRequest = request.json_body()?;
            let namespace = payload.namespace.clone().into_namespace()?;
            let policy = apply_policy_update(state.store().get_policy(&namespace)?, payload)?;
            let policy = state.store().set_policy(policy)?;
            Ok(HttpResponse::ok(policy_response(policy)))
        }
        ("POST", "/v1/events") => {
            state.authorize(request)?;
            let payload: EventWriteRequest = request.json_body()?;
            let outcome = state
                .store()
                .append_event(payload.into_event(request.header("idempotency-key"))?)?;
            Ok(HttpResponse::ok(event_write_response(outcome)))
        }
        ("POST", "/v1/events/batch") => {
            state.authorize(request)?;
            let payload: EventBatchWriteRequest = request.json_body()?;
            let mut outcomes = Vec::with_capacity(payload.events.len());
            for event in payload.events {
                outcomes.push(state.store().append_event(event.into_event(None)?)?);
            }
            Ok(HttpResponse::ok(event_batch_write_response(outcomes)))
        }
        ("POST", "/v1/events/search") => {
            state.authorize(request)?;
            let payload: EventSearchRequest = request.json_body()?;
            let events = state
                .store()
                .search_events(&payload.namespace.into_namespace()?, payload.q.as_deref())?;
            Ok(HttpResponse::ok(event_search_response(events)))
        }
        ("POST", "/v1/threads/memory-mode") => {
            state.authorize(request)?;
            let payload: ThreadMemoryModeSetRequest = request.json_body()?;
            let mode = ThreadMemoryMode::parse(payload.mode.as_str())?;
            let namespace = payload.namespace.into_namespace()?;
            let updated = state.store().set_memory_mode(&namespace, mode)?;
            Ok(HttpResponse::ok(thread_memory_mode_response(
                namespace.thread_id().unwrap_or_default(),
                updated,
            )))
        }
        ("POST", "/v1/namespaces/status") => {
            state.authorize(request)?;
            let payload: NamespaceStatusRequest = request.json_body()?;
            let namespace = payload.namespace.into_namespace()?;
            let memory_mode = namespace
                .thread_id()
                .map(|_| state.store().get_memory_mode(&namespace))
                .transpose()?;
            Ok(HttpResponse::ok(namespace_status_response(
                &namespace,
                memory_mode,
            )))
        }
        ("POST", "/v1/memories") => {
            state.authorize(request)?;
            let payload: MemoryWriteRequest = request.json_body()?;
            let memory_id = state.store().next_memory_id()?;
            let memory = payload.into_memory(memory_id)?;
            let outcome = state.store().write_memory(memory)?;
            Ok(HttpResponse::ok(memory_write_response(outcome)))
        }
        ("POST", "/v1/memories/search") => {
            state.authorize(request)?;
            let payload: MemorySearchRequest = request.json_body()?;
            let memories = state.store().search_memories(
                &payload.namespace.into_namespace()?,
                payload.q.as_deref(),
                payload.include_inactive.unwrap_or(false),
            )?;
            Ok(HttpResponse::ok(memory_search_response(memories)))
        }
        ("POST", "/v1/context-pack") => {
            state.authorize(request)?;
            let payload: ContextPackRequest = request.json_body()?;
            let namespace = payload.namespace.into_namespace()?;
            let policy = state.store().get_policy(&namespace)?;
            let active_memories = if policy.auto_use_memories() {
                state.store().active_memories(&namespace)?
            } else {
                Vec::new()
            };
            let max_tokens = payload
                .budget
                .as_ref()
                .and_then(|budget| budget.max_tokens)
                .unwrap_or(policy.context_pack_max_tokens());
            let cache_key = context_pack_cache_key(&namespace, max_tokens, &active_memories);
            if let Some(entry) = state.store().get_context_pack_cache(&cache_key)? {
                return Ok(HttpResponse::ok(context_pack_response(entry, true)));
            }
            let entry = build_context_pack_cache_entry(cache_key, active_memories, max_tokens)?;
            let entry = state.store().put_context_pack_cache(entry)?;
            Ok(HttpResponse::ok(context_pack_response(entry, false)))
        }
        ("POST", "/v1/sessions/wrapup") => {
            state.authorize(request)?;
            let payload: WrapupRequest = request.json_body()?;
            let outcome = run_wrapup(state.store(), state.wrapup_provider(), payload)?;
            Ok(HttpResponse::ok(wrapup_response(outcome)))
        }
        ("POST", "/v1/session-summaries/search") => {
            state.authorize(request)?;
            let payload: SessionSummarySearchRequest = request.json_body()?;
            let summaries = state.store().search_session_summaries(
                &payload.namespace.into_namespace()?,
                payload.q.as_deref(),
            )?;
            Ok(HttpResponse::ok(session_summary_search_response(summaries)))
        }
        ("POST", "/v1/usage") => {
            state.authorize(request)?;
            let payload: UsageReportRequest = request.json_body()?;
            let usage_id = state.store().next_usage_id()?;
            let report = payload.into_usage_report(usage_id, unix_timestamp_string())?;
            let report = state.store().write_usage_report(report)?;
            Ok(HttpResponse::ok(usage_report_response(report)))
        }
        ("POST", "/v1/usage/search") => {
            state.authorize(request)?;
            let payload: UsageSearchRequest = request.json_body()?;
            let namespace = payload
                .namespace
                .map(NamespaceRequest::into_namespace)
                .transpose()?;
            let reports = state.store().search_usage_reports(namespace.as_ref())?;
            Ok(HttpResponse::ok(usage_search_response(reports)))
        }
        ("POST", "/v1/conflicts/search") => {
            state.authorize(request)?;
            let payload: ConflictSearchRequest = request.json_body()?;
            let namespace = payload.namespace.into_namespace()?;
            let conflicts = state.store().search_conflicts(&namespace)?;
            Ok(HttpResponse::ok(conflict_search_response(conflicts)))
        }
        ("POST", "/v1/conflicts/resolve") => {
            state.authorize(request)?;
            let payload: ConflictResolveRequest = request.json_body()?;
            let namespace = payload.namespace.into_namespace()?;
            let outcome = state.store().resolve_conflict(
                &namespace,
                payload.conflict_key.as_str(),
                payload.winner_memory_id.as_str(),
            )?;
            Ok(HttpResponse::ok(conflict_resolve_response(outcome)))
        }
        ("POST", "/v1/forget") => {
            state.authorize(request)?;
            let payload: ForgetRequest = request.json_body()?;
            let namespace = payload.namespace.into_namespace()?;
            let outcome = state.store().forget_memories(
                &namespace,
                &payload.memory_ids,
                payload.reason,
                unix_timestamp_string(),
            )?;
            Ok(HttpResponse::ok(forget_response(outcome)))
        }
        _ => Err(MnemoError::NotFound("route not found".to_string())),
    }
}

fn path_suffix<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    path.strip_prefix(prefix)
        .map(str::trim)
        .filter(|suffix| !suffix.is_empty() && !suffix.contains('/'))
}
