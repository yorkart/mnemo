use serde::Deserialize;
use serde::Serialize;

use crate::error::{MnemoError, MnemoResult};
use crate::namespace::Namespace;
use crate::normalize::{normalize_optional, normalize_required};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UsageSignal {
    Positive,
    Neutral,
    Negative,
}

impl UsageSignal {
    pub fn parse(value: &str) -> MnemoResult<Self> {
        match value {
            "positive" => Ok(Self::Positive),
            "neutral" => Ok(Self::Neutral),
            "negative" => Ok(Self::Negative),
            other => Err(MnemoError::InvalidRequest(format!(
                "unsupported usage signal: {other}"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Positive => "positive",
            Self::Neutral => "neutral",
            Self::Negative => "negative",
        }
    }

    pub fn score_delta(self) -> i64 {
        match self {
            Self::Positive => 1,
            Self::Neutral => 0,
            Self::Negative => -2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageReport {
    usage_id: String,
    namespace: Namespace,
    context_pack_id: String,
    signal: UsageSignal,
    memory_ids: Vec<String>,
    notes: Option<String>,
    reported_at: String,
}

impl UsageReport {
    pub fn new(
        usage_id: impl Into<String>,
        namespace: Namespace,
        context_pack_id: impl Into<String>,
        signal: UsageSignal,
        memory_ids: Vec<String>,
        notes: Option<String>,
        reported_at: impl Into<String>,
    ) -> MnemoResult<Self> {
        Ok(Self {
            usage_id: normalize_required("usage_id", usage_id.into())?,
            namespace,
            context_pack_id: normalize_required("context_pack_id", context_pack_id.into())?,
            signal,
            memory_ids: memory_ids
                .into_iter()
                .filter_map(normalize_optional)
                .collect(),
            notes: notes.and_then(normalize_optional),
            reported_at: normalize_required("reported_at", reported_at.into())?,
        })
    }

    pub fn usage_id(&self) -> &str {
        &self.usage_id
    }

    pub fn namespace(&self) -> &Namespace {
        &self.namespace
    }

    pub fn context_pack_id(&self) -> &str {
        &self.context_pack_id
    }

    pub fn signal(&self) -> UsageSignal {
        self.signal
    }

    pub fn memory_ids(&self) -> &[String] {
        &self.memory_ids
    }

    pub fn notes(&self) -> Option<&str> {
        self.notes.as_deref()
    }

    pub fn reported_at(&self) -> &str {
        &self.reported_at
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForgetTombstone {
    tombstone_id: String,
    namespace: Namespace,
    memory_ids: Vec<String>,
    contents: Vec<String>,
    reason: Option<String>,
    created_at: String,
}

impl ForgetTombstone {
    pub fn new(
        tombstone_id: impl Into<String>,
        namespace: Namespace,
        memory_ids: Vec<String>,
        contents: Vec<String>,
        reason: Option<String>,
        created_at: impl Into<String>,
    ) -> MnemoResult<Self> {
        Ok(Self {
            tombstone_id: normalize_required("tombstone_id", tombstone_id.into())?,
            namespace,
            memory_ids: memory_ids
                .into_iter()
                .filter_map(normalize_optional)
                .collect(),
            contents: contents
                .into_iter()
                .filter_map(normalize_optional)
                .collect(),
            reason: reason.and_then(normalize_optional),
            created_at: normalize_required("created_at", created_at.into())?,
        })
    }

    pub fn tombstone_id(&self) -> &str {
        &self.tombstone_id
    }

    pub fn namespace(&self) -> &Namespace {
        &self.namespace
    }

    pub fn memory_ids(&self) -> &[String] {
        &self.memory_ids
    }

    pub fn contents(&self) -> &[String] {
        &self.contents
    }

    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    pub fn created_at(&self) -> &str {
        &self.created_at
    }
}
