use serde::Deserialize;
use serde::Serialize;

use crate::error::MnemoResult;
use crate::memory::Memory;
use crate::normalize::normalize_required;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextPackItem {
    item_id: String,
    memory_id: String,
    summary: String,
    memory_type: String,
    status: String,
    provenance_event_ids: Vec<String>,
}

impl ContextPackItem {
    pub fn from_memory(memory: &Memory) -> Self {
        Self {
            item_id: format!("ctx-item-{}", memory.memory_id()),
            memory_id: memory.memory_id().to_string(),
            summary: memory.content().to_string(),
            memory_type: memory.memory_type().to_string(),
            status: memory.status().as_str().to_string(),
            provenance_event_ids: memory.source_event_ids().to_vec(),
        }
    }

    pub fn item_id(&self) -> &str {
        &self.item_id
    }

    pub fn memory_id(&self) -> &str {
        &self.memory_id
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }

    pub fn memory_type(&self) -> &str {
        &self.memory_type
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn provenance_event_ids(&self) -> &[String] {
        &self.provenance_event_ids
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextPackCacheEntry {
    cache_key: String,
    context_pack_id: String,
    version: String,
    generated_at: String,
    content: String,
    max_tokens: usize,
    estimated_tokens: usize,
    budget_exceeded_items: usize,
    conflicted_items: usize,
    items: Vec<ContextPackItem>,
}

impl ContextPackCacheEntry {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cache_key: impl Into<String>,
        context_pack_id: impl Into<String>,
        version: impl Into<String>,
        generated_at: impl Into<String>,
        content: impl Into<String>,
        max_tokens: usize,
        estimated_tokens: usize,
        budget_exceeded_items: usize,
        conflicted_items: usize,
        items: Vec<ContextPackItem>,
    ) -> MnemoResult<Self> {
        Ok(Self {
            cache_key: normalize_required("cache_key", cache_key.into())?,
            context_pack_id: normalize_required("context_pack_id", context_pack_id.into())?,
            version: normalize_required("version", version.into())?,
            generated_at: normalize_required("generated_at", generated_at.into())?,
            content: normalize_required("content", content.into())?,
            max_tokens,
            estimated_tokens,
            budget_exceeded_items,
            conflicted_items,
            items,
        })
    }

    pub fn cache_key(&self) -> &str {
        &self.cache_key
    }

    pub fn context_pack_id(&self) -> &str {
        &self.context_pack_id
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn generated_at(&self) -> &str {
        &self.generated_at
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn max_tokens(&self) -> usize {
        self.max_tokens
    }

    pub fn estimated_tokens(&self) -> usize {
        self.estimated_tokens
    }

    pub fn budget_exceeded_items(&self) -> usize {
        self.budget_exceeded_items
    }

    pub fn conflicted_items(&self) -> usize {
        self.conflicted_items
    }

    pub fn items(&self) -> &[ContextPackItem] {
        &self.items
    }
}
