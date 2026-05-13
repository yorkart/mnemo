use serde::Deserialize;
use serde::Serialize;

use crate::error::MnemoResult;
use crate::normalize::{normalize_optional, normalize_required};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Namespace {
    tenant_id: String,
    user_id: String,
    workspace_id: Option<String>,
    thread_id: Option<String>,
    agent_id: Option<String>,
    source: Option<String>,
}

impl Namespace {
    pub fn new(user_id: impl Into<String>) -> MnemoResult<Self> {
        let user_id = normalize_required("user_id", user_id.into())?;
        Ok(Self {
            tenant_id: "default".to_string(),
            user_id,
            workspace_id: None,
            thread_id: None,
            agent_id: None,
            source: None,
        })
    }

    pub fn with_tenant(mut self, tenant_id: impl Into<String>) -> Self {
        let tenant_id = tenant_id.into();
        self.tenant_id = normalize_optional(tenant_id).unwrap_or_else(|| "default".to_string());
        self
    }

    pub fn with_workspace(mut self, workspace_id: impl Into<String>) -> Self {
        self.workspace_id = normalize_optional(workspace_id.into());
        self
    }

    pub fn with_thread(mut self, thread_id: impl Into<String>) -> Self {
        self.thread_id = normalize_optional(thread_id.into());
        self
    }

    pub fn with_agent(mut self, agent_id: impl Into<String>) -> Self {
        self.agent_id = normalize_optional(agent_id.into());
        self
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = normalize_optional(source.into());
        self
    }

    pub fn tenant_id(&self) -> &str {
        &self.tenant_id
    }

    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    pub fn workspace_id(&self) -> Option<&str> {
        self.workspace_id.as_deref()
    }

    pub fn thread_id(&self) -> Option<&str> {
        self.thread_id.as_deref()
    }

    pub fn agent_id(&self) -> Option<&str> {
        self.agent_id.as_deref()
    }

    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    pub fn stable_key(&self) -> String {
        format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
            self.tenant_id,
            self.user_id,
            self.workspace_id.as_deref().unwrap_or(""),
            self.thread_id.as_deref().unwrap_or(""),
            self.agent_id.as_deref().unwrap_or(""),
            self.source.as_deref().unwrap_or("")
        )
    }
}
