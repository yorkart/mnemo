use std::collections::BTreeMap;

use mnemo_domain::*;
use mnemo_ports::StorageError;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::helpers::{collect_rows, sql_err};
use crate::ranking::tokenize;

#[derive(Clone)]
pub(crate) struct EntityCandidate {
    pub name: String,
    pub kind: String,
}

#[derive(Clone)]
struct RelationCandidate {
    source_name: String,
    target_name: String,
    relation_type: String,
    confidence: f64,
}

pub(crate) fn index_memory_graph(conn: &Connection, memory: &MemoryRecord) -> Result<(), StorageError> {
    let entities = extract_entities(memory);
    let mut entity_ids = BTreeMap::new();
    for entity in entities {
        let entity_id = upsert_entity(conn, &memory.namespace.key(), &entity)?;
        conn.execute(
            "INSERT OR IGNORE INTO memory_entities (memory_id, entity_id, namespace_key, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                memory.memory_id,
                entity_id,
                memory.namespace.key(),
                now_rfc3339()
            ],
        )
        .map_err(sql_err)?;
        entity_ids.insert(entity.name.to_lowercase(), entity_id);
    }
    for relation in extract_relations(memory) {
        let source_id = match entity_ids.get(&relation.source_name.to_lowercase()) {
            Some(id) => id.clone(),
            None => {
                let entity = EntityCandidate {
                    name: relation.source_name.clone(),
                    kind: infer_entity_kind(&relation.source_name),
                };
                upsert_entity(conn, &memory.namespace.key(), &entity)?
            }
        };
        let target_id = match entity_ids.get(&relation.target_name.to_lowercase()) {
            Some(id) => id.clone(),
            None => {
                let entity = EntityCandidate {
                    name: relation.target_name.clone(),
                    kind: infer_entity_kind(&relation.target_name),
                };
                upsert_entity(conn, &memory.namespace.key(), &entity)?
            }
        };
        conn.execute(
            "INSERT OR IGNORE INTO relations
             (relation_id, namespace_key, source_entity_id, target_entity_id, relation_type, memory_id, confidence, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
            params![
                format!("rel-{}", Uuid::new_v4()),
                memory.namespace.key(),
                source_id,
                target_id,
                relation.relation_type,
                memory.memory_id,
                relation.confidence,
                now_rfc3339(),
            ],
        )
        .map_err(sql_err)?;
    }
    Ok(())
}

fn upsert_entity(
    conn: &Connection,
    namespace_key: &str,
    entity: &EntityCandidate,
) -> Result<String, StorageError> {
    if let Some(entity_id) = conn
        .query_row(
            "SELECT entity_id FROM entities WHERE namespace_key = ?1 AND name = ?2 AND kind = ?3",
            params![namespace_key, entity.name, entity.kind],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(sql_err)?
    {
        conn.execute(
            "UPDATE entities SET updated_at = ?1 WHERE entity_id = ?2",
            params![now_rfc3339(), entity_id],
        )
        .map_err(sql_err)?;
        return Ok(entity_id);
    }
    let entity_id = format!("ent-{}", Uuid::new_v4());
    conn.execute(
        "INSERT INTO entities (entity_id, namespace_key, name, kind, aliases_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, '[]', ?5, ?5)",
        params![
            entity_id,
            namespace_key,
            entity.name,
            entity.kind,
            now_rfc3339(),
        ],
    )
    .map_err(sql_err)?;
    Ok(entity_id)
}

fn extract_entities(memory: &MemoryRecord) -> Vec<EntityCandidate> {
    let mut entities = BTreeMap::new();
    entities.insert(
        "user".to_string(),
        EntityCandidate {
            name: "user".to_string(),
            kind: "person".to_string(),
        },
    );
    if let Some(workspace_id) = memory.namespace.workspace_id.as_deref() {
        entities.insert(
            workspace_id.to_lowercase(),
            EntityCandidate {
                name: workspace_id.to_string(),
                kind: "workspace".to_string(),
            },
        );
    }
    for token in tokenize(&memory.content) {
        if token.chars().count() < 2 {
            continue;
        }
        let kind = infer_entity_kind(&token);
        if kind == "concept" && token.chars().all(|ch| ch.is_ascii_lowercase()) {
            continue;
        }
        entities
            .entry(token.to_lowercase())
            .or_insert(EntityCandidate { name: token, kind });
    }
    entities.into_values().take(16).collect()
}

fn extract_relations(memory: &MemoryRecord) -> Vec<RelationCandidate> {
    let mut relations = Vec::new();
    if memory.memory_type == "preference" {
        relations.push(RelationCandidate {
            source_name: "user".to_string(),
            target_name: memory
                .conflict_key
                .as_deref()
                .unwrap_or("preference")
                .to_string(),
            relation_type: "prefers".to_string(),
            confidence: 0.72,
        });
    }
    if memory.memory_type == "project_context" {
        relations.push(RelationCandidate {
            source_name: memory
                .namespace
                .workspace_id
                .clone()
                .unwrap_or_else(|| "workspace".to_string()),
            target_name: "project_context".to_string(),
            relation_type: "has_context".to_string(),
            confidence: 0.65,
        });
    }
    relations
}

fn infer_entity_kind(name: &str) -> String {
    let lower = name.to_lowercase();
    if matches!(lower.as_str(), "rust" | "python" | "typescript" | "java") {
        "technology".to_string()
    } else if lower.contains("project") || lower.contains("项目") {
        "project".to_string()
    } else if lower.contains("preference") || lower.contains("偏好") {
        "concept".to_string()
    } else if name == "user" {
        "person".to_string()
    } else {
        "concept".to_string()
    }
}

pub(crate) fn filter_graph_candidates(
    conn: &Connection,
    memories: Vec<MemoryRecord>,
    filters: &QueryFilters,
) -> Result<Vec<MemoryRecord>, StorageError> {
    if filters.entity_ids.is_empty() && filters.relation_types.is_empty() {
        return Ok(memories);
    }
    let mut out = Vec::new();
    for memory in memories {
        if !filters.entity_ids.is_empty() && !memory_has_entity(conn, &memory, &filters.entity_ids)?
        {
            continue;
        }
        if !filters.relation_types.is_empty()
            && !memory_has_relation_type(conn, &memory, &filters.relation_types)?
        {
            continue;
        }
        out.push(memory);
    }
    Ok(out)
}

fn memory_has_entity(
    conn: &Connection,
    memory: &MemoryRecord,
    entity_ids: &[String],
) -> Result<bool, StorageError> {
    for entity_id in entity_ids {
        let exists = conn
            .query_row(
                "SELECT 1 FROM memory_entities
                 WHERE namespace_key = ?1 AND memory_id = ?2 AND entity_id = ?3
                 LIMIT 1",
                params![memory.namespace.key(), memory.memory_id, entity_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(sql_err)?
            .is_some();
        if exists {
            return Ok(true);
        }
    }
    Ok(false)
}

fn memory_has_relation_type(
    conn: &Connection,
    memory: &MemoryRecord,
    relation_types: &[String],
) -> Result<bool, StorageError> {
    for relation_type in relation_types {
        let exists = conn
            .query_row(
                "SELECT 1 FROM relations
                 WHERE namespace_key = ?1 AND memory_id = ?2 AND relation_type = ?3
                 LIMIT 1",
                params![memory.namespace.key(), memory.memory_id, relation_type],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(sql_err)?
            .is_some();
        if exists {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn attach_phase4_metadata(conn: &Connection, memory: &mut MemoryRecord) -> Result<(), StorageError> {
    let entities = load_memory_entities(conn, memory)?;
    let relations = load_memory_relations(conn, memory)?;
    let conflicts = load_memory_conflict_suggestions(conn, memory)?;
    if entities.is_empty() && relations.is_empty() && conflicts.is_empty() {
        return Ok(());
    }
    let mut metadata = match memory.metadata.clone() {
        Value::Object(map) => map,
        _ => serde_json::Map::new(),
    };
    metadata.insert(
        "phase4".to_string(),
        json!({
            "entities": entities,
            "relations": relations,
            "conflict_suggestions": conflicts
        }),
    );
    memory.metadata = Value::Object(metadata);
    Ok(())
}

fn load_memory_entities(
    conn: &Connection,
    memory: &MemoryRecord,
) -> Result<Vec<Value>, StorageError> {
    let mut stmt = conn
        .prepare(
            "SELECT e.entity_id, e.name, e.kind
             FROM entities e
             JOIN memory_entities me ON me.entity_id = e.entity_id
             WHERE me.namespace_key = ?1 AND me.memory_id = ?2
             ORDER BY e.kind, e.name
             LIMIT 32",
        )
        .map_err(sql_err)?;
    let rows = stmt
        .query_map(params![memory.namespace.key(), memory.memory_id], |row| {
            Ok(json!({
                "entity_id": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "kind": row.get::<_, String>(2)?
            }))
        })
        .map_err(sql_err)?;
    collect_rows(rows)
}

fn load_memory_relations(
    conn: &Connection,
    memory: &MemoryRecord,
) -> Result<Vec<Value>, StorageError> {
    let mut stmt = conn
        .prepare(
            "SELECT relation_id, source_entity_id, target_entity_id, relation_type, confidence
             FROM relations
             WHERE namespace_key = ?1 AND memory_id = ?2
             ORDER BY relation_type
             LIMIT 32",
        )
        .map_err(sql_err)?;
    let rows = stmt
        .query_map(params![memory.namespace.key(), memory.memory_id], |row| {
            Ok(json!({
                "relation_id": row.get::<_, String>(0)?,
                "source_entity_id": row.get::<_, String>(1)?,
                "target_entity_id": row.get::<_, String>(2)?,
                "relation_type": row.get::<_, String>(3)?,
                "confidence": row.get::<_, f64>(4)?
            }))
        })
        .map_err(sql_err)?;
    collect_rows(rows)
}

fn load_memory_conflict_suggestions(
    conn: &Connection,
    memory: &MemoryRecord,
) -> Result<Vec<Value>, StorageError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, conflict_key, candidate_memory_id, existing_memory_id, status, suggestion_json
             FROM conflict_suggestions
             WHERE namespace_key = ?1 AND (candidate_memory_id = ?2 OR existing_memory_id = ?2)
             ORDER BY created_at DESC
             LIMIT 32",
        )
        .map_err(sql_err)?;
    let rows = stmt
        .query_map(params![memory.namespace.key(), memory.memory_id], |row| {
            let suggestion_json: String = row.get(5)?;
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "conflict_key": row.get::<_, String>(1)?,
                "candidate_memory_id": row.get::<_, String>(2)?,
                "existing_memory_id": row.get::<_, String>(3)?,
                "status": row.get::<_, String>(4)?,
                "suggestion": serde_json::from_str::<Value>(&suggestion_json).unwrap_or(Value::Null)
            }))
        })
        .map_err(sql_err)?;
    collect_rows(rows)
}

#[derive(Default)]
pub(crate) struct MaintenanceSummary {
    pub expired_count: usize,
    pub entity_count: usize,
    pub relation_count: usize,
}

impl From<MaintenanceSummary> for mnemo_ports::MaintenanceResult {
    fn from(s: MaintenanceSummary) -> Self {
        mnemo_ports::MaintenanceResult {
            expired_count: s.expired_count,
            entity_count: s.entity_count,
            relation_count: s.relation_count,
        }
    }
}

pub(crate) fn run_maintenance(
    conn: &Connection,
    namespace_key: &str,
) -> Result<MaintenanceSummary, StorageError> {
    let expired_count = 0;
    let entity_count = conn
        .query_row(
            "SELECT COUNT(*) FROM entities WHERE namespace_key = ?1",
            params![namespace_key],
            |row| row.get::<_, i64>(0),
        )
        .map_err(sql_err)? as usize;
    let relation_count = conn
        .query_row(
            "SELECT COUNT(*) FROM relations WHERE namespace_key = ?1",
            params![namespace_key],
            |row| row.get::<_, i64>(0),
        )
        .map_err(sql_err)? as usize;
    Ok(MaintenanceSummary {
        expired_count,
        entity_count,
        relation_count,
    })
}
