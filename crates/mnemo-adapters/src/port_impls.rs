use async_trait::async_trait;
use mnemo_domain::*;
use mnemo_ports::{
    ArtifactStore, AuditStore, ContextPackCache, ConflictStore, CursorStore, EventStore,
    GraphStore, IdempotencyStore, JobStore, MaintenanceOps, MaintenanceResult, MemoryIndexStore,
    MemoryStore, NamespaceEventStatusInfo, OutboxStore, PolicyStore, RankedMemory, StorageError,
    TombstoneStore, TransactionContext, UsageStore, WrapupEvent as PortWrapupEvent,
};
use rusqlite::{OptionalExtension, params};
use serde_json::Value;
use uuid::Uuid;

use crate::artifacts::refresh_namespace_artifacts;
use crate::extraction::WrapupEvent;
use crate::graph::{attach_phase4_metadata, filter_graph_candidates, index_memory_graph, run_maintenance};
use crate::helpers::*;
use crate::ranking::{aggregate_usage_feedback, rank_memories, upsert_memory_index};
use crate::store::SqliteStore;

#[async_trait]
impl EventStore for SqliteStore {
    async fn append_event(
        &self,
        _tx: &mut dyn TransactionContext,
        _namespace: &Namespace,
        event: &EventInput,
    ) -> Result<EventWriteResult, StorageError> {
        self.ingest_event_internal(event.clone()).await
    }

    async fn search_events(
        &self,
        _namespace: &Namespace,
        query: &SearchRequest,
    ) -> Result<(Vec<Value>, PageInfo), StorageError> {
        self.search_events_internal(query.clone()).await
    }

    async fn namespace_event_status(
        &self,
        namespace: &Namespace,
    ) -> Result<NamespaceEventStatusInfo, StorageError> {
        let namespace = namespace.canonical();
        let key = namespace.key();
        let conn = self.conn.lock().map_err(lock_err)?;
        let event_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM events WHERE namespace_key = ?1",
                params![key],
                |row| row.get(0),
            )
            .map_err(sql_err)?;
        let latest_event_id: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(id), 0) FROM events WHERE namespace_key = ?1",
                params![key],
                |row| row.get(0),
            )
            .map_err(sql_err)?;
        let organized_event_id: i64 = conn
            .query_row(
                "SELECT organized_event_id FROM namespace_cursors WHERE namespace_key = ?1",
                params![key],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(sql_err)?
            .unwrap_or(0);
        Ok(NamespaceEventStatusInfo {
            event_count: event_count as usize,
            latest_event_id,
            organized_event_id,
        })
    }

    async fn load_wrapup_events(
        &self,
        namespace: &Namespace,
        range: Option<&EventRange>,
    ) -> Result<Vec<PortWrapupEvent>, StorageError> {
        let namespace = namespace.canonical();
        let key = namespace.key();
        let conn = self.conn.lock().map_err(lock_err)?;
        let internal = load_wrapup_events_internal(&conn, &key, range)?;
        Ok(internal
            .into_iter()
            .map(|e| PortWrapupEvent {
                position: e.position,
                event_id: e.event_id,
                role: e.role,
                content: e.content,
                occurred_at: e.occurred_at,
            })
            .collect())
    }

    async fn event_position(
        &self,
        namespace: &Namespace,
        event_id: &str,
    ) -> Result<Option<i64>, StorageError> {
        let namespace = namespace.canonical();
        let conn = self.conn.lock().map_err(lock_err)?;
        event_position_internal(&conn, &namespace.key(), event_id)
    }
}

#[async_trait]
impl MemoryStore for SqliteStore {
    async fn create_memory(
        &self,
        _tx: &mut dyn TransactionContext,
        memory: &MemoryRecord,
    ) -> Result<(), StorageError> {
        let conn = self.conn.lock().map_err(lock_err)?;
        insert_memory_record(&conn, memory)
    }

    async fn get_memory(
        &self,
        namespace: &Namespace,
        memory_id: &str,
    ) -> Result<MemoryRecord, StorageError> {
        let namespace = namespace.canonical();
        let mut memory = get_memory(&self.conn, &namespace, memory_id)?;
        let conn = self.conn.lock().map_err(lock_err)?;
        attach_phase4_metadata(&conn, &mut memory)?;
        Ok(memory)
    }

    async fn update_memory(
        &self,
        _tx: &mut dyn TransactionContext,
        memory: &MemoryRecord,
    ) -> Result<(), StorageError> {
        let conn = self.conn.lock().map_err(lock_err)?;
        update_memory(&conn, memory)
    }

    async fn search_memories(
        &self,
        _namespace: &Namespace,
        query: &SearchRequest,
    ) -> Result<(Vec<MemoryRecord>, PageInfo), StorageError> {
        self.search_memories_internal(query.clone()).await
    }

    async fn query_memories(
        &self,
        _namespace: &Namespace,
        scope_keys: &[String],
        request: &QueryRequest,
    ) -> Result<Vec<MemoryRecord>, StorageError> {
        let conn = self.conn.lock().map_err(lock_err)?;
        fetch_filtered_memories(&conn, scope_keys, request)
    }

    async fn mark_forgotten(
        &self,
        _tx: &mut dyn TransactionContext,
        namespace: &Namespace,
        memory_ids: &[String],
        mode: &str,
    ) -> Result<(), StorageError> {
        let namespace = namespace.canonical();
        let now = now_rfc3339();
        let conn = self.conn.lock().map_err(lock_err)?;
        for memory_id in memory_ids {
            let (status, content) = match mode {
                "disable" => ("disabled", None),
                "hard_delete" => ("forgotten", Some("[hard deleted]".to_string())),
                "anonymize" => ("forgotten", Some("[anonymized]".to_string())),
                _ => ("forgotten", None),
            };
            if let Some(content) = content {
                conn.execute(
                    "UPDATE memories SET status = ?1, content = ?2, updated_at = ?3 WHERE namespace_key = ?4 AND memory_id = ?5",
                    params![status, content, now, namespace.key(), memory_id],
                ).map_err(sql_err)?;
            } else {
                conn.execute(
                    "UPDATE memories SET status = ?1, updated_at = ?2 WHERE namespace_key = ?3 AND memory_id = ?4",
                    params![status, now, namespace.key(), memory_id],
                ).map_err(sql_err)?;
            }
        }
        Ok(())
    }

    async fn memory_content_exists(
        &self,
        namespace: &Namespace,
        content: &str,
    ) -> Result<bool, StorageError> {
        let namespace = namespace.canonical();
        let conn = self.conn.lock().map_err(lock_err)?;
        memory_content_exists(&conn, &namespace.key(), content)
    }

    async fn find_superseded_memories(
        &self,
        namespace: &Namespace,
        conflict_key: &str,
    ) -> Result<Vec<String>, StorageError> {
        let namespace = namespace.canonical();
        let conn = self.conn.lock().map_err(lock_err)?;
        find_superseded_memories(&conn, &namespace.key(), conflict_key)
    }

    async fn context_state_version(&self, namespace: &Namespace) -> Result<String, StorageError> {
        self.context_state_version_internal(namespace.clone()).await
    }
}

#[async_trait]
impl JobStore for SqliteStore {
    async fn create_job(
        &self,
        _tx: &mut dyn TransactionContext,
        job: &JobRecord,
    ) -> Result<(), StorageError> {
        let conn = self.conn.lock().map_err(lock_err)?;
        insert_job(&conn, job)
    }

    async fn get_job(&self, namespace: &Namespace, job_id: &str) -> Result<JobRecord, StorageError> {
        let namespace = namespace.canonical();
        let conn = self.conn.lock().map_err(lock_err)?;
        conn.query_row(
            "SELECT job_id, namespace_json, job_type, status, result_json, created_at FROM jobs WHERE namespace_key = ?1 AND job_id = ?2",
            params![namespace.key(), job_id],
            job_from_row,
        )
        .optional()
        .map_err(sql_err)?
        .ok_or(StorageError::NotFound)
    }

    async fn list_jobs(
        &self,
        namespace: &Namespace,
        limit: usize,
        cursor: Option<&str>,
    ) -> Result<(Vec<JobRecord>, PageInfo), StorageError> {
        let namespace = namespace.canonical();
        let limit = page_limit(limit);
        let offset = page_offset(cursor)?;
        let conn = self.conn.lock().map_err(lock_err)?;
        let mut stmt = conn
            .prepare(
                "SELECT job_id, namespace_json, job_type, status, result_json, created_at
                 FROM jobs WHERE namespace_key = ?1 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
            )
            .map_err(sql_err)?;
        let rows = stmt
            .query_map(
                params![namespace.key(), (limit + 1) as i64, offset as i64],
                job_from_row,
            )
            .map_err(sql_err)?;
        Ok(finish_window_page(collect_rows(rows)?, limit, offset))
    }

    async fn update_job_status(
        &self,
        _tx: &mut dyn TransactionContext,
        job_id: &str,
        status: &str,
        result: &Value,
    ) -> Result<(), StorageError> {
        let conn = self.conn.lock().map_err(lock_err)?;
        conn.execute(
            "UPDATE jobs SET status = ?1, result_json = ?2, updated_at = ?3 WHERE job_id = ?4",
            params![status, serde_json::to_string(result).map_err(json_err)?, now_rfc3339(), job_id],
        )
        .map_err(sql_err)?;
        Ok(())
    }

    async fn claim_queued_jobs(
        &self,
        job_type: &str,
        limit: usize,
    ) -> Result<Vec<String>, StorageError> {
        let limit = limit.clamp(1, 100);
        let conn = self.conn.lock().map_err(lock_err)?;
        let mut stmt = conn
            .prepare(
                "SELECT job_id FROM jobs WHERE status = 'queued' AND job_type = ?1 ORDER BY created_at ASC LIMIT ?2",
            )
            .map_err(sql_err)?;
        stmt.query_map(params![job_type, limit as i64], |row| row.get::<_, String>(0))
            .map_err(sql_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_err)
    }
}

#[async_trait]
impl PolicyStore for SqliteStore {
    async fn get_policy(&self, namespace: &Namespace) -> Result<Value, StorageError> {
        let namespace = namespace.canonical();
        let conn = self.conn.lock().map_err(lock_err)?;
        let value = conn
            .query_row(
                "SELECT policy_json FROM policies WHERE namespace_key = ?1",
                params![namespace.key()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(sql_err)?;
        match value {
            Some(value) => serde_json::from_str(&value).map_err(json_err),
            None => Ok(default_policy()),
        }
    }

    async fn put_policy(
        &self,
        _tx: &mut dyn TransactionContext,
        namespace: &Namespace,
        policy: &Value,
    ) -> Result<(), StorageError> {
        let namespace = namespace.canonical();
        let policy_json = serde_json::to_string(policy).map_err(json_err)?;
        let conn = self.conn.lock().map_err(lock_err)?;
        conn.execute(
            "INSERT INTO policies (namespace_key, policy_json, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(namespace_key) DO UPDATE SET policy_json = excluded.policy_json, updated_at = excluded.updated_at",
            params![namespace.key(), policy_json, now_rfc3339()],
        )
        .map_err(sql_err)?;
        Ok(())
    }
}

#[async_trait]
impl UsageStore for SqliteStore {
    async fn record_usage(
        &self,
        _tx: &mut dyn TransactionContext,
        namespace: &Namespace,
        request: &UsageRequest,
    ) -> Result<(), StorageError> {
        let namespace = namespace.canonical();
        let payload = serde_json::to_string(request).map_err(json_err)?;
        let conn = self.conn.lock().map_err(lock_err)?;
        let usage_id = format!("usage-{}", Uuid::new_v4());
        conn.execute(
            "INSERT INTO usage_feedback (id, namespace_key, payload, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![usage_id, namespace.key(), payload, now_rfc3339()],
        )
        .map_err(sql_err)?;
        Ok(())
    }

    async fn aggregate_usage(
        &self,
        _tx: &mut dyn TransactionContext,
        namespace: &Namespace,
        used_items: &[Value],
    ) -> Result<(), StorageError> {
        let namespace = namespace.canonical();
        let conn = self.conn.lock().map_err(lock_err)?;
        aggregate_usage_feedback(&conn, &namespace.key(), used_items)
    }
}

#[async_trait]
impl IdempotencyStore for SqliteStore {
    async fn check(
        &self,
        _tx: &mut dyn TransactionContext,
        scope_key: &str,
    ) -> Result<Option<(String, String)>, StorageError> {
        let conn = self.conn.lock().map_err(lock_err)?;
        conn.query_row(
            "SELECT request_hash, response_json FROM idempotency WHERE scope_key = ?1",
            params![scope_key],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(sql_err)
    }

    async fn record(
        &self,
        _tx: &mut dyn TransactionContext,
        scope_key: &str,
        request_hash: &str,
        response_json: &str,
    ) -> Result<(), StorageError> {
        let conn = self.conn.lock().map_err(lock_err)?;
        conn.execute(
            "INSERT INTO idempotency (scope_key, request_hash, response_json, status, expires_at) VALUES (?1, ?2, ?3, 'completed', ?4)",
            params![scope_key, request_hash, response_json, "9999-12-31T00:00:00Z"],
        )
        .map_err(sql_err)?;
        Ok(())
    }

    async fn update_response(
        &self,
        scope_key: &str,
        response_json: &str,
    ) -> Result<(), StorageError> {
        let conn = self.conn.lock().map_err(lock_err)?;
        conn.execute(
            "UPDATE idempotency SET response_json = ?1 WHERE scope_key = ?2",
            params![response_json, scope_key],
        )
        .map_err(sql_err)?;
        Ok(())
    }
}

#[async_trait]
impl OutboxStore for SqliteStore {
    async fn enqueue(
        &self,
        _tx: &mut dyn TransactionContext,
        namespace_key: &str,
        task_type: &str,
        payload: &Value,
    ) -> Result<(), StorageError> {
        let conn = self.conn.lock().map_err(lock_err)?;
        enqueue_outbox(&conn, namespace_key, task_type, payload.clone())
    }

    async fn process_pending(&self, limit: usize) -> Result<usize, StorageError> {
        self.run_outbox_once_internal(limit).await
    }
}

#[async_trait]
impl AuditStore for SqliteStore {
    async fn write_audit(
        &self,
        _tx: &mut dyn TransactionContext,
        namespace_key: &str,
        action: &str,
        resource_type: &str,
        resource_id: Option<&str>,
        metadata: &Value,
    ) -> Result<(), StorageError> {
        let conn = self.conn.lock().map_err(lock_err)?;
        write_audit(&conn, namespace_key, action, resource_type, resource_id, metadata.clone())
    }
}

#[async_trait]
impl TombstoneStore for SqliteStore {
    async fn create_tombstone(
        &self,
        _tx: &mut dyn TransactionContext,
        namespace: &Namespace,
        target: &ForgetTarget,
        mode: &str,
        reason: Option<&str>,
    ) -> Result<(), StorageError> {
        let namespace = namespace.canonical();
        let target_json = serde_json::to_string(target).map_err(json_err)?;
        let conn = self.conn.lock().map_err(lock_err)?;
        conn.execute(
            "INSERT INTO forgotten_tombstones (id, namespace_key, target_json, mode, reason, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![format!("tomb-{}", Uuid::new_v4()), namespace.key(), target_json, mode, reason, now_rfc3339()],
        )
        .map_err(sql_err)?;
        Ok(())
    }
}

#[async_trait]
impl ContextPackCache for SqliteStore {
    async fn get(&self, cache_key: &str) -> Result<Option<ContextPackResponse>, StorageError> {
        self.get_context_pack_cache_internal(cache_key.to_string()).await
    }

    async fn put(
        &self,
        namespace: &Namespace,
        cache_key: &str,
        response: &ContextPackResponse,
    ) -> Result<(), StorageError> {
        self.put_context_pack_cache_internal(namespace.clone(), cache_key.to_string(), response.clone())
            .await
    }
}

#[async_trait]
impl CursorStore for SqliteStore {
    async fn get_organized_cursor(&self, namespace: &Namespace) -> Result<i64, StorageError> {
        let namespace = namespace.canonical();
        let conn = self.conn.lock().map_err(lock_err)?;
        conn.query_row(
            "SELECT organized_event_id FROM namespace_cursors WHERE namespace_key = ?1",
            params![namespace.key()],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(sql_err)
        .map(|v| v.unwrap_or(0))
    }

    async fn update_organized_cursor(
        &self,
        _tx: &mut dyn TransactionContext,
        namespace: &Namespace,
        position: i64,
    ) -> Result<(), StorageError> {
        let namespace = namespace.canonical();
        let conn = self.conn.lock().map_err(lock_err)?;
        update_organized_cursor(&conn, &namespace.key(), position)
    }
}

#[async_trait]
impl GraphStore for SqliteStore {
    async fn index_memory_graph(
        &self,
        _tx: &mut dyn TransactionContext,
        memory: &MemoryRecord,
    ) -> Result<(), StorageError> {
        let conn = self.conn.lock().map_err(lock_err)?;
        index_memory_graph(&conn, memory)
    }

    async fn delete_memory_graph(
        &self,
        _tx: &mut dyn TransactionContext,
        namespace: &Namespace,
        memory_id: &str,
    ) -> Result<(), StorageError> {
        let namespace = namespace.canonical();
        let conn = self.conn.lock().map_err(lock_err)?;
        conn.execute(
            "DELETE FROM memory_entities WHERE namespace_key = ?1 AND memory_id = ?2",
            params![namespace.key(), memory_id],
        )
        .map_err(sql_err)?;
        conn.execute(
            "DELETE FROM relations WHERE namespace_key = ?1 AND memory_id = ?2",
            params![namespace.key(), memory_id],
        )
        .map_err(sql_err)?;
        Ok(())
    }

    async fn filter_by_entities(
        &self,
        _namespace: &Namespace,
        memories: Vec<MemoryRecord>,
        entity_ids: &[String],
        relation_types: &[String],
    ) -> Result<Vec<MemoryRecord>, StorageError> {
        let conn = self.conn.lock().map_err(lock_err)?;
        let filters = QueryFilters {
            entity_ids: entity_ids.to_vec(),
            relation_types: relation_types.to_vec(),
            ..QueryFilters::default()
        };
        filter_graph_candidates(&conn, memories, &filters)
    }
}

#[async_trait]
impl ConflictStore for SqliteStore {
    async fn create_conflict_suggestions(
        &self,
        _tx: &mut dyn TransactionContext,
        memory: &MemoryRecord,
    ) -> Result<usize, StorageError> {
        let conn = self.conn.lock().map_err(lock_err)?;
        create_conflict_suggestions(&conn, memory)
    }

    async fn link_superseded_memories(
        &self,
        _tx: &mut dyn TransactionContext,
        memory: &MemoryRecord,
    ) -> Result<(), StorageError> {
        let conn = self.conn.lock().map_err(lock_err)?;
        link_superseded_memories(&conn, memory)
    }
}

#[async_trait]
impl MemoryIndexStore for SqliteStore {
    async fn upsert_index(
        &self,
        _tx: &mut dyn TransactionContext,
        namespace: &Namespace,
        memory_id: &str,
    ) -> Result<(), StorageError> {
        let namespace = namespace.canonical();
        let conn = self.conn.lock().map_err(lock_err)?;
        upsert_memory_index(&conn, &namespace.key(), memory_id)
    }

    async fn delete_index(
        &self,
        namespace: &Namespace,
        memory_id: &str,
    ) -> Result<(), StorageError> {
        let namespace = namespace.canonical();
        let conn = self.conn.lock().map_err(lock_err)?;
        conn.execute(
            "DELETE FROM memory_index WHERE namespace_key = ?1 AND memory_id = ?2",
            params![namespace.key(), memory_id],
        )
        .map_err(sql_err)?;
        Ok(())
    }

    async fn rank_memories(
        &self,
        _namespace: &Namespace,
        memories: Vec<MemoryRecord>,
        query: &str,
    ) -> Result<Vec<RankedMemory>, StorageError> {
        let conn = self.conn.lock().map_err(lock_err)?;
        rank_memories(&conn, memories, query)
    }
}

#[async_trait]
impl ArtifactStore for SqliteStore {
    async fn refresh_namespace_artifacts(&self, namespace: &Namespace) -> Result<(), StorageError> {
        let namespace = namespace.canonical();
        if let Some(artifact_dir) = &self.artifact_dir {
            let conn = self.conn.lock().map_err(lock_err)?;
            refresh_namespace_artifacts(&conn, artifact_dir, &namespace.key())?;
        }
        Ok(())
    }
}

#[async_trait]
impl MaintenanceOps for SqliteStore {
    async fn run_maintenance(&self, namespace: &Namespace) -> Result<MaintenanceResult, StorageError> {
        let namespace = namespace.canonical();
        let conn = self.conn.lock().map_err(lock_err)?;
        run_maintenance(&conn, &namespace.key()).map(Into::into)
    }

    async fn process_queued_jobs(&self, limit: usize) -> Result<usize, StorageError> {
        self.run_jobs_once_internal(limit).await
    }
}

// ─── Internal helpers used by port impls ──────────────────────────────────

pub(crate) fn load_wrapup_events_internal(
    conn: &rusqlite::Connection,
    namespace_key: &str,
    range: Option<&EventRange>,
) -> Result<Vec<WrapupEvent>, StorageError> {
    let (from_pos, to_pos) = if let Some(range) = range {
        (
            event_position_internal(conn, namespace_key, &range.from_event_id)?.unwrap_or(0),
            event_position_internal(conn, namespace_key, &range.to_event_id)?.unwrap_or(i64::MAX),
        )
    } else {
        let organized = conn
            .query_row(
                "SELECT organized_event_id FROM namespace_cursors WHERE namespace_key = ?1",
                params![namespace_key],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(sql_err)?
            .unwrap_or(0);
        (organized + 1, i64::MAX)
    };

    let mut stmt = conn
        .prepare(
            "SELECT id, event_id, role, content, occurred_at
             FROM events
             WHERE namespace_key = ?1
               AND id >= ?2
               AND id <= ?3
               AND eligible = 1
               AND external_context = 0
             ORDER BY id ASC
             LIMIT 256",
        )
        .map_err(sql_err)?;
    let rows = stmt
        .query_map(params![namespace_key, from_pos, to_pos], |row| {
            Ok(WrapupEvent {
                position: row.get(0)?,
                event_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                occurred_at: row.get(4)?,
            })
        })
        .map_err(sql_err)?;
    collect_rows(rows)
}

pub(crate) fn event_position_internal(
    conn: &rusqlite::Connection,
    namespace_key: &str,
    event_id: &str,
) -> Result<Option<i64>, StorageError> {
    conn.query_row(
        "SELECT id FROM events WHERE namespace_key = ?1 AND event_id = ?2",
        params![namespace_key, event_id],
        |row| row.get::<_, i64>(0),
    )
    .optional()
    .map_err(sql_err)
}
