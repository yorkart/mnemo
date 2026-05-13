use serde::Deserialize;
use serde::Serialize;

use crate::error::MnemoResult;
use crate::namespace::Namespace;
use crate::normalize::{normalize_optional, normalize_required};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSummary {
    session_summary_id: String,
    namespace: Namespace,
    content: String,
    source_event_ids: Vec<String>,
    inferred_memory_ids: Vec<String>,
    generated_at: String,
}

impl SessionSummary {
    pub fn new(
        session_summary_id: impl Into<String>,
        namespace: Namespace,
        content: impl Into<String>,
        source_event_ids: Vec<String>,
        generated_at: impl Into<String>,
    ) -> MnemoResult<Self> {
        Ok(Self {
            session_summary_id: normalize_required(
                "session_summary_id",
                session_summary_id.into(),
            )?,
            namespace,
            content: normalize_required("content", content.into())?,
            source_event_ids: source_event_ids
                .into_iter()
                .filter_map(normalize_optional)
                .collect(),
            inferred_memory_ids: Vec::new(),
            generated_at: normalize_required("generated_at", generated_at.into())?,
        })
    }

    pub fn with_inferred_memory_ids(mut self, inferred_memory_ids: Vec<String>) -> Self {
        self.inferred_memory_ids = inferred_memory_ids
            .into_iter()
            .filter_map(normalize_optional)
            .collect();
        self
    }

    pub fn session_summary_id(&self) -> &str {
        &self.session_summary_id
    }

    pub fn namespace(&self) -> &Namespace {
        &self.namespace
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn source_event_ids(&self) -> &[String] {
        &self.source_event_ids
    }

    pub fn inferred_memory_ids(&self) -> &[String] {
        &self.inferred_memory_ids
    }

    pub fn generated_at(&self) -> &str {
        &self.generated_at
    }
}
