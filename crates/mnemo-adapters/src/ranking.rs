use std::collections::BTreeMap;

use mnemo_domain::*;
use mnemo_ports::{RankedMemory, StorageError};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use crate::helpers::{json_err, memory_from_row, sql_err};

pub(crate) fn upsert_memory_index(
    conn: &Connection,
    namespace_key: &str,
    memory_id: &str,
) -> Result<(), StorageError> {
    let memory = conn
        .query_row(
            "SELECT memory_id, namespace_json, content, origin, memory_type, importance, status,
                    source_event_id, conflict_key, valid_from, valid_until,
                    supersedes_json, superseded_by_json, metadata, created_at, updated_at
             FROM memories
             WHERE namespace_key = ?1 AND memory_id = ?2",
            params![namespace_key, memory_id],
            memory_from_row,
        )
        .optional()
        .map_err(sql_err)?;
    let Some(memory) = memory else {
        return Ok(());
    };
    if memory.status != "active" {
        conn.execute(
            "DELETE FROM memory_index WHERE namespace_key = ?1 AND memory_id = ?2",
            params![namespace_key, memory_id],
        )
        .map_err(sql_err)?;
        return Ok(());
    }
    let terms = tokenize(&format!(
        "{} {} {}",
        memory.content, memory.memory_type, memory.importance
    ));
    let embedding = embed_text(&memory.content);
    conn.execute(
        "INSERT INTO memory_index (memory_id, namespace_key, terms_json, embedding_json, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(memory_id) DO UPDATE SET
           namespace_key = excluded.namespace_key,
           terms_json = excluded.terms_json,
           embedding_json = excluded.embedding_json,
           updated_at = excluded.updated_at",
        params![
            memory.memory_id,
            namespace_key,
            serde_json::to_string(&terms).map_err(json_err)?,
            serde_json::to_string(&embedding).map_err(json_err)?,
            now_rfc3339(),
        ],
    )
    .map_err(sql_err)?;
    Ok(())
}

fn load_index_embedding(conn: &Connection, memory: &MemoryRecord) -> Result<Vec<f64>, StorageError> {
    let embedding_json = conn
        .query_row(
            "SELECT embedding_json FROM memory_index WHERE memory_id = ?1 AND namespace_key = ?2",
            params![memory.memory_id, memory.namespace.key()],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(sql_err)?;
    if let Some(embedding_json) = embedding_json {
        if let Ok(embedding) = serde_json::from_str::<Vec<f64>>(&embedding_json) {
            return Ok(embedding);
        }
    }
    Ok(embed_text(&memory.content))
}

#[derive(Default, Clone)]
struct UsageStats {
    usage_count: i64,
    positive_count: i64,
    negative_count: i64,
}

fn load_usage_stats(
    conn: &Connection,
    memory_ids: &[String],
) -> Result<BTreeMap<String, UsageStats>, StorageError> {
    let mut out = BTreeMap::new();
    for memory_id in memory_ids {
        if let Some(stats) = conn
            .query_row(
                "SELECT usage_count, positive_count, negative_count
                 FROM usage_aggregate WHERE memory_id = ?1",
                params![memory_id],
                |row| {
                    Ok(UsageStats {
                        usage_count: row.get(0)?,
                        positive_count: row.get(1)?,
                        negative_count: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(sql_err)?
        {
            out.insert(memory_id.clone(), stats);
        }
    }
    Ok(out)
}

pub(crate) fn aggregate_usage_feedback(
    conn: &Connection,
    namespace_key: &str,
    used_items: &[Value],
) -> Result<(), StorageError> {
    let now = now_rfc3339();
    for item in used_items {
        let Some(memory_id) = item.get("memory_id").and_then(Value::as_str) else {
            continue;
        };
        let usage = item
            .get("usage")
            .or_else(|| item.get("outcome"))
            .and_then(Value::as_str)
            .unwrap_or("used");
        let (positive, negative) = usage_polarity(usage);
        conn.execute(
            "INSERT INTO usage_aggregate
             (memory_id, namespace_key, usage_count, positive_count, negative_count, last_used_at, updated_at)
             VALUES (?1, ?2, 1, ?3, ?4, ?5, ?5)
             ON CONFLICT(memory_id) DO UPDATE SET
               usage_count = usage_count + 1,
               positive_count = positive_count + excluded.positive_count,
               negative_count = negative_count + excluded.negative_count,
               last_used_at = excluded.last_used_at,
               updated_at = excluded.updated_at",
            params![memory_id, namespace_key, positive, negative, now],
        )
        .map_err(sql_err)?;
    }
    Ok(())
}

fn usage_polarity(usage: &str) -> (i64, i64) {
    match usage {
        "rejected" | "unused" | "irrelevant" | "bad" => (0, 1),
        _ => (1, 0),
    }
}

pub(crate) fn rank_memories(
    conn: &Connection,
    memories: Vec<MemoryRecord>,
    query: &str,
) -> Result<Vec<RankedMemory>, StorageError> {
    let query_tokens = tokenize(query);
    let query_embedding = embed_text(query);
    let memory_ids = memories
        .iter()
        .map(|memory| memory.memory_id.clone())
        .collect::<Vec<_>>();
    let usage_stats = load_usage_stats(conn, &memory_ids)?;

    let mut scored = memories
        .into_iter()
        .map(|memory| {
            let terms = tokenize(&format!("{} {}", memory.content, memory.memory_type));
            let lexical_score = lexical_score(&query_tokens, &terms, query, &memory.content);
            let embedding = load_index_embedding(conn, &memory)?;
            let vector_score = if query.trim().is_empty() {
                0.0
            } else {
                cosine_similarity(&query_embedding, &embedding).max(0.0)
            };
            let importance_score = importance_score(&memory.importance);
            let usage_score = usage_stats
                .get(&memory.memory_id)
                .map(usage_score_fn)
                .unwrap_or(0.0);
            Ok(RankedMemory {
                memory,
                score: 0.0,
                lexical_score,
                vector_score,
                importance_score,
                usage_score,
            })
        })
        .collect::<Result<Vec<_>, StorageError>>()?;

    let lexical_ranks = ranks_by(&scored, |item| item.lexical_score);
    let vector_ranks = ranks_by(&scored, |item| item.vector_score);
    let metadata_ranks = ranks_by(&scored, |item| item.importance_score + item.usage_score);
    let max_rrf = scored
        .iter()
        .map(|item| {
            rrf_score(&item.memory.memory_id, &lexical_ranks)
                + rrf_score(&item.memory.memory_id, &vector_ranks)
                + rrf_score(&item.memory.memory_id, &metadata_ranks)
        })
        .fold(0.0_f64, f64::max)
        .max(1.0);

    for item in &mut scored {
        let rrf = rrf_score(&item.memory.memory_id, &lexical_ranks)
            + rrf_score(&item.memory.memory_id, &vector_ranks)
            + rrf_score(&item.memory.memory_id, &metadata_ranks);
        let rrf_normalized = rrf / max_rrf;
        item.score = (0.60 * rrf_normalized
            + 0.18 * item.importance_score
            + 0.14 * item.usage_score
            + 0.05 * item.lexical_score
            + 0.03 * item.vector_score)
            .clamp(0.0, 1.0);
    }
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.memory.updated_at.cmp(&a.memory.updated_at))
    });
    Ok(scored)
}

fn ranks_by(
    items: &[RankedMemory],
    score_fn: impl Fn(&RankedMemory) -> f64,
) -> BTreeMap<String, usize> {
    let mut ranked = items
        .iter()
        .filter_map(|item| {
            let score = score_fn(item);
            (score > 0.0).then(|| (item.memory.memory_id.clone(), score))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked
        .into_iter()
        .enumerate()
        .map(|(idx, (memory_id, _))| (memory_id, idx + 1))
        .collect()
}

fn rrf_score(memory_id: &str, ranks: &BTreeMap<String, usize>) -> f64 {
    ranks
        .get(memory_id)
        .map(|rank| 1.0 / (60.0 + *rank as f64))
        .unwrap_or(0.0)
}

pub(crate) fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            current.push(ch.to_ascii_lowercase());
            continue;
        }
        if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
        if !ch.is_whitespace() && !ch.is_ascii_punctuation() {
            tokens.push(ch.to_string());
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn lexical_score(query_tokens: &[String], terms: &[String], raw_query: &str, content: &str) -> f64 {
    if query_tokens.is_empty() {
        return 0.0;
    }
    let matched = query_tokens
        .iter()
        .filter(|token| terms.iter().any(|term| term == *token))
        .count();
    let overlap = matched as f64 / query_tokens.len().max(1) as f64;
    let phrase_bonus = if !raw_query.trim().is_empty()
        && content
            .to_lowercase()
            .contains(&raw_query.trim().to_lowercase())
    {
        0.2
    } else {
        0.0
    };
    (overlap + phrase_bonus).clamp(0.0, 1.0)
}

pub(crate) fn embed_text(text: &str) -> Vec<f64> {
    const DIM: usize = 64;
    let mut vector = vec![0.0; DIM];
    for token in tokenize(text) {
        let hash = stable_hash(&token);
        let index = (hash as usize) % DIM;
        let sign = if (hash >> 63) == 0 { 1.0 } else { -1.0 };
        vector[index] += sign;
    }
    let norm = vector.iter().map(|value| value * value).sum::<f64>().sqrt();
    if norm > 0.0 {
        for value in &mut vector {
            *value /= norm;
        }
    }
    vector
}

fn stable_hash(value: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in value.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
    a.iter()
        .zip(b.iter())
        .map(|(left, right)| left * right)
        .sum()
}

fn importance_score(importance: &str) -> f64 {
    match importance {
        "critical" => 1.0,
        "high" => 0.82,
        "low" => 0.25,
        _ => 0.55,
    }
}

fn usage_score_fn(stats: &UsageStats) -> f64 {
    let positive = stats.positive_count.max(0) as f64;
    let negative = stats.negative_count.max(0) as f64;
    let total = stats.usage_count.max(1) as f64;
    let polarity = ((positive - negative) / total).max(0.0);
    let frequency = (1.0 + total).ln() / 6.0;
    (0.7 * polarity + 0.3 * frequency).clamp(0.0, 1.0)
}
