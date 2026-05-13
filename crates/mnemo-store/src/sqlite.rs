use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::MutexGuard;

use mnemo_core::ContextPackCacheEntry;
use mnemo_core::Event;
use mnemo_core::EventRole;
use mnemo_core::EventType;
use mnemo_core::ForgetTombstone;
use mnemo_core::Job;
use mnemo_core::JobStatus;
use mnemo_core::Memory;
use mnemo_core::MemoryHints;
use mnemo_core::MemoryImportance;
use mnemo_core::MemoryOrigin;
use mnemo_core::MemoryPatch;
use mnemo_core::MemoryStatus;
use mnemo_core::MnemoError;
use mnemo_core::MnemoResult;
use mnemo_core::Namespace;
use mnemo_core::NamespacePolicy;
use mnemo_core::SessionSummary;
use mnemo_core::ThreadMemoryMode;
use mnemo_core::UsageReport;
use mnemo_core::UsageSignal;
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use rusqlite::Row;
use rusqlite::params;
use rusqlite::types::Type;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::traits::{
    ConflictResolveOutcome, ConflictStore, ContextPackCacheStore, EventAppendOutcome, EventStore,
    ForgetOutcome, ForgetStore, JobStore, MemoryConflict, MemoryStore, MemoryWriteOutcome,
    PolicyStore, SessionSummaryStore, ThreadStateStore, UsageStore,
};
use crate::SQLITE_INIT_SQL;

#[derive(Debug)]
pub struct SqliteStore {
    connection: Mutex<Connection>,
}

impl SqliteStore {
    pub fn open(path: impl Into<PathBuf>) -> MnemoResult<Self> {
        let path = path.into();
        let connection = open_initialized_connection(path.as_path())?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn open_in_memory() -> MnemoResult<Self> {
        let connection = Connection::open_in_memory()
            .map_err(|err| sqlite_error("failed to open in-memory sqlite db", err))?;
        connection
            .execute_batch(SQLITE_INIT_SQL)
            .map_err(|err| sqlite_error("failed to initialize sqlite schema", err))?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    fn connection(&self) -> MnemoResult<MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| MnemoError::Internal("sqlite connection lock poisoned".to_string()))
    }
}

pub fn init_sqlite_database(path: impl AsRef<Path>) -> MnemoResult<i64> {
    let connection = open_initialized_connection(path.as_ref())?;
    connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|err| sqlite_error("failed to read sqlite user_version", err))
}

fn open_initialized_connection(path: &Path) -> MnemoResult<Connection> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .map_err(|err| MnemoError::Internal(format!("failed to create db directory: {err}")))?;
    }
    let connection = Connection::open(path)
        .map_err(|err| sqlite_error("failed to open sqlite database", err))?;
    connection
        .execute_batch(SQLITE_INIT_SQL)
        .map_err(|err| sqlite_error("failed to initialize sqlite schema", err))?;
    ensure_sqlite_schema(&connection)?;
    Ok(connection)
}

fn ensure_sqlite_schema(connection: &Connection) -> MnemoResult<()> {
    add_column_if_missing(connection, "jobs", "retry_at", "TEXT")?;
    add_column_if_missing(connection, "jobs", "lease_until", "TEXT")?;
    Ok(())
}

fn add_column_if_missing(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> MnemoResult<()> {
    let mut stmt = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|err| sqlite_error("failed to inspect sqlite table", err))?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|err| sqlite_error("failed to query sqlite table info", err))?;
    for row in rows {
        let existing = row.map_err(|err| sqlite_error("failed to read sqlite table info", err))?;
        if existing == column {
            return Ok(());
        }
    }
    connection
        .execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )
        .map_err(|err| sqlite_error("failed to alter sqlite table", err))?;
    Ok(())
}

fn sqlite_error(context: &str, err: rusqlite::Error) -> MnemoError {
    MnemoError::Internal(format!("{context}: {err}"))
}

fn to_sql_error(index: usize, err: MnemoError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(index, Type::Text, Box::new(err))
}

fn namespace_from_parts(
    tenant_id: String,
    user_id: String,
    workspace_id: Option<String>,
    thread_id: Option<String>,
    agent_id: Option<String>,
    source: Option<String>,
) -> MnemoResult<Namespace> {
    let mut namespace = Namespace::new(user_id)?.with_tenant(tenant_id);
    if let Some(workspace_id) = workspace_id {
        namespace = namespace.with_workspace(workspace_id);
    }
    if let Some(thread_id) = thread_id {
        namespace = namespace.with_thread(thread_id);
    }
    if let Some(agent_id) = agent_id {
        namespace = namespace.with_agent(agent_id);
    }
    if let Some(source) = source {
        namespace = namespace.with_source(source);
    }
    Ok(namespace)
}

fn row_namespace(row: &Row<'_>, start: usize) -> rusqlite::Result<Namespace> {
    namespace_from_parts(
        row.get(start)?,
        row.get(start + 1)?,
        row.get(start + 2)?,
        row.get(start + 3)?,
        row.get(start + 4)?,
        row.get(start + 5)?,
    )
    .map_err(|err| to_sql_error(start + 1, err))
}

fn thread_state_key(namespace: &Namespace) -> MnemoResult<String> {
    let thread_id = namespace
        .thread_id()
        .ok_or_else(|| MnemoError::InvalidRequest("namespace.thread_id is required".to_string()))?;
    Ok(format!(
        "{}\u{1e}{}\u{1e}{}",
        namespace.tenant_id(),
        namespace.user_id(),
        thread_id
    ))
}

fn bool_to_i64(value: bool) -> i64 {
    if value { 1 } else { 0 }
}

fn i64_to_bool(value: i64) -> bool {
    value != 0
}

fn opt_bool_to_i64(value: Option<bool>) -> Option<i64> {
    value.map(i64::from)
}

fn opt_i64_to_bool(value: Option<i64>) -> Option<bool> {
    value.map(i64_to_bool)
}

fn usize_to_i64(value: usize) -> MnemoResult<i64> {
    i64::try_from(value)
        .map_err(|_| MnemoError::InvalidRequest("numeric value is too large".to_string()))
}

fn i64_to_usize(value: i64, field: &str) -> MnemoResult<usize> {
    usize::try_from(value)
        .map_err(|_| MnemoError::Internal(format!("sqlite {field} value is out of range")))
}

fn encode_json<T: Serialize>(value: &T) -> MnemoResult<String> {
    serde_json::to_string(value)
        .map_err(|err| MnemoError::Internal(format!("failed to encode json: {err}")))
}

fn decode_json<T: DeserializeOwned>(value: &str) -> MnemoResult<T> {
    serde_json::from_str(value)
        .map_err(|err| MnemoError::Internal(format!("failed to decode json: {err}")))
}

fn row_event(row: &Row<'_>) -> rusqlite::Result<Event> {
    let namespace = row_namespace(row, 1)?;
    let event_type_text: String = row.get(7)?;
    let role_text: String = row.get(8)?;
    let event_type =
        EventType::parse(event_type_text.as_str()).map_err(|err| to_sql_error(7, err))?;
    let role = EventRole::parse(role_text.as_str()).map_err(|err| to_sql_error(8, err))?;
    let eligible: i64 = row.get(11)?;
    let explicit_memory_intent: i64 = row.get(12)?;
    let external_context: i64 = row.get(13)?;
    Event::new(
        row.get::<_, String>(0)?,
        namespace,
        event_type,
        role,
        row.get::<_, String>(9)?,
        row.get::<_, String>(10)?,
    )
    .map(|event| {
        event.with_memory_hints(MemoryHints {
            eligible: i64_to_bool(eligible),
            explicit_memory_intent: i64_to_bool(explicit_memory_intent),
            external_context: i64_to_bool(external_context),
        })
    })
    .map_err(|err| to_sql_error(0, err))
}

fn row_memory(row: &Row<'_>) -> rusqlite::Result<Memory> {
    let memory_id: String = row.get(0)?;
    let namespace = row_namespace(row, 1)?;
    let origin_text: String = row.get(10)?;
    let status_text: String = row.get(11)?;
    let importance_text: String = row.get(12)?;
    let origin = MemoryOrigin::parse(origin_text.as_str()).map_err(|err| to_sql_error(10, err))?;
    let status = MemoryStatus::parse(status_text.as_str()).map_err(|err| to_sql_error(11, err))?;
    let importance =
        MemoryImportance::parse(importance_text.as_str()).map_err(|err| to_sql_error(12, err))?;
    let supersedes_json: String = row.get(15)?;
    let superseded_by_json: String = row.get(16)?;
    let source_event_ids_json: String = row.get(17)?;
    let supersedes = decode_json::<Vec<String>>(supersedes_json.as_str())
        .map_err(|err| to_sql_error(15, err))?;
    let superseded_by = decode_json::<Vec<String>>(superseded_by_json.as_str())
        .map_err(|err| to_sql_error(16, err))?;
    let source_event_ids = decode_json::<Vec<String>>(source_event_ids_json.as_str())
        .map_err(|err| to_sql_error(17, err))?;

    let mut memory = Memory::new(
        memory_id,
        namespace,
        row.get::<_, String>(8)?,
        row.get::<_, String>(9)?,
        origin,
        importance,
    )
    .map_err(|err| to_sql_error(0, err))?
    .with_conflict_key(row.get(13)?)
    .with_valid_from(row.get(14)?)
    .with_source_event_ids(source_event_ids);
    memory.set_status(status);
    for memory_id in supersedes {
        memory.add_supersedes(memory_id);
    }
    for memory_id in superseded_by {
        memory.add_superseded_by(memory_id);
    }
    Ok(memory)
}

fn memory_columns() -> &'static str {
    "memory_id, tenant_id, user_id, workspace_id, thread_id, agent_id, source, namespace_key, content, memory_type, origin, status, importance, conflict_key, valid_from, supersedes_json, superseded_by_json, source_event_ids_json"
}

fn upsert_memory(connection: &Connection, memory: &Memory) -> MnemoResult<()> {
    let supersedes_json = encode_json(&memory.supersedes())?;
    let superseded_by_json = encode_json(&memory.superseded_by())?;
    let source_event_ids_json = encode_json(&memory.source_event_ids())?;
    connection
        .execute(
            "INSERT INTO memories (
                memory_id, namespace_key, tenant_id, user_id, workspace_id, thread_id, agent_id, source,
                content, memory_type, origin, status, importance, conflict_key, valid_from,
                supersedes_json, superseded_by_json, source_event_ids_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
            ON CONFLICT(memory_id) DO UPDATE SET
                namespace_key=excluded.namespace_key,
                tenant_id=excluded.tenant_id,
                user_id=excluded.user_id,
                workspace_id=excluded.workspace_id,
                thread_id=excluded.thread_id,
                agent_id=excluded.agent_id,
                source=excluded.source,
                content=excluded.content,
                memory_type=excluded.memory_type,
                origin=excluded.origin,
                status=excluded.status,
                importance=excluded.importance,
                conflict_key=excluded.conflict_key,
                valid_from=excluded.valid_from,
                supersedes_json=excluded.supersedes_json,
                superseded_by_json=excluded.superseded_by_json,
                source_event_ids_json=excluded.source_event_ids_json,
                updated_at=strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
            params![
                memory.memory_id(),
                memory.namespace().stable_key(),
                memory.namespace().tenant_id(),
                memory.namespace().user_id(),
                memory.namespace().workspace_id(),
                memory.namespace().thread_id(),
                memory.namespace().agent_id(),
                memory.namespace().source(),
                memory.content(),
                memory.memory_type(),
                memory.origin().as_str(),
                memory.status().as_str(),
                memory.importance().as_str(),
                memory.conflict_key(),
                memory.valid_from(),
                supersedes_json,
                superseded_by_json,
                source_event_ids_json,
            ],
        )
        .map_err(|err| sqlite_error("failed to upsert memory", err))?;
    Ok(())
}

fn select_memory_by_id(connection: &Connection, memory_id: &str) -> MnemoResult<Option<Memory>> {
    let sql = format!(
        "SELECT {} FROM memories WHERE memory_id = ?1",
        memory_columns()
    );
    connection
        .query_row(sql.as_str(), params![memory_id], row_memory)
        .optional()
        .map_err(|err| sqlite_error("failed to load memory", err))
}

fn select_memory_by_namespace(
    connection: &Connection,
    namespace: &Namespace,
    memory_id: &str,
) -> MnemoResult<Option<Memory>> {
    let sql = format!(
        "SELECT {} FROM memories WHERE namespace_key = ?1 AND memory_id = ?2",
        memory_columns()
    );
    connection
        .query_row(
            sql.as_str(),
            params![namespace.stable_key(), memory_id],
            row_memory,
        )
        .optional()
        .map_err(|err| sqlite_error("failed to load memory", err))
}

fn row_summary(row: &Row<'_>) -> rusqlite::Result<SessionSummary> {
    let namespace = row_namespace(row, 1)?;
    let source_event_ids_json: String = row.get(9)?;
    let inferred_memory_ids_json: String = row.get(10)?;
    let source_event_ids = decode_json::<Vec<String>>(source_event_ids_json.as_str())
        .map_err(|err| to_sql_error(9, err))?;
    let inferred_memory_ids = decode_json::<Vec<String>>(inferred_memory_ids_json.as_str())
        .map_err(|err| to_sql_error(10, err))?;
    SessionSummary::new(
        row.get::<_, String>(0)?,
        namespace,
        row.get::<_, String>(8)?,
        source_event_ids,
        row.get::<_, String>(11)?,
    )
    .map(|summary| summary.with_inferred_memory_ids(inferred_memory_ids))
    .map_err(|err| to_sql_error(0, err))
}

fn row_job(row: &Row<'_>) -> rusqlite::Result<Job> {
    let namespace = row_namespace(row, 3)?;
    let status_text: String = row.get(2)?;
    let status = JobStatus::parse(status_text.as_str()).map_err(|err| to_sql_error(2, err))?;
    let generate_memories: Option<i64> = row.get(13)?;
    let attempts: i64 = row.get(14)?;
    Job::from_stored_parts(
        row.get::<_, String>(0)?,
        row.get::<_, String>(1)?,
        status,
        namespace,
        row.get::<_, String>(11)?,
        row.get::<_, String>(12)?,
        row.get(10)?,
        opt_i64_to_bool(generate_memories),
        u64::try_from(attempts).unwrap_or_default(),
        row.get(15)?,
        row.get(16)?,
        row.get(17)?,
        row.get(18)?,
    )
    .map_err(|err| to_sql_error(0, err))
}

fn job_columns() -> &'static str {
    "job_id, job_type, status, tenant_id, user_id, workspace_id, thread_id, agent_id, source, namespace_key, query, created_at, updated_at, generate_memories, attempts, retry_at, lease_until, error, output_summary_id"
}

fn upsert_job(connection: &Connection, job: &Job) -> MnemoResult<()> {
    connection
        .execute(
            "INSERT INTO jobs (
                job_id, job_type, status, namespace_key, tenant_id, user_id, workspace_id, thread_id,
                agent_id, source, query, generate_memories, attempts, error, output_summary_id,
                retry_at, lease_until, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
            ON CONFLICT(job_id) DO UPDATE SET
                job_type=excluded.job_type,
                status=excluded.status,
                namespace_key=excluded.namespace_key,
                tenant_id=excluded.tenant_id,
                user_id=excluded.user_id,
                workspace_id=excluded.workspace_id,
                thread_id=excluded.thread_id,
                agent_id=excluded.agent_id,
                source=excluded.source,
                query=excluded.query,
                generate_memories=excluded.generate_memories,
                attempts=excluded.attempts,
                error=excluded.error,
                output_summary_id=excluded.output_summary_id,
                retry_at=excluded.retry_at,
                lease_until=excluded.lease_until,
                updated_at=excluded.updated_at",
            params![
                job.job_id(),
                job.job_type(),
                job.status().as_str(),
                job.namespace().stable_key(),
                job.namespace().tenant_id(),
                job.namespace().user_id(),
                job.namespace().workspace_id(),
                job.namespace().thread_id(),
                job.namespace().agent_id(),
                job.namespace().source(),
                job.query(),
                opt_bool_to_i64(job.generate_memories()),
                i64::try_from(job.attempts()).unwrap_or(i64::MAX),
                job.error(),
                job.output_summary_id(),
                job.retry_at(),
                job.lease_until(),
                job.created_at(),
                job.updated_at(),
            ],
        )
        .map_err(|err| sqlite_error("failed to upsert job", err))?;
    Ok(())
}

fn row_usage_report(row: &Row<'_>) -> rusqlite::Result<UsageReport> {
    let namespace = row_namespace(row, 1)?;
    let signal_text: String = row.get(9)?;
    let signal = UsageSignal::parse(signal_text.as_str()).map_err(|err| to_sql_error(9, err))?;
    let memory_ids_json: String = row.get(10)?;
    let memory_ids = decode_json::<Vec<String>>(memory_ids_json.as_str())
        .map_err(|err| to_sql_error(10, err))?;
    UsageReport::new(
        row.get::<_, String>(0)?,
        namespace,
        row.get::<_, String>(8)?,
        signal,
        memory_ids,
        row.get(11)?,
        row.get::<_, String>(12)?,
    )
    .map_err(|err| to_sql_error(0, err))
}

fn usage_score(connection: &Connection, memory_id: &str) -> MnemoResult<i64> {
    let mut stmt = connection
        .prepare("SELECT signal, memory_ids_json FROM usage_reports")
        .map_err(|err| sqlite_error("failed to prepare usage score query", err))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|err| sqlite_error("failed to query usage scores", err))?;
    let mut score = 0;
    for row in rows {
        let (signal, memory_ids_json) =
            row.map_err(|err| sqlite_error("failed to read usage score row", err))?;
        let memory_ids = decode_json::<Vec<String>>(memory_ids_json.as_str())?;
        if memory_ids.iter().any(|id| id == memory_id) {
            score += UsageSignal::parse(signal.as_str())?.score_delta();
        }
    }
    Ok(score)
}

fn next_id(
    connection: &Connection,
    table: &str,
    column: &str,
    prefix: &str,
) -> MnemoResult<String> {
    let sql = format!(
        "SELECT {column} FROM {table} WHERE {column} LIKE ?1 ORDER BY {column} DESC LIMIT 1"
    );
    let like = format!("{prefix}-%");
    let last = connection
        .query_row(sql.as_str(), params![like], |row| row.get::<_, String>(0))
        .optional()
        .map_err(|err| sqlite_error("failed to query next id", err))?;
    let next = last
        .as_deref()
        .and_then(|value| value.strip_prefix(prefix))
        .and_then(|value| value.strip_prefix('-'))
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or_default()
        + 1;
    Ok(format!("{prefix}-{next:06}"))
}

impl EventStore for SqliteStore {
    fn append_event(&self, event: Event) -> MnemoResult<EventAppendOutcome> {
        let connection = self.connection()?;
        let namespace_key = event.namespace().stable_key();
        let fingerprint = event.conflict_fingerprint();
        let existing = connection
            .query_row(
                "SELECT fingerprint FROM events WHERE namespace_key = ?1 AND event_id = ?2",
                params![namespace_key, event.event_id()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|err| sqlite_error("failed to check existing event", err))?;
        if let Some(existing) = existing {
            if existing == fingerprint {
                return Ok(EventAppendOutcome {
                    event_id: event.event_id().to_string(),
                    deduplicated: true,
                });
            }
            return Err(MnemoError::EventConflict(format!(
                "event_id {} already exists with different content",
                event.event_id()
            )));
        }

        let hints = event.memory_hints();
        connection
            .execute(
                "INSERT INTO events (
                    namespace_key, event_id, tenant_id, user_id, workspace_id, thread_id, agent_id,
                    source, event_type, role, content, occurred_at, eligible,
                    explicit_memory_intent, external_context, fingerprint
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                params![
                    namespace_key,
                    event.event_id(),
                    event.namespace().tenant_id(),
                    event.namespace().user_id(),
                    event.namespace().workspace_id(),
                    event.namespace().thread_id(),
                    event.namespace().agent_id(),
                    event.namespace().source(),
                    event.event_type().as_str(),
                    event.role().as_str(),
                    event.content(),
                    event.occurred_at(),
                    bool_to_i64(hints.eligible),
                    bool_to_i64(hints.explicit_memory_intent),
                    bool_to_i64(hints.external_context),
                    fingerprint,
                ],
            )
            .map_err(|err| sqlite_error("failed to insert event", err))?;
        Ok(EventAppendOutcome {
            event_id: event.event_id().to_string(),
            deduplicated: false,
        })
    }

    fn get_event(&self, namespace: &Namespace, event_id: &str) -> MnemoResult<Option<Event>> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT event_id, tenant_id, user_id, workspace_id, thread_id, agent_id, source,
                    event_type, role, content, occurred_at, eligible, explicit_memory_intent, external_context
                 FROM events WHERE namespace_key = ?1 AND event_id = ?2",
                params![namespace.stable_key(), event_id],
                row_event,
            )
            .optional()
            .map_err(|err| sqlite_error("failed to load event", err))
    }

    fn search_events(&self, namespace: &Namespace, query: Option<&str>) -> MnemoResult<Vec<Event>> {
        let connection = self.connection()?;
        let mut stmt = connection
            .prepare(
                "SELECT event_id, tenant_id, user_id, workspace_id, thread_id, agent_id, source,
                    event_type, role, content, occurred_at, eligible, explicit_memory_intent, external_context
                 FROM events WHERE namespace_key = ?1 ORDER BY occurred_at ASC, event_id ASC",
            )
            .map_err(|err| sqlite_error("failed to prepare event search", err))?;
        let rows = stmt
            .query_map(params![namespace.stable_key()], row_event)
            .map_err(|err| sqlite_error("failed to query events", err))?;
        let query = query.map(str::trim).filter(|value| !value.is_empty());
        let mut events = Vec::new();
        for row in rows {
            let event = row.map_err(|err| sqlite_error("failed to read event row", err))?;
            if query.is_none_or(|query| {
                event.event_id().contains(query)
                    || event.content().contains(query)
                    || event.event_type().as_str().contains(query)
                    || event.role().as_str().contains(query)
            }) {
                events.push(event);
            }
        }
        Ok(events)
    }
}

impl ThreadStateStore for SqliteStore {
    fn set_memory_mode(
        &self,
        namespace: &Namespace,
        mode: ThreadMemoryMode,
    ) -> MnemoResult<ThreadMemoryMode> {
        let connection = self.connection()?;
        let key = thread_state_key(namespace)?;
        let old_mode = connection
            .query_row(
                "SELECT memory_mode FROM thread_states WHERE namespace_key = ?1",
                params![key],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|err| sqlite_error("failed to query thread state", err))?;
        connection
            .execute(
                "INSERT INTO thread_states (
                    namespace_key, tenant_id, user_id, workspace_id, thread_id, agent_id, source, memory_mode
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(namespace_key) DO UPDATE SET
                    memory_mode=excluded.memory_mode,
                    updated_at=strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
                params![
                    key,
                    namespace.tenant_id(),
                    namespace.user_id(),
                    namespace.workspace_id(),
                    namespace.thread_id(),
                    namespace.agent_id(),
                    namespace.source(),
                    mode.as_str(),
                ],
            )
            .map_err(|err| sqlite_error("failed to upsert thread state", err))?;
        connection
            .execute(
                "INSERT INTO thread_state_changes(namespace_key, old_memory_mode, new_memory_mode)
                 VALUES (?1, ?2, ?3)",
                params![thread_state_key(namespace)?, old_mode, mode.as_str()],
            )
            .map_err(|err| sqlite_error("failed to insert thread state change", err))?;
        Ok(mode)
    }

    fn get_memory_mode(&self, namespace: &Namespace) -> MnemoResult<ThreadMemoryMode> {
        let connection = self.connection()?;
        let key = thread_state_key(namespace)?;
        let mode = connection
            .query_row(
                "SELECT memory_mode FROM thread_states WHERE namespace_key = ?1",
                params![key],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|err| sqlite_error("failed to load thread state", err))?;
        mode.as_deref()
            .map(ThreadMemoryMode::parse)
            .transpose()
            .map(|mode| mode.unwrap_or(ThreadMemoryMode::Enabled))
    }
}

impl PolicyStore for SqliteStore {
    fn get_policy(&self, namespace: &Namespace) -> MnemoResult<NamespacePolicy> {
        let connection = self.connection()?;
        let row = connection
            .query_row(
                "SELECT auto_generate_memories, auto_use_memories, context_pack_max_tokens,
                    external_context_policy, conflict_resolution_mode, max_unused_days,
                    max_thread_age_days, min_thread_idle_seconds
                 FROM namespace_policies WHERE namespace_key = ?1",
                params![namespace.stable_key()],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<i64>>(5)?,
                        row.get::<_, Option<i64>>(6)?,
                        row.get::<_, Option<i64>>(7)?,
                    ))
                },
            )
            .optional()
            .map_err(|err| sqlite_error("failed to load namespace policy", err))?;
        let Some(row) = row else {
            return Ok(NamespacePolicy::default_for(namespace.clone()));
        };
        let mut policy = NamespacePolicy::default_for(namespace.clone());
        policy.set_auto_generate_memories(i64_to_bool(row.0));
        policy.set_auto_use_memories(i64_to_bool(row.1));
        policy.set_context_pack_max_tokens(i64_to_usize(row.2, "context_pack_max_tokens")?)?;
        policy.set_external_context_policy(row.3)?;
        policy.set_conflict_resolution_mode(row.4)?;
        policy.set_max_unused_days(row.5.and_then(|value| u64::try_from(value).ok()));
        policy.set_max_thread_age_days(row.6.and_then(|value| u64::try_from(value).ok()));
        policy.set_min_thread_idle_seconds(row.7.and_then(|value| u64::try_from(value).ok()));
        Ok(policy)
    }

    fn set_policy(&self, policy: NamespacePolicy) -> MnemoResult<NamespacePolicy> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO namespace_policies (
                    namespace_key, tenant_id, user_id, workspace_id, thread_id, agent_id, source,
                    auto_generate_memories, auto_use_memories, context_pack_max_tokens,
                    external_context_policy, conflict_resolution_mode, max_unused_days,
                    max_thread_age_days, min_thread_idle_seconds
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
                 ON CONFLICT(namespace_key) DO UPDATE SET
                    auto_generate_memories=excluded.auto_generate_memories,
                    auto_use_memories=excluded.auto_use_memories,
                    context_pack_max_tokens=excluded.context_pack_max_tokens,
                    external_context_policy=excluded.external_context_policy,
                    conflict_resolution_mode=excluded.conflict_resolution_mode,
                    max_unused_days=excluded.max_unused_days,
                    max_thread_age_days=excluded.max_thread_age_days,
                    min_thread_idle_seconds=excluded.min_thread_idle_seconds,
                    updated_at=strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
                params![
                    policy.namespace().stable_key(),
                    policy.namespace().tenant_id(),
                    policy.namespace().user_id(),
                    policy.namespace().workspace_id(),
                    policy.namespace().thread_id(),
                    policy.namespace().agent_id(),
                    policy.namespace().source(),
                    bool_to_i64(policy.auto_generate_memories()),
                    bool_to_i64(policy.auto_use_memories()),
                    usize_to_i64(policy.context_pack_max_tokens())?,
                    policy.external_context_policy(),
                    policy.conflict_resolution_mode(),
                    policy
                        .max_unused_days()
                        .and_then(|value| i64::try_from(value).ok()),
                    policy
                        .max_thread_age_days()
                        .and_then(|value| i64::try_from(value).ok()),
                    policy
                        .min_thread_idle_seconds()
                        .and_then(|value| i64::try_from(value).ok()),
                ],
            )
            .map_err(|err| sqlite_error("failed to upsert namespace policy", err))?;
        Ok(policy)
    }
}

impl ContextPackCacheStore for SqliteStore {
    fn get_context_pack_cache(
        &self,
        cache_key: &str,
    ) -> MnemoResult<Option<ContextPackCacheEntry>> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT cache_key, context_pack_id, version, generated_at, content, max_tokens,
                    estimated_tokens, budget_exceeded_items, conflicted_items, items_json
                 FROM context_pack_cache_entries WHERE cache_key = ?1",
                params![cache_key],
                |row| {
                    let items_json: String = row.get(9)?;
                    let items =
                        decode_json(items_json.as_str()).map_err(|err| to_sql_error(9, err))?;
                    let max_tokens = i64_to_usize(row.get::<_, i64>(5)?, "max_tokens")
                        .map_err(|err| to_sql_error(5, err))?;
                    let estimated_tokens = i64_to_usize(row.get::<_, i64>(6)?, "estimated_tokens")
                        .map_err(|err| to_sql_error(6, err))?;
                    let budget_exceeded_items =
                        i64_to_usize(row.get::<_, i64>(7)?, "budget_exceeded_items")
                            .map_err(|err| to_sql_error(7, err))?;
                    let conflicted_items = i64_to_usize(row.get::<_, i64>(8)?, "conflicted_items")
                        .map_err(|err| to_sql_error(8, err))?;
                    ContextPackCacheEntry::new(
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        max_tokens,
                        estimated_tokens,
                        budget_exceeded_items,
                        conflicted_items,
                        items,
                    )
                    .map_err(|err| to_sql_error(0, err))
                },
            )
            .optional()
            .map_err(|err| sqlite_error("failed to load context pack cache", err))
    }

    fn put_context_pack_cache(
        &self,
        entry: ContextPackCacheEntry,
    ) -> MnemoResult<ContextPackCacheEntry> {
        let connection = self.connection()?;
        let items_json = encode_json(&entry.items())?;
        connection
            .execute(
                "INSERT INTO context_pack_cache_entries (
                    cache_key, context_pack_id, version, generated_at, content, max_tokens,
                    estimated_tokens, budget_exceeded_items, conflicted_items, items_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(cache_key) DO UPDATE SET
                    context_pack_id=excluded.context_pack_id,
                    version=excluded.version,
                    generated_at=excluded.generated_at,
                    content=excluded.content,
                    max_tokens=excluded.max_tokens,
                    estimated_tokens=excluded.estimated_tokens,
                    budget_exceeded_items=excluded.budget_exceeded_items,
                    conflicted_items=excluded.conflicted_items,
                    items_json=excluded.items_json",
                params![
                    entry.cache_key(),
                    entry.context_pack_id(),
                    entry.version(),
                    entry.generated_at(),
                    entry.content(),
                    usize_to_i64(entry.max_tokens())?,
                    usize_to_i64(entry.estimated_tokens())?,
                    usize_to_i64(entry.budget_exceeded_items())?,
                    usize_to_i64(entry.conflicted_items())?,
                    items_json,
                ],
            )
            .map_err(|err| sqlite_error("failed to upsert context pack cache", err))?;
        Ok(entry)
    }
}

impl MemoryStore for SqliteStore {
    fn write_memory(&self, mut memory: Memory) -> MnemoResult<MemoryWriteOutcome> {
        let connection = self.connection()?;
        let namespace_key = memory.namespace().stable_key();
        let mut superseded_memory_ids = Vec::new();
        if let Some(conflict_key) = memory.conflict_key() {
            let sql = format!(
                "SELECT {} FROM memories WHERE namespace_key = ?1 AND conflict_key = ?2 AND status = 'active'",
                memory_columns()
            );
            let mut stmt = connection
                .prepare(sql.as_str())
                .map_err(|err| sqlite_error("failed to prepare supersession query", err))?;
            let rows = stmt
                .query_map(params![namespace_key, conflict_key], row_memory)
                .map_err(|err| sqlite_error("failed to query superseded memories", err))?;
            let mut existing_memories = Vec::new();
            for row in rows {
                existing_memories.push(
                    row.map_err(|err| sqlite_error("failed to read superseded memory", err))?,
                );
            }
            drop(stmt);
            for mut existing in existing_memories {
                let existing_id = existing.memory_id().to_string();
                existing.mark_superseded_by(memory.memory_id().to_string());
                memory.add_supersedes(existing_id.clone());
                upsert_memory(&connection, &existing)?;
                superseded_memory_ids.push(existing_id);
            }
        }
        upsert_memory(&connection, &memory)?;
        Ok(MemoryWriteOutcome {
            memory,
            superseded_memory_ids,
        })
    }

    fn get_memory(&self, namespace: &Namespace, memory_id: &str) -> MnemoResult<Option<Memory>> {
        let connection = self.connection()?;
        select_memory_by_namespace(&connection, namespace, memory_id)
    }

    fn find_memory(&self, memory_id: &str) -> MnemoResult<Option<Memory>> {
        let connection = self.connection()?;
        select_memory_by_id(&connection, memory_id)
    }

    fn patch_memory(
        &self,
        namespace: &Namespace,
        memory_id: &str,
        patch: MemoryPatch,
    ) -> MnemoResult<Memory> {
        let connection = self.connection()?;
        let mut memory = select_memory_by_namespace(&connection, namespace, memory_id)?
            .ok_or_else(|| MnemoError::NotFound("memory not found".to_string()))?;
        memory.apply_patch(patch)?;
        upsert_memory(&connection, &memory)?;
        Ok(memory)
    }

    fn search_memories(
        &self,
        namespace: &Namespace,
        query: Option<&str>,
        include_inactive: bool,
    ) -> MnemoResult<Vec<Memory>> {
        let connection = self.connection()?;
        let sql = format!(
            "SELECT {} FROM memories WHERE namespace_key = ?1",
            memory_columns()
        );
        let mut stmt = connection
            .prepare(sql.as_str())
            .map_err(|err| sqlite_error("failed to prepare memory search", err))?;
        let rows = stmt
            .query_map(params![namespace.stable_key()], row_memory)
            .map_err(|err| sqlite_error("failed to query memories", err))?;
        let query = query.map(str::trim).filter(|value| !value.is_empty());
        let mut memories = Vec::new();
        for row in rows {
            let memory = row.map_err(|err| sqlite_error("failed to read memory row", err))?;
            if !include_inactive && memory.status() != MemoryStatus::Active {
                continue;
            }
            if query.is_none_or(|query| {
                memory.memory_id().contains(query)
                    || memory.content().contains(query)
                    || memory.memory_type().contains(query)
                    || memory.conflict_key().is_some_and(|key| key.contains(query))
                    || memory
                        .source_event_ids()
                        .iter()
                        .any(|id| id.contains(query))
            }) {
                memories.push(memory);
            }
        }
        memories.sort_by(|left, right| {
            right
                .importance()
                .cmp(&left.importance())
                .then_with(|| left.memory_id().cmp(right.memory_id()))
        });
        Ok(memories)
    }

    fn active_memories(&self, namespace: &Namespace) -> MnemoResult<Vec<Memory>> {
        let mut memories = self
            .search_memories(namespace, None, false)?
            .into_iter()
            .filter(|memory| memory.status().is_context_pack_eligible())
            .collect::<Vec<_>>();
        let connection = self.connection()?;
        let mut scores = HashMap::<String, i64>::new();
        for memory in &memories {
            scores.insert(
                memory.memory_id().to_string(),
                usage_score(&connection, memory.memory_id())?,
            );
        }
        memories.sort_by(|left, right| {
            let left_score = scores.get(left.memory_id()).copied().unwrap_or_default();
            let right_score = scores.get(right.memory_id()).copied().unwrap_or_default();
            right
                .importance()
                .cmp(&left.importance())
                .then_with(|| right_score.cmp(&left_score))
                .then_with(|| left.memory_id().cmp(right.memory_id()))
        });
        Ok(memories)
    }

    fn next_memory_id(&self) -> MnemoResult<String> {
        let connection = self.connection()?;
        next_id(&connection, "memories", "memory_id", "mem")
    }
}

impl SessionSummaryStore for SqliteStore {
    fn write_session_summary(&self, summary: SessionSummary) -> MnemoResult<SessionSummary> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO session_summaries (
                    session_summary_id, namespace_key, tenant_id, user_id, workspace_id, thread_id,
                    agent_id, source, content, source_event_ids_json, inferred_memory_ids_json, generated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                 ON CONFLICT(session_summary_id) DO UPDATE SET
                    namespace_key=excluded.namespace_key,
                    tenant_id=excluded.tenant_id,
                    user_id=excluded.user_id,
                    workspace_id=excluded.workspace_id,
                    thread_id=excluded.thread_id,
                    agent_id=excluded.agent_id,
                    source=excluded.source,
                    content=excluded.content,
                    source_event_ids_json=excluded.source_event_ids_json,
                    inferred_memory_ids_json=excluded.inferred_memory_ids_json,
                    generated_at=excluded.generated_at",
                params![
                    summary.session_summary_id(),
                    summary.namespace().stable_key(),
                    summary.namespace().tenant_id(),
                    summary.namespace().user_id(),
                    summary.namespace().workspace_id(),
                    summary.namespace().thread_id(),
                    summary.namespace().agent_id(),
                    summary.namespace().source(),
                    summary.content(),
                    encode_json(&summary.source_event_ids())?,
                    encode_json(&summary.inferred_memory_ids())?,
                    summary.generated_at(),
                ],
            )
            .map_err(|err| sqlite_error("failed to upsert session summary", err))?;
        Ok(summary)
    }

    fn get_session_summary(&self, summary_id: &str) -> MnemoResult<Option<SessionSummary>> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT session_summary_id, tenant_id, user_id, workspace_id, thread_id, agent_id,
                    source, namespace_key, content, source_event_ids_json, inferred_memory_ids_json,
                    generated_at
                 FROM session_summaries WHERE session_summary_id = ?1",
                params![summary_id],
                row_summary,
            )
            .optional()
            .map_err(|err| sqlite_error("failed to load session summary", err))
    }

    fn search_session_summaries(
        &self,
        namespace: &Namespace,
        query: Option<&str>,
    ) -> MnemoResult<Vec<SessionSummary>> {
        let connection = self.connection()?;
        let mut stmt = connection
            .prepare(
                "SELECT session_summary_id, tenant_id, user_id, workspace_id, thread_id, agent_id,
                    source, namespace_key, content, source_event_ids_json, inferred_memory_ids_json,
                    generated_at
                 FROM session_summaries WHERE namespace_key = ?1",
            )
            .map_err(|err| sqlite_error("failed to prepare summary search", err))?;
        let rows = stmt
            .query_map(params![namespace.stable_key()], row_summary)
            .map_err(|err| sqlite_error("failed to query session summaries", err))?;
        let query = query.map(str::trim).filter(|value| !value.is_empty());
        let mut summaries = Vec::new();
        for row in rows {
            let summary = row.map_err(|err| sqlite_error("failed to read summary row", err))?;
            if query.is_none_or(|query| {
                summary.session_summary_id().contains(query)
                    || summary.content().contains(query)
                    || summary
                        .source_event_ids()
                        .iter()
                        .any(|id| id.contains(query))
            }) {
                summaries.push(summary);
            }
        }
        summaries.sort_by(|left, right| {
            right
                .generated_at()
                .cmp(left.generated_at())
                .then_with(|| left.session_summary_id().cmp(right.session_summary_id()))
        });
        Ok(summaries)
    }

    fn next_session_summary_id(&self) -> MnemoResult<String> {
        let connection = self.connection()?;
        next_id(
            &connection,
            "session_summaries",
            "session_summary_id",
            "sum",
        )
    }
}

impl JobStore for SqliteStore {
    fn write_job(&self, job: Job) -> MnemoResult<Job> {
        let connection = self.connection()?;
        upsert_job(&connection, &job)?;
        Ok(job)
    }

    fn update_job(&self, job: Job) -> MnemoResult<Job> {
        let connection = self.connection()?;
        let exists = connection
            .query_row(
                "SELECT 1 FROM jobs WHERE job_id = ?1",
                params![job.job_id()],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(|err| sqlite_error("failed to query job", err))?;
        if exists.is_none() {
            return Err(MnemoError::NotFound("job not found".to_string()));
        }
        upsert_job(&connection, &job)?;
        Ok(job)
    }

    fn get_job(&self, job_id: &str) -> MnemoResult<Option<Job>> {
        let connection = self.connection()?;
        let sql = format!("SELECT {} FROM jobs WHERE job_id = ?1", job_columns());
        connection
            .query_row(sql.as_str(), params![job_id], row_job)
            .optional()
            .map_err(|err| sqlite_error("failed to load job", err))
    }

    fn list_jobs(&self) -> MnemoResult<Vec<Job>> {
        let connection = self.connection()?;
        let sql = format!(
            "SELECT {} FROM jobs ORDER BY created_at DESC, job_id ASC",
            job_columns()
        );
        let mut stmt = connection
            .prepare(sql.as_str())
            .map_err(|err| sqlite_error("failed to prepare job list", err))?;
        let rows = stmt
            .query_map([], row_job)
            .map_err(|err| sqlite_error("failed to query jobs", err))?;
        let mut jobs = Vec::new();
        for row in rows {
            jobs.push(row.map_err(|err| sqlite_error("failed to read job row", err))?);
        }
        Ok(jobs)
    }

    fn claim_next_runnable_job(
        &self,
        job_type: &str,
        timestamp: String,
        lease_until: Option<String>,
    ) -> MnemoResult<Option<Job>> {
        let connection = self.connection()?;
        let job_id = connection
            .query_row(
                "SELECT job_id FROM jobs
                 WHERE job_type = ?1
                   AND (
                       (status = 'queued' AND (retry_at IS NULL OR CAST(retry_at AS INTEGER) <= CAST(?2 AS INTEGER)))
                       OR (status = 'running' AND lease_until IS NOT NULL AND CAST(lease_until AS INTEGER) <= CAST(?2 AS INTEGER))
                   )
                 ORDER BY created_at ASC, job_id ASC LIMIT 1",
                params![job_type, timestamp],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|err| sqlite_error("failed to query runnable job", err))?;
        let Some(job_id) = job_id else {
            return Ok(None);
        };
        let sql = format!("SELECT {} FROM jobs WHERE job_id = ?1", job_columns());
        let mut job = connection
            .query_row(sql.as_str(), params![job_id], row_job)
            .map_err(|err| sqlite_error("failed to load runnable job", err))?;
        job.mark_running_until(timestamp, lease_until)?;
        upsert_job(&connection, &job)?;
        Ok(Some(job))
    }

    fn next_job_id(&self) -> MnemoResult<String> {
        let connection = self.connection()?;
        next_id(&connection, "jobs", "job_id", "job")
    }
}

impl ConflictStore for SqliteStore {
    fn search_conflicts(&self, namespace: &Namespace) -> MnemoResult<Vec<MemoryConflict>> {
        let memories = self.search_memories(namespace, None, true)?;
        let mut grouped = HashMap::<String, Vec<Memory>>::new();
        for memory in memories {
            let Some(conflict_key) = memory.conflict_key() else {
                continue;
            };
            if matches!(
                memory.status(),
                MemoryStatus::Active | MemoryStatus::Conflicted
            ) {
                grouped
                    .entry(conflict_key.to_string())
                    .or_default()
                    .push(memory);
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
        let connection = self.connection()?;
        let sql = format!(
            "SELECT {} FROM memories WHERE namespace_key = ?1 AND conflict_key = ?2",
            memory_columns()
        );
        let mut stmt = connection
            .prepare(sql.as_str())
            .map_err(|err| sqlite_error("failed to prepare conflict query", err))?;
        let rows = stmt
            .query_map(params![namespace.stable_key(), conflict_key], row_memory)
            .map_err(|err| sqlite_error("failed to query conflict memories", err))?;
        let mut memories = Vec::new();
        for row in rows {
            memories.push(row.map_err(|err| sqlite_error("failed to read conflict memory", err))?);
        }
        drop(stmt);
        if !memories
            .iter()
            .any(|memory| memory.memory_id() == winner_memory_id)
        {
            return Err(MnemoError::NotFound(
                "winner memory not found in conflict slot".to_string(),
            ));
        }

        let mut winner = None;
        let mut superseded_memory_ids = Vec::new();
        for mut memory in memories {
            if memory.memory_id() == winner_memory_id {
                memory.set_status(MemoryStatus::Active);
                winner = Some(memory.clone());
            } else if matches!(
                memory.status(),
                MemoryStatus::Active | MemoryStatus::Conflicted
            ) {
                memory.mark_superseded_by(winner_memory_id.to_string());
                superseded_memory_ids.push(memory.memory_id().to_string());
            }
            upsert_memory(&connection, &memory)?;
        }
        Ok(ConflictResolveOutcome {
            winner: winner
                .ok_or_else(|| MnemoError::NotFound("winner memory not found".to_string()))?,
            superseded_memory_ids,
        })
    }
}

impl ForgetStore for SqliteStore {
    fn forget_memories(
        &self,
        namespace: &Namespace,
        memory_ids: &[String],
        reason: Option<String>,
        created_at: String,
    ) -> MnemoResult<ForgetOutcome> {
        let connection = self.connection()?;
        let mut forgotten_memory_ids = Vec::new();
        let mut forgotten_contents = Vec::new();
        for memory_id in memory_ids {
            if let Some(mut memory) = select_memory_by_namespace(&connection, namespace, memory_id)?
            {
                memory.mark_forgotten();
                forgotten_memory_ids.push(memory.memory_id().to_string());
                forgotten_contents.push(memory.content().to_string());
                upsert_memory(&connection, &memory)?;
            }
        }
        let tombstone_id = next_id(&connection, "forget_tombstones", "tombstone_id", "forget")?;
        let tombstone = ForgetTombstone::new(
            tombstone_id,
            namespace.clone(),
            forgotten_memory_ids.clone(),
            forgotten_contents,
            reason,
            created_at,
        )?;
        connection
            .execute(
                "INSERT INTO forget_tombstones (
                    tombstone_id, namespace_key, tenant_id, user_id, workspace_id, thread_id,
                    agent_id, source, memory_ids_json, contents_json, reason, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    tombstone.tombstone_id(),
                    namespace.stable_key(),
                    namespace.tenant_id(),
                    namespace.user_id(),
                    namespace.workspace_id(),
                    namespace.thread_id(),
                    namespace.agent_id(),
                    namespace.source(),
                    encode_json(&tombstone.memory_ids())?,
                    encode_json(&tombstone.contents())?,
                    tombstone.reason(),
                    tombstone.created_at(),
                ],
            )
            .map_err(|err| sqlite_error("failed to insert forget tombstone", err))?;
        Ok(ForgetOutcome {
            tombstone,
            forgotten_memory_ids,
        })
    }

    fn is_content_forgotten(&self, namespace: &Namespace, content: &str) -> MnemoResult<bool> {
        let connection = self.connection()?;
        let mut stmt = connection
            .prepare("SELECT contents_json FROM forget_tombstones WHERE namespace_key = ?1")
            .map_err(|err| sqlite_error("failed to prepare forget query", err))?;
        let rows = stmt
            .query_map(params![namespace.stable_key()], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|err| sqlite_error("failed to query forget tombstones", err))?;
        let normalized = content.trim();
        for row in rows {
            let contents_json =
                row.map_err(|err| sqlite_error("failed to read forget tombstone", err))?;
            let contents = decode_json::<Vec<String>>(contents_json.as_str())?;
            if contents.iter().any(|stored| stored.trim() == normalized) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn next_tombstone_id(&self) -> MnemoResult<String> {
        let connection = self.connection()?;
        next_id(&connection, "forget_tombstones", "tombstone_id", "forget")
    }
}

impl UsageStore for SqliteStore {
    fn write_usage_report(&self, report: UsageReport) -> MnemoResult<UsageReport> {
        let connection = self.connection()?;
        let memory_ids_json = encode_json(&report.memory_ids())?;
        connection
            .execute(
                "INSERT INTO usage_reports (
                    usage_id, namespace_key, tenant_id, user_id, workspace_id, thread_id, agent_id,
                    source, context_pack_id, signal, memory_ids_json, notes, reported_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    report.usage_id(),
                    report.namespace().stable_key(),
                    report.namespace().tenant_id(),
                    report.namespace().user_id(),
                    report.namespace().workspace_id(),
                    report.namespace().thread_id(),
                    report.namespace().agent_id(),
                    report.namespace().source(),
                    report.context_pack_id(),
                    report.signal().as_str(),
                    memory_ids_json,
                    report.notes(),
                    report.reported_at(),
                ],
            )
            .map_err(|err| sqlite_error("failed to insert usage report", err))?;
        for memory_id in report.memory_ids() {
            connection
                .execute(
                    "INSERT INTO usage_citations(usage_id, target_type, target_id, signal)
                     VALUES (?1, 'memory', ?2, ?3)",
                    params![report.usage_id(), memory_id, report.signal().as_str()],
                )
                .map_err(|err| sqlite_error("failed to insert usage citation", err))?;
        }
        Ok(report)
    }

    fn list_usage_reports(&self) -> MnemoResult<Vec<UsageReport>> {
        self.search_usage_reports(None)
    }

    fn search_usage_reports(&self, namespace: Option<&Namespace>) -> MnemoResult<Vec<UsageReport>> {
        let connection = self.connection()?;
        let (sql, namespace_key) = if let Some(namespace) = namespace {
            (
                "SELECT usage_id, tenant_id, user_id, workspace_id, thread_id, agent_id, source,
                    namespace_key, context_pack_id, signal, memory_ids_json, notes, reported_at
                 FROM usage_reports WHERE namespace_key = ?1 ORDER BY reported_at DESC, usage_id ASC",
                Some(namespace.stable_key()),
            )
        } else {
            (
                "SELECT usage_id, tenant_id, user_id, workspace_id, thread_id, agent_id, source,
                    namespace_key, context_pack_id, signal, memory_ids_json, notes, reported_at
                 FROM usage_reports ORDER BY reported_at DESC, usage_id ASC",
                None,
            )
        };
        let mut stmt = connection
            .prepare(sql)
            .map_err(|err| sqlite_error("failed to prepare usage report query", err))?;
        let mut reports = Vec::new();
        if let Some(namespace_key) = namespace_key {
            let rows = stmt
                .query_map(params![namespace_key], row_usage_report)
                .map_err(|err| sqlite_error("failed to query usage reports", err))?;
            for row in rows {
                reports.push(row.map_err(|err| sqlite_error("failed to read usage row", err))?);
            }
        } else {
            let rows = stmt
                .query_map([], row_usage_report)
                .map_err(|err| sqlite_error("failed to query usage reports", err))?;
            for row in rows {
                reports.push(row.map_err(|err| sqlite_error("failed to read usage row", err))?);
            }
        }
        Ok(reports)
    }

    fn next_usage_id(&self) -> MnemoResult<String> {
        let connection = self.connection()?;
        next_id(&connection, "usage_reports", "usage_id", "usage")
    }
}
