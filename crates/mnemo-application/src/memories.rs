use mnemo_domain::*;
use mnemo_ports::StorageError;

use crate::app::{require_user_namespace, MnemoApp};

impl MnemoApp {
    pub async fn create_memory(
        &self,
        token: Option<&str>,
        request: MemoryCreateRequest,
    ) -> Result<MemoryRecord, StorageError> {
        let namespace = request.namespace.canonical();
        require_user_namespace(&namespace)?;
        self.check_auth(token, &namespace, "memories:write").await?;
        if request.content.trim().is_empty() {
            return Err(StorageError::InvalidRequest(
                "content is required".to_string(),
            ));
        }

        let request_hash = serde_json::to_string(&serde_json::json!({
            "content": &request.content,
            "memory_type": &request.memory_type,
            "importance": &request.importance,
            "source_event_id": &request.source_event_id,
            "conflict_key": &request.conflict_key,
            "valid_from": &request.valid_from,
            "valid_until": &request.valid_until,
            "supersedes": &request.supersedes,
            "metadata": &request.metadata,
        }))
        .unwrap_or_default();
        let idempotency_scope = request
            .idempotency_key
            .as_deref()
            .filter(|key| !key.is_empty())
            .map(|key| format!("{}|POST /v1/memories|{}", namespace.key(), key));

        if let Some(scope) = &idempotency_scope {
            let mut tx = self.stores.begin().await?;
            if let Some((stored_hash, response_json)) =
                self.stores.check(&mut *tx, scope).await?
            {
                self.stores.commit(tx).await?;
                if stored_hash == request_hash {
                    return serde_json::from_str(&response_json)
                        .map_err(|e| StorageError::Storage(e.to_string()));
                } else {
                    return Err(StorageError::IdempotencyConflict(
                        "request body changed for same idempotency key".to_string(),
                    ));
                }
            }
            self.stores.rollback(tx).await?;
        }

        let now = now_rfc3339();
        let memory = MemoryRecord {
            memory_id: format!("mem-{}", uuid::Uuid::new_v4()),
            namespace: namespace.clone(),
            content: request.content,
            origin: "explicit".to_string(),
            memory_type: request.memory_type,
            importance: request.importance,
            status: "active".to_string(),
            source_event_id: request.source_event_id,
            conflict_key: request.conflict_key,
            valid_from: request.valid_from,
            valid_until: request.valid_until,
            supersedes: request.supersedes,
            superseded_by: Vec::new(),
            metadata: request.metadata,
            created_at: now.clone(),
            updated_at: now,
        };
        let mut tx = self.stores.begin().await?;
        self.stores.create_memory(&mut *tx, &memory).await?;
        if let Some(scope) = &idempotency_scope {
            let response_json = serde_json::to_string(&memory)
                .map_err(|e| StorageError::Storage(e.to_string()))?;
            self.stores
                .record(&mut *tx, scope, &request_hash, &response_json)
                .await?;
        }
        self.stores.commit(tx).await?;
        Ok(memory)
    }

    pub async fn query_memories(
        &self,
        token: Option<&str>,
        request: QueryRequest,
    ) -> Result<QueryResponse, StorageError> {
        let namespace = request.namespace.canonical();
        require_user_namespace(&namespace)?;
        self.check_auth_scope(token, &namespace, request.scope.as_ref(), "memories:read")
            .await?;
        let scope_keys = namespace.memory_scope_keys(request.scope.as_ref());
        let limit = request.limit.clamp(1, 200);
        let memories = self
            .stores
            .query_memories(&namespace, &scope_keys, &request)
            .await?;
        let ranked = self
            .stores
            .rank_memories(&namespace, memories, &request.query)
            .await?;
        let results: Vec<QueryResult> = ranked
            .into_iter()
            .take(limit)
            .map(|r| {
                let provenance = serde_json::json!({
                    "event_id": r.memory.source_event_id,
                    "source": r.memory.namespace.source,
                    "signals": {
                        "lexical": r.lexical_score,
                        "vector": r.vector_score,
                        "importance": r.importance_score,
                        "usage": r.usage_score
                    }
                });
                QueryResult {
                    memory_id: r.memory.memory_id,
                    content: r.memory.content,
                    memory_type: r.memory.memory_type,
                    status: r.memory.status,
                    valid_from: r.memory.valid_from,
                    valid_until: r.memory.valid_until,
                    conflict_key: r.memory.conflict_key,
                    supersedes: r.memory.supersedes,
                    superseded_by: r.memory.superseded_by,
                    score: r.score,
                    provenance,
                }
            })
            .collect();
        Ok(QueryResponse {
            query_request_id: format!("qry-{}", uuid::Uuid::new_v4()),
            response_format: request.response_format,
            results,
            backend: BackendMetadata {
                mode: "hybrid_local".to_string(),
                degraded: false,
                reason: Some("local_lexical_and_hash_embedding".to_string()),
            },
        })
    }

    pub async fn search_memories(
        &self,
        token: Option<&str>,
        request: SearchRequest,
    ) -> Result<(Vec<MemoryRecord>, PageInfo), StorageError> {
        let namespace = request.namespace.canonical();
        require_user_namespace(&namespace)?;
        self.check_auth(token, &namespace, "memories:read").await?;
        self.stores.search_memories(&namespace, &request).await
    }

    pub async fn get_memory(
        &self,
        token: Option<&str>,
        namespace: Namespace,
        memory_id: String,
    ) -> Result<MemoryRecord, StorageError> {
        let namespace = namespace.canonical();
        require_user_namespace(&namespace)?;
        self.check_auth(token, &namespace, "memories:read").await?;
        self.stores.get_memory(&namespace, &memory_id).await
    }

    pub async fn patch_memory(
        &self,
        token: Option<&str>,
        namespace: Namespace,
        memory_id: String,
        patch: MemoryPatch,
    ) -> Result<MemoryRecord, StorageError> {
        let namespace = patch
            .namespace
            .as_ref()
            .unwrap_or(&namespace)
            .canonical();
        require_user_namespace(&namespace)?;
        self.check_auth(token, &namespace, "memories:write").await?;
        let mut memory = self.stores.get_memory(&namespace, &memory_id).await?;
        if let Some(content) = patch.content {
            memory.content = content;
        }
        if let Some(importance) = patch.importance {
            memory.importance = importance;
        }
        if let Some(status) = patch.status {
            memory.status = status;
        }
        if let Some(metadata) = patch.metadata {
            memory.metadata = metadata;
        }
        memory.updated_at = now_rfc3339();
        let mut tx = self.stores.begin().await?;
        self.stores.update_memory(&mut *tx, &memory).await?;
        self.stores.commit(tx).await?;
        Ok(memory)
    }

    pub async fn forget(
        &self,
        token: Option<&str>,
        request: ForgetRequest,
    ) -> Result<JobRecord, StorageError> {
        let namespace = request.namespace.canonical();
        require_user_namespace(&namespace)?;
        self.check_auth(token, &namespace, "memories:delete").await?;
        if request.target.memory_ids.is_empty() {
            return Err(StorageError::InvalidRequest(
                "at least one memory_id is required".to_string(),
            ));
        }
        let now = now_rfc3339();
        let mut tx = self.stores.begin().await?;
        self.stores
            .create_tombstone(
                &mut *tx,
                &namespace,
                &request.target,
                &request.mode,
                request.reason.as_deref(),
            )
            .await?;
        self.stores
            .mark_forgotten(
                &mut *tx,
                &namespace,
                &request.target.memory_ids,
                &request.mode,
            )
            .await?;
        let job = JobRecord {
            job_id: format!("job-{}", uuid::Uuid::new_v4()),
            namespace: namespace.clone(),
            job_type: "forget".to_string(),
            status: "completed".to_string(),
            created_at: now,
            result: serde_json::json!({
                "target": request.target,
                "mode": request.mode,
                "affected_memories": request.target.memory_ids.len(),
            }),
        };
        self.stores.create_job(&mut *tx, &job).await?;
        self.stores.commit(tx).await?;
        Ok(job)
    }
}
