use serde::Deserialize;
use serde::Serialize;

use crate::error::{MnemoError, MnemoResult};
use crate::namespace::Namespace;
use crate::normalize::{normalize_optional, normalize_required};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
}

impl JobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> MnemoResult<Self> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            other => Err(MnemoError::InvalidRequest(format!(
                "unsupported job status: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    job_id: String,
    job_type: String,
    status: JobStatus,
    namespace: Namespace,
    created_at: String,
    updated_at: String,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    generate_memories: Option<bool>,
    #[serde(default)]
    attempts: u64,
    #[serde(default)]
    retry_at: Option<String>,
    #[serde(default)]
    lease_until: Option<String>,
    error: Option<String>,
    output_summary_id: Option<String>,
}

impl Job {
    #[allow(clippy::too_many_arguments)]
    pub fn from_stored_parts(
        job_id: impl Into<String>,
        job_type: impl Into<String>,
        status: JobStatus,
        namespace: Namespace,
        created_at: impl Into<String>,
        updated_at: impl Into<String>,
        query: Option<String>,
        generate_memories: Option<bool>,
        attempts: u64,
        retry_at: Option<String>,
        lease_until: Option<String>,
        error: Option<String>,
        output_summary_id: Option<String>,
    ) -> MnemoResult<Self> {
        Ok(Self {
            job_id: normalize_required("job_id", job_id.into())?,
            job_type: normalize_required("job_type", job_type.into())?,
            status,
            namespace,
            created_at: normalize_required("created_at", created_at.into())?,
            updated_at: normalize_required("updated_at", updated_at.into())?,
            query: query.and_then(normalize_optional),
            generate_memories,
            attempts,
            retry_at: retry_at.and_then(normalize_optional),
            lease_until: lease_until.and_then(normalize_optional),
            error: error.and_then(normalize_optional),
            output_summary_id: output_summary_id.and_then(normalize_optional),
        })
    }

    pub fn queued(
        job_id: impl Into<String>,
        job_type: impl Into<String>,
        namespace: Namespace,
        timestamp: impl Into<String>,
        query: Option<String>,
        generate_memories: Option<bool>,
    ) -> MnemoResult<Self> {
        let timestamp = normalize_required("timestamp", timestamp.into())?;
        Ok(Self {
            job_id: normalize_required("job_id", job_id.into())?,
            job_type: normalize_required("job_type", job_type.into())?,
            status: JobStatus::Queued,
            namespace,
            created_at: timestamp.clone(),
            updated_at: timestamp,
            query: query.and_then(normalize_optional),
            generate_memories,
            attempts: 0,
            retry_at: None,
            lease_until: None,
            error: None,
            output_summary_id: None,
        })
    }

    pub fn succeeded(
        job_id: impl Into<String>,
        job_type: impl Into<String>,
        namespace: Namespace,
        timestamp: impl Into<String>,
        output_summary_id: Option<String>,
    ) -> MnemoResult<Self> {
        let timestamp = normalize_required("timestamp", timestamp.into())?;
        Ok(Self {
            job_id: normalize_required("job_id", job_id.into())?,
            job_type: normalize_required("job_type", job_type.into())?,
            status: JobStatus::Succeeded,
            namespace,
            created_at: timestamp.clone(),
            updated_at: timestamp,
            query: None,
            generate_memories: None,
            attempts: 1,
            retry_at: None,
            lease_until: None,
            error: None,
            output_summary_id,
        })
    }

    pub fn failed(
        job_id: impl Into<String>,
        job_type: impl Into<String>,
        namespace: Namespace,
        timestamp: impl Into<String>,
        error: impl Into<String>,
    ) -> MnemoResult<Self> {
        let timestamp = normalize_required("timestamp", timestamp.into())?;
        Ok(Self {
            job_id: normalize_required("job_id", job_id.into())?,
            job_type: normalize_required("job_type", job_type.into())?,
            status: JobStatus::Failed,
            namespace,
            created_at: timestamp.clone(),
            updated_at: timestamp,
            query: None,
            generate_memories: None,
            attempts: 1,
            retry_at: None,
            lease_until: None,
            error: normalize_optional(error.into()),
            output_summary_id: None,
        })
    }

    pub fn mark_running(&mut self, timestamp: impl Into<String>) -> MnemoResult<()> {
        self.mark_running_until(timestamp, None)
    }

    pub fn mark_running_until(
        &mut self,
        timestamp: impl Into<String>,
        lease_until: Option<String>,
    ) -> MnemoResult<()> {
        self.status = JobStatus::Running;
        self.updated_at = normalize_required("timestamp", timestamp.into())?;
        self.attempts += 1;
        self.retry_at = None;
        self.lease_until = lease_until.and_then(normalize_optional);
        self.error = None;
        Ok(())
    }

    pub fn mark_succeeded(
        &mut self,
        timestamp: impl Into<String>,
        output_summary_id: Option<String>,
    ) -> MnemoResult<()> {
        self.status = JobStatus::Succeeded;
        self.updated_at = normalize_required("timestamp", timestamp.into())?;
        self.output_summary_id = output_summary_id.and_then(normalize_optional);
        self.retry_at = None;
        self.lease_until = None;
        self.error = None;
        Ok(())
    }

    pub fn mark_failed(
        &mut self,
        timestamp: impl Into<String>,
        error: impl Into<String>,
    ) -> MnemoResult<()> {
        self.status = JobStatus::Failed;
        self.updated_at = normalize_required("timestamp", timestamp.into())?;
        self.lease_until = None;
        self.error = normalize_optional(error.into());
        Ok(())
    }

    pub fn set_retry_at(&mut self, retry_at: Option<String>) {
        self.retry_at = retry_at.and_then(normalize_optional);
    }

    pub fn job_id(&self) -> &str {
        &self.job_id
    }

    pub fn job_type(&self) -> &str {
        &self.job_type
    }

    pub fn status(&self) -> JobStatus {
        self.status
    }

    pub fn namespace(&self) -> &Namespace {
        &self.namespace
    }

    pub fn created_at(&self) -> &str {
        &self.created_at
    }

    pub fn updated_at(&self) -> &str {
        &self.updated_at
    }

    pub fn query(&self) -> Option<&str> {
        self.query.as_deref()
    }

    pub fn generate_memories(&self) -> Option<bool> {
        self.generate_memories
    }

    pub fn attempts(&self) -> u64 {
        self.attempts
    }

    pub fn retry_at(&self) -> Option<&str> {
        self.retry_at.as_deref()
    }

    pub fn lease_until(&self) -> Option<&str> {
        self.lease_until.as_deref()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn output_summary_id(&self) -> Option<&str> {
        self.output_summary_id.as_deref()
    }
}
