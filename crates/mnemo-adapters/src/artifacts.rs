use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use mnemo_domain::*;
use mnemo_ports::StorageError;
use rusqlite::{Connection, params};
use serde_json::{Value, json};

use crate::helpers::{collect_rows, io_err, json_err, memory_from_row, sql_err};
use crate::ranking::upsert_memory_index;

pub(crate) fn refresh_namespace_artifacts(
    conn: &Connection,
    artifact_dir: &Path,
    namespace_key: &str,
) -> Result<(), StorageError> {
    let namespace = namespace_from_key(namespace_key);
    let generated_at = now_rfc3339();
    let namespace_dir = artifact_dir
        .join("namespaces")
        .join(safe_component(namespace_key));
    fs::create_dir_all(namespace_dir.join("periods")).map_err(io_err)?;
    fs::create_dir_all(namespace_dir.join("threads")).map_err(io_err)?;
    fs::create_dir_all(namespace_dir.join("context-packs")).map_err(io_err)?;

    let memories = load_artifact_memories(conn, namespace_key)?;
    let events = load_artifact_events(conn, namespace_key)?;
    let mut files = Vec::new();

    let profile = render_profile(namespace_key, &namespace, &generated_at, &memories);
    write_file(&namespace_dir.join("profile.md"), &profile)?;
    files.push(json!({
        "path": "profile.md",
        "type": "profile",
        "source": "memories",
        "memory_count": memories.len()
    }));

    let mut periods: BTreeMap<String, Vec<ArtifactEvent>> = BTreeMap::new();
    for event in events.iter().cloned() {
        periods
            .entry(event_month(&event, &generated_at))
            .or_default()
            .push(event);
    }
    for (month, month_events) in periods {
        let relative = format!("periods/{month}.md");
        let content = render_period(namespace_key, &generated_at, &month, &month_events);
        write_file(&namespace_dir.join(&relative), &content)?;
        files.push(json!({
            "path": relative,
            "type": "period",
            "source": "events",
            "event_count": month_events.len()
        }));
    }

    if let Some(thread_id) = namespace.thread_id.as_deref() {
        let relative = format!("threads/{}.md", safe_component(thread_id));
        let content = render_thread(namespace_key, &generated_at, thread_id, &events, &memories);
        write_file(&namespace_dir.join(&relative), &content)?;
        files.push(json!({
            "path": relative,
            "type": "thread",
            "source": "events_and_memories",
            "event_count": events.len(),
            "memory_count": memories.len()
        }));
    }

    let index = json!({
        "version": "artifact-v1",
        "namespace_key": namespace_key,
        "namespace": namespace,
        "generated_at": generated_at,
        "source": "durable_store",
        "files": files
    });
    write_file(
        &namespace_dir.join("index.json"),
        &serde_json::to_string_pretty(&index).map_err(json_err)?,
    )?;
    Ok(())
}

pub(crate) fn process_outbox_task(
    conn: &Connection,
    artifact_dir: Option<&Path>,
    namespace_key: &str,
    task_type: &str,
    _payload: &str,
) -> Result<(), StorageError> {
    match task_type {
        "index_memory" => {
            if let Ok(payload) = serde_json::from_str::<Value>(_payload) {
                if let Some(memory_id) = payload.get("memory_id").and_then(Value::as_str) {
                    upsert_memory_index(conn, namespace_key, memory_id)?;
                }
            }
            if let Some(artifact_dir) = artifact_dir {
                refresh_namespace_artifacts(conn, artifact_dir, namespace_key)?;
            }
            Ok(())
        }
        "forget_cascade" => {
            if let Ok(payload) = serde_json::from_str::<Value>(_payload) {
                if let Some(memory_ids) = payload.get("memory_ids").and_then(Value::as_array) {
                    for memory_id in memory_ids.iter().filter_map(Value::as_str) {
                        conn.execute(
                            "DELETE FROM memory_index WHERE namespace_key = ?1 AND memory_id = ?2",
                            params![namespace_key, memory_id],
                        )
                        .map_err(sql_err)?;
                        conn.execute(
                            "DELETE FROM memory_entities WHERE namespace_key = ?1 AND memory_id = ?2",
                            params![namespace_key, memory_id],
                        )
                        .map_err(sql_err)?;
                        conn.execute(
                            "DELETE FROM relations WHERE namespace_key = ?1 AND memory_id = ?2",
                            params![namespace_key, memory_id],
                        )
                        .map_err(sql_err)?;
                    }
                }
            }
            if let Some(artifact_dir) = artifact_dir {
                refresh_namespace_artifacts(conn, artifact_dir, namespace_key)?;
            }
            Ok(())
        }
        "index_event" | "refresh_artifact" => {
            if let Some(artifact_dir) = artifact_dir {
                refresh_namespace_artifacts(conn, artifact_dir, namespace_key)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn load_artifact_memories(
    conn: &Connection,
    namespace_key: &str,
) -> Result<Vec<MemoryRecord>, StorageError> {
    let mut stmt = conn
        .prepare(
            "SELECT memory_id, namespace_json, content, origin, memory_type, importance, status,
                    source_event_id, conflict_key, valid_from, valid_until,
                    supersedes_json, superseded_by_json, metadata, created_at, updated_at
             FROM memories
             WHERE namespace_key = ?1 AND status = 'active'
             ORDER BY updated_at DESC
             LIMIT 200",
        )
        .map_err(sql_err)?;
    let rows = stmt
        .query_map(params![namespace_key], memory_from_row)
        .map_err(sql_err)?;
    collect_rows(rows)
}

#[derive(Clone)]
struct ArtifactEvent {
    event_id: String,
    event_type: Option<String>,
    role: Option<String>,
    content: String,
    occurred_at: Option<String>,
    created_at: String,
}

fn load_artifact_events(
    conn: &Connection,
    namespace_key: &str,
) -> Result<Vec<ArtifactEvent>, StorageError> {
    let mut stmt = conn
        .prepare(
            "SELECT event_id, event_type, role, content, occurred_at, created_at
             FROM events
             WHERE namespace_key = ?1
             ORDER BY id ASC
             LIMIT 500",
        )
        .map_err(sql_err)?;
    let rows = stmt
        .query_map(params![namespace_key], |row| {
            Ok(ArtifactEvent {
                event_id: row.get(0)?,
                event_type: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                occurred_at: row.get(4)?,
                created_at: row.get(5)?,
            })
        })
        .map_err(sql_err)?;
    collect_rows(rows)
}

fn render_profile(
    namespace_key: &str,
    namespace: &Namespace,
    generated_at: &str,
    memories: &[MemoryRecord],
) -> String {
    let mut out = format!(
        "# Mnemo Profile\n\n- namespace_key: `{}`\n- generated_at: `{}`\n- tenant_id: `{}`\n- user_id: `{}`\n\n## Active Memories\n\n",
        namespace_key,
        generated_at,
        namespace.tenant_id.clone().unwrap_or_default(),
        namespace.user_id.clone().unwrap_or_default()
    );
    if memories.is_empty() {
        out.push_str("No active memories.\n");
        return out;
    }
    out.push_str("| Updated | Type | Importance | Memory |\n");
    out.push_str("|---|---|---|---|\n");
    for memory in memories {
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            markdown_cell(&memory.updated_at),
            markdown_cell(&memory.memory_type),
            markdown_cell(&memory.importance),
            markdown_cell(&memory.content),
        ));
    }
    out
}

fn render_period(
    namespace_key: &str,
    generated_at: &str,
    month: &str,
    events: &[ArtifactEvent],
) -> String {
    let mut out = format!(
        "# Mnemo Events {month}\n\n- namespace_key: `{namespace_key}`\n- generated_at: `{generated_at}`\n\n"
    );
    if events.is_empty() {
        out.push_str("No events.\n");
        return out;
    }
    for event in events {
        out.push_str(&format!(
            "## {}\n\n- event_id: `{}`\n- type: `{}`\n- role: `{}`\n- occurred_at: `{}`\n\n{}\n\n",
            event.occurred_at.as_deref().unwrap_or(&event.created_at),
            event.event_id,
            event.event_type.as_deref().unwrap_or(""),
            event.role.as_deref().unwrap_or(""),
            event.occurred_at.as_deref().unwrap_or(""),
            event.content
        ));
    }
    out
}

fn render_thread(
    namespace_key: &str,
    generated_at: &str,
    thread_id: &str,
    events: &[ArtifactEvent],
    memories: &[MemoryRecord],
) -> String {
    let mut out = format!(
        "# Mnemo Thread {thread_id}\n\n- namespace_key: `{namespace_key}`\n- generated_at: `{generated_at}`\n\n## Active Memories\n\n"
    );
    if memories.is_empty() {
        out.push_str("No active memories.\n\n");
    } else {
        for memory in memories {
            out.push_str(&format!("- [{}] {}\n", memory.memory_type, memory.content));
        }
        out.push('\n');
    }
    out.push_str("## Events\n\n");
    if events.is_empty() {
        out.push_str("No events.\n");
    } else {
        for event in events {
            out.push_str(&format!(
                "- `{}` {}: {}\n",
                event.event_id,
                event.role.as_deref().unwrap_or("event"),
                event.content.replace('\n', " ")
            ));
        }
    }
    out
}

fn event_month(event: &ArtifactEvent, fallback: &str) -> String {
    let value = event
        .occurred_at
        .as_deref()
        .filter(|value| value.len() >= 7)
        .unwrap_or_else(|| {
            if event.created_at.len() >= 7 {
                &event.created_at
            } else {
                fallback
            }
        });
    value.chars().take(7).collect()
}

fn namespace_from_key(namespace_key: &str) -> Namespace {
    let mut namespace = Namespace::default();
    for part in namespace_key.split('|') {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        let value = if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        };
        match key {
            "tenant" => namespace.tenant_id = value,
            "user" => namespace.user_id = value,
            "workspace" => namespace.workspace_id = value,
            "thread" => namespace.thread_id = value,
            "agent" => namespace.agent_id = value,
            "source" => namespace.source = value,
            _ => {}
        }
    }
    namespace
}

pub(crate) fn safe_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn markdown_cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', "<br>")
}

fn write_file(path: &Path, content: &str) -> Result<(), StorageError> {
    fs::write(path, content).map_err(io_err)
}
