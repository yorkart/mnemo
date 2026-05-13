use mnemo_core::Event;
use mnemo_core::EventRole;
use mnemo_core::EventType;
use mnemo_core::Memory;
use mnemo_core::MemoryHints;
use mnemo_core::MemoryImportance;
use mnemo_core::MemoryOrigin;
use mnemo_core::MemoryPatch;
use mnemo_core::MemoryStatus;
use mnemo_core::MnemoResult;
use mnemo_core::Namespace;
use mnemo_core::UsageReport;
use mnemo_core::UsageSignal;
use serde::Deserialize;

use crate::util::stable_hashed_id;

#[derive(Debug, Clone, Deserialize)]
pub struct NamespaceRequest {
    pub tenant_id: Option<String>,
    pub user_id: String,
    pub workspace_id: Option<String>,
    pub thread_id: Option<String>,
    pub agent_id: Option<String>,
    pub source: Option<String>,
}

impl NamespaceRequest {
    pub fn into_namespace(self) -> MnemoResult<Namespace> {
        let mut namespace = Namespace::new(self.user_id)?;
        if let Some(tenant_id) = self.tenant_id {
            namespace = namespace.with_tenant(tenant_id);
        }
        if let Some(workspace_id) = self.workspace_id {
            namespace = namespace.with_workspace(workspace_id);
        }
        if let Some(thread_id) = self.thread_id {
            namespace = namespace.with_thread(thread_id);
        }
        if let Some(agent_id) = self.agent_id {
            namespace = namespace.with_agent(agent_id);
        }
        if let Some(source) = self.source {
            namespace = namespace.with_source(source);
        }
        Ok(namespace)
    }
}

#[derive(Debug, Deserialize)]
pub struct MemoryHintsRequest {
    #[serde(default)]
    pub eligible: bool,
    #[serde(default)]
    pub explicit_memory_intent: bool,
    #[serde(default)]
    pub external_context: bool,
}

impl MemoryHintsRequest {
    pub fn into_memory_hints(value: Option<Self>) -> MemoryHints {
        let Some(value) = value else {
            return MemoryHints::default();
        };
        MemoryHints {
            eligible: value.eligible,
            explicit_memory_intent: value.explicit_memory_intent,
            external_context: value.external_context,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct EventWriteRequest {
    pub event_id: Option<String>,
    pub namespace: NamespaceRequest,
    #[serde(rename = "type")]
    pub event_type: String,
    pub role: String,
    pub content: String,
    pub occurred_at: String,
    pub memory_hints: Option<MemoryHintsRequest>,
}

impl EventWriteRequest {
    pub fn into_event(self, idempotency_key: Option<&str>) -> MnemoResult<Event> {
        let event_type = EventType::parse(self.event_type.as_str())?;
        let role = EventRole::parse(self.role.as_str())?;
        let namespace = self.namespace.into_namespace()?;
        let event_id = self
            .event_id
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                idempotency_key
                    .filter(|value| !value.trim().is_empty())
                    .map(|value| stable_hashed_id("evt-idem", &[value]))
            })
            .unwrap_or_else(|| {
                stable_hashed_id(
                    "evt-auto",
                    &[
                        namespace.stable_key().as_str(),
                        event_type.as_str(),
                        role.as_str(),
                        self.content.as_str(),
                        self.occurred_at.as_str(),
                    ],
                )
            });
        Event::new(
            event_id,
            namespace,
            event_type,
            role,
            self.content,
            self.occurred_at,
        )
        .map(|event| {
            event.with_memory_hints(MemoryHintsRequest::into_memory_hints(self.memory_hints))
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct EventBatchWriteRequest {
    pub events: Vec<EventWriteRequest>,
}

#[derive(Debug, Deserialize)]
pub struct EventSearchRequest {
    pub namespace: NamespaceRequest,
    pub q: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ThreadMemoryModeSetRequest {
    pub namespace: NamespaceRequest,
    pub mode: String,
}

#[derive(Debug, Deserialize)]
pub struct NamespaceStatusRequest {
    pub namespace: NamespaceRequest,
}

#[derive(Debug, Deserialize)]
pub struct NamespacePolicyGetRequest {
    pub namespace: NamespaceRequest,
}

#[derive(Debug, Deserialize)]
pub struct NamespacePolicySetRequest {
    pub namespace: NamespaceRequest,
    pub auto_generate_memories: Option<bool>,
    pub auto_use_memories: Option<bool>,
    pub context_pack_max_tokens: Option<usize>,
    pub external_context_policy: Option<String>,
    pub conflict_resolution_mode: Option<String>,
    pub max_unused_days: Option<Option<u64>>,
    pub max_thread_age_days: Option<Option<u64>>,
    pub min_thread_idle_seconds: Option<Option<u64>>,
}

#[derive(Debug, Deserialize)]
pub struct MemoryWriteRequest {
    pub namespace: NamespaceRequest,
    pub content: String,
    pub memory_type: String,
    pub importance: Option<String>,
    pub conflict_key: Option<String>,
    pub source_event_ids: Option<Vec<String>>,
    pub valid_from: Option<String>,
}

impl MemoryWriteRequest {
    pub fn into_memory(self, memory_id: String) -> MnemoResult<Memory> {
        let importance = self
            .importance
            .as_deref()
            .map(MemoryImportance::parse)
            .transpose()?
            .unwrap_or(MemoryImportance::Normal);
        Memory::new(
            memory_id,
            self.namespace.into_namespace()?,
            self.content,
            self.memory_type,
            MemoryOrigin::Explicit,
            importance,
        )
        .map(|memory| {
            memory
                .with_conflict_key(self.conflict_key)
                .with_valid_from(self.valid_from)
                .with_source_event_ids(self.source_event_ids.unwrap_or_default())
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct MemorySearchRequest {
    pub namespace: NamespaceRequest,
    pub q: Option<String>,
    pub include_inactive: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct MemoryPatchRequest {
    pub namespace: NamespaceRequest,
    pub content: Option<String>,
    pub memory_type: Option<String>,
    pub status: Option<String>,
    pub importance: Option<String>,
    pub conflict_key: Option<Option<String>>,
    pub valid_from: Option<Option<String>>,
    pub source_event_ids: Option<Vec<String>>,
}

impl MemoryPatchRequest {
    pub fn into_patch(self) -> MnemoResult<MemoryPatch> {
        Ok(MemoryPatch {
            content: self.content,
            memory_type: self.memory_type,
            status: self
                .status
                .as_deref()
                .map(MemoryStatus::parse)
                .transpose()?,
            importance: self
                .importance
                .as_deref()
                .map(MemoryImportance::parse)
                .transpose()?,
            conflict_key: self.conflict_key,
            valid_from: self.valid_from,
            source_event_ids: self.source_event_ids,
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct ContextPackRequest {
    pub namespace: NamespaceRequest,
    pub budget: Option<ContextPackBudgetRequest>,
}

#[derive(Debug, Deserialize)]
pub struct ContextPackBudgetRequest {
    pub max_tokens: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct WrapupRequest {
    pub namespace: NamespaceRequest,
    pub q: Option<String>,
    pub generate_memories: Option<bool>,
    #[serde(rename = "async")]
    pub async_job: Option<bool>,
    pub simulate_failure: Option<bool>,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct JobRetryRequest {
    pub job_id: String,
    pub q: Option<String>,
    pub generate_memories: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
pub struct WorkerRunOnceRequest {
    pub lease_seconds: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct SessionSummarySearchRequest {
    pub namespace: NamespaceRequest,
    pub q: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UsageReportRequest {
    pub namespace: NamespaceRequest,
    pub context_pack_id: String,
    pub signal: String,
    pub memory_ids: Option<Vec<String>>,
    pub notes: Option<String>,
}

impl UsageReportRequest {
    pub fn into_usage_report(self, usage_id: String, reported_at: String) -> MnemoResult<UsageReport> {
        UsageReport::new(
            usage_id,
            self.namespace.into_namespace()?,
            self.context_pack_id,
            UsageSignal::parse(self.signal.as_str())?,
            self.memory_ids.unwrap_or_default(),
            self.notes,
            reported_at,
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct UsageSearchRequest {
    pub namespace: Option<NamespaceRequest>,
}

#[derive(Debug, Deserialize)]
pub struct ConflictSearchRequest {
    pub namespace: NamespaceRequest,
}

#[derive(Debug, Deserialize)]
pub struct ConflictResolveRequest {
    pub namespace: NamespaceRequest,
    pub conflict_key: String,
    pub winner_memory_id: String,
}

#[derive(Debug, Deserialize)]
pub struct ForgetRequest {
    pub namespace: NamespaceRequest,
    pub memory_ids: Vec<String>,
    pub reason: Option<String>,
}
