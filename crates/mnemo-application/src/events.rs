use mnemo_domain::*;
use mnemo_ports::StorageError;
use serde_json::Value;

use crate::app::{require_user_namespace, MnemoApp};

impl MnemoApp {
    pub async fn ingest_event(
        &self,
        token: Option<&str>,
        request: EventInput,
    ) -> Result<EventWriteResult, StorageError> {
        let namespace = request.namespace.canonical();
        require_user_namespace(&namespace)?;
        self.check_auth(token, &namespace, "events:write").await?;
        let mut tx = self.stores.begin().await?;
        let result = self
            .stores
            .append_event(&mut *tx, &namespace, &request)
            .await?;
        self.stores.commit(tx).await?;
        Ok(result)
    }

    pub async fn ingest_events_batch(
        &self,
        token: Option<&str>,
        request: BatchEventsRequest,
    ) -> Result<BatchEventsResponse, StorageError> {
        let namespace = request.namespace.canonical();
        require_user_namespace(&namespace)?;
        self.check_auth(token, &namespace, "events:write").await?;

        let total = request.events.len();
        let mut results = Vec::with_capacity(total);
        let mut summary = BatchSummary {
            total,
            ..Default::default()
        };

        for mut event in request.events {
            event.namespace = namespace.clone();
            let event_id = event.event_id.clone();
            let mut tx = self.stores.begin().await?;
            match self
                .stores
                .append_event(&mut *tx, &namespace, &event)
                .await
            {
                Ok(result) => {
                    self.stores.commit(tx).await?;
                    if result.deduplicated {
                        summary.deduplicated += 1;
                    } else {
                        summary.accepted += 1;
                    }
                    results.push(result);
                }
                Err(err) => {
                    let _ = self.stores.rollback(tx).await;
                    summary.failed += 1;
                    results.push(EventWriteResult {
                        event_id,
                        status: "failed".to_string(),
                        deduplicated: false,
                        error: Some(ApiErrorBody {
                            code: err.code().to_string(),
                            message: err.to_string(),
                            request_id: None,
                        }),
                    });
                }
            }
        }

        Ok(BatchEventsResponse { summary, results })
    }

    pub async fn search_events(
        &self,
        token: Option<&str>,
        request: SearchRequest,
    ) -> Result<(Vec<Value>, PageInfo), StorageError> {
        let namespace = request.namespace.canonical();
        require_user_namespace(&namespace)?;
        self.check_auth(token, &namespace, "events:read").await?;
        self.stores.search_events(&namespace, &request).await
    }
}
