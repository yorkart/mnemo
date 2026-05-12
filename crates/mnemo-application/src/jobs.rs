use mnemo_domain::*;
use mnemo_ports::StorageError;
use serde_json::Value;

use crate::app::{require_user_namespace, MnemoApp};

impl MnemoApp {
    pub async fn create_wrapup_job(
        &self,
        token: Option<&str>,
        request: WrapupRequest,
    ) -> Result<JobRecord, StorageError> {
        let namespace = request.namespace.canonical();
        require_user_namespace(&namespace)?;
        self.check_auth(token, &namespace, "jobs:write").await?;
        if let Some(ref instructions) = request.extraction_instructions {
            if instructions.len() > 2000 {
                return Err(StorageError::InvalidRequest(
                    "extraction_instructions must not exceed 2000 characters".to_string(),
                ));
            }
        }
        let now = now_rfc3339();
        let job = JobRecord {
            job_id: format!("job-{}", uuid::Uuid::new_v4()),
            namespace: namespace.clone(),
            job_type: "wrapup".to_string(),
            status: "queued".to_string(),
            created_at: now,
            result: serde_json::json!({
                "status": "queued",
                "stage": "accepted",
                "requested_event_range": request.event_range,
                "extraction_instructions": request.extraction_instructions,
            }),
        };
        let mut tx = self.stores.begin().await?;
        self.stores.create_job(&mut *tx, &job).await?;
        self.stores.commit(tx).await?;
        Ok(job)
    }

    pub async fn list_jobs(
        &self,
        token: Option<&str>,
        namespace: Namespace,
        limit: usize,
        cursor: Option<String>,
    ) -> Result<(Vec<JobRecord>, PageInfo), StorageError> {
        let namespace = namespace.canonical();
        require_user_namespace(&namespace)?;
        self.check_auth(token, &namespace, "jobs:read").await?;
        self.stores
            .list_jobs(&namespace, limit, cursor.as_deref())
            .await
    }

    pub async fn get_job(
        &self,
        token: Option<&str>,
        namespace: Namespace,
        job_id: String,
    ) -> Result<JobRecord, StorageError> {
        let namespace = namespace.canonical();
        require_user_namespace(&namespace)?;
        self.check_auth(token, &namespace, "jobs:read").await?;
        self.stores.get_job(&namespace, &job_id).await
    }

    pub async fn namespace_status(
        &self,
        token: Option<&str>,
        request: NamespaceStatusRequest,
    ) -> Result<NamespaceStats, StorageError> {
        let namespace = request.namespace.canonical();
        require_user_namespace(&namespace)?;
        self.check_auth(token, &namespace, "memories:read").await?;

        let event_info = self.stores.namespace_event_status(&namespace).await?;
        let organized = self
            .stores
            .get_organized_cursor(&namespace)
            .await
            .unwrap_or(0);
        let pending_events = (event_info.latest_event_id - organized).max(0) as usize;

        let search = SearchRequest {
            namespace: namespace.clone(),
            query: String::new(),
            filters: QueryFilters::default(),
            pagination: PageRequest {
                limit: 10_000,
                cursor: None,
            },
        };
        let (memories_list, _) = self.stores.search_memories(&namespace, &search).await?;

        let (jobs_list, _) = self.stores.list_jobs(&namespace, 10_000, None).await?;
        let pending_jobs = jobs_list
            .iter()
            .filter(|j| j.status == "queued" || j.status == "running")
            .count();

        Ok(NamespaceStats {
            events: event_info.event_count,
            memories: memories_list.len(),
            pending_jobs,
            latest_event_cursor: if event_info.latest_event_id > 0 {
                Some(event_info.latest_event_id.to_string())
            } else {
                None
            },
            organized_cursor: if organized > 0 {
                Some(organized.to_string())
            } else {
                None
            },
            pending_events,
        })
    }

    pub async fn get_policy(
        &self,
        token: Option<&str>,
        namespace: Namespace,
    ) -> Result<Value, StorageError> {
        let namespace = namespace.canonical();
        require_user_namespace(&namespace)?;
        self.check_auth(token, &namespace, "policy:read").await?;
        self.stores.get_policy(&namespace).await
    }

    pub async fn put_policy(
        &self,
        token: Option<&str>,
        request: PolicyRequest,
    ) -> Result<Value, StorageError> {
        let namespace = request.namespace.canonical();
        require_user_namespace(&namespace)?;
        self.check_auth(token, &namespace, "policy:write").await?;
        let mut tx = self.stores.begin().await?;
        self.stores
            .put_policy(&mut *tx, &namespace, &request.policy)
            .await?;
        self.stores.commit(tx).await?;
        Ok(request.policy)
    }

    pub async fn run_jobs_once(&self, limit: usize) -> Result<usize, StorageError> {
        self.stores.process_queued_jobs(limit).await
    }

    pub async fn run_outbox_once(&self, limit: usize) -> Result<usize, StorageError> {
        self.stores.process_pending(limit).await
    }
}
