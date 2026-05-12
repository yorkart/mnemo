use axum::{
    Json,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, Request, header},
    middleware::Next,
    response::Response,
};
use mnemo_domain::*;
use mnemo_ports::StoreError;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::auth::AuthConfig;
use crate::error::{HttpError, REQUEST_ID};
use crate::router::HttpState;

pub(crate) async fn health() -> Json<Value> {
    Json(json!({ "ok": true, "service": "mnemo", "version": env!("CARGO_PKG_VERSION") }))
}

pub(crate) async fn auth_middleware(
    State(auth): State<AuthConfig>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, HttpError> {
    let Some(expected) = auth.token.as_deref() else {
        return Ok(next.run(request).await);
    };
    let authorized = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|token| token == expected);
    if authorized {
        Ok(next.run(request).await)
    } else {
        Err(HttpError::unauthorized())
    }
}

pub(crate) async fn request_context_middleware(request: Request<Body>, next: Next) -> Response {
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("req-{}", uuid::Uuid::new_v4()));
    let traceparent = request.headers().get("traceparent").cloned();

    let mut response = REQUEST_ID
        .scope(request_id.clone(), next.run(request))
        .await;
    if let Ok(value) = HeaderValue::from_str(&request_id) {
        response.headers_mut().insert("x-request-id", value);
    }
    if let Some(value) = traceparent {
        response.headers_mut().insert("traceparent", value);
    }
    response
}

pub(crate) async fn ingest_event(
    State(state): State<HttpState>,
    Json(request): Json<EventInput>,
) -> Result<Json<Value>, HttpError> {
    require_user_namespace(&request.namespace)?;
    state
        .auth
        .check_namespace(&request.namespace, "events:write")?;
    let result = state.app.ingest_event(None, request).await?;
    Ok(ok(json!({
        "event_id": result.event_id,
        "deduplicated": result.deduplicated,
        "status": result.status
    })))
}

pub(crate) async fn ingest_events_batch(
    State(state): State<HttpState>,
    Json(request): Json<BatchEventsRequest>,
) -> Result<Json<Value>, HttpError> {
    require_user_namespace(&request.namespace)?;
    state
        .auth
        .check_namespace(&request.namespace, "events:write")?;
    let response = state.app.ingest_events_batch(None, request).await?;
    Ok(ok(json!(response)))
}

pub(crate) async fn create_memory(
    State(state): State<HttpState>,
    headers: HeaderMap,
    Json(mut request): Json<MemoryCreateRequest>,
) -> Result<Json<Value>, HttpError> {
    if request.idempotency_key.is_none() {
        request.idempotency_key = headers
            .get("Idempotency-Key")
            .and_then(|value| value.to_str().ok())
            .map(ToString::to_string);
    }
    require_user_namespace(&request.namespace)?;
    state
        .auth
        .check_namespace(&request.namespace, "memories:write")?;
    let memory = state.app.create_memory(None, request).await?;
    Ok(ok(json!({
        "memory_id": memory.memory_id,
        "origin": memory.origin,
        "memory_type": memory.memory_type,
        "memory_status": memory.status,
        "write_status": "accepted"
    })))
}

pub(crate) async fn query_memories(
    State(state): State<HttpState>,
    Json(request): Json<QueryRequest>,
) -> Result<Json<Value>, HttpError> {
    require_user_namespace(&request.namespace)?;
    state
        .auth
        .check_scope(&request.namespace, request.scope.as_ref(), "memories:read")?;
    Ok(ok(json!(state.app.query_memories(None, request).await?)))
}

pub(crate) async fn record_usage(
    State(state): State<HttpState>,
    headers: HeaderMap,
    Json(mut request): Json<UsageRequest>,
) -> Result<Json<Value>, HttpError> {
    if request.idempotency_key.is_none() {
        request.idempotency_key = idempotency_header(&headers);
    }
    require_user_namespace(&request.namespace)?;
    state
        .auth
        .check_namespace(&request.namespace, "usage:write")?;
    state.app.record_usage(None, request).await?;
    Ok(ok(json!({ "status": "accepted" })))
}

pub(crate) async fn context_pack(
    State(state): State<HttpState>,
    Json(request): Json<ContextPackRequest>,
) -> Result<Json<Value>, HttpError> {
    require_user_namespace(&request.namespace)?;
    let context_scope = QueryScope {
        include_thread: true,
        include_workspace: true,
        include_user: true,
        include_tenant: false,
    };
    state.auth.check_scope(
        &request.namespace,
        Some(&context_scope),
        "context_pack:read",
    )?;
    Ok(ok(json!(state.app.context_pack(None, request).await?)))
}

pub(crate) async fn wrapup(
    State(state): State<HttpState>,
    headers: HeaderMap,
    Json(mut request): Json<WrapupRequest>,
) -> Result<Json<Value>, HttpError> {
    if request.idempotency_key.is_none() {
        request.idempotency_key = idempotency_header(&headers);
    }
    require_user_namespace(&request.namespace)?;
    state
        .auth
        .check_namespace(&request.namespace, "jobs:write")?;
    let job = state.app.create_wrapup_job(None, request).await?;
    Ok(ok(json!({
        "job_id": job.job_id,
        "status": job.status,
        "result": job.result
    })))
}

#[derive(Debug, Deserialize)]
pub(crate) struct NamespaceQuery {
    tenant_id: Option<String>,
    user_id: Option<String>,
    workspace_id: Option<String>,
    thread_id: Option<String>,
    agent_id: Option<String>,
    source: Option<String>,
    pub(crate) limit: Option<usize>,
    pub(crate) cursor: Option<String>,
}

impl NamespaceQuery {
    pub(crate) fn namespace(&self) -> Namespace {
        Namespace {
            tenant_id: self.tenant_id.clone(),
            user_id: self.user_id.clone(),
            workspace_id: self.workspace_id.clone(),
            thread_id: self.thread_id.clone(),
            agent_id: self.agent_id.clone(),
            source: self.source.clone(),
        }
    }
}

pub(crate) async fn list_jobs(
    State(state): State<HttpState>,
    Query(query): Query<NamespaceQuery>,
) -> Result<Json<Value>, HttpError> {
    require_user_namespace(&query.namespace())?;
    state
        .auth
        .check_namespace(&query.namespace(), "jobs:read")?;
    let jobs = state
        .app
        .list_jobs(
            None,
            query.namespace(),
            query.limit.unwrap_or(50),
            query.cursor.clone(),
        )
        .await?;
    Ok(ok(json!({
        "jobs": jobs.0,
        "page": jobs.1
    })))
}

pub(crate) async fn get_job(
    State(state): State<HttpState>,
    Query(query): Query<NamespaceQuery>,
    Path(job_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    require_user_namespace(&query.namespace())?;
    state
        .auth
        .check_namespace(&query.namespace(), "jobs:read")?;
    Ok(ok(json!(
        state.app.get_job(None, query.namespace(), job_id).await?
    )))
}

pub(crate) async fn namespace_status(
    State(state): State<HttpState>,
    Json(request): Json<NamespaceStatusRequest>,
) -> Result<Json<Value>, HttpError> {
    require_user_namespace(&request.namespace)?;
    state
        .auth
        .check_namespace(&request.namespace, "memories:read")?;
    Ok(ok(
        json!({ "stats": state.app.namespace_status(None, request).await? }),
    ))
}

pub(crate) async fn get_policy(
    State(state): State<HttpState>,
    Query(query): Query<NamespaceQuery>,
) -> Result<Json<Value>, HttpError> {
    require_user_namespace(&query.namespace())?;
    state
        .auth
        .check_namespace(&query.namespace(), "policy:read")?;
    Ok(ok(
        json!({ "policy": state.app.get_policy(None, query.namespace()).await? }),
    ))
}

pub(crate) async fn put_policy(
    State(state): State<HttpState>,
    Json(request): Json<PolicyRequest>,
) -> Result<Json<Value>, HttpError> {
    require_user_namespace(&request.namespace)?;
    state
        .auth
        .check_namespace(&request.namespace, "policy:write")?;
    Ok(ok(
        json!({ "policy": state.app.put_policy(None, request).await? }),
    ))
}

pub(crate) async fn search_memories(
    State(state): State<HttpState>,
    Json(request): Json<SearchRequest>,
) -> Result<Json<Value>, HttpError> {
    require_user_namespace(&request.namespace)?;
    state
        .auth
        .check_namespace(&request.namespace, "memories:read")?;
    let (memories, page) = state.app.search_memories(None, request).await?;
    Ok(ok(json!({ "memories": memories, "page": page })))
}

pub(crate) async fn get_memory(
    State(state): State<HttpState>,
    Query(query): Query<NamespaceQuery>,
    Path(memory_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    require_user_namespace(&query.namespace())?;
    state
        .auth
        .check_namespace(&query.namespace(), "memories:read")?;
    Ok(ok(json!(
        state.app.get_memory(None, query.namespace(), memory_id).await?
    )))
}

pub(crate) async fn patch_memory(
    State(state): State<HttpState>,
    Query(query): Query<NamespaceQuery>,
    Path(memory_id): Path<String>,
    Json(patch): Json<MemoryPatch>,
) -> Result<Json<Value>, HttpError> {
    require_user_namespace(&query.namespace())?;
    state
        .auth
        .check_namespace(&query.namespace(), "memories:write")?;
    Ok(ok(json!(
        state
            .app
            .patch_memory(None, query.namespace(), memory_id, patch)
            .await?
    )))
}

pub(crate) async fn search_events(
    State(state): State<HttpState>,
    Json(request): Json<SearchRequest>,
) -> Result<Json<Value>, HttpError> {
    require_user_namespace(&request.namespace)?;
    state
        .auth
        .check_namespace(&request.namespace, "events:read")?;
    let (events, page) = state.app.search_events(None, request).await?;
    Ok(ok(json!({ "events": events, "page": page })))
}

pub(crate) async fn forget(
    State(state): State<HttpState>,
    headers: HeaderMap,
    Json(mut request): Json<ForgetRequest>,
) -> Result<Json<Value>, HttpError> {
    if request.idempotency_key.is_none() {
        request.idempotency_key = idempotency_header(&headers);
    }
    require_user_namespace(&request.namespace)?;
    state
        .auth
        .check_namespace(&request.namespace, "memories:delete")?;
    Ok(ok(json!(state.app.forget(None, request).await?)))
}

fn ok(value: Value) -> Json<Value> {
    let mut body = serde_json::Map::new();
    body.insert("ok".to_string(), Value::Bool(true));
    match value {
        Value::Object(map) => body.extend(map),
        other => {
            body.insert("data".to_string(), other);
        }
    }
    Json(Value::Object(body))
}

fn idempotency_header(headers: &HeaderMap) -> Option<String> {
    headers
        .get("Idempotency-Key")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn require_user_namespace(namespace: &Namespace) -> Result<(), HttpError> {
    let namespace = namespace.canonical();
    if namespace.user_id.is_none() {
        return Err(StoreError::InvalidRequest("namespace.user_id is required".to_string()).into());
    }
    Ok(())
}
