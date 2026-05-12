use mnemo_domain::*;
use mnemo_ports::StorageError;
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::artifacts::process_outbox_task;
use crate::extraction::{continuation_summary, run_extraction};
use crate::graph::{filter_graph_candidates, index_memory_graph, run_maintenance};
use crate::helpers::*;
use crate::port_impls::load_wrapup_events_internal;
use crate::store::SqliteStore;

impl SqliteStore {
    pub async fn ingest_event_internal(&self, mut event: EventInput) -> Result<EventWriteResult, StorageError> {
        event.namespace = event.namespace.canonical();
        if event.event_id.trim().is_empty() {
            return Err(StorageError::InvalidRequest(
                "event_id is required".to_string(),
            ));
        }
        if event.content.trim().is_empty() {
            return Err(StorageError::InvalidRequest(
                "content is required".to_string(),
            ));
        }

        let now = now_rfc3339();
        let hints = event.memory_hints.clone().unwrap_or_default();
        let ns_key = event.namespace.key();
        let hints_json = serde_json::to_string(&hints).map_err(json_err)?;
        let metadata_json = serde_json::to_string(&event.metadata).map_err(json_err)?;
        let mut conn = self.conn.lock().map_err(lock_err)?;
        let tx = conn.transaction().map_err(sql_err)?;

        if let Some(existing) = tx
            .query_row(
                "SELECT content, event_type, role FROM events WHERE namespace_key = ?1 AND event_id = ?2",
                params![ns_key, event.event_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(sql_err)?
        {
            if existing.0 == event.content
                && existing.1 == event.event_type
                && existing.2 == event.role
            {
                return Ok(EventWriteResult {
                    event_id: event.event_id,
                    status: "deduplicated".to_string(),
                    deduplicated: true,
                    error: None,
                });
            }
            return Err(StorageError::Conflict(
                "same event_id was written with different content".to_string(),
            ));
        }

        tx.execute(
            "INSERT INTO events (namespace_key, event_id, event_type, role, content, occurred_at, memory_hints, metadata, created_at, eligible, external_context)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                ns_key,
                event.event_id,
                event.event_type,
                event.role,
                event.content,
                event.occurred_at,
                hints_json,
                metadata_json,
                now,
                hints.eligible as i64,
                hints.external_context as i64,
            ],
        )
        .map_err(sql_err)?;
        enqueue_outbox(
            &tx,
            &event.namespace.key(),
            "index_event",
            json!({ "event_id": event.event_id }),
        )?;
        write_audit(
            &tx,
            &event.namespace.key(),
            "event.ingest",
            "event",
            Some(&event.event_id),
            json!({
                "event_type": event.event_type,
                "role": event.role,
                "eligible": hints.eligible,
                "external_context": hints.external_context
            }),
        )?;
        tx.commit().map_err(sql_err)?;

        Ok(EventWriteResult {
            event_id: event.event_id,
            status: "accepted".to_string(),
            deduplicated: false,
            error: None,
        })
    }

    pub(crate) async fn search_events_internal(
        &self,
        request: SearchRequest,
    ) -> Result<(Vec<Value>, PageInfo), StorageError> {
        let namespace = request.namespace.canonical();
        let query = format!("%{}%", request.query);
        let limit = page_limit(request.pagination.limit);
        let offset = page_offset(request.pagination.cursor.as_deref())?;
        let conn = self.conn.lock().map_err(lock_err)?;
        let mut stmt = conn
            .prepare(
                "SELECT event_id, event_type, role, content, occurred_at, metadata, created_at
                 FROM events WHERE namespace_key = ?1 AND (?2 = '%%' OR content LIKE ?2)
                 ORDER BY id DESC LIMIT ?3 OFFSET ?4",
            )
            .map_err(sql_err)?;
        let rows = stmt
            .query_map(
                params![namespace.key(), query, (limit + 1) as i64, offset as i64],
                |row| {
                    let metadata: String = row.get(5)?;
                    Ok(json!({
                        "event_id": row.get::<_, String>(0)?,
                        "type": row.get::<_, Option<String>>(1)?,
                        "role": row.get::<_, Option<String>>(2)?,
                        "content": row.get::<_, String>(3)?,
                        "occurred_at": row.get::<_, Option<String>>(4)?,
                        "metadata": serde_json::from_str::<Value>(&metadata).unwrap_or(Value::Null),
                        "created_at": row.get::<_, String>(6)?,
                    }))
                },
            )
            .map_err(sql_err)?;
        Ok(finish_window_page(collect_rows(rows)?, limit, offset))
    }

    pub(crate) async fn search_memories_internal(
        &self,
        request: SearchRequest,
    ) -> Result<(Vec<MemoryRecord>, PageInfo), StorageError> {
        let namespace = request.namespace.canonical();
        let keys = namespace.memory_scope_keys(None);
        let query = format!("%{}%", request.query);
        let limit = page_limit(request.pagination.limit);
        let offset = page_offset(request.pagination.cursor.as_deref())?;
        let conn = self.conn.lock().map_err(lock_err)?;
        let mut memories = Vec::new();
        for key in keys {
            let mut stmt = conn
                .prepare(
                    "SELECT memory_id, namespace_json, content, origin, memory_type, importance, status,
                            source_event_id, conflict_key, valid_from, valid_until,
                            supersedes_json, superseded_by_json, metadata, created_at, updated_at
                     FROM memories
                     WHERE namespace_key = ?1 AND (?2 = '%%' OR content LIKE ?2 OR memory_type LIKE ?2)
                     ORDER BY updated_at DESC",
                )
                .map_err(sql_err)?;
            let rows = stmt
                .query_map(params![key, query], memory_from_row)
                .map_err(sql_err)?;
            memories.extend(collect_rows(rows)?.into_iter().filter(|memory| {
                if request.filters.status.is_empty() && memory.status != "active" {
                    return false;
                }
                matches_filters(memory, &request.filters)
            }));
        }
        memories = filter_graph_candidates(&conn, memories, &request.filters)?;
        memories.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| right.memory_id.cmp(&left.memory_id))
        });
        Ok(paginate_full(memories, limit, offset))
    }

    pub(crate) async fn context_state_version_internal(&self, namespace: Namespace) -> Result<String, StorageError> {
        let namespace = namespace.canonical();
        let key = namespace.key();
        let conn = self.conn.lock().map_err(lock_err)?;
        let memory_revision: Option<String> = conn
            .query_row(
                "SELECT MAX(updated_at) FROM memories WHERE namespace_key = ?1",
                params![key],
                |row| row.get(0),
            )
            .map_err(sql_err)?;
        let tombstone_count = conn
            .query_row(
                "SELECT COUNT(*) FROM forgotten_tombstones WHERE namespace_key = ?1",
                params![key],
                |row| row.get::<_, i64>(0),
            )
            .map_err(sql_err)?;
        let policy_revision: Option<String> = conn
            .query_row(
                "SELECT updated_at FROM policies WHERE namespace_key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql_err)?;
        let usage_revision: Option<String> = conn
            .query_row(
                "SELECT MAX(updated_at) FROM usage_aggregate WHERE namespace_key = ?1",
                params![key],
                |row| row.get(0),
            )
            .map_err(sql_err)?;
        Ok(format!(
            "mem={}|tomb={}|policy={}|usage={}",
            memory_revision.unwrap_or_default(),
            tombstone_count,
            policy_revision.unwrap_or_default(),
            usage_revision.unwrap_or_default()
        ))
    }

    pub(crate) async fn get_context_pack_cache_internal(
        &self,
        cache_key: String,
    ) -> Result<Option<ContextPackResponse>, StorageError> {
        let conn = self.conn.lock().map_err(lock_err)?;
        let response_json = conn
            .query_row(
                "SELECT response_json FROM context_pack_cache WHERE cache_key = ?1",
                params![cache_key],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(sql_err)?;
        response_json
            .map(|raw| serde_json::from_str(&raw).map_err(json_err))
            .transpose()
    }

    pub(crate) async fn put_context_pack_cache_internal(
        &self,
        namespace: Namespace,
        cache_key: String,
        response: ContextPackResponse,
    ) -> Result<(), StorageError> {
        let namespace = namespace.canonical();
        let state_version = self.context_state_version_internal(namespace.clone()).await?;
        let response_json = serde_json::to_string(&response).map_err(json_err)?;
        let conn = self.conn.lock().map_err(lock_err)?;
        conn.execute(
            "INSERT INTO context_pack_cache
             (cache_key, namespace_key, response_json, state_version, generated_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)
             ON CONFLICT(cache_key) DO UPDATE SET
               response_json = excluded.response_json,
               state_version = excluded.state_version,
               updated_at = excluded.updated_at",
            params![
                cache_key,
                namespace.key(),
                response_json,
                state_version,
                response.generated_at,
            ],
        )
        .map_err(sql_err)?;
        Ok(())
    }

    pub(crate) async fn run_jobs_once_internal(&self, limit: usize) -> Result<usize, StorageError> {
        let limit = limit.clamp(1, 100);
        let job_ids = {
            let conn = self.conn.lock().map_err(lock_err)?;
            let mut stmt = conn
                .prepare(
                    "SELECT job_id FROM jobs
                     WHERE status = 'queued' AND job_type = 'wrapup'
                     ORDER BY created_at ASC
                     LIMIT ?1",
                )
                .map_err(sql_err)?;
            stmt.query_map(params![limit as i64], |row| row.get::<_, String>(0))
                .map_err(sql_err)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(sql_err)?
        };

        let mut processed = 0;
        for job_id in &job_ids {
            if let Err(error) =
                process_wrapup_job(&self.conn, job_id, &self.extraction_provider)
            {
                let conn = self.conn.lock().map_err(lock_err)?;
                fail_job(&conn, job_id, &error.to_string())?;
            }
            processed += 1;
        }
        Ok(processed)
    }

    pub(crate) async fn run_outbox_once_internal(&self, limit: usize) -> Result<usize, StorageError> {
        let limit = limit.clamp(1, 500);
        let conn = self.conn.lock().map_err(lock_err)?;
        let mut stmt = conn
            .prepare(
                "SELECT id, namespace_key, task_type, payload FROM outbox_tasks
                 WHERE status = 'queued'
                 ORDER BY created_at ASC
                 LIMIT ?1",
            )
            .map_err(sql_err)?;
        let tasks = stmt
            .query_map(params![limit as i64], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(sql_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_err)?;
        drop(stmt);

        let mut processed = 0;
        for (id, namespace_key, task_type, payload) in &tasks {
            match process_outbox_task(
                &conn,
                self.artifact_dir.as_deref(),
                namespace_key,
                task_type,
                payload,
            ) {
                Ok(()) => {
                    conn.execute(
                        "UPDATE outbox_tasks
                         SET status = 'completed', attempts = attempts + 1, updated_at = ?1, last_error = NULL
                         WHERE id = ?2 AND status = 'queued'",
                        params![now_rfc3339(), id],
                    )
                    .map_err(sql_err)?;
                    processed += 1;
                }
                Err(error) => {
                    conn.execute(
                        "UPDATE outbox_tasks
                         SET status = 'failed', attempts = attempts + 1, last_error = ?1, updated_at = ?2
                         WHERE id = ?3 AND status = 'queued'",
                        params![error.to_string(), now_rfc3339(), id],
                    )
                    .map_err(sql_err)?;
                }
            }
        }
        Ok(processed)
    }
}

fn process_wrapup_job(
    conn_mutex: &std::sync::Arc<std::sync::Mutex<rusqlite::Connection>>,
    job_id: &str,
    extraction_provider: &crate::store::ExtractionProviderConfig,
) -> Result<(), StorageError> {
    let (job, namespace_key, events, extraction_instructions) = {
        let conn = conn_mutex.lock().map_err(lock_err)?;
        let job = conn
            .query_row(
                "SELECT job_id, namespace_json, job_type, status, result_json, created_at
                 FROM jobs WHERE job_id = ?1 AND job_type = 'wrapup'",
                params![job_id],
                job_from_row,
            )
            .optional()
            .map_err(sql_err)?
            .ok_or(StorageError::NotFound)?;
        if !matches!(job.status.as_str(), "queued" | "running") {
            return Ok(());
        }
        let namespace_key = job.namespace.key();
        conn.execute(
            "UPDATE jobs SET status = 'running', updated_at = ?1 WHERE job_id = ?2",
            params![now_rfc3339(), job.job_id],
        )
        .map_err(sql_err)?;

        let requested_range = job
            .result
            .get("requested_event_range")
            .cloned()
            .and_then(|value| serde_json::from_value::<Option<EventRange>>(value).ok())
            .flatten();
        let events = load_wrapup_events_internal(&conn, &namespace_key, requested_range.as_ref())?;

        // Resolve extraction_instructions: per-request > policy > none
        let request_instructions = job
            .result
            .get("extraction_instructions")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .map(String::from);
        let extraction_instructions = if request_instructions.is_some() {
            request_instructions
        } else {
            conn.query_row(
                "SELECT policy_json FROM policies WHERE namespace_key = ?1",
                params![namespace_key],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .ok()
            .flatten()
            .and_then(|json_str| serde_json::from_str::<Value>(&json_str).ok())
            .and_then(|v| v.get("extraction_instructions")?.as_str().map(String::from))
            .filter(|s| !s.trim().is_empty())
        };

        (job, namespace_key, events, extraction_instructions)
    };

    let extraction = run_extraction(extraction_provider, &events, extraction_instructions.as_deref())?;

    let conn = conn_mutex.lock().map_err(lock_err)?;
    let extraction_provider_name = extraction.provider_name.clone();
    let extraction_warning = extraction.warning.clone();
    let mut created_memory_ids = Vec::new();
    let mut skipped_duplicates = 0usize;
    let mut conflict_suggestion_count = 0usize;
    let mut max_event_pos = 0_i64;
    let processed_range = events.first().zip(events.last()).map(|(first, last)| {
        json!({
            "from_event_id": first.event_id,
            "to_event_id": last.event_id
        })
    });

    for event in &events {
        max_event_pos = max_event_pos.max(event.position);
    }

    for extracted in extraction.candidates {
        let event = extracted.event;
        let candidate = extracted.candidate;
        if memory_content_exists(&conn, &namespace_key, &candidate.content)? {
            skipped_duplicates += 1;
            continue;
        }
        let supersedes = if conflict_resolution_mode(&conn, &namespace_key)? == "auto" {
            candidate
                .conflict_key
                .as_deref()
                .map(|conflict_key| find_superseded_memories(&conn, &namespace_key, conflict_key))
                .transpose()?
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let now = now_rfc3339();
        let memory = MemoryRecord {
            memory_id: format!("mem-{}", Uuid::new_v4()),
            namespace: job.namespace.clone(),
            content: candidate.content,
            origin: "inferred".to_string(),
            memory_type: candidate.memory_type,
            importance: candidate.importance,
            status: "active".to_string(),
            source_event_id: Some(event.event_id.clone()),
            conflict_key: candidate.conflict_key,
            valid_from: event.occurred_at.clone(),
            valid_until: None,
            supersedes,
            superseded_by: Vec::new(),
            metadata: json!({
                "extraction_provider": &extraction_provider_name,
                "source_role": &event.role,
                "confidence": candidate.confidence
            }),
            created_at: now.clone(),
            updated_at: now,
        };
        insert_memory_record(&conn, &memory)?;
        conflict_suggestion_count += create_conflict_suggestions(&conn, &memory)?;
        link_superseded_memories(&conn, &memory)?;
        index_memory_graph(&conn, &memory)?;
        enqueue_outbox(
            &conn,
            &namespace_key,
            "index_memory",
            json!({ "memory_id": &memory.memory_id }),
        )?;
        write_audit(
            &conn,
            &namespace_key,
            "memory.infer",
            "memory",
            Some(&memory.memory_id),
            json!({
                "source_event_id": &event.event_id,
                "memory_type": &memory.memory_type,
                "conflict_key": &memory.conflict_key,
                "conflict_suggestion_count": conflict_suggestion_count
            }),
        )?;
        created_memory_ids.push(memory.memory_id);
    }

    if max_event_pos > 0 {
        update_organized_cursor(&conn, &namespace_key, max_event_pos)?;
    }
    let maintenance = run_maintenance(&conn, &namespace_key)?;
    let status = if events.is_empty() || created_memory_ids.is_empty() {
        "completed_noop"
    } else {
        "completed"
    };
    let result = json!({
        "status": status,
        "stage": "maintain",
        "processed_event_range": processed_range,
        "input_count": events.len(),
        "output_count": created_memory_ids.len(),
        "skipped_duplicates": skipped_duplicates,
        "conflict_suggestion_count": conflict_suggestion_count,
        "memory_ids": created_memory_ids,
        "extraction_provider": extraction_provider_name,
        "extraction_warning": extraction_warning,
        "continuation_summary": continuation_summary(&events),
        "maintenance": {
            "organized_cursor": format!("evtpos:{max_event_pos}"),
            "expired_count": maintenance.expired_count,
            "conflict_links_updated": true,
            "graph_entities": maintenance.entity_count,
            "graph_relations": maintenance.relation_count
        }
    });
    conn.execute(
        "UPDATE jobs SET status = ?1, result_json = ?2, updated_at = ?3 WHERE job_id = ?4",
        params![
            status,
            serde_json::to_string(&result).map_err(json_err)?,
            now_rfc3339(),
            job.job_id,
        ],
    )
    .map_err(sql_err)?;
    enqueue_outbox(
        &conn,
        &namespace_key,
        "refresh_artifact",
        json!({ "job_id": job.job_id, "type": "wrapup" }),
    )?;
    write_audit(
        &conn,
        &namespace_key,
        "job.wrapup.complete",
        "job",
        Some(job_id),
        result,
    )?;
    Ok(())
}
