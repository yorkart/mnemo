use mnemo_domain::*;
use mnemo_ports::StorageError;
use rusqlite::{Connection, params};
use serde_json::Value;

pub(crate) type StoreError = StorageError;

pub(crate) fn sql_err(error: rusqlite::Error) -> StoreError {
    StoreError::Storage(error.to_string())
}

pub(crate) fn json_err(error: serde_json::Error) -> StoreError {
    StoreError::Storage(error.to_string())
}

pub(crate) fn lock_err<T>(error: std::sync::PoisonError<T>) -> StoreError {
    StoreError::Storage(error.to_string())
}

pub(crate) fn io_err(error: std::io::Error) -> StoreError {
    StoreError::Storage(error.to_string())
}

pub(crate) fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    column_sql: &str,
) -> Result<(), StoreError> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(sql_err)?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(sql_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_err)?;
    if columns.iter().any(|name| name == column) {
        return Ok(());
    }
    conn.execute(
        &format!("ALTER TABLE {table} ADD COLUMN {column} {column_sql}"),
        [],
    )
    .map_err(sql_err)?;
    Ok(())
}

pub(crate) fn memory_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryRecord> {
    let namespace_json: String = row.get(1)?;
    let supersedes_json: String = row.get(11)?;
    let superseded_by_json: String = row.get(12)?;
    let metadata_json: String = row.get(13)?;
    Ok(MemoryRecord {
        memory_id: row.get(0)?,
        namespace: serde_json::from_str(&namespace_json).unwrap_or_default(),
        content: row.get(2)?,
        origin: row.get(3)?,
        memory_type: row.get(4)?,
        importance: row.get(5)?,
        status: row.get(6)?,
        source_event_id: row.get(7)?,
        conflict_key: row.get(8)?,
        valid_from: row.get(9)?,
        valid_until: row.get(10)?,
        supersedes: serde_json::from_str(&supersedes_json).unwrap_or_default(),
        superseded_by: serde_json::from_str(&superseded_by_json).unwrap_or_default(),
        metadata: serde_json::from_str(&metadata_json).unwrap_or(Value::Null),
        created_at: row.get(14)?,
        updated_at: row.get(15)?,
    })
}

pub(crate) fn job_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<JobRecord> {
    let namespace_json: String = row.get(1)?;
    let result_json: String = row.get(4)?;
    Ok(JobRecord {
        job_id: row.get(0)?,
        namespace: serde_json::from_str(&namespace_json).unwrap_or_default(),
        job_type: row.get(2)?,
        status: row.get(3)?,
        result: serde_json::from_str(&result_json).unwrap_or(Value::Null),
        created_at: row.get(5)?,
    })
}

pub(crate) fn collect_rows<T>(
    rows: rusqlite::MappedRows<'_, impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>>,
) -> Result<Vec<T>, StoreError> {
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(sql_err)?);
    }
    Ok(out)
}

pub(crate) fn page_limit(limit: usize) -> usize {
    limit.clamp(1, 200)
}

pub(crate) fn page_offset(cursor: Option<&str>) -> Result<usize, StoreError> {
    let Some(cursor) = cursor.filter(|cursor| !cursor.is_empty()) else {
        return Ok(0);
    };
    let raw = cursor.strip_prefix("page:").ok_or_else(|| {
        StoreError::InvalidRequest("pagination.cursor must use page:<offset>".to_string())
    })?;
    let offset = raw.parse::<usize>().map_err(|_| {
        StoreError::InvalidRequest("pagination.cursor must use page:<offset>".to_string())
    })?;
    if offset > 100_000 {
        return Err(StoreError::InvalidRequest(
            "pagination.cursor offset exceeds 100000".to_string(),
        ));
    }
    Ok(offset)
}

pub(crate) fn finish_window_page<T>(mut items: Vec<T>, limit: usize, offset: usize) -> (Vec<T>, PageInfo) {
    let has_more = items.len() > limit;
    if has_more {
        items.truncate(limit);
    }
    (
        items,
        PageInfo {
            limit,
            next_cursor: has_more.then(|| format!("page:{}", offset.saturating_add(limit))),
            has_more,
        },
    )
}

pub(crate) fn paginate_full<T>(items: Vec<T>, limit: usize, offset: usize) -> (Vec<T>, PageInfo) {
    let total = items.len();
    let page = items.into_iter().skip(offset).take(limit).collect();
    let next_offset = offset.saturating_add(limit);
    let has_more = total > next_offset;
    (
        page,
        PageInfo {
            limit,
            next_cursor: has_more.then(|| format!("page:{next_offset}")),
            has_more,
        },
    )
}

pub(crate) fn is_valid_at(memory: &MemoryRecord, as_of: &Option<String>) -> bool {
    if memory.status != "active" {
        return false;
    }
    let as_of = parse_time(as_of)
        .or_else(|| parse_time(&Some(now_rfc3339())))
        .expect("now_rfc3339 parses");
    if let Some(valid_from) = parse_time(&memory.valid_from) {
        if valid_from > as_of {
            return false;
        }
    }
    if let Some(valid_until) = parse_time(&memory.valid_until) {
        if valid_until <= as_of {
            return false;
        }
    }
    true
}

pub(crate) fn matches_temporal_scope(
    memory: &MemoryRecord,
    as_of: &Option<String>,
    temporal_scope: &str,
) -> bool {
    match temporal_scope {
        "all" => true,
        "historical" => memory.status == "active" && !is_valid_at(memory, as_of),
        _ => is_valid_at(memory, as_of),
    }
}

pub(crate) fn matches_filters(memory: &MemoryRecord, filters: &QueryFilters) -> bool {
    if filters.status.is_empty() && memory.status != "active" {
        return false;
    }
    (filters.memory_types.is_empty() || filters.memory_types.contains(&memory.memory_type))
        && (filters.origins.is_empty() || filters.origins.contains(&memory.origin))
        && (filters.importance.is_empty() || filters.importance.contains(&memory.importance))
        && (filters.status.is_empty() || filters.status.contains(&memory.status))
}

#[allow(dead_code)]
pub(crate) fn count_where(conn: &Connection, table: &str, namespace_key: &str) -> Result<usize, StoreError> {
    let sql = format!("SELECT COUNT(*) FROM {table} WHERE namespace_key = ?1");
    Ok(conn
        .query_row(&sql, params![namespace_key], |row| row.get::<_, i64>(0))
        .map_err(sql_err)? as usize)
}

pub(crate) fn enqueue_outbox(
    conn: &Connection,
    namespace_key: &str,
    task_type: &str,
    payload: Value,
) -> Result<(), StoreError> {
    let id = format!("outbox-{}", uuid::Uuid::new_v4());
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO outbox_tasks
         (id, namespace_key, task_type, payload, idempotency_key, status, next_run_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'queued', ?6, ?6, ?6)",
        params![
            id,
            namespace_key,
            task_type,
            serde_json::to_string(&payload).map_err(json_err)?,
            &id,
            now,
        ],
    )
    .map_err(sql_err)?;
    Ok(())
}

pub(crate) fn write_audit(
    conn: &Connection,
    namespace_key: &str,
    action: &str,
    resource_type: &str,
    resource_id: Option<&str>,
    metadata: Value,
) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO audit_log
         (id, namespace_key, action, resource_type, resource_id, metadata_json, request_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7)",
        params![
            format!("audit-{}", uuid::Uuid::new_v4()),
            namespace_key,
            action,
            resource_type,
            resource_id,
            serde_json::to_string(&metadata).map_err(json_err)?,
            now_rfc3339(),
        ],
    )
    .map_err(sql_err)?;
    Ok(())
}

pub(crate) fn insert_job(conn: &Connection, job: &JobRecord) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO jobs (job_id, namespace_key, namespace_json, job_type, status, result_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
        params![
            job.job_id,
            job.namespace.key(),
            serde_json::to_string(&job.namespace).map_err(json_err)?,
            job.job_type,
            job.status,
            serde_json::to_string(&job.result).map_err(json_err)?,
            job.created_at,
        ],
    )
    .map_err(sql_err)?;
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn build_job(namespace: Namespace, job_type: &str, status: &str, result: Value) -> JobRecord {
    JobRecord {
        job_id: format!("job-{}", uuid::Uuid::new_v4()),
        namespace,
        job_type: job_type.to_string(),
        status: status.to_string(),
        created_at: now_rfc3339(),
        result,
    }
}

pub(crate) fn insert_memory_record(conn: &Connection, memory: &MemoryRecord) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO memories
         (memory_id, namespace_key, namespace_json, content, origin, memory_type, importance, status,
          source_event_id, conflict_key, valid_from, valid_until, supersedes_json, superseded_by_json,
          metadata, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            memory.memory_id,
            memory.namespace.key(),
            serde_json::to_string(&memory.namespace).map_err(json_err)?,
            memory.content,
            memory.origin,
            memory.memory_type,
            memory.importance,
            memory.status,
            memory.source_event_id,
            memory.conflict_key,
            memory.valid_from,
            memory.valid_until,
            serde_json::to_string(&memory.supersedes).map_err(json_err)?,
            serde_json::to_string(&memory.superseded_by).map_err(json_err)?,
            serde_json::to_string(&memory.metadata).map_err(json_err)?,
            memory.created_at,
            memory.updated_at,
        ],
    )
    .map_err(sql_err)?;
    Ok(())
}

pub(crate) fn update_memory(conn: &Connection, memory: &MemoryRecord) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE memories SET content = ?1, importance = ?2, status = ?3, metadata = ?4, updated_at = ?5
         WHERE namespace_key = ?6 AND memory_id = ?7",
        params![
            memory.content,
            memory.importance,
            memory.status,
            serde_json::to_string(&memory.metadata).map_err(json_err)?,
            memory.updated_at,
            memory.namespace.key(),
            memory.memory_id,
        ],
    )
    .map_err(sql_err)?;
    enqueue_outbox(
        conn,
        &memory.namespace.key(),
        "index_memory",
        serde_json::json!({ "memory_id": memory.memory_id }),
    )?;
    Ok(())
}

pub(crate) fn get_memory(
    conn: &std::sync::Arc<std::sync::Mutex<Connection>>,
    namespace: &Namespace,
    memory_id: &str,
) -> Result<MemoryRecord, StoreError> {
    let conn = conn.lock().map_err(lock_err)?;
    conn.query_row(
        "SELECT memory_id, namespace_json, content, origin, memory_type, importance, status,
                source_event_id, conflict_key, valid_from, valid_until,
                supersedes_json, superseded_by_json, metadata, created_at, updated_at
         FROM memories WHERE namespace_key = ?1 AND memory_id = ?2",
        params![namespace.key(), memory_id],
        memory_from_row,
    )
    .optional()
    .map_err(sql_err)?
    .ok_or(StoreError::NotFound)
}

pub(crate) fn memory_content_exists(
    conn: &Connection,
    namespace_key: &str,
    content: &str,
) -> Result<bool, StoreError> {
    let exists = conn
        .query_row(
            "SELECT 1 FROM memories
             WHERE namespace_key = ?1 AND content = ?2 AND status != 'forgotten'
             LIMIT 1",
            params![namespace_key, content],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(sql_err)?
        .is_some();
    Ok(exists)
}

pub(crate) fn find_superseded_memories(
    conn: &Connection,
    namespace_key: &str,
    conflict_key: &str,
) -> Result<Vec<String>, StoreError> {
    let mut stmt = conn
        .prepare(
            "SELECT memory_id FROM memories
             WHERE namespace_key = ?1 AND conflict_key = ?2 AND status = 'active'
             ORDER BY updated_at DESC
             LIMIT 8",
        )
        .map_err(sql_err)?;
    let rows = stmt
        .query_map(params![namespace_key, conflict_key], |row| {
            row.get::<_, String>(0)
        })
        .map_err(sql_err)?;
    collect_rows(rows)
}

pub(crate) fn create_conflict_suggestions(
    conn: &Connection,
    memory: &MemoryRecord,
) -> Result<usize, StoreError> {
    let Some(conflict_key) = memory.conflict_key.as_deref() else {
        return Ok(0);
    };
    let existing = find_superseded_memories(conn, &memory.namespace.key(), conflict_key)?;
    let mut created = 0;
    for existing_id in existing {
        if existing_id == memory.memory_id || memory.supersedes.iter().any(|id| id == &existing_id)
        {
            continue;
        }
        conn.execute(
            "INSERT OR IGNORE INTO conflict_suggestions
             (id, namespace_key, conflict_key, candidate_memory_id, existing_memory_id, status, suggestion_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'open', ?6, ?7, ?7)",
            params![
                format!("conflict-{}", uuid::Uuid::new_v4()),
                memory.namespace.key(),
                conflict_key,
                memory.memory_id,
                existing_id,
                serde_json::to_string(&serde_json::json!({
                    "suggested_action": "review_supersession",
                    "reason": "same_conflict_key",
                    "candidate_memory_id": memory.memory_id,
                    "existing_memory_id": existing_id
                }))
                .map_err(json_err)?,
                now_rfc3339(),
            ],
        )
        .map_err(sql_err)?;
        if conn.changes() > 0 {
            created += 1;
        }
    }
    Ok(created)
}

pub(crate) fn link_superseded_memories(conn: &Connection, memory: &MemoryRecord) -> Result<(), StoreError> {
    for superseded_id in &memory.supersedes {
        let Some(current_json) = conn
            .query_row(
                "SELECT superseded_by_json FROM memories WHERE namespace_key = ?1 AND memory_id = ?2",
                params![memory.namespace.key(), superseded_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(sql_err)?
        else {
            continue;
        };
        let mut superseded_by =
            serde_json::from_str::<Vec<String>>(&current_json).unwrap_or_default();
        if !superseded_by.iter().any(|id| id == &memory.memory_id) {
            superseded_by.push(memory.memory_id.clone());
            conn.execute(
                "UPDATE memories SET superseded_by_json = ?1, valid_until = COALESCE(valid_until, ?2), updated_at = ?2
                 WHERE namespace_key = ?3 AND memory_id = ?4",
                params![
                    serde_json::to_string(&superseded_by).map_err(json_err)?,
                    memory.created_at,
                    memory.namespace.key(),
                    superseded_id,
                ],
            )
            .map_err(sql_err)?;
        }
    }
    Ok(())
}

pub(crate) fn update_organized_cursor(
    conn: &Connection,
    namespace_key: &str,
    event_pos: i64,
) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO namespace_cursors (namespace_key, organized_event_id, updated_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(namespace_key) DO UPDATE SET
           organized_event_id = MAX(namespace_cursors.organized_event_id, excluded.organized_event_id),
           updated_at = excluded.updated_at",
        params![namespace_key, event_pos, now_rfc3339()],
    )
    .map_err(sql_err)?;
    Ok(())
}

pub(crate) fn conflict_resolution_mode(conn: &Connection, namespace_key: &str) -> Result<String, StoreError> {
    let policy = conn
        .query_row(
            "SELECT policy_json FROM policies WHERE namespace_key = ?1",
            params![namespace_key],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(sql_err)?
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .unwrap_or_else(default_policy);
    Ok(policy
        .get("conflict_resolution_mode")
        .and_then(Value::as_str)
        .unwrap_or("suggest")
        .to_string())
}

pub(crate) fn fail_job(conn: &Connection, job_id: &str, error: &str) -> Result<(), StoreError> {
    let result = serde_json::json!({
        "status": "failed",
        "error": {
            "code": "wrapup_failed",
            "message": error
        }
    });
    conn.execute(
        "UPDATE jobs SET status = 'failed', result_json = ?1, updated_at = ?2 WHERE job_id = ?3",
        params![
            serde_json::to_string(&result).map_err(json_err)?,
            now_rfc3339(),
            job_id
        ],
    )
    .map_err(sql_err)?;
    Ok(())
}

pub(crate) fn fetch_filtered_memories(
    conn: &Connection,
    scope_keys: &[String],
    request: &QueryRequest,
) -> Result<Vec<MemoryRecord>, StoreError> {
    let mut memories = Vec::new();
    for key in scope_keys {
        let mut sql = String::from(
            "SELECT memory_id, namespace_json, content, origin, memory_type, importance, status,
                    source_event_id, conflict_key, valid_from, valid_until,
                    supersedes_json, superseded_by_json, metadata, created_at, updated_at
             FROM memories
             WHERE namespace_key = ?1",
        );
        let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(key.clone())];
        if request.filters.status.is_empty() {
            sql.push_str(" AND status = 'active'");
        } else {
            let placeholders: Vec<String> = request
                .filters
                .status
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", i + 2))
                .collect();
            sql.push_str(&format!(" AND status IN ({})", placeholders.join(",")));
            for s in &request.filters.status {
                params_vec.push(Box::new(s.clone()));
            }
        }
        if !request.filters.memory_types.is_empty() {
            let start = params_vec.len() + 1;
            let placeholders: Vec<String> = request
                .filters
                .memory_types
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", start + i))
                .collect();
            sql.push_str(&format!(" AND memory_type IN ({})", placeholders.join(",")));
            for s in &request.filters.memory_types {
                params_vec.push(Box::new(s.clone()));
            }
        }
        if !request.filters.origins.is_empty() {
            let start = params_vec.len() + 1;
            let placeholders: Vec<String> = request
                .filters
                .origins
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", start + i))
                .collect();
            sql.push_str(&format!(" AND origin IN ({})", placeholders.join(",")));
            for s in &request.filters.origins {
                params_vec.push(Box::new(s.clone()));
            }
        }
        if !request.filters.importance.is_empty() {
            let start = params_vec.len() + 1;
            let placeholders: Vec<String> = request
                .filters
                .importance
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", start + i))
                .collect();
            sql.push_str(&format!(" AND importance IN ({})", placeholders.join(",")));
            for s in &request.filters.importance {
                params_vec.push(Box::new(s.clone()));
            }
        }
        sql.push_str(" ORDER BY updated_at DESC");
        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(sql_err)?;
        let rows = stmt
            .query_map(params_refs.as_slice(), memory_from_row)
            .map_err(sql_err)?;
        memories.extend(
            collect_rows(rows)?
                .into_iter()
                .filter(|memory| {
                    matches_temporal_scope(memory, &request.as_of, &request.temporal_scope)
                }),
        );
    }
    memories = crate::graph::filter_graph_candidates(conn, memories, &request.filters)?;
    Ok(memories)
}

use rusqlite::OptionalExtension;
