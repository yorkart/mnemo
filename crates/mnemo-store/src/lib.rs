#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use mnemo_core::ContextPackCacheEntry;
use mnemo_core::Event;
use mnemo_core::ForgetTombstone;
use mnemo_core::Job;
use mnemo_core::JobStatus;
use mnemo_core::Memory;
use mnemo_core::MemoryPatch;
use mnemo_core::MemoryStatus;
use mnemo_core::MnemoError;
use mnemo_core::MnemoResult;
use mnemo_core::Namespace;
use mnemo_core::NamespacePolicy;
use mnemo_core::SessionSummary;
use mnemo_core::ThreadMemoryMode;
use mnemo_core::UsageReport;
use serde::Deserialize;
use serde::Serialize;

mod sqlite;

pub use sqlite::SqliteStore;
pub use sqlite::init_sqlite_database;

pub const SQLITE_INIT_SQL: &str = include_str!("../migrations/0001_init.sql");

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

#[derive(Debug, Default)]
pub struct InMemoryStore {
    events: Mutex<HashMap<String, StoredEvent>>,
    memory_modes: Mutex<HashMap<String, ThreadMemoryMode>>,
    namespace_policies: Mutex<HashMap<String, NamespacePolicy>>,
    context_pack_cache: Mutex<HashMap<String, ContextPackCacheEntry>>,
    memories: Mutex<HashMap<String, Memory>>,
    session_summaries: Mutex<HashMap<String, SessionSummary>>,
    jobs: Mutex<HashMap<String, Job>>,
    forget_tombstones: Mutex<HashMap<String, ForgetTombstone>>,
    usage_reports: Mutex<Vec<UsageReport>>,
    usage_scores: Mutex<HashMap<String, i64>>,
    next_memory_seq: Mutex<u64>,
    next_summary_seq: Mutex<u64>,
    next_job_seq: Mutex<u64>,
    next_tombstone_seq: Mutex<u64>,
    next_usage_seq: Mutex<u64>,
    persistence_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredEvent {
    event: Event,
    fingerprint: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoreSnapshot {
    events: Vec<StoredEvent>,
    memory_modes: Vec<(String, ThreadMemoryMode)>,
    #[serde(default)]
    namespace_policies: Vec<NamespacePolicy>,
    #[serde(default)]
    context_pack_cache: Vec<ContextPackCacheEntry>,
    memories: Vec<Memory>,
    session_summaries: Vec<SessionSummary>,
    jobs: Vec<Job>,
    forget_tombstones: Vec<ForgetTombstone>,
    usage_reports: Vec<UsageReport>,
    usage_scores: Vec<(String, i64)>,
    next_memory_seq: u64,
    next_summary_seq: u64,
    next_job_seq: u64,
    next_tombstone_seq: u64,
    next_usage_seq: u64,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open(path: impl Into<PathBuf>) -> MnemoResult<Self> {
        let path = path.into();
        if !path.exists() {
            return Ok(Self {
                persistence_path: Some(path),
                ..Self::default()
            });
        }

        let bytes = std::fs::read(&path)
            .map_err(|err| MnemoError::Internal(format!("failed to read store snapshot: {err}")))?;
        let snapshot: StoreSnapshot = serde_json::from_slice(&bytes).map_err(|err| {
            MnemoError::Internal(format!("failed to decode store snapshot: {err}"))
        })?;
        let events = snapshot
            .events
            .into_iter()
            .map(|stored| {
                (
                    Self::event_key(stored.event.namespace(), stored.event.event_id()),
                    stored,
                )
            })
            .collect();
        let memories = snapshot
            .memories
            .into_iter()
            .map(|memory| {
                (
                    Self::memory_key(memory.namespace(), memory.memory_id()),
                    memory,
                )
            })
            .collect();
        let session_summaries = snapshot
            .session_summaries
            .into_iter()
            .map(|summary| (summary.session_summary_id().to_string(), summary))
            .collect();
        let jobs = snapshot
            .jobs
            .into_iter()
            .map(|job| (job.job_id().to_string(), job))
            .collect();
        let forget_tombstones = snapshot
            .forget_tombstones
            .into_iter()
            .map(|tombstone| (tombstone.tombstone_id().to_string(), tombstone))
            .collect();
        let namespace_policies = snapshot
            .namespace_policies
            .into_iter()
            .map(|policy| (policy.namespace().stable_key(), policy))
            .collect();
        let context_pack_cache = snapshot
            .context_pack_cache
            .into_iter()
            .map(|entry| (entry.cache_key().to_string(), entry))
            .collect();

        Ok(Self {
            events: Mutex::new(events),
            memory_modes: Mutex::new(snapshot.memory_modes.into_iter().collect()),
            namespace_policies: Mutex::new(namespace_policies),
            context_pack_cache: Mutex::new(context_pack_cache),
            memories: Mutex::new(memories),
            session_summaries: Mutex::new(session_summaries),
            jobs: Mutex::new(jobs),
            forget_tombstones: Mutex::new(forget_tombstones),
            usage_reports: Mutex::new(snapshot.usage_reports),
            usage_scores: Mutex::new(snapshot.usage_scores.into_iter().collect()),
            next_memory_seq: Mutex::new(snapshot.next_memory_seq),
            next_summary_seq: Mutex::new(snapshot.next_summary_seq),
            next_job_seq: Mutex::new(snapshot.next_job_seq),
            next_tombstone_seq: Mutex::new(snapshot.next_tombstone_seq),
            next_usage_seq: Mutex::new(snapshot.next_usage_seq),
            persistence_path: Some(path),
        })
    }

    fn persist(&self) -> MnemoResult<()> {
        let Some(path) = self.persistence_path.as_ref() else {
            return Ok(());
        };
        let snapshot = StoreSnapshot {
            events: self
                .events
                .lock()
                .map_err(|_| MnemoError::Internal("event store lock poisoned".to_string()))?
                .values()
                .cloned()
                .collect(),
            memory_modes: self
                .memory_modes
                .lock()
                .map_err(|_| MnemoError::Internal("thread state lock poisoned".to_string()))?
                .iter()
                .map(|(key, value)| (key.clone(), *value))
                .collect(),
            namespace_policies: self
                .namespace_policies
                .lock()
                .map_err(|_| MnemoError::Internal("policy store lock poisoned".to_string()))?
                .values()
                .cloned()
                .collect(),
            context_pack_cache: self
                .context_pack_cache
                .lock()
                .map_err(|_| MnemoError::Internal("context pack cache lock poisoned".to_string()))?
                .values()
                .cloned()
                .collect(),
            memories: self
                .memories
                .lock()
                .map_err(|_| MnemoError::Internal("memory store lock poisoned".to_string()))?
                .values()
                .cloned()
                .collect(),
            session_summaries: self
                .session_summaries
                .lock()
                .map_err(|_| {
                    MnemoError::Internal("session summary store lock poisoned".to_string())
                })?
                .values()
                .cloned()
                .collect(),
            jobs: self
                .jobs
                .lock()
                .map_err(|_| MnemoError::Internal("job store lock poisoned".to_string()))?
                .values()
                .cloned()
                .collect(),
            forget_tombstones: self
                .forget_tombstones
                .lock()
                .map_err(|_| MnemoError::Internal("forget store lock poisoned".to_string()))?
                .values()
                .cloned()
                .collect(),
            usage_reports: self
                .usage_reports
                .lock()
                .map_err(|_| MnemoError::Internal("usage store lock poisoned".to_string()))?
                .clone(),
            usage_scores: self
                .usage_scores
                .lock()
                .map_err(|_| MnemoError::Internal("usage score lock poisoned".to_string()))?
                .iter()
                .map(|(key, value)| (key.clone(), *value))
                .collect(),
            next_memory_seq: *self
                .next_memory_seq
                .lock()
                .map_err(|_| MnemoError::Internal("memory sequence lock poisoned".to_string()))?,
            next_summary_seq: *self
                .next_summary_seq
                .lock()
                .map_err(|_| MnemoError::Internal("summary sequence lock poisoned".to_string()))?,
            next_job_seq: *self
                .next_job_seq
                .lock()
                .map_err(|_| MnemoError::Internal("job sequence lock poisoned".to_string()))?,
            next_tombstone_seq: *self.next_tombstone_seq.lock().map_err(|_| {
                MnemoError::Internal("tombstone sequence lock poisoned".to_string())
            })?,
            next_usage_seq: *self
                .next_usage_seq
                .lock()
                .map_err(|_| MnemoError::Internal("usage sequence lock poisoned".to_string()))?,
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                MnemoError::Internal(format!("failed to create store directory: {err}"))
            })?;
        }
        let bytes = serde_json::to_vec_pretty(&snapshot).map_err(|err| {
            MnemoError::Internal(format!("failed to encode store snapshot: {err}"))
        })?;
        let tmp_path = path.with_extension("tmp");
        std::fs::write(&tmp_path, bytes).map_err(|err| {
            MnemoError::Internal(format!("failed to write store snapshot: {err}"))
        })?;
        std::fs::rename(&tmp_path, path).map_err(|err| {
            MnemoError::Internal(format!("failed to replace store snapshot: {err}"))
        })?;
        Ok(())
    }

    fn event_key(namespace: &Namespace, event_id: &str) -> String {
        format!("{}\u{1e}{}", namespace.stable_key(), event_id)
    }

    fn thread_key(namespace: &Namespace) -> MnemoResult<String> {
        let thread_id = namespace.thread_id().ok_or_else(|| {
            MnemoError::InvalidRequest("namespace.thread_id is required".to_string())
        })?;
        Ok(format!(
            "{}\u{1e}{}\u{1e}{}",
            namespace.tenant_id(),
            namespace.user_id(),
            thread_id
        ))
    }

    fn memory_key(namespace: &Namespace, memory_id: &str) -> String {
        format!("{}\u{1d}{}", namespace.stable_key(), memory_id)
    }

    fn namespace_matches(left: &Namespace, right: &Namespace) -> bool {
        left.stable_key() == right.stable_key()
    }

    fn seq_id(seq: &Mutex<u64>, prefix: &str) -> MnemoResult<String> {
        let mut seq = seq
            .lock()
            .map_err(|_| MnemoError::Internal(format!("{prefix} sequence lock poisoned")))?;
        *seq += 1;
        Ok(format!("{prefix}-{:06}", *seq))
    }
}

impl EventStore for InMemoryStore {
    fn append_event(&self, event: Event) -> MnemoResult<EventAppendOutcome> {
        let key = Self::event_key(event.namespace(), event.event_id());
        let fingerprint = event.conflict_fingerprint();
        let event_id = event.event_id().to_string();
        {
            let mut events = self
                .events
                .lock()
                .map_err(|_| MnemoError::Internal("event store lock poisoned".to_string()))?;

            if let Some(existing) = events.get(&key) {
                if existing.fingerprint == fingerprint {
                    return Ok(EventAppendOutcome {
                        event_id,
                        deduplicated: true,
                    });
                }

                return Err(MnemoError::EventConflict(format!(
                    "event_id {} already exists with different content",
                    event.event_id()
                )));
            }

            events.insert(key, StoredEvent { event, fingerprint });
        }

        self.persist()?;
        Ok(EventAppendOutcome {
            event_id,
            deduplicated: false,
        })
    }

    fn get_event(&self, namespace: &Namespace, event_id: &str) -> MnemoResult<Option<Event>> {
        let key = Self::event_key(namespace, event_id);
        let events = self
            .events
            .lock()
            .map_err(|_| MnemoError::Internal("event store lock poisoned".to_string()))?;
        Ok(events.get(&key).map(|stored| stored.event.clone()))
    }

    fn search_events(&self, namespace: &Namespace, query: Option<&str>) -> MnemoResult<Vec<Event>> {
        let namespace_key = namespace.stable_key();
        let query = query.map(str::trim).filter(|value| !value.is_empty());
        let events = self
            .events
            .lock()
            .map_err(|_| MnemoError::Internal("event store lock poisoned".to_string()))?;
        let mut matches = events
            .values()
            .filter(|stored| stored.event.namespace().stable_key() == namespace_key)
            .filter(|stored| {
                query.is_none_or(|query| {
                    stored.event.event_id().contains(query)
                        || stored.event.content().contains(query)
                        || stored.event.event_type().as_str().contains(query)
                        || stored.event.role().as_str().contains(query)
                })
            })
            .map(|stored| stored.event.clone())
            .collect::<Vec<_>>();
        matches.sort_by(|left, right| {
            left.occurred_at()
                .cmp(right.occurred_at())
                .then_with(|| left.event_id().cmp(right.event_id()))
        });
        Ok(matches)
    }
}

impl ThreadStateStore for InMemoryStore {
    fn set_memory_mode(
        &self,
        namespace: &Namespace,
        mode: ThreadMemoryMode,
    ) -> MnemoResult<ThreadMemoryMode> {
        let key = Self::thread_key(namespace)?;
        let mut memory_modes = self
            .memory_modes
            .lock()
            .map_err(|_| MnemoError::Internal("thread state lock poisoned".to_string()))?;
        memory_modes.insert(key, mode);
        drop(memory_modes);
        self.persist()?;
        Ok(mode)
    }

    fn get_memory_mode(&self, namespace: &Namespace) -> MnemoResult<ThreadMemoryMode> {
        let key = Self::thread_key(namespace)?;
        let memory_modes = self
            .memory_modes
            .lock()
            .map_err(|_| MnemoError::Internal("thread state lock poisoned".to_string()))?;
        Ok(memory_modes
            .get(&key)
            .copied()
            .unwrap_or(ThreadMemoryMode::Enabled))
    }
}

impl PolicyStore for InMemoryStore {
    fn get_policy(&self, namespace: &Namespace) -> MnemoResult<NamespacePolicy> {
        let policies = self
            .namespace_policies
            .lock()
            .map_err(|_| MnemoError::Internal("policy store lock poisoned".to_string()))?;
        Ok(policies
            .get(&namespace.stable_key())
            .cloned()
            .unwrap_or_else(|| NamespacePolicy::default_for(namespace.clone())))
    }

    fn set_policy(&self, policy: NamespacePolicy) -> MnemoResult<NamespacePolicy> {
        {
            let mut policies = self
                .namespace_policies
                .lock()
                .map_err(|_| MnemoError::Internal("policy store lock poisoned".to_string()))?;
            policies.insert(policy.namespace().stable_key(), policy.clone());
        }
        self.persist()?;
        Ok(policy)
    }
}

impl ContextPackCacheStore for InMemoryStore {
    fn get_context_pack_cache(
        &self,
        cache_key: &str,
    ) -> MnemoResult<Option<ContextPackCacheEntry>> {
        let cache = self
            .context_pack_cache
            .lock()
            .map_err(|_| MnemoError::Internal("context pack cache lock poisoned".to_string()))?;
        Ok(cache.get(cache_key).cloned())
    }

    fn put_context_pack_cache(
        &self,
        entry: ContextPackCacheEntry,
    ) -> MnemoResult<ContextPackCacheEntry> {
        {
            let mut cache = self.context_pack_cache.lock().map_err(|_| {
                MnemoError::Internal("context pack cache lock poisoned".to_string())
            })?;
            cache.insert(entry.cache_key().to_string(), entry.clone());
        }
        self.persist()?;
        Ok(entry)
    }
}

impl MemoryStore for InMemoryStore {
    fn write_memory(&self, mut memory: Memory) -> MnemoResult<MemoryWriteOutcome> {
        let namespace_key = memory.namespace().stable_key();
        let new_memory_id = memory.memory_id().to_string();
        let conflict_key = memory.conflict_key().map(str::to_string);
        let mut superseded_memory_ids = Vec::new();
        {
            let mut memories = self
                .memories
                .lock()
                .map_err(|_| MnemoError::Internal("memory store lock poisoned".to_string()))?;

            if let Some(conflict_key) = conflict_key.as_deref() {
                for existing in memories.values_mut() {
                    if existing.namespace().stable_key() == namespace_key
                        && existing.status() == MemoryStatus::Active
                        && existing.conflict_key() == Some(conflict_key)
                    {
                        let existing_id = existing.memory_id().to_string();
                        existing.mark_superseded_by(new_memory_id.clone());
                        memory.add_supersedes(existing_id.clone());
                        superseded_memory_ids.push(existing_id);
                    }
                }
            }

            let key = Self::memory_key(memory.namespace(), memory.memory_id());
            memories.insert(key, memory.clone());
        }

        self.persist()?;
        Ok(MemoryWriteOutcome {
            memory,
            superseded_memory_ids,
        })
    }

    fn get_memory(&self, namespace: &Namespace, memory_id: &str) -> MnemoResult<Option<Memory>> {
        let key = Self::memory_key(namespace, memory_id);
        let memories = self
            .memories
            .lock()
            .map_err(|_| MnemoError::Internal("memory store lock poisoned".to_string()))?;
        Ok(memories.get(&key).cloned())
    }

    fn find_memory(&self, memory_id: &str) -> MnemoResult<Option<Memory>> {
        let memories = self
            .memories
            .lock()
            .map_err(|_| MnemoError::Internal("memory store lock poisoned".to_string()))?;
        Ok(memories
            .values()
            .find(|memory| memory.memory_id() == memory_id)
            .cloned())
    }

    fn patch_memory(
        &self,
        namespace: &Namespace,
        memory_id: &str,
        patch: MemoryPatch,
    ) -> MnemoResult<Memory> {
        let key = Self::memory_key(namespace, memory_id);
        let memory = {
            let mut memories = self
                .memories
                .lock()
                .map_err(|_| MnemoError::Internal("memory store lock poisoned".to_string()))?;
            let memory = memories
                .get_mut(&key)
                .ok_or_else(|| MnemoError::NotFound("memory not found".to_string()))?;
            memory.apply_patch(patch)?;
            memory.clone()
        };
        self.persist()?;
        Ok(memory)
    }

    fn search_memories(
        &self,
        namespace: &Namespace,
        query: Option<&str>,
        include_inactive: bool,
    ) -> MnemoResult<Vec<Memory>> {
        let namespace_key = namespace.stable_key();
        let query = query.map(str::trim).filter(|value| !value.is_empty());
        let memories = self
            .memories
            .lock()
            .map_err(|_| MnemoError::Internal("memory store lock poisoned".to_string()))?;
        let mut matches = memories
            .values()
            .filter(|memory| memory.namespace().stable_key() == namespace_key)
            .filter(|memory| include_inactive || memory.status() == MemoryStatus::Active)
            .filter(|memory| {
                query.is_none_or(|query| {
                    memory.memory_id().contains(query)
                        || memory.content().contains(query)
                        || memory.memory_type().contains(query)
                        || memory.conflict_key().is_some_and(|key| key.contains(query))
                        || memory
                            .source_event_ids()
                            .iter()
                            .any(|event_id| event_id.contains(query))
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        matches.sort_by(|left, right| {
            right
                .importance()
                .cmp(&left.importance())
                .then_with(|| left.memory_id().cmp(right.memory_id()))
        });
        Ok(matches)
    }

    fn active_memories(&self, namespace: &Namespace) -> MnemoResult<Vec<Memory>> {
        let mut memories = self
            .search_memories(namespace, None, /*include_inactive*/ false)?
            .into_iter()
            .filter(|memory| memory.status().is_context_pack_eligible())
            .collect::<Vec<_>>();
        let usage_scores = self
            .usage_scores
            .lock()
            .map_err(|_| MnemoError::Internal("usage score lock poisoned".to_string()))?;
        memories.sort_by(|left, right| {
            let left_score = usage_scores
                .get(left.memory_id())
                .copied()
                .unwrap_or_default();
            let right_score = usage_scores
                .get(right.memory_id())
                .copied()
                .unwrap_or_default();
            right
                .importance()
                .cmp(&left.importance())
                .then_with(|| right_score.cmp(&left_score))
                .then_with(|| left.memory_id().cmp(right.memory_id()))
        });
        Ok(memories)
    }

    fn next_memory_id(&self) -> MnemoResult<String> {
        Self::seq_id(&self.next_memory_seq, "mem")
    }
}

impl SessionSummaryStore for InMemoryStore {
    fn write_session_summary(&self, summary: SessionSummary) -> MnemoResult<SessionSummary> {
        {
            let mut summaries = self.session_summaries.lock().map_err(|_| {
                MnemoError::Internal("session summary store lock poisoned".to_string())
            })?;
            summaries.insert(summary.session_summary_id().to_string(), summary.clone());
        }
        self.persist()?;
        Ok(summary)
    }

    fn get_session_summary(&self, summary_id: &str) -> MnemoResult<Option<SessionSummary>> {
        let summaries = self
            .session_summaries
            .lock()
            .map_err(|_| MnemoError::Internal("session summary store lock poisoned".to_string()))?;
        Ok(summaries.get(summary_id).cloned())
    }

    fn search_session_summaries(
        &self,
        namespace: &Namespace,
        query: Option<&str>,
    ) -> MnemoResult<Vec<SessionSummary>> {
        let query = query.map(str::trim).filter(|value| !value.is_empty());
        let summaries = self
            .session_summaries
            .lock()
            .map_err(|_| MnemoError::Internal("session summary store lock poisoned".to_string()))?;
        let mut matches = summaries
            .values()
            .filter(|summary| Self::namespace_matches(summary.namespace(), namespace))
            .filter(|summary| {
                query.is_none_or(|query| {
                    summary.session_summary_id().contains(query)
                        || summary.content().contains(query)
                        || summary
                            .source_event_ids()
                            .iter()
                            .any(|event_id| event_id.contains(query))
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        matches.sort_by(|left, right| {
            right
                .generated_at()
                .cmp(left.generated_at())
                .then_with(|| left.session_summary_id().cmp(right.session_summary_id()))
        });
        Ok(matches)
    }

    fn next_session_summary_id(&self) -> MnemoResult<String> {
        Self::seq_id(&self.next_summary_seq, "sum")
    }
}

impl JobStore for InMemoryStore {
    fn write_job(&self, job: Job) -> MnemoResult<Job> {
        {
            let mut jobs = self
                .jobs
                .lock()
                .map_err(|_| MnemoError::Internal("job store lock poisoned".to_string()))?;
            jobs.insert(job.job_id().to_string(), job.clone());
        }
        self.persist()?;
        Ok(job)
    }

    fn update_job(&self, job: Job) -> MnemoResult<Job> {
        {
            let mut jobs = self
                .jobs
                .lock()
                .map_err(|_| MnemoError::Internal("job store lock poisoned".to_string()))?;
            if !jobs.contains_key(job.job_id()) {
                return Err(MnemoError::NotFound("job not found".to_string()));
            }
            jobs.insert(job.job_id().to_string(), job.clone());
        }
        self.persist()?;
        Ok(job)
    }

    fn get_job(&self, job_id: &str) -> MnemoResult<Option<Job>> {
        let jobs = self
            .jobs
            .lock()
            .map_err(|_| MnemoError::Internal("job store lock poisoned".to_string()))?;
        Ok(jobs.get(job_id).cloned())
    }

    fn list_jobs(&self) -> MnemoResult<Vec<Job>> {
        let jobs = self
            .jobs
            .lock()
            .map_err(|_| MnemoError::Internal("job store lock poisoned".to_string()))?;
        let mut values = jobs.values().cloned().collect::<Vec<_>>();
        values.sort_by(|left, right| {
            right
                .created_at()
                .cmp(left.created_at())
                .then_with(|| left.job_id().cmp(right.job_id()))
        });
        Ok(values)
    }

    fn claim_next_runnable_job(
        &self,
        job_type: &str,
        timestamp: String,
        lease_until: Option<String>,
    ) -> MnemoResult<Option<Job>> {
        let claimed = {
            let mut jobs = self
                .jobs
                .lock()
                .map_err(|_| MnemoError::Internal("job store lock poisoned".to_string()))?;
            let next_job_id = jobs
                .values()
                .filter(|job| job_is_runnable(job, job_type, timestamp.as_str()))
                .min_by(|left, right| {
                    left.created_at()
                        .cmp(right.created_at())
                        .then_with(|| left.job_id().cmp(right.job_id()))
                })
                .map(|job| job.job_id().to_string());
            let Some(next_job_id) = next_job_id else {
                return Ok(None);
            };
            let job = jobs
                .get_mut(next_job_id.as_str())
                .ok_or_else(|| MnemoError::NotFound("job not found".to_string()))?;
            job.mark_running_until(timestamp, lease_until)?;
            job.clone()
        };
        self.persist()?;
        Ok(Some(claimed))
    }

    fn next_job_id(&self) -> MnemoResult<String> {
        Self::seq_id(&self.next_job_seq, "job")
    }
}

fn job_is_runnable(job: &Job, job_type: &str, timestamp: &str) -> bool {
    if job.job_type() != job_type {
        return false;
    }
    match job.status() {
        JobStatus::Queued => job
            .retry_at()
            .is_none_or(|retry_at| timestamp_lte(retry_at, timestamp)),
        JobStatus::Running => job
            .lease_until()
            .is_some_and(|lease_until| timestamp_lte(lease_until, timestamp)),
        JobStatus::Succeeded | JobStatus::Failed => false,
    }
}

fn timestamp_lte(left: &str, right: &str) -> bool {
    match (left.parse::<i128>(), right.parse::<i128>()) {
        (Ok(left), Ok(right)) => left <= right,
        _ => left <= right,
    }
}

impl ConflictStore for InMemoryStore {
    fn search_conflicts(&self, namespace: &Namespace) -> MnemoResult<Vec<MemoryConflict>> {
        let namespace_key = namespace.stable_key();
        let memories = self
            .memories
            .lock()
            .map_err(|_| MnemoError::Internal("memory store lock poisoned".to_string()))?;
        let mut grouped = HashMap::<String, Vec<Memory>>::new();
        for memory in memories.values() {
            if memory.namespace().stable_key() != namespace_key {
                continue;
            }
            let Some(conflict_key) = memory.conflict_key() else {
                continue;
            };
            if memory.status() == MemoryStatus::Conflicted
                || memory.status() == MemoryStatus::Active
            {
                grouped
                    .entry(conflict_key.to_string())
                    .or_default()
                    .push(memory.clone());
            }
        }
        let mut conflicts = grouped
            .into_iter()
            .filter_map(|(conflict_key, mut memories)| {
                let active_count = memories
                    .iter()
                    .filter(|memory| memory.status() == MemoryStatus::Active)
                    .count();
                let has_conflicted = memories
                    .iter()
                    .any(|memory| memory.status() == MemoryStatus::Conflicted);
                (active_count > 1 || has_conflicted).then(|| {
                    memories.sort_by(|left, right| left.memory_id().cmp(right.memory_id()));
                    MemoryConflict {
                        namespace: namespace.clone(),
                        conflict_key,
                        memories,
                    }
                })
            })
            .collect::<Vec<_>>();
        conflicts.sort_by(|left, right| left.conflict_key.cmp(&right.conflict_key));
        Ok(conflicts)
    }

    fn resolve_conflict(
        &self,
        namespace: &Namespace,
        conflict_key: &str,
        winner_memory_id: &str,
    ) -> MnemoResult<ConflictResolveOutcome> {
        let namespace_key = namespace.stable_key();
        let mut superseded_memory_ids = Vec::new();
        let winner = {
            let mut memories = self
                .memories
                .lock()
                .map_err(|_| MnemoError::Internal("memory store lock poisoned".to_string()))?;
            let winner_exists = memories.values().any(|memory| {
                memory.namespace().stable_key() == namespace_key
                    && memory.conflict_key() == Some(conflict_key)
                    && memory.memory_id() == winner_memory_id
            });
            if !winner_exists {
                return Err(MnemoError::NotFound(
                    "winner memory not found in conflict slot".to_string(),
                ));
            }

            for memory in memories.values_mut() {
                if memory.namespace().stable_key() != namespace_key
                    || memory.conflict_key() != Some(conflict_key)
                {
                    continue;
                }
                if memory.memory_id() == winner_memory_id {
                    memory.set_status(MemoryStatus::Active);
                } else if matches!(
                    memory.status(),
                    MemoryStatus::Active | MemoryStatus::Conflicted
                ) {
                    memory.mark_superseded_by(winner_memory_id.to_string());
                    superseded_memory_ids.push(memory.memory_id().to_string());
                }
            }

            memories
                .values()
                .find(|memory| {
                    memory.namespace().stable_key() == namespace_key
                        && memory.memory_id() == winner_memory_id
                })
                .cloned()
                .ok_or_else(|| MnemoError::NotFound("winner memory not found".to_string()))?
        };
        self.persist()?;
        Ok(ConflictResolveOutcome {
            winner,
            superseded_memory_ids,
        })
    }
}

impl ForgetStore for InMemoryStore {
    fn forget_memories(
        &self,
        namespace: &Namespace,
        memory_ids: &[String],
        reason: Option<String>,
        created_at: String,
    ) -> MnemoResult<ForgetOutcome> {
        let namespace_key = namespace.stable_key();
        let mut forgotten_memory_ids = Vec::new();
        let mut forgotten_contents = Vec::new();
        {
            let mut memories = self
                .memories
                .lock()
                .map_err(|_| MnemoError::Internal("memory store lock poisoned".to_string()))?;
            for memory in memories.values_mut() {
                if memory.namespace().stable_key() == namespace_key
                    && memory_ids.iter().any(|id| id == memory.memory_id())
                {
                    memory.mark_forgotten();
                    forgotten_memory_ids.push(memory.memory_id().to_string());
                    forgotten_contents.push(memory.content().to_string());
                }
            }
        }

        let tombstone_id = self.next_tombstone_id()?;
        let tombstone = ForgetTombstone::new(
            tombstone_id,
            namespace.clone(),
            forgotten_memory_ids.clone(),
            forgotten_contents,
            reason,
            created_at,
        )?;
        let mut tombstones = self
            .forget_tombstones
            .lock()
            .map_err(|_| MnemoError::Internal("forget store lock poisoned".to_string()))?;
        tombstones.insert(tombstone.tombstone_id().to_string(), tombstone.clone());
        drop(tombstones);
        self.persist()?;

        Ok(ForgetOutcome {
            tombstone,
            forgotten_memory_ids,
        })
    }

    fn is_content_forgotten(&self, namespace: &Namespace, content: &str) -> MnemoResult<bool> {
        let namespace_key = namespace.stable_key();
        let normalized_content = content.trim();
        let tombstones = self
            .forget_tombstones
            .lock()
            .map_err(|_| MnemoError::Internal("forget store lock poisoned".to_string()))?;
        Ok(tombstones.values().any(|tombstone| {
            tombstone.namespace().stable_key() == namespace_key
                && tombstone
                    .contents()
                    .iter()
                    .any(|stored| stored.trim() == normalized_content)
        }))
    }

    fn next_tombstone_id(&self) -> MnemoResult<String> {
        Self::seq_id(&self.next_tombstone_seq, "forget")
    }
}

impl UsageStore for InMemoryStore {
    fn write_usage_report(&self, report: UsageReport) -> MnemoResult<UsageReport> {
        {
            let mut scores = self
                .usage_scores
                .lock()
                .map_err(|_| MnemoError::Internal("usage score lock poisoned".to_string()))?;
            for memory_id in report.memory_ids() {
                *scores.entry(memory_id.to_string()).or_default() += report.signal().score_delta();
            }
        }

        {
            let mut reports = self
                .usage_reports
                .lock()
                .map_err(|_| MnemoError::Internal("usage store lock poisoned".to_string()))?;
            reports.push(report.clone());
        }
        self.persist()?;
        Ok(report)
    }

    fn list_usage_reports(&self) -> MnemoResult<Vec<UsageReport>> {
        let reports = self
            .usage_reports
            .lock()
            .map_err(|_| MnemoError::Internal("usage store lock poisoned".to_string()))?;
        Ok(reports.clone())
    }

    fn search_usage_reports(&self, namespace: Option<&Namespace>) -> MnemoResult<Vec<UsageReport>> {
        let reports = self
            .usage_reports
            .lock()
            .map_err(|_| MnemoError::Internal("usage store lock poisoned".to_string()))?;
        let mut matches = reports
            .iter()
            .filter(|report| {
                namespace.is_none_or(|namespace| {
                    report.namespace().stable_key() == namespace.stable_key()
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        matches.sort_by(|left, right| {
            right
                .reported_at()
                .cmp(left.reported_at())
                .then_with(|| left.usage_id().cmp(right.usage_id()))
        });
        Ok(matches)
    }

    fn next_usage_id(&self) -> MnemoResult<String> {
        Self::seq_id(&self.next_usage_seq, "usage")
    }
}

#[cfg(test)]
mod tests {
    use mnemo_core::EventRole;
    use mnemo_core::EventType;
    use mnemo_core::MemoryHints;
    use mnemo_core::MemoryImportance;
    use mnemo_core::MemoryOrigin;

    use super::*;

    fn namespace() -> Namespace {
        Namespace::new("u1")
            .expect("namespace")
            .with_workspace("w1")
            .with_thread("t1")
            .with_source("chat")
    }

    fn event(content: &str) -> Event {
        Event::new(
            "evt-1",
            namespace(),
            EventType::UserMessage,
            EventRole::User,
            content,
            "2026-05-13T10:00:00+08:00",
        )
        .expect("event")
        .with_memory_hints(MemoryHints {
            eligible: true,
            explicit_memory_intent: false,
            external_context: false,
        })
    }

    #[test]
    fn sqlite_init_schema_covers_core_tables() {
        for table in [
            "CREATE TABLE IF NOT EXISTS events",
            "CREATE TABLE IF NOT EXISTS memories",
            "CREATE TABLE IF NOT EXISTS jobs",
            "CREATE TABLE IF NOT EXISTS session_summaries",
            "CREATE TABLE IF NOT EXISTS context_pack_cache_entries",
            "CREATE TABLE IF NOT EXISTS forget_tombstones",
        ] {
            assert!(
                SQLITE_INIT_SQL.contains(table),
                "schema should contain {table}"
            );
        }
        assert!(SQLITE_INIT_SQL.contains("PRAGMA user_version = 1"));
    }

    #[test]
    fn append_event_is_idempotent_for_same_content() {
        let store = InMemoryStore::new();
        let first = store
            .append_event(event("remember apples"))
            .expect("append");
        let second = store
            .append_event(event("remember apples"))
            .expect("append");

        assert!(!first.deduplicated);
        assert!(second.deduplicated);
    }

    #[test]
    fn append_event_rejects_same_id_with_different_content() {
        let store = InMemoryStore::new();
        store
            .append_event(event("remember apples"))
            .expect("append");

        let err = store
            .append_event(event("remember watermelon"))
            .expect_err("conflict");
        assert_eq!(err.code(), "event_conflict");
    }

    #[test]
    fn memory_mode_does_not_block_event_writes() {
        let store = InMemoryStore::new();
        let ns = namespace();
        store
            .set_memory_mode(&ns, ThreadMemoryMode::Polluted)
            .expect("mode");

        let outcome = store
            .append_event(event("external result"))
            .expect("append");

        assert_eq!(outcome.event_id, "evt-1");
        assert_eq!(
            store.get_memory_mode(&ns).expect("mode"),
            ThreadMemoryMode::Polluted
        );
    }

    #[test]
    fn search_events_filters_by_namespace_and_query() {
        let store = InMemoryStore::new();
        store
            .append_event(event("remember apples"))
            .expect("append");
        let other_namespace_event = Event::new(
            "evt-2",
            Namespace::new("u2").expect("namespace").with_thread("t1"),
            EventType::UserMessage,
            EventRole::User,
            "remember apples",
            "2026-05-13T10:00:00+08:00",
        )
        .expect("event");
        store
            .append_event(other_namespace_event)
            .expect("append other");

        let matches = store
            .search_events(&namespace(), Some("apples"))
            .expect("search");

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].event_id(), "evt-1");
    }

    #[test]
    fn write_memory_supersedes_active_memory_with_same_conflict_key() {
        let store = InMemoryStore::new();
        let ns = namespace();
        let first = Memory::new(
            "mem-1",
            ns.clone(),
            "favorite fruit is watermelon",
            "preference",
            MemoryOrigin::Explicit,
            MemoryImportance::High,
        )
        .expect("memory")
        .with_conflict_key(Some("user.favorite_fruit".to_string()));
        let second = Memory::new(
            "mem-2",
            ns.clone(),
            "favorite fruit is apple",
            "preference",
            MemoryOrigin::Explicit,
            MemoryImportance::High,
        )
        .expect("memory")
        .with_conflict_key(Some("user.favorite_fruit".to_string()));

        store.write_memory(first).expect("write first");
        let outcome = store.write_memory(second).expect("write second");

        assert_eq!(outcome.superseded_memory_ids, vec!["mem-1"]);
        assert_eq!(outcome.memory.supersedes(), &["mem-1".to_string()]);
        let active = store.active_memories(&ns).expect("active");
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].memory_id(), "mem-2");
    }

    #[test]
    fn open_reloads_persisted_snapshot() {
        let path = std::env::temp_dir().join(format!(
            "mnemo-store-test-{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let ns = namespace();
        {
            let store = InMemoryStore::open(path.clone()).expect("open");
            store
                .append_event(event("remember apples"))
                .expect("append");
            let memory = Memory::new(
                "mem-1",
                ns.clone(),
                "favorite fruit is apple",
                "preference",
                MemoryOrigin::Explicit,
                MemoryImportance::High,
            )
            .expect("memory");
            store.write_memory(memory).expect("write memory");
        }

        let reopened = InMemoryStore::open(path.clone()).expect("reopen");
        assert!(
            reopened
                .get_event(&ns, "evt-1")
                .expect("get event")
                .is_some()
        );
        assert_eq!(reopened.active_memories(&ns).expect("active").len(), 1);

        let _ = std::fs::remove_file(path);
    }

    fn assert_job_claim_respects_retry_and_expired_lease(store: &impl JobStore) {
        let ns = namespace();
        let mut job =
            Job::queued("job-1", "session_wrapup", ns, "10", None, Some(true)).expect("job");
        job.set_retry_at(Some("20".to_string()));
        store.write_job(job).expect("write job");

        assert!(
            store
                .claim_next_runnable_job("session_wrapup", "19".to_string(), Some("25".to_string()))
                .expect("claim before retry")
                .is_none()
        );

        let claimed = store
            .claim_next_runnable_job("session_wrapup", "20".to_string(), Some("25".to_string()))
            .expect("claim at retry")
            .expect("claimed");
        assert_eq!(claimed.status(), JobStatus::Running);
        assert_eq!(claimed.attempts(), 1);
        assert_eq!(claimed.retry_at(), None);
        assert_eq!(claimed.lease_until(), Some("25"));

        assert!(
            store
                .claim_next_runnable_job("session_wrapup", "24".to_string(), Some("30".to_string()))
                .expect("claim before lease expiry")
                .is_none()
        );

        let reclaimed = store
            .claim_next_runnable_job("session_wrapup", "25".to_string(), Some("30".to_string()))
            .expect("claim after lease expiry")
            .expect("reclaimed");
        assert_eq!(reclaimed.job_id(), "job-1");
        assert_eq!(reclaimed.status(), JobStatus::Running);
        assert_eq!(reclaimed.attempts(), 2);
        assert_eq!(reclaimed.lease_until(), Some("30"));
    }

    #[test]
    fn in_memory_job_claim_respects_retry_and_expired_lease() {
        assert_job_claim_respects_retry_and_expired_lease(&InMemoryStore::new());
    }

    #[test]
    fn sqlite_job_claim_respects_retry_and_expired_lease() {
        let store = SqliteStore::open_in_memory().expect("sqlite");
        assert_job_claim_respects_retry_and_expired_lease(&store);
    }

    #[test]
    fn sqlite_store_reopens_persisted_data() {
        let path = std::env::temp_dir().join(format!(
            "mnemo-store-test-{}.db",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let ns = namespace();
        {
            let store = SqliteStore::open(path.clone()).expect("open sqlite");
            store
                .append_event(event("remember apples"))
                .expect("append");
            store
                .set_memory_mode(&ns, ThreadMemoryMode::Polluted)
                .expect("mode");
            let memory = Memory::new(
                "mem-1",
                ns.clone(),
                "favorite fruit is apple",
                "preference",
                MemoryOrigin::Explicit,
                MemoryImportance::High,
            )
            .expect("memory");
            store.write_memory(memory).expect("write memory");
        }

        let reopened = SqliteStore::open(path.clone()).expect("reopen sqlite");
        assert!(
            reopened
                .get_event(&ns, "evt-1")
                .expect("get event")
                .is_some()
        );
        assert_eq!(
            reopened.get_memory_mode(&ns).expect("mode"),
            ThreadMemoryMode::Polluted
        );
        assert_eq!(reopened.active_memories(&ns).expect("active").len(), 1);

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }
}
