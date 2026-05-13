use mnemo_core::ContextPackCacheEntry;
use mnemo_core::Event;
use mnemo_core::ForgetTombstone;
use mnemo_core::Job;
use mnemo_core::Memory;
use mnemo_core::MemoryPatch;
use mnemo_core::MnemoResult;
use mnemo_core::Namespace;
use mnemo_core::NamespacePolicy;
use mnemo_core::SessionSummary;
use mnemo_core::ThreadMemoryMode;
use mnemo_core::UsageReport;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventAppendOutcome {
    pub event_id: String,
    pub deduplicated: bool,
}

pub trait EventStore {
    fn append_event(&self, event: Event) -> MnemoResult<EventAppendOutcome>;
    fn get_event(&self, namespace: &Namespace, event_id: &str) -> MnemoResult<Option<Event>>;
    fn search_events(&self, namespace: &Namespace, query: Option<&str>) -> MnemoResult<Vec<Event>>;
}

pub trait ThreadStateStore {
    fn set_memory_mode(
        &self,
        namespace: &Namespace,
        mode: ThreadMemoryMode,
    ) -> MnemoResult<ThreadMemoryMode>;

    fn get_memory_mode(&self, namespace: &Namespace) -> MnemoResult<ThreadMemoryMode>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryWriteOutcome {
    pub memory: Memory,
    pub superseded_memory_ids: Vec<String>,
}

pub trait MemoryStore {
    fn write_memory(&self, memory: Memory) -> MnemoResult<MemoryWriteOutcome>;
    fn get_memory(&self, namespace: &Namespace, memory_id: &str) -> MnemoResult<Option<Memory>>;
    fn find_memory(&self, memory_id: &str) -> MnemoResult<Option<Memory>>;
    fn patch_memory(
        &self,
        namespace: &Namespace,
        memory_id: &str,
        patch: MemoryPatch,
    ) -> MnemoResult<Memory>;
    fn search_memories(
        &self,
        namespace: &Namespace,
        query: Option<&str>,
        include_inactive: bool,
    ) -> MnemoResult<Vec<Memory>>;
    fn active_memories(&self, namespace: &Namespace) -> MnemoResult<Vec<Memory>>;
    fn next_memory_id(&self) -> MnemoResult<String>;
}

pub trait PolicyStore {
    fn get_policy(&self, namespace: &Namespace) -> MnemoResult<NamespacePolicy>;
    fn set_policy(&self, policy: NamespacePolicy) -> MnemoResult<NamespacePolicy>;
}

pub trait ContextPackCacheStore {
    fn get_context_pack_cache(&self, cache_key: &str)
    -> MnemoResult<Option<ContextPackCacheEntry>>;
    fn put_context_pack_cache(
        &self,
        entry: ContextPackCacheEntry,
    ) -> MnemoResult<ContextPackCacheEntry>;
}

pub trait SessionSummaryStore {
    fn write_session_summary(&self, summary: SessionSummary) -> MnemoResult<SessionSummary>;
    fn get_session_summary(&self, summary_id: &str) -> MnemoResult<Option<SessionSummary>>;
    fn search_session_summaries(
        &self,
        namespace: &Namespace,
        query: Option<&str>,
    ) -> MnemoResult<Vec<SessionSummary>>;
    fn next_session_summary_id(&self) -> MnemoResult<String>;
}

pub trait JobStore {
    fn write_job(&self, job: Job) -> MnemoResult<Job>;
    fn update_job(&self, job: Job) -> MnemoResult<Job>;
    fn get_job(&self, job_id: &str) -> MnemoResult<Option<Job>>;
    fn list_jobs(&self) -> MnemoResult<Vec<Job>>;
    fn claim_next_queued_job(&self, job_type: &str, timestamp: String) -> MnemoResult<Option<Job>> {
        self.claim_next_runnable_job(job_type, timestamp, None)
    }
    fn claim_next_runnable_job(
        &self,
        job_type: &str,
        timestamp: String,
        lease_until: Option<String>,
    ) -> MnemoResult<Option<Job>>;
    fn next_job_id(&self) -> MnemoResult<String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryConflict {
    pub namespace: Namespace,
    pub conflict_key: String,
    pub memories: Vec<Memory>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictResolveOutcome {
    pub winner: Memory,
    pub superseded_memory_ids: Vec<String>,
}

pub trait ConflictStore {
    fn search_conflicts(&self, namespace: &Namespace) -> MnemoResult<Vec<MemoryConflict>>;
    fn resolve_conflict(
        &self,
        namespace: &Namespace,
        conflict_key: &str,
        winner_memory_id: &str,
    ) -> MnemoResult<ConflictResolveOutcome>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgetOutcome {
    pub tombstone: ForgetTombstone,
    pub forgotten_memory_ids: Vec<String>,
}

pub trait ForgetStore {
    fn forget_memories(
        &self,
        namespace: &Namespace,
        memory_ids: &[String],
        reason: Option<String>,
        created_at: String,
    ) -> MnemoResult<ForgetOutcome>;
    fn is_content_forgotten(&self, namespace: &Namespace, content: &str) -> MnemoResult<bool>;
    fn next_tombstone_id(&self) -> MnemoResult<String>;
}

pub trait UsageStore {
    fn write_usage_report(&self, report: UsageReport) -> MnemoResult<UsageReport>;
    fn list_usage_reports(&self) -> MnemoResult<Vec<UsageReport>>;
    fn search_usage_reports(&self, namespace: Option<&Namespace>) -> MnemoResult<Vec<UsageReport>>;
    fn next_usage_id(&self) -> MnemoResult<String>;
}

pub trait MnemoStore:
    EventStore
    + ThreadStateStore
    + PolicyStore
    + ContextPackCacheStore
    + MemoryStore
    + SessionSummaryStore
    + JobStore
    + ConflictStore
    + ForgetStore
    + UsageStore
    + Send
    + Sync
{
}

impl<T> MnemoStore for T where
    T: EventStore
        + ThreadStateStore
        + PolicyStore
        + ContextPackCacheStore
        + MemoryStore
        + SessionSummaryStore
        + JobStore
        + ConflictStore
        + ForgetStore
        + UsageStore
        + Send
        + Sync
{
}
