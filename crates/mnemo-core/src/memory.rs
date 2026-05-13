use serde::Deserialize;
use serde::Serialize;

use crate::error::{MnemoError, MnemoResult};
use crate::namespace::Namespace;
use crate::normalize::{normalize_optional, normalize_required};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThreadMemoryMode {
    Enabled,
    Disabled,
    Polluted,
}

impl ThreadMemoryMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
            Self::Polluted => "polluted",
        }
    }

    pub fn allows_inferred_memory(self) -> bool {
        matches!(self, Self::Enabled)
    }

    pub fn parse(value: &str) -> MnemoResult<Self> {
        match value {
            "enabled" => Ok(Self::Enabled),
            "disabled" => Ok(Self::Disabled),
            "polluted" => Ok(Self::Polluted),
            other => Err(MnemoError::InvalidRequest(format!(
                "unsupported thread memory mode: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryOrigin {
    Explicit,
    Inferred,
    Manual,
}

impl MemoryOrigin {
    pub fn parse(value: &str) -> MnemoResult<Self> {
        match value {
            "explicit" => Ok(Self::Explicit),
            "inferred" => Ok(Self::Inferred),
            "manual" => Ok(Self::Manual),
            other => Err(MnemoError::InvalidRequest(format!(
                "unsupported memory origin: {other}"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::Inferred => "inferred",
            Self::Manual => "manual",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryStatus {
    Active,
    Inactive,
    Superseded,
    Conflicted,
    Expired,
    Forgotten,
}

impl MemoryStatus {
    pub fn parse(value: &str) -> MnemoResult<Self> {
        match value {
            "active" => Ok(Self::Active),
            "inactive" => Ok(Self::Inactive),
            "superseded" => Ok(Self::Superseded),
            "conflicted" => Ok(Self::Conflicted),
            "expired" => Ok(Self::Expired),
            "forgotten" => Ok(Self::Forgotten),
            other => Err(MnemoError::InvalidRequest(format!(
                "unsupported memory status: {other}"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Inactive => "inactive",
            Self::Superseded => "superseded",
            Self::Conflicted => "conflicted",
            Self::Expired => "expired",
            Self::Forgotten => "forgotten",
        }
    }

    pub fn is_context_pack_eligible(self) -> bool {
        matches!(self, Self::Active)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MemoryImportance {
    Low,
    Normal,
    High,
    Critical,
}

impl MemoryImportance {
    pub fn parse(value: &str) -> MnemoResult<Self> {
        match value {
            "low" => Ok(Self::Low),
            "normal" => Ok(Self::Normal),
            "high" => Ok(Self::High),
            "critical" => Ok(Self::Critical),
            other => Err(MnemoError::InvalidRequest(format!(
                "unsupported memory importance: {other}"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Normal => "normal",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Memory {
    memory_id: String,
    namespace: Namespace,
    content: String,
    memory_type: String,
    origin: MemoryOrigin,
    status: MemoryStatus,
    importance: MemoryImportance,
    conflict_key: Option<String>,
    valid_from: Option<String>,
    supersedes: Vec<String>,
    superseded_by: Vec<String>,
    source_event_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MemoryPatch {
    pub content: Option<String>,
    pub memory_type: Option<String>,
    pub status: Option<MemoryStatus>,
    pub importance: Option<MemoryImportance>,
    pub conflict_key: Option<Option<String>>,
    pub valid_from: Option<Option<String>>,
    pub source_event_ids: Option<Vec<String>>,
}

impl Memory {
    pub fn new(
        memory_id: impl Into<String>,
        namespace: Namespace,
        content: impl Into<String>,
        memory_type: impl Into<String>,
        origin: MemoryOrigin,
        importance: MemoryImportance,
    ) -> MnemoResult<Self> {
        Ok(Self {
            memory_id: normalize_required("memory_id", memory_id.into())?,
            namespace,
            content: normalize_required("content", content.into())?,
            memory_type: normalize_required("memory_type", memory_type.into())?,
            origin,
            status: MemoryStatus::Active,
            importance,
            conflict_key: None,
            valid_from: None,
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            source_event_ids: Vec::new(),
        })
    }

    pub fn with_conflict_key(mut self, conflict_key: Option<String>) -> Self {
        self.conflict_key = conflict_key.and_then(normalize_optional);
        self
    }

    pub fn with_valid_from(mut self, valid_from: Option<String>) -> Self {
        self.valid_from = valid_from.and_then(normalize_optional);
        self
    }

    pub fn with_source_event_ids(mut self, source_event_ids: Vec<String>) -> Self {
        self.source_event_ids = source_event_ids
            .into_iter()
            .filter_map(normalize_optional)
            .collect();
        self
    }

    pub fn mark_superseded_by(&mut self, memory_id: impl Into<String>) {
        self.status = MemoryStatus::Superseded;
        self.add_superseded_by(memory_id);
    }

    pub fn add_supersedes(&mut self, memory_id: impl Into<String>) {
        let memory_id = memory_id.into();
        if !self.supersedes.contains(&memory_id) {
            self.supersedes.push(memory_id);
        }
    }

    pub fn add_superseded_by(&mut self, memory_id: impl Into<String>) {
        let memory_id = memory_id.into();
        if !self.superseded_by.contains(&memory_id) {
            self.superseded_by.push(memory_id);
        }
    }

    pub fn mark_forgotten(&mut self) {
        self.status = MemoryStatus::Forgotten;
    }

    pub fn set_status(&mut self, status: MemoryStatus) {
        self.status = status;
    }

    pub fn apply_patch(&mut self, patch: MemoryPatch) -> MnemoResult<()> {
        if let Some(content) = patch.content {
            self.content = normalize_required("content", content)?;
        }
        if let Some(memory_type) = patch.memory_type {
            self.memory_type = normalize_required("memory_type", memory_type)?;
        }
        if let Some(status) = patch.status {
            self.status = status;
        }
        if let Some(importance) = patch.importance {
            self.importance = importance;
        }
        if let Some(conflict_key) = patch.conflict_key {
            self.conflict_key = conflict_key.and_then(normalize_optional);
        }
        if let Some(valid_from) = patch.valid_from {
            self.valid_from = valid_from.and_then(normalize_optional);
        }
        if let Some(source_event_ids) = patch.source_event_ids {
            self.source_event_ids = source_event_ids
                .into_iter()
                .filter_map(normalize_optional)
                .collect();
        }
        Ok(())
    }

    pub fn memory_id(&self) -> &str {
        &self.memory_id
    }

    pub fn namespace(&self) -> &Namespace {
        &self.namespace
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn memory_type(&self) -> &str {
        &self.memory_type
    }

    pub fn origin(&self) -> MemoryOrigin {
        self.origin
    }

    pub fn status(&self) -> MemoryStatus {
        self.status
    }

    pub fn importance(&self) -> MemoryImportance {
        self.importance
    }

    pub fn conflict_key(&self) -> Option<&str> {
        self.conflict_key.as_deref()
    }

    pub fn valid_from(&self) -> Option<&str> {
        self.valid_from.as_deref()
    }

    pub fn supersedes(&self) -> &[String] {
        &self.supersedes
    }

    pub fn superseded_by(&self) -> &[String] {
        &self.superseded_by
    }

    pub fn source_event_ids(&self) -> &[String] {
        &self.source_event_ids
    }
}
