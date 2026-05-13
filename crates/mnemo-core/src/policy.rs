use serde::Deserialize;
use serde::Serialize;

use crate::error::{MnemoError, MnemoResult};
use crate::namespace::Namespace;
use crate::normalize::normalize_required;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamespacePolicy {
    namespace: Namespace,
    auto_generate_memories: bool,
    auto_use_memories: bool,
    context_pack_max_tokens: usize,
    external_context_policy: String,
    conflict_resolution_mode: String,
    max_unused_days: Option<u64>,
    max_thread_age_days: Option<u64>,
    min_thread_idle_seconds: Option<u64>,
}

impl NamespacePolicy {
    pub fn default_for(namespace: Namespace) -> Self {
        Self {
            namespace,
            auto_generate_memories: true,
            auto_use_memories: true,
            context_pack_max_tokens: 1200,
            external_context_policy: "ignore_unless_explicit".to_string(),
            conflict_resolution_mode: "supersede_by_conflict_key".to_string(),
            max_unused_days: Some(180),
            max_thread_age_days: Some(90),
            min_thread_idle_seconds: Some(300),
        }
    }

    pub fn namespace(&self) -> &Namespace {
        &self.namespace
    }

    pub fn auto_generate_memories(&self) -> bool {
        self.auto_generate_memories
    }

    pub fn auto_use_memories(&self) -> bool {
        self.auto_use_memories
    }

    pub fn context_pack_max_tokens(&self) -> usize {
        self.context_pack_max_tokens
    }

    pub fn external_context_policy(&self) -> &str {
        &self.external_context_policy
    }

    pub fn conflict_resolution_mode(&self) -> &str {
        &self.conflict_resolution_mode
    }

    pub fn max_unused_days(&self) -> Option<u64> {
        self.max_unused_days
    }

    pub fn max_thread_age_days(&self) -> Option<u64> {
        self.max_thread_age_days
    }

    pub fn min_thread_idle_seconds(&self) -> Option<u64> {
        self.min_thread_idle_seconds
    }

    pub fn set_auto_generate_memories(&mut self, value: bool) {
        self.auto_generate_memories = value;
    }

    pub fn set_auto_use_memories(&mut self, value: bool) {
        self.auto_use_memories = value;
    }

    pub fn set_context_pack_max_tokens(&mut self, value: usize) -> MnemoResult<()> {
        if value == 0 {
            return Err(MnemoError::InvalidRequest(
                "context_pack_max_tokens must be greater than zero".to_string(),
            ));
        }
        self.context_pack_max_tokens = value;
        Ok(())
    }

    pub fn set_external_context_policy(&mut self, value: impl Into<String>) -> MnemoResult<()> {
        self.external_context_policy = normalize_required("external_context_policy", value.into())?;
        Ok(())
    }

    pub fn set_conflict_resolution_mode(&mut self, value: impl Into<String>) -> MnemoResult<()> {
        self.conflict_resolution_mode =
            normalize_required("conflict_resolution_mode", value.into())?;
        Ok(())
    }

    pub fn set_max_unused_days(&mut self, value: Option<u64>) {
        self.max_unused_days = value;
    }

    pub fn set_max_thread_age_days(&mut self, value: Option<u64>) {
        self.max_thread_age_days = value;
    }

    pub fn set_min_thread_idle_seconds(&mut self, value: Option<u64>) {
        self.min_thread_idle_seconds = value;
    }
}
