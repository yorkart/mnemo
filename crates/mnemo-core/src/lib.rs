#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt;

use serde::Deserialize;
use serde::Serialize;

pub type MnemoResult<T> = Result<T, MnemoError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MnemoError {
    InvalidRequest(String),
    Unauthorized,
    Forbidden,
    NotFound(String),
    IdempotencyConflict(String),
    EventConflict(String),
    Internal(String),
}

impl MnemoError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidRequest(_) => "invalid_request",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::NotFound(_) => "not_found",
            Self::IdempotencyConflict(_) => "idempotency_conflict",
            Self::EventConflict(_) => "event_conflict",
            Self::Internal(_) => "internal_error",
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::InvalidRequest(message)
            | Self::NotFound(message)
            | Self::IdempotencyConflict(message)
            | Self::EventConflict(message)
            | Self::Internal(message) => message.as_str(),
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
        }
    }
}

impl fmt::Display for MnemoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code(), self.message())
    }
}

impl Error for MnemoError {}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Namespace {
    tenant_id: String,
    user_id: String,
    workspace_id: Option<String>,
    thread_id: Option<String>,
    agent_id: Option<String>,
    source: Option<String>,
}

impl Namespace {
    pub fn new(user_id: impl Into<String>) -> MnemoResult<Self> {
        let user_id = normalize_required("user_id", user_id.into())?;
        Ok(Self {
            tenant_id: "default".to_string(),
            user_id,
            workspace_id: None,
            thread_id: None,
            agent_id: None,
            source: None,
        })
    }

    pub fn with_tenant(mut self, tenant_id: impl Into<String>) -> Self {
        let tenant_id = tenant_id.into();
        self.tenant_id = normalize_optional(tenant_id).unwrap_or_else(|| "default".to_string());
        self
    }

    pub fn with_workspace(mut self, workspace_id: impl Into<String>) -> Self {
        self.workspace_id = normalize_optional(workspace_id.into());
        self
    }

    pub fn with_thread(mut self, thread_id: impl Into<String>) -> Self {
        self.thread_id = normalize_optional(thread_id.into());
        self
    }

    pub fn with_agent(mut self, agent_id: impl Into<String>) -> Self {
        self.agent_id = normalize_optional(agent_id.into());
        self
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = normalize_optional(source.into());
        self
    }

    pub fn tenant_id(&self) -> &str {
        &self.tenant_id
    }

    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    pub fn workspace_id(&self) -> Option<&str> {
        self.workspace_id.as_deref()
    }

    pub fn thread_id(&self) -> Option<&str> {
        self.thread_id.as_deref()
    }

    pub fn agent_id(&self) -> Option<&str> {
        self.agent_id.as_deref()
    }

    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    pub fn stable_key(&self) -> String {
        format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
            self.tenant_id,
            self.user_id,
            self.workspace_id.as_deref().unwrap_or(""),
            self.thread_id.as_deref().unwrap_or(""),
            self.agent_id.as_deref().unwrap_or(""),
            self.source.as_deref().unwrap_or("")
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventType {
    UserMessage,
    AgentMessage,
    AssistantFinal,
    ToolSummary,
    ToolResultSummary,
    TaskSummary,
    ContextInjected,
    SystemEvent,
    ExplicitRemember,
    ExplicitForget,
    SessionSummary,
}

impl EventType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserMessage => "user_message",
            Self::AgentMessage => "agent_message",
            Self::AssistantFinal => "assistant_final",
            Self::ToolSummary => "tool_summary",
            Self::ToolResultSummary => "tool_result_summary",
            Self::TaskSummary => "task_summary",
            Self::ContextInjected => "context_injected",
            Self::SystemEvent => "system_event",
            Self::ExplicitRemember => "explicit_remember",
            Self::ExplicitForget => "explicit_forget",
            Self::SessionSummary => "session_summary",
        }
    }

    pub fn parse(value: &str) -> MnemoResult<Self> {
        match value {
            "user_message" => Ok(Self::UserMessage),
            "agent_message" => Ok(Self::AgentMessage),
            "assistant_final" => Ok(Self::AssistantFinal),
            "tool_summary" => Ok(Self::ToolSummary),
            "tool_result_summary" => Ok(Self::ToolResultSummary),
            "task_summary" => Ok(Self::TaskSummary),
            "context_injected" => Ok(Self::ContextInjected),
            "system_event" => Ok(Self::SystemEvent),
            "explicit_remember" => Ok(Self::ExplicitRemember),
            "explicit_forget" => Ok(Self::ExplicitForget),
            "session_summary" => Ok(Self::SessionSummary),
            other => Err(MnemoError::InvalidRequest(format!(
                "unsupported event type: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventRole {
    User,
    Agent,
    Tool,
    System,
}

impl EventRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Agent => "agent",
            Self::Tool => "tool",
            Self::System => "system",
        }
    }

    pub fn parse(value: &str) -> MnemoResult<Self> {
        match value {
            "user" => Ok(Self::User),
            "agent" | "assistant" => Ok(Self::Agent),
            "tool" => Ok(Self::Tool),
            "system" => Ok(Self::System),
            other => Err(MnemoError::InvalidRequest(format!(
                "unsupported event role: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MemoryHints {
    pub eligible: bool,
    pub explicit_memory_intent: bool,
    pub external_context: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    event_id: String,
    namespace: Namespace,
    event_type: EventType,
    role: EventRole,
    content: String,
    occurred_at: String,
    memory_hints: MemoryHints,
}

impl Event {
    pub fn new(
        event_id: impl Into<String>,
        namespace: Namespace,
        event_type: EventType,
        role: EventRole,
        content: impl Into<String>,
        occurred_at: impl Into<String>,
    ) -> MnemoResult<Self> {
        Ok(Self {
            event_id: normalize_required("event_id", event_id.into())?,
            namespace,
            event_type,
            role,
            content: normalize_required("content", content.into())?,
            occurred_at: normalize_required("occurred_at", occurred_at.into())?,
            memory_hints: MemoryHints::default(),
        })
    }

    pub fn with_memory_hints(mut self, memory_hints: MemoryHints) -> Self {
        self.memory_hints = memory_hints;
        self
    }

    pub fn event_id(&self) -> &str {
        &self.event_id
    }

    pub fn namespace(&self) -> &Namespace {
        &self.namespace
    }

    pub fn event_type(&self) -> EventType {
        self.event_type
    }

    pub fn role(&self) -> EventRole {
        self.role
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn occurred_at(&self) -> &str {
        &self.occurred_at
    }

    pub fn memory_hints(&self) -> &MemoryHints {
        &self.memory_hints
    }

    pub fn conflict_fingerprint(&self) -> String {
        format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
            self.namespace.stable_key(),
            self.event_id,
            self.event_type.as_str(),
            self.role.as_str(),
            self.content,
            self.occurred_at
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThreadMemoryMode {
    Enabled,
    Disabled,
    Polluted,
}

impl ThreadMemoryMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
            Self::Polluted => "polluted",
        }
    }

    pub fn allows_inferred_memory(self) -> bool {
        matches!(self, Self::Enabled)
    }

    pub fn parse(value: &str) -> MnemoResult<Self> {
        match value {
            "enabled" => Ok(Self::Enabled),
            "disabled" => Ok(Self::Disabled),
            "polluted" => Ok(Self::Polluted),
            other => Err(MnemoError::InvalidRequest(format!(
                "unsupported thread memory mode: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamespacePolicy {
    namespace: Namespace,
    auto_generate_memories: bool,
    auto_use_memories: bool,
    context_pack_max_tokens: usize,
    external_context_policy: String,
    conflict_resolution_mode: String,
    max_unused_days: Option<u64>,
    max_thread_age_days: Option<u64>,
    min_thread_idle_seconds: Option<u64>,
}

impl NamespacePolicy {
    pub fn default_for(namespace: Namespace) -> Self {
        Self {
            namespace,
            auto_generate_memories: true,
            auto_use_memories: true,
            context_pack_max_tokens: 1200,
            external_context_policy: "ignore_unless_explicit".to_string(),
            conflict_resolution_mode: "supersede_by_conflict_key".to_string(),
            max_unused_days: Some(180),
            max_thread_age_days: Some(90),
            min_thread_idle_seconds: Some(300),
        }
    }

    pub fn namespace(&self) -> &Namespace {
        &self.namespace
    }

    pub fn auto_generate_memories(&self) -> bool {
        self.auto_generate_memories
    }

    pub fn auto_use_memories(&self) -> bool {
        self.auto_use_memories
    }

    pub fn context_pack_max_tokens(&self) -> usize {
        self.context_pack_max_tokens
    }

    pub fn external_context_policy(&self) -> &str {
        &self.external_context_policy
    }

    pub fn conflict_resolution_mode(&self) -> &str {
        &self.conflict_resolution_mode
    }

    pub fn max_unused_days(&self) -> Option<u64> {
        self.max_unused_days
    }

    pub fn max_thread_age_days(&self) -> Option<u64> {
        self.max_thread_age_days
    }

    pub fn min_thread_idle_seconds(&self) -> Option<u64> {
        self.min_thread_idle_seconds
    }

    pub fn set_auto_generate_memories(&mut self, value: bool) {
        self.auto_generate_memories = value;
    }

    pub fn set_auto_use_memories(&mut self, value: bool) {
        self.auto_use_memories = value;
    }

    pub fn set_context_pack_max_tokens(&mut self, value: usize) -> MnemoResult<()> {
        if value == 0 {
            return Err(MnemoError::InvalidRequest(
                "context_pack_max_tokens must be greater than zero".to_string(),
            ));
        }
        self.context_pack_max_tokens = value;
        Ok(())
    }

    pub fn set_external_context_policy(&mut self, value: impl Into<String>) -> MnemoResult<()> {
        self.external_context_policy = normalize_required("external_context_policy", value.into())?;
        Ok(())
    }

    pub fn set_conflict_resolution_mode(&mut self, value: impl Into<String>) -> MnemoResult<()> {
        self.conflict_resolution_mode =
            normalize_required("conflict_resolution_mode", value.into())?;
        Ok(())
    }

    pub fn set_max_unused_days(&mut self, value: Option<u64>) {
        self.max_unused_days = value;
    }

    pub fn set_max_thread_age_days(&mut self, value: Option<u64>) {
        self.max_thread_age_days = value;
    }

    pub fn set_min_thread_idle_seconds(&mut self, value: Option<u64>) {
        self.min_thread_idle_seconds = value;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryOrigin {
    Explicit,
    Inferred,
    Manual,
}

impl MemoryOrigin {
    pub fn parse(value: &str) -> MnemoResult<Self> {
        match value {
            "explicit" => Ok(Self::Explicit),
            "inferred" => Ok(Self::Inferred),
            "manual" => Ok(Self::Manual),
            other => Err(MnemoError::InvalidRequest(format!(
                "unsupported memory origin: {other}"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::Inferred => "inferred",
            Self::Manual => "manual",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryStatus {
    Active,
    Inactive,
    Superseded,
    Conflicted,
    Expired,
    Forgotten,
}

impl MemoryStatus {
    pub fn parse(value: &str) -> MnemoResult<Self> {
        match value {
            "active" => Ok(Self::Active),
            "inactive" => Ok(Self::Inactive),
            "superseded" => Ok(Self::Superseded),
            "conflicted" => Ok(Self::Conflicted),
            "expired" => Ok(Self::Expired),
            "forgotten" => Ok(Self::Forgotten),
            other => Err(MnemoError::InvalidRequest(format!(
                "unsupported memory status: {other}"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Inactive => "inactive",
            Self::Superseded => "superseded",
            Self::Conflicted => "conflicted",
            Self::Expired => "expired",
            Self::Forgotten => "forgotten",
        }
    }

    pub fn is_context_pack_eligible(self) -> bool {
        matches!(self, Self::Active)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MemoryImportance {
    Low,
    Normal,
    High,
    Critical,
}

impl MemoryImportance {
    pub fn parse(value: &str) -> MnemoResult<Self> {
        match value {
            "low" => Ok(Self::Low),
            "normal" => Ok(Self::Normal),
            "high" => Ok(Self::High),
            "critical" => Ok(Self::Critical),
            other => Err(MnemoError::InvalidRequest(format!(
                "unsupported memory importance: {other}"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Normal => "normal",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Memory {
    memory_id: String,
    namespace: Namespace,
    content: String,
    memory_type: String,
    origin: MemoryOrigin,
    status: MemoryStatus,
    importance: MemoryImportance,
    conflict_key: Option<String>,
    valid_from: Option<String>,
    supersedes: Vec<String>,
    superseded_by: Vec<String>,
    source_event_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MemoryPatch {
    pub content: Option<String>,
    pub memory_type: Option<String>,
    pub status: Option<MemoryStatus>,
    pub importance: Option<MemoryImportance>,
    pub conflict_key: Option<Option<String>>,
    pub valid_from: Option<Option<String>>,
    pub source_event_ids: Option<Vec<String>>,
}

impl Memory {
    pub fn new(
        memory_id: impl Into<String>,
        namespace: Namespace,
        content: impl Into<String>,
        memory_type: impl Into<String>,
        origin: MemoryOrigin,
        importance: MemoryImportance,
    ) -> MnemoResult<Self> {
        Ok(Self {
            memory_id: normalize_required("memory_id", memory_id.into())?,
            namespace,
            content: normalize_required("content", content.into())?,
            memory_type: normalize_required("memory_type", memory_type.into())?,
            origin,
            status: MemoryStatus::Active,
            importance,
            conflict_key: None,
            valid_from: None,
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            source_event_ids: Vec::new(),
        })
    }

    pub fn with_conflict_key(mut self, conflict_key: Option<String>) -> Self {
        self.conflict_key = conflict_key.and_then(normalize_optional);
        self
    }

    pub fn with_valid_from(mut self, valid_from: Option<String>) -> Self {
        self.valid_from = valid_from.and_then(normalize_optional);
        self
    }

    pub fn with_source_event_ids(mut self, source_event_ids: Vec<String>) -> Self {
        self.source_event_ids = source_event_ids
            .into_iter()
            .filter_map(normalize_optional)
            .collect();
        self
    }

    pub fn mark_superseded_by(&mut self, memory_id: impl Into<String>) {
        self.status = MemoryStatus::Superseded;
        self.add_superseded_by(memory_id);
    }

    pub fn add_supersedes(&mut self, memory_id: impl Into<String>) {
        let memory_id = memory_id.into();
        if !self.supersedes.contains(&memory_id) {
            self.supersedes.push(memory_id);
        }
    }

    pub fn add_superseded_by(&mut self, memory_id: impl Into<String>) {
        let memory_id = memory_id.into();
        if !self.superseded_by.contains(&memory_id) {
            self.superseded_by.push(memory_id);
        }
    }

    pub fn mark_forgotten(&mut self) {
        self.status = MemoryStatus::Forgotten;
    }

    pub fn set_status(&mut self, status: MemoryStatus) {
        self.status = status;
    }

    pub fn apply_patch(&mut self, patch: MemoryPatch) -> MnemoResult<()> {
        if let Some(content) = patch.content {
            self.content = normalize_required("content", content)?;
        }
        if let Some(memory_type) = patch.memory_type {
            self.memory_type = normalize_required("memory_type", memory_type)?;
        }
        if let Some(status) = patch.status {
            self.status = status;
        }
        if let Some(importance) = patch.importance {
            self.importance = importance;
        }
        if let Some(conflict_key) = patch.conflict_key {
            self.conflict_key = conflict_key.and_then(normalize_optional);
        }
        if let Some(valid_from) = patch.valid_from {
            self.valid_from = valid_from.and_then(normalize_optional);
        }
        if let Some(source_event_ids) = patch.source_event_ids {
            self.source_event_ids = source_event_ids
                .into_iter()
                .filter_map(normalize_optional)
                .collect();
        }
        Ok(())
    }

    pub fn memory_id(&self) -> &str {
        &self.memory_id
    }

    pub fn namespace(&self) -> &Namespace {
        &self.namespace
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn memory_type(&self) -> &str {
        &self.memory_type
    }

    pub fn origin(&self) -> MemoryOrigin {
        self.origin
    }

    pub fn status(&self) -> MemoryStatus {
        self.status
    }

    pub fn importance(&self) -> MemoryImportance {
        self.importance
    }

    pub fn conflict_key(&self) -> Option<&str> {
        self.conflict_key.as_deref()
    }

    pub fn valid_from(&self) -> Option<&str> {
        self.valid_from.as_deref()
    }

    pub fn supersedes(&self) -> &[String] {
        &self.supersedes
    }

    pub fn superseded_by(&self) -> &[String] {
        &self.superseded_by
    }

    pub fn source_event_ids(&self) -> &[String] {
        &self.source_event_ids
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextPackItem {
    item_id: String,
    memory_id: String,
    summary: String,
    memory_type: String,
    status: String,
    provenance_event_ids: Vec<String>,
}

impl ContextPackItem {
    pub fn from_memory(memory: &Memory) -> Self {
        Self {
            item_id: format!("ctx-item-{}", memory.memory_id()),
            memory_id: memory.memory_id().to_string(),
            summary: memory.content().to_string(),
            memory_type: memory.memory_type().to_string(),
            status: memory.status().as_str().to_string(),
            provenance_event_ids: memory.source_event_ids().to_vec(),
        }
    }

    pub fn item_id(&self) -> &str {
        &self.item_id
    }

    pub fn memory_id(&self) -> &str {
        &self.memory_id
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }

    pub fn memory_type(&self) -> &str {
        &self.memory_type
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn provenance_event_ids(&self) -> &[String] {
        &self.provenance_event_ids
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextPackCacheEntry {
    cache_key: String,
    context_pack_id: String,
    version: String,
    generated_at: String,
    content: String,
    max_tokens: usize,
    estimated_tokens: usize,
    budget_exceeded_items: usize,
    conflicted_items: usize,
    items: Vec<ContextPackItem>,
}

impl ContextPackCacheEntry {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cache_key: impl Into<String>,
        context_pack_id: impl Into<String>,
        version: impl Into<String>,
        generated_at: impl Into<String>,
        content: impl Into<String>,
        max_tokens: usize,
        estimated_tokens: usize,
        budget_exceeded_items: usize,
        conflicted_items: usize,
        items: Vec<ContextPackItem>,
    ) -> MnemoResult<Self> {
        Ok(Self {
            cache_key: normalize_required("cache_key", cache_key.into())?,
            context_pack_id: normalize_required("context_pack_id", context_pack_id.into())?,
            version: normalize_required("version", version.into())?,
            generated_at: normalize_required("generated_at", generated_at.into())?,
            content: normalize_required("content", content.into())?,
            max_tokens,
            estimated_tokens,
            budget_exceeded_items,
            conflicted_items,
            items,
        })
    }

    pub fn cache_key(&self) -> &str {
        &self.cache_key
    }

    pub fn context_pack_id(&self) -> &str {
        &self.context_pack_id
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn generated_at(&self) -> &str {
        &self.generated_at
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn max_tokens(&self) -> usize {
        self.max_tokens
    }

    pub fn estimated_tokens(&self) -> usize {
        self.estimated_tokens
    }

    pub fn budget_exceeded_items(&self) -> usize {
        self.budget_exceeded_items
    }

    pub fn conflicted_items(&self) -> usize {
        self.conflicted_items
    }

    pub fn items(&self) -> &[ContextPackItem] {
        &self.items
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
}

impl JobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> MnemoResult<Self> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            other => Err(MnemoError::InvalidRequest(format!(
                "unsupported job status: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    job_id: String,
    job_type: String,
    status: JobStatus,
    namespace: Namespace,
    created_at: String,
    updated_at: String,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    generate_memories: Option<bool>,
    #[serde(default)]
    attempts: u64,
    #[serde(default)]
    retry_at: Option<String>,
    #[serde(default)]
    lease_until: Option<String>,
    error: Option<String>,
    output_summary_id: Option<String>,
}

impl Job {
    #[allow(clippy::too_many_arguments)]
    pub fn from_stored_parts(
        job_id: impl Into<String>,
        job_type: impl Into<String>,
        status: JobStatus,
        namespace: Namespace,
        created_at: impl Into<String>,
        updated_at: impl Into<String>,
        query: Option<String>,
        generate_memories: Option<bool>,
        attempts: u64,
        retry_at: Option<String>,
        lease_until: Option<String>,
        error: Option<String>,
        output_summary_id: Option<String>,
    ) -> MnemoResult<Self> {
        Ok(Self {
            job_id: normalize_required("job_id", job_id.into())?,
            job_type: normalize_required("job_type", job_type.into())?,
            status,
            namespace,
            created_at: normalize_required("created_at", created_at.into())?,
            updated_at: normalize_required("updated_at", updated_at.into())?,
            query: query.and_then(normalize_optional),
            generate_memories,
            attempts,
            retry_at: retry_at.and_then(normalize_optional),
            lease_until: lease_until.and_then(normalize_optional),
            error: error.and_then(normalize_optional),
            output_summary_id: output_summary_id.and_then(normalize_optional),
        })
    }

    pub fn queued(
        job_id: impl Into<String>,
        job_type: impl Into<String>,
        namespace: Namespace,
        timestamp: impl Into<String>,
        query: Option<String>,
        generate_memories: Option<bool>,
    ) -> MnemoResult<Self> {
        let timestamp = normalize_required("timestamp", timestamp.into())?;
        Ok(Self {
            job_id: normalize_required("job_id", job_id.into())?,
            job_type: normalize_required("job_type", job_type.into())?,
            status: JobStatus::Queued,
            namespace,
            created_at: timestamp.clone(),
            updated_at: timestamp,
            query: query.and_then(normalize_optional),
            generate_memories,
            attempts: 0,
            retry_at: None,
            lease_until: None,
            error: None,
            output_summary_id: None,
        })
    }

    pub fn succeeded(
        job_id: impl Into<String>,
        job_type: impl Into<String>,
        namespace: Namespace,
        timestamp: impl Into<String>,
        output_summary_id: Option<String>,
    ) -> MnemoResult<Self> {
        let timestamp = normalize_required("timestamp", timestamp.into())?;
        Ok(Self {
            job_id: normalize_required("job_id", job_id.into())?,
            job_type: normalize_required("job_type", job_type.into())?,
            status: JobStatus::Succeeded,
            namespace,
            created_at: timestamp.clone(),
            updated_at: timestamp,
            query: None,
            generate_memories: None,
            attempts: 1,
            retry_at: None,
            lease_until: None,
            error: None,
            output_summary_id,
        })
    }

    pub fn failed(
        job_id: impl Into<String>,
        job_type: impl Into<String>,
        namespace: Namespace,
        timestamp: impl Into<String>,
        error: impl Into<String>,
    ) -> MnemoResult<Self> {
        let timestamp = normalize_required("timestamp", timestamp.into())?;
        Ok(Self {
            job_id: normalize_required("job_id", job_id.into())?,
            job_type: normalize_required("job_type", job_type.into())?,
            status: JobStatus::Failed,
            namespace,
            created_at: timestamp.clone(),
            updated_at: timestamp,
            query: None,
            generate_memories: None,
            attempts: 1,
            retry_at: None,
            lease_until: None,
            error: normalize_optional(error.into()),
            output_summary_id: None,
        })
    }

    pub fn mark_running(&mut self, timestamp: impl Into<String>) -> MnemoResult<()> {
        self.mark_running_until(timestamp, None)
    }

    pub fn mark_running_until(
        &mut self,
        timestamp: impl Into<String>,
        lease_until: Option<String>,
    ) -> MnemoResult<()> {
        self.status = JobStatus::Running;
        self.updated_at = normalize_required("timestamp", timestamp.into())?;
        self.attempts += 1;
        self.retry_at = None;
        self.lease_until = lease_until.and_then(normalize_optional);
        self.error = None;
        Ok(())
    }

    pub fn mark_succeeded(
        &mut self,
        timestamp: impl Into<String>,
        output_summary_id: Option<String>,
    ) -> MnemoResult<()> {
        self.status = JobStatus::Succeeded;
        self.updated_at = normalize_required("timestamp", timestamp.into())?;
        self.output_summary_id = output_summary_id.and_then(normalize_optional);
        self.retry_at = None;
        self.lease_until = None;
        self.error = None;
        Ok(())
    }

    pub fn mark_failed(
        &mut self,
        timestamp: impl Into<String>,
        error: impl Into<String>,
    ) -> MnemoResult<()> {
        self.status = JobStatus::Failed;
        self.updated_at = normalize_required("timestamp", timestamp.into())?;
        self.lease_until = None;
        self.error = normalize_optional(error.into());
        Ok(())
    }

    pub fn set_retry_at(&mut self, retry_at: Option<String>) {
        self.retry_at = retry_at.and_then(normalize_optional);
    }

    pub fn job_id(&self) -> &str {
        &self.job_id
    }

    pub fn job_type(&self) -> &str {
        &self.job_type
    }

    pub fn status(&self) -> JobStatus {
        self.status
    }

    pub fn namespace(&self) -> &Namespace {
        &self.namespace
    }

    pub fn created_at(&self) -> &str {
        &self.created_at
    }

    pub fn updated_at(&self) -> &str {
        &self.updated_at
    }

    pub fn query(&self) -> Option<&str> {
        self.query.as_deref()
    }

    pub fn generate_memories(&self) -> Option<bool> {
        self.generate_memories
    }

    pub fn attempts(&self) -> u64 {
        self.attempts
    }

    pub fn retry_at(&self) -> Option<&str> {
        self.retry_at.as_deref()
    }

    pub fn lease_until(&self) -> Option<&str> {
        self.lease_until.as_deref()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn output_summary_id(&self) -> Option<&str> {
        self.output_summary_id.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSummary {
    session_summary_id: String,
    namespace: Namespace,
    content: String,
    source_event_ids: Vec<String>,
    inferred_memory_ids: Vec<String>,
    generated_at: String,
}

impl SessionSummary {
    pub fn new(
        session_summary_id: impl Into<String>,
        namespace: Namespace,
        content: impl Into<String>,
        source_event_ids: Vec<String>,
        generated_at: impl Into<String>,
    ) -> MnemoResult<Self> {
        Ok(Self {
            session_summary_id: normalize_required(
                "session_summary_id",
                session_summary_id.into(),
            )?,
            namespace,
            content: normalize_required("content", content.into())?,
            source_event_ids: source_event_ids
                .into_iter()
                .filter_map(normalize_optional)
                .collect(),
            inferred_memory_ids: Vec::new(),
            generated_at: normalize_required("generated_at", generated_at.into())?,
        })
    }

    pub fn with_inferred_memory_ids(mut self, inferred_memory_ids: Vec<String>) -> Self {
        self.inferred_memory_ids = inferred_memory_ids
            .into_iter()
            .filter_map(normalize_optional)
            .collect();
        self
    }

    pub fn session_summary_id(&self) -> &str {
        &self.session_summary_id
    }

    pub fn namespace(&self) -> &Namespace {
        &self.namespace
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn source_event_ids(&self) -> &[String] {
        &self.source_event_ids
    }

    pub fn inferred_memory_ids(&self) -> &[String] {
        &self.inferred_memory_ids
    }

    pub fn generated_at(&self) -> &str {
        &self.generated_at
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UsageSignal {
    Positive,
    Neutral,
    Negative,
}

impl UsageSignal {
    pub fn parse(value: &str) -> MnemoResult<Self> {
        match value {
            "positive" => Ok(Self::Positive),
            "neutral" => Ok(Self::Neutral),
            "negative" => Ok(Self::Negative),
            other => Err(MnemoError::InvalidRequest(format!(
                "unsupported usage signal: {other}"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Positive => "positive",
            Self::Neutral => "neutral",
            Self::Negative => "negative",
        }
    }

    pub fn score_delta(self) -> i64 {
        match self {
            Self::Positive => 1,
            Self::Neutral => 0,
            Self::Negative => -2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageReport {
    usage_id: String,
    namespace: Namespace,
    context_pack_id: String,
    signal: UsageSignal,
    memory_ids: Vec<String>,
    notes: Option<String>,
    reported_at: String,
}

impl UsageReport {
    pub fn new(
        usage_id: impl Into<String>,
        namespace: Namespace,
        context_pack_id: impl Into<String>,
        signal: UsageSignal,
        memory_ids: Vec<String>,
        notes: Option<String>,
        reported_at: impl Into<String>,
    ) -> MnemoResult<Self> {
        Ok(Self {
            usage_id: normalize_required("usage_id", usage_id.into())?,
            namespace,
            context_pack_id: normalize_required("context_pack_id", context_pack_id.into())?,
            signal,
            memory_ids: memory_ids
                .into_iter()
                .filter_map(normalize_optional)
                .collect(),
            notes: notes.and_then(normalize_optional),
            reported_at: normalize_required("reported_at", reported_at.into())?,
        })
    }

    pub fn usage_id(&self) -> &str {
        &self.usage_id
    }

    pub fn namespace(&self) -> &Namespace {
        &self.namespace
    }

    pub fn context_pack_id(&self) -> &str {
        &self.context_pack_id
    }

    pub fn signal(&self) -> UsageSignal {
        self.signal
    }

    pub fn memory_ids(&self) -> &[String] {
        &self.memory_ids
    }

    pub fn notes(&self) -> Option<&str> {
        self.notes.as_deref()
    }

    pub fn reported_at(&self) -> &str {
        &self.reported_at
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForgetTombstone {
    tombstone_id: String,
    namespace: Namespace,
    memory_ids: Vec<String>,
    contents: Vec<String>,
    reason: Option<String>,
    created_at: String,
}

impl ForgetTombstone {
    pub fn new(
        tombstone_id: impl Into<String>,
        namespace: Namespace,
        memory_ids: Vec<String>,
        contents: Vec<String>,
        reason: Option<String>,
        created_at: impl Into<String>,
    ) -> MnemoResult<Self> {
        Ok(Self {
            tombstone_id: normalize_required("tombstone_id", tombstone_id.into())?,
            namespace,
            memory_ids: memory_ids
                .into_iter()
                .filter_map(normalize_optional)
                .collect(),
            contents: contents
                .into_iter()
                .filter_map(normalize_optional)
                .collect(),
            reason: reason.and_then(normalize_optional),
            created_at: normalize_required("created_at", created_at.into())?,
        })
    }

    pub fn tombstone_id(&self) -> &str {
        &self.tombstone_id
    }

    pub fn namespace(&self) -> &Namespace {
        &self.namespace
    }

    pub fn memory_ids(&self) -> &[String] {
        &self.memory_ids
    }

    pub fn contents(&self) -> &[String] {
        &self.contents
    }

    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    pub fn created_at(&self) -> &str {
        &self.created_at
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthStatus {
    pub service: &'static str,
    pub status: &'static str,
}

impl Default for HealthStatus {
    fn default() -> Self {
        Self {
            service: "mnemo",
            status: "ok",
        }
    }
}

fn normalize_required(field: &str, value: String) -> MnemoResult<String> {
    let normalized = value.trim().to_string();
    if normalized.is_empty() {
        Err(MnemoError::InvalidRequest(format!("{field} is required")))
    } else {
        Ok(normalized)
    }
}

fn normalize_optional(value: String) -> Option<String> {
    let normalized = value.trim().to_string();
    (!normalized.is_empty()).then_some(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_defaults_tenant_and_requires_user() {
        let ns = Namespace::new(" u1 ").expect("namespace");
        assert_eq!(ns.tenant_id(), "default");
        assert_eq!(ns.user_id(), "u1");
        assert!(Namespace::new(" ").is_err());
    }

    #[test]
    fn memory_mode_controls_only_inferred_generation() {
        assert!(ThreadMemoryMode::Enabled.allows_inferred_memory());
        assert!(!ThreadMemoryMode::Disabled.allows_inferred_memory());
        assert!(!ThreadMemoryMode::Polluted.allows_inferred_memory());
    }
}
