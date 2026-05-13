#![forbid(unsafe_code)]

pub mod traits;
mod in_memory;
mod sqlite;

pub use traits::{
    ConflictResolveOutcome, ConflictStore, ContextPackCacheStore, EventAppendOutcome, EventStore,
    ForgetOutcome, ForgetStore, JobStore, MemoryConflict, MemoryStore, MemoryWriteOutcome,
    MnemoStore, PolicyStore, SessionSummaryStore, ThreadStateStore, UsageStore,
};
pub use in_memory::InMemoryStore;
pub use sqlite::{SqliteStore, init_sqlite_database};

pub const SQLITE_INIT_SQL: &str = include_str!("../migrations/0001_init.sql");

#[cfg(test)]
mod tests {
    use mnemo_core::Event;
    use mnemo_core::EventRole;
    use mnemo_core::EventType;
    use mnemo_core::Job;
    use mnemo_core::JobStatus;
    use mnemo_core::Memory;
    use mnemo_core::MemoryHints;
    use mnemo_core::MemoryImportance;
    use mnemo_core::MemoryOrigin;
    use mnemo_core::Namespace;
    use mnemo_core::ThreadMemoryMode;

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
