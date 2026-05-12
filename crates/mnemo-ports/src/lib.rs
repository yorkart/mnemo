use async_trait::async_trait;
use mnemo_domain::*;
use serde_json::Value;
use thiserror::Error;

// ─── Error Types ───────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("not found")]
    NotFound,
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("idempotency conflict: {0}")]
    IdempotencyConflict(String),
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("invalid transaction")]
    InvalidTransaction,
}

impl StorageError {
    pub fn code(&self) -> &'static str {
        match self {
            StorageError::InvalidRequest(_) => "invalid_request",
            StorageError::NotFound => "not_found",
            StorageError::Conflict(_) => "event_conflict",
            StorageError::IdempotencyConflict(_) => "idempotency_conflict",
            StorageError::Unauthorized => "unauthorized",
            StorageError::Forbidden(_) => "forbidden",
            StorageError::Storage(_) => "internal_error",
            StorageError::InvalidTransaction => "internal_error",
        }
    }
}

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden: {0}")]
    Forbidden(String),
}

impl From<AuthError> for StorageError {
    fn from(err: AuthError) -> Self {
        match err {
            AuthError::Unauthorized => StorageError::Unauthorized,
            AuthError::Forbidden(msg) => StorageError::Forbidden(msg),
        }
    }
}

// ─── Auth Port ─────────────────────────────────────────────────────────────

/// Token information extracted from the request.
#[derive(Debug, Clone, Default)]
pub struct TokenInfo {
    pub token_id: Option<String>,
    pub scopes: Vec<TokenScope>,
}

#[derive(Debug, Clone)]
pub struct TokenScope {
    pub tenant_id: Option<String>,
    pub user_id: Option<String>,
    pub workspace_id: Option<String>,
    pub thread_id: Option<String>,
    pub permissions: Vec<String>,
}

#[async_trait]
pub trait AuthPort: Send + Sync {
    /// Validate a bearer token and return token info.
    async fn validate_token(&self, token: &str) -> Result<TokenInfo, AuthError>;

    /// Check if token has permission on the given namespace.
    fn check_namespace(
        &self,
        token: &TokenInfo,
        namespace: &Namespace,
        permission: &str,
    ) -> Result<(), AuthError>;

    /// Check if token has permission on expanded scope namespaces.
    fn check_scope(
        &self,
        token: &TokenInfo,
        namespace: &Namespace,
        scope: Option<&QueryScope>,
        permission: &str,
    ) -> Result<(), AuthError>;
}

// ─── Transaction / UnitOfWork ──────────────────────────────────────────────

/// Opaque transaction handle. All durable store write operations within the
/// same use case must share one TransactionContext.
///
/// For SQLite: this wraps the Mutex lock + SQL transaction.
/// For Postgres: this would wrap a database transaction.
pub trait TransactionContext: Send {
    fn as_any(&self) -> &dyn std::any::Any;
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

/// Bundle of durable stores sharing the same backing connection pool.
/// Application layer obtains a transaction from here and passes it to stores.
#[async_trait]
pub trait DurableBundle: Send + Sync {
    /// Begin a new transaction. Returns a boxed TransactionContext.
    async fn begin(&self) -> Result<Box<dyn TransactionContext>, StorageError>;

    /// Commit a transaction.
    async fn commit(&self, tx: Box<dyn TransactionContext>) -> Result<(), StorageError>;

    /// Rollback a transaction.
    async fn rollback(&self, tx: Box<dyn TransactionContext>) -> Result<(), StorageError>;
}

// ─── Event Store ───────────────────────────────────────────────────────────

#[async_trait]
pub trait EventStore: Send + Sync {
    async fn append_event(
        &self,
        tx: &mut dyn TransactionContext,
        namespace: &Namespace,
        event: &EventInput,
    ) -> Result<EventWriteResult, StorageError>;

    async fn search_events(
        &self,
        namespace: &Namespace,
        query: &SearchRequest,
    ) -> Result<(Vec<Value>, PageInfo), StorageError>;

    async fn namespace_event_status(
        &self,
        namespace: &Namespace,
    ) -> Result<NamespaceEventStatusInfo, StorageError>;

    async fn load_wrapup_events(
        &self,
        namespace: &Namespace,
        range: Option<&EventRange>,
    ) -> Result<Vec<WrapupEvent>, StorageError>;

    async fn event_position(
        &self,
        namespace: &Namespace,
        event_id: &str,
    ) -> Result<Option<i64>, StorageError>;
}

#[derive(Debug, Clone)]
pub struct NamespaceEventStatusInfo {
    pub event_count: usize,
    pub latest_event_id: i64,
    pub organized_event_id: i64,
}

#[derive(Debug, Clone)]
pub struct WrapupEvent {
    pub position: i64,
    pub event_id: String,
    pub role: Option<String>,
    pub content: String,
    pub occurred_at: Option<String>,
}

// ─── Memory Store ──────────────────────────────────────────────────────────

#[async_trait]
pub trait MemoryStore: Send + Sync {
    async fn create_memory(
        &self,
        tx: &mut dyn TransactionContext,
        memory: &MemoryRecord,
    ) -> Result<(), StorageError>;

    async fn get_memory(
        &self,
        namespace: &Namespace,
        memory_id: &str,
    ) -> Result<MemoryRecord, StorageError>;

    async fn update_memory(
        &self,
        tx: &mut dyn TransactionContext,
        memory: &MemoryRecord,
    ) -> Result<(), StorageError>;

    async fn search_memories(
        &self,
        namespace: &Namespace,
        query: &SearchRequest,
    ) -> Result<(Vec<MemoryRecord>, PageInfo), StorageError>;

    async fn query_memories(
        &self,
        namespace: &Namespace,
        scope_keys: &[String],
        request: &QueryRequest,
    ) -> Result<Vec<MemoryRecord>, StorageError>;

    async fn mark_forgotten(
        &self,
        tx: &mut dyn TransactionContext,
        namespace: &Namespace,
        memory_ids: &[String],
        mode: &str,
    ) -> Result<(), StorageError>;

    async fn memory_content_exists(
        &self,
        namespace: &Namespace,
        content: &str,
    ) -> Result<bool, StorageError>;

    async fn find_superseded_memories(
        &self,
        namespace: &Namespace,
        conflict_key: &str,
    ) -> Result<Vec<String>, StorageError>;

    async fn context_state_version(&self, namespace: &Namespace) -> Result<String, StorageError>;
}

// ─── Job Store ─────────────────────────────────────────────────────────────

#[async_trait]
pub trait JobStore: Send + Sync {
    async fn create_job(
        &self,
        tx: &mut dyn TransactionContext,
        job: &JobRecord,
    ) -> Result<(), StorageError>;

    async fn get_job(
        &self,
        namespace: &Namespace,
        job_id: &str,
    ) -> Result<JobRecord, StorageError>;

    async fn list_jobs(
        &self,
        namespace: &Namespace,
        limit: usize,
        cursor: Option<&str>,
    ) -> Result<(Vec<JobRecord>, PageInfo), StorageError>;

    async fn update_job_status(
        &self,
        tx: &mut dyn TransactionContext,
        job_id: &str,
        status: &str,
        result: &Value,
    ) -> Result<(), StorageError>;

    async fn claim_queued_jobs(
        &self,
        job_type: &str,
        limit: usize,
    ) -> Result<Vec<String>, StorageError>;
}

// ─── Policy Store ──────────────────────────────────────────────────────────

#[async_trait]
pub trait PolicyStore: Send + Sync {
    async fn get_policy(&self, namespace: &Namespace) -> Result<Value, StorageError>;
    async fn put_policy(
        &self,
        tx: &mut dyn TransactionContext,
        namespace: &Namespace,
        policy: &Value,
    ) -> Result<(), StorageError>;
}

// ─── Usage Store ───────────────────────────────────────────────────────────

#[async_trait]
pub trait UsageStore: Send + Sync {
    async fn record_usage(
        &self,
        tx: &mut dyn TransactionContext,
        namespace: &Namespace,
        request: &UsageRequest,
    ) -> Result<(), StorageError>;

    async fn aggregate_usage(
        &self,
        tx: &mut dyn TransactionContext,
        namespace: &Namespace,
        used_items: &[Value],
    ) -> Result<(), StorageError>;
}

// ─── Idempotency Store ─────────────────────────────────────────────────────

#[async_trait]
pub trait IdempotencyStore: Send + Sync {
    /// Check if the idempotency key exists. Returns (request_hash, response_json) if found.
    async fn check(
        &self,
        tx: &mut dyn TransactionContext,
        scope_key: &str,
    ) -> Result<Option<(String, String)>, StorageError>;

    /// Record an idempotency entry.
    async fn record(
        &self,
        tx: &mut dyn TransactionContext,
        scope_key: &str,
        request_hash: &str,
        response_json: &str,
    ) -> Result<(), StorageError>;

    /// Update the stored response for an existing key.
    async fn update_response(
        &self,
        scope_key: &str,
        response_json: &str,
    ) -> Result<(), StorageError>;
}

// ─── Outbox Store ──────────────────────────────────────────────────────────

#[async_trait]
pub trait OutboxStore: Send + Sync {
    async fn enqueue(
        &self,
        tx: &mut dyn TransactionContext,
        namespace_key: &str,
        task_type: &str,
        payload: &Value,
    ) -> Result<(), StorageError>;

    async fn process_pending(&self, limit: usize) -> Result<usize, StorageError>;
}

// ─── Audit Store ───────────────────────────────────────────────────────────

#[async_trait]
pub trait AuditStore: Send + Sync {
    async fn write_audit(
        &self,
        tx: &mut dyn TransactionContext,
        namespace_key: &str,
        action: &str,
        resource_type: &str,
        resource_id: Option<&str>,
        metadata: &Value,
    ) -> Result<(), StorageError>;
}

// ─── Tombstone Store ───────────────────────────────────────────────────────

#[async_trait]
pub trait TombstoneStore: Send + Sync {
    async fn create_tombstone(
        &self,
        tx: &mut dyn TransactionContext,
        namespace: &Namespace,
        target: &ForgetTarget,
        mode: &str,
        reason: Option<&str>,
    ) -> Result<(), StorageError>;
}

// ─── Context Pack Cache ────────────────────────────────────────────────────

#[async_trait]
pub trait ContextPackCache: Send + Sync {
    async fn get(
        &self,
        cache_key: &str,
    ) -> Result<Option<ContextPackResponse>, StorageError>;

    async fn put(
        &self,
        namespace: &Namespace,
        cache_key: &str,
        response: &ContextPackResponse,
    ) -> Result<(), StorageError>;
}

// ─── Cursor Store ──────────────────────────────────────────────────────────

#[async_trait]
pub trait CursorStore: Send + Sync {
    async fn get_organized_cursor(&self, namespace: &Namespace) -> Result<i64, StorageError>;
    async fn update_organized_cursor(
        &self,
        tx: &mut dyn TransactionContext,
        namespace: &Namespace,
        position: i64,
    ) -> Result<(), StorageError>;
}

// ─── Graph Store (Entity/Relation) ────────────────────────────────────────

#[async_trait]
pub trait GraphStore: Send + Sync {
    async fn index_memory_graph(
        &self,
        tx: &mut dyn TransactionContext,
        memory: &MemoryRecord,
    ) -> Result<(), StorageError>;

    async fn delete_memory_graph(
        &self,
        tx: &mut dyn TransactionContext,
        namespace: &Namespace,
        memory_id: &str,
    ) -> Result<(), StorageError>;

    async fn filter_by_entities(
        &self,
        namespace: &Namespace,
        memories: Vec<MemoryRecord>,
        entity_ids: &[String],
        relation_types: &[String],
    ) -> Result<Vec<MemoryRecord>, StorageError>;
}

// ─── Conflict Suggestion Store ─────────────────────────────────────────────

#[async_trait]
pub trait ConflictStore: Send + Sync {
    async fn create_conflict_suggestions(
        &self,
        tx: &mut dyn TransactionContext,
        memory: &MemoryRecord,
    ) -> Result<usize, StorageError>;

    async fn link_superseded_memories(
        &self,
        tx: &mut dyn TransactionContext,
        memory: &MemoryRecord,
    ) -> Result<(), StorageError>;
}

// ─── Memory Index Store ────────────────────────────────────────────────────

#[async_trait]
pub trait MemoryIndexStore: Send + Sync {
    async fn upsert_index(
        &self,
        tx: &mut dyn TransactionContext,
        namespace: &Namespace,
        memory_id: &str,
    ) -> Result<(), StorageError>;

    async fn delete_index(
        &self,
        namespace: &Namespace,
        memory_id: &str,
    ) -> Result<(), StorageError>;

    async fn rank_memories(
        &self,
        namespace: &Namespace,
        memories: Vec<MemoryRecord>,
        query: &str,
    ) -> Result<Vec<RankedMemory>, StorageError>;
}

#[derive(Debug, Clone)]
pub struct RankedMemory {
    pub memory: MemoryRecord,
    pub score: f64,
    pub lexical_score: f64,
    pub vector_score: f64,
    pub importance_score: f64,
    pub usage_score: f64,
}

// ─── Artifact Store ────────────────────────────────────────────────────────

#[async_trait]
pub trait ArtifactStore: Send + Sync {
    async fn refresh_namespace_artifacts(&self, namespace: &Namespace) -> Result<(), StorageError>;
}

// ─── Extraction Provider ───────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct MemoryCandidate {
    pub content: String,
    pub memory_type: String,
    pub importance: String,
    pub conflict_key: Option<String>,
    pub confidence: f64,
}

#[derive(Debug, Clone)]
pub struct ExtractedMemoryCandidate {
    pub event: WrapupEvent,
    pub candidate: MemoryCandidate,
}

#[derive(Debug, Clone)]
pub struct ExtractionResult {
    pub provider_name: String,
    pub candidates: Vec<ExtractedMemoryCandidate>,
    pub warning: Option<String>,
}

#[async_trait]
pub trait ExtractionProvider: Send + Sync {
    async fn extract(&self, events: &[WrapupEvent]) -> Result<ExtractionResult, StorageError>;
    fn label(&self) -> &str;
}

// ─── Maintenance ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct MaintenanceResult {
    pub expired_count: usize,
    pub entity_count: usize,
    pub relation_count: usize,
}

#[async_trait]
pub trait MaintenanceOps: Send + Sync {
    async fn run_maintenance(&self, namespace: &Namespace) -> Result<MaintenanceResult, StorageError>;
    async fn process_queued_jobs(&self, limit: usize) -> Result<usize, StorageError>;
}

// ─── Combined Store Trait ──────────────────────────────────────────────────

/// Combined supertrait providing access to all store capabilities.
/// Application layer holds `Arc<dyn Store>` for convenience.
/// Individual traits still exist for focused testing and future adapter splitting.
pub trait Store:
    DurableBundle
    + EventStore
    + MemoryStore
    + JobStore
    + PolicyStore
    + UsageStore
    + IdempotencyStore
    + OutboxStore
    + AuditStore
    + TombstoneStore
    + ContextPackCache
    + CursorStore
    + GraphStore
    + ConflictStore
    + MemoryIndexStore
    + ArtifactStore
    + MaintenanceOps
    + Send
    + Sync
{
}

impl<T> Store for T where
    T: DurableBundle
        + EventStore
        + MemoryStore
        + JobStore
        + PolicyStore
        + UsageStore
        + IdempotencyStore
        + OutboxStore
        + AuditStore
        + TombstoneStore
        + ContextPackCache
        + CursorStore
        + GraphStore
        + ConflictStore
        + MemoryIndexStore
        + ArtifactStore
        + MaintenanceOps
        + Send
        + Sync
{
}

// ─── Re-export for convenience ─────────────────────────────────────────────

// Legacy alias for backward compat during migration
pub type StoreError = StorageError;
