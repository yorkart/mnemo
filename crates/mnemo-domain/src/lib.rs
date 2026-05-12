use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const DEFAULT_TENANT: &str = "default";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Namespace {
    #[serde(default)]
    pub tenant_id: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
}

impl Namespace {
    pub fn canonical(&self) -> Self {
        Self {
            tenant_id: Some(
                self.tenant_id
                    .clone()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| DEFAULT_TENANT.to_string()),
            ),
            user_id: clean_opt(&self.user_id),
            workspace_id: clean_opt(&self.workspace_id),
            thread_id: clean_opt(&self.thread_id),
            agent_id: clean_opt(&self.agent_id),
            source: clean_opt(&self.source),
        }
    }

    pub fn key(&self) -> String {
        let ns = self.canonical();
        format!(
            "tenant={}|user={}|workspace={}|thread={}|agent={}|source={}",
            ns.tenant_id.unwrap_or_else(|| DEFAULT_TENANT.to_string()),
            ns.user_id.unwrap_or_default(),
            ns.workspace_id.unwrap_or_default(),
            ns.thread_id.unwrap_or_default(),
            ns.agent_id.unwrap_or_default(),
            ns.source.unwrap_or_default()
        )
    }

    pub fn memory_scope_keys(&self, scope: Option<&QueryScope>) -> Vec<String> {
        let ns = self.canonical();
        let mut out = vec![ns.key()];
        if let Some(scope) = scope {
            if scope.include_thread && ns.thread_id.is_some() {
                out.push(ns.key());
            }
            if scope.include_workspace && ns.workspace_id.is_some() {
                let mut parent = ns.clone();
                parent.thread_id = None;
                out.push(parent.key());
            }
            if scope.include_user && ns.user_id.is_some() {
                let mut parent = ns.clone();
                parent.workspace_id = None;
                parent.thread_id = None;
                out.push(parent.key());
            }
            if scope.include_tenant {
                let mut parent = ns.clone();
                parent.user_id = None;
                parent.workspace_id = None;
                parent.thread_id = None;
                out.push(parent.key());
            }
        }
        out.sort();
        out.dedup();
        out
    }
}

fn clean_opt(value: &Option<String>) -> Option<String> {
    value.clone().filter(|s| !s.is_empty())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryHints {
    #[serde(default = "default_true")]
    pub eligible: bool,
    #[serde(default)]
    pub external_context: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventInput {
    pub event_id: String,
    #[serde(default)]
    pub namespace: Namespace,
    #[serde(rename = "type", default)]
    pub event_type: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    pub content: String,
    #[serde(default)]
    pub occurred_at: Option<String>,
    #[serde(default)]
    pub memory_hints: Option<MemoryHints>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchEventsRequest {
    pub namespace: Namespace,
    #[serde(default)]
    pub events: Vec<EventInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventWriteResult {
    pub event_id: String,
    pub status: String,
    pub deduplicated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiErrorBody>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchEventsResponse {
    pub summary: BatchSummary,
    pub results: Vec<EventWriteResult>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BatchSummary {
    pub total: usize,
    pub accepted: usize,
    pub deduplicated: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCreateRequest {
    pub namespace: Namespace,
    pub content: String,
    #[serde(default = "default_memory_type")]
    pub memory_type: String,
    #[serde(default = "default_importance")]
    pub importance: String,
    #[serde(default)]
    pub source_event_id: Option<String>,
    #[serde(default)]
    pub conflict_key: Option<String>,
    #[serde(default)]
    pub valid_from: Option<String>,
    #[serde(default)]
    pub valid_until: Option<String>,
    #[serde(default)]
    pub supersedes: Vec<String>,
    #[serde(default)]
    pub metadata: Value,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

fn default_memory_type() -> String {
    "fact".to_string()
}

fn default_importance() -> String {
    "normal".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub memory_id: String,
    pub namespace: Namespace,
    pub content: String,
    pub origin: String,
    pub memory_type: String,
    pub importance: String,
    pub status: String,
    #[serde(default)]
    pub source_event_id: Option<String>,
    #[serde(default)]
    pub conflict_key: Option<String>,
    #[serde(default)]
    pub valid_from: Option<String>,
    #[serde(default)]
    pub valid_until: Option<String>,
    #[serde(default)]
    pub supersedes: Vec<String>,
    #[serde(default)]
    pub superseded_by: Vec<String>,
    #[serde(default)]
    pub metadata: Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QueryScope {
    #[serde(default)]
    pub include_thread: bool,
    #[serde(default)]
    pub include_workspace: bool,
    #[serde(default)]
    pub include_user: bool,
    #[serde(default)]
    pub include_tenant: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QueryFilters {
    #[serde(default)]
    pub memory_types: Vec<String>,
    #[serde(default)]
    pub origins: Vec<String>,
    #[serde(default)]
    pub importance: Vec<String>,
    #[serde(default)]
    pub status: Vec<String>,
    #[serde(default)]
    pub entity_ids: Vec<String>,
    #[serde(default)]
    pub relation_types: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryRequest {
    pub namespace: Namespace,
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub as_of: Option<String>,
    #[serde(default = "default_temporal_scope")]
    pub temporal_scope: String,
    #[serde(default)]
    pub filters: QueryFilters,
    #[serde(default)]
    pub scope: Option<QueryScope>,
    #[serde(default = "default_query_limit")]
    pub limit: usize,
    #[serde(default = "default_response_format")]
    pub response_format: String,
}

fn default_temporal_scope() -> String {
    "current".to_string()
}

fn default_query_limit() -> usize {
    8
}

fn default_response_format() -> String {
    "results_only".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResponse {
    pub query_request_id: String,
    pub response_format: String,
    pub results: Vec<QueryResult>,
    pub backend: BackendMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub memory_id: String,
    pub content: String,
    pub memory_type: String,
    pub status: String,
    #[serde(default)]
    pub valid_from: Option<String>,
    #[serde(default)]
    pub valid_until: Option<String>,
    #[serde(default)]
    pub conflict_key: Option<String>,
    #[serde(default)]
    pub supersedes: Vec<String>,
    #[serde(default)]
    pub superseded_by: Vec<String>,
    pub score: f64,
    #[serde(default)]
    pub provenance: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendMetadata {
    pub mode: String,
    pub degraded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPackRequest {
    pub namespace: Namespace,
    #[serde(default = "default_context_purpose")]
    pub purpose: String,
    #[serde(default)]
    pub budget: TokenBudget,
}

fn default_context_purpose() -> String {
    "agent_bootstrap".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenBudget {
    #[serde(default = "default_context_tokens")]
    pub max_tokens: usize,
}

impl Default for TokenBudget {
    fn default() -> Self {
        Self {
            max_tokens: default_context_tokens(),
        }
    }
}

fn default_context_tokens() -> usize {
    1200
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPackResponse {
    pub context_pack_id: String,
    pub content: String,
    pub generated_at: String,
    pub version: String,
    pub cache: Value,
    pub guidance: Value,
    pub citation_policy: Value,
    pub items: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRequest {
    pub namespace: Namespace,
    #[serde(default)]
    pub idempotency_key: Option<String>,
    #[serde(default)]
    pub usage_batch_id: Option<String>,
    #[serde(default)]
    pub response_id: Option<String>,
    #[serde(default)]
    pub source_requests: Vec<Value>,
    #[serde(default)]
    pub used_items: Vec<Value>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WrapupRequest {
    pub namespace: Namespace,
    #[serde(default)]
    pub idempotency_key: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub event_range: Option<EventRange>,
    #[serde(default)]
    pub wait: bool,
    #[serde(default)]
    pub timeout_ms: u64,
    #[serde(default)]
    pub extraction_instructions: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRange {
    pub from_event_id: String,
    pub to_event_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRecord {
    pub job_id: String,
    pub namespace: Namespace,
    #[serde(rename = "type")]
    pub job_type: String,
    pub status: String,
    pub created_at: String,
    #[serde(default)]
    pub result: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceStatusRequest {
    pub namespace: Namespace,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceStats {
    pub events: usize,
    pub memories: usize,
    pub pending_jobs: usize,
    #[serde(default)]
    pub latest_event_cursor: Option<String>,
    #[serde(default)]
    pub organized_cursor: Option<String>,
    pub pending_events: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRequest {
    pub namespace: Namespace,
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub filters: QueryFilters,
    #[serde(default)]
    pub pagination: PageRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageRequest {
    #[serde(default = "default_page_limit")]
    pub limit: usize,
    #[serde(default)]
    pub cursor: Option<String>,
}

impl Default for PageRequest {
    fn default() -> Self {
        Self {
            limit: default_page_limit(),
            cursor: None,
        }
    }
}

fn default_page_limit() -> usize {
    50
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageInfo {
    pub limit: usize,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgetRequest {
    pub namespace: Namespace,
    #[serde(default)]
    pub idempotency_key: Option<String>,
    pub target: ForgetTarget,
    #[serde(default = "default_forget_mode")]
    pub mode: String,
    #[serde(default)]
    pub reason: Option<String>,
}

fn default_forget_mode() -> String {
    "soft_delete".to_string()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ForgetTarget {
    #[serde(default)]
    pub memory_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyRequest {
    pub namespace: Namespace,
    #[serde(default)]
    pub policy: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryPatch {
    #[serde(default)]
    pub namespace: Option<Namespace>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub importance: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiErrorBody {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

pub fn parse_time(value: &Option<String>) -> Option<DateTime<Utc>> {
    value
        .as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
}

pub fn default_policy() -> Value {
    json!({
        "auto_organize": true,
        "auto_generate_memories": true,
        "auto_use_memories": true,
        "disable_on_external_context": true,
        "context_pack_max_tokens": 1200,
        "raw_events_retention": "180d",
        "memory_retention": "365d",
        "keep_explicit_memories": true,
        "max_events_per_wrapup": 256,
        "min_event_idle_duration": "6h",
        "max_memories_per_wrapup": 128,
        "conflict_resolution_mode": "suggest",
        "policy_version": "default-v1",
        "compaction": {
            "aggressiveness": "balanced"
        }
    })
}
