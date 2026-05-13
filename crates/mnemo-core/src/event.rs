use serde::Deserialize;
use serde::Serialize;

use crate::error::{MnemoError, MnemoResult};
use crate::namespace::Namespace;
use crate::normalize::normalize_required;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventType {
    UserMessage,
    AgentMessage,
    AssistantFinal,
    ToolSummary,
    ToolResultSummary,
    TaskSummary,
    ContextInjected,
    SystemEvent,
    ExplicitRemember,
    ExplicitForget,
    SessionSummary,
}

impl EventType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserMessage => "user_message",
            Self::AgentMessage => "agent_message",
            Self::AssistantFinal => "assistant_final",
            Self::ToolSummary => "tool_summary",
            Self::ToolResultSummary => "tool_result_summary",
            Self::TaskSummary => "task_summary",
            Self::ContextInjected => "context_injected",
            Self::SystemEvent => "system_event",
            Self::ExplicitRemember => "explicit_remember",
            Self::ExplicitForget => "explicit_forget",
            Self::SessionSummary => "session_summary",
        }
    }

    pub fn parse(value: &str) -> MnemoResult<Self> {
        match value {
            "user_message" => Ok(Self::UserMessage),
            "agent_message" => Ok(Self::AgentMessage),
            "assistant_final" => Ok(Self::AssistantFinal),
            "tool_summary" => Ok(Self::ToolSummary),
            "tool_result_summary" => Ok(Self::ToolResultSummary),
            "task_summary" => Ok(Self::TaskSummary),
            "context_injected" => Ok(Self::ContextInjected),
            "system_event" => Ok(Self::SystemEvent),
            "explicit_remember" => Ok(Self::ExplicitRemember),
            "explicit_forget" => Ok(Self::ExplicitForget),
            "session_summary" => Ok(Self::SessionSummary),
            other => Err(MnemoError::InvalidRequest(format!(
                "unsupported event type: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventRole {
    User,
    Agent,
    Tool,
    System,
}

impl EventRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Agent => "agent",
            Self::Tool => "tool",
            Self::System => "system",
        }
    }

    pub fn parse(value: &str) -> MnemoResult<Self> {
        match value {
            "user" => Ok(Self::User),
            "agent" | "assistant" => Ok(Self::Agent),
            "tool" => Ok(Self::Tool),
            "system" => Ok(Self::System),
            other => Err(MnemoError::InvalidRequest(format!(
                "unsupported event role: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MemoryHints {
    pub eligible: bool,
    pub explicit_memory_intent: bool,
    pub external_context: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    event_id: String,
    namespace: Namespace,
    event_type: EventType,
    role: EventRole,
    content: String,
    occurred_at: String,
    memory_hints: MemoryHints,
}

impl Event {
    pub fn new(
        event_id: impl Into<String>,
        namespace: Namespace,
        event_type: EventType,
        role: EventRole,
        content: impl Into<String>,
        occurred_at: impl Into<String>,
    ) -> MnemoResult<Self> {
        Ok(Self {
            event_id: normalize_required("event_id", event_id.into())?,
            namespace,
            event_type,
            role,
            content: normalize_required("content", content.into())?,
            occurred_at: normalize_required("occurred_at", occurred_at.into())?,
            memory_hints: MemoryHints::default(),
        })
    }

    pub fn with_memory_hints(mut self, memory_hints: MemoryHints) -> Self {
        self.memory_hints = memory_hints;
        self
    }

    pub fn event_id(&self) -> &str {
        &self.event_id
    }

    pub fn namespace(&self) -> &Namespace {
        &self.namespace
    }

    pub fn event_type(&self) -> EventType {
        self.event_type
    }

    pub fn role(&self) -> EventRole {
        self.role
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn occurred_at(&self) -> &str {
        &self.occurred_at
    }

    pub fn memory_hints(&self) -> &MemoryHints {
        &self.memory_hints
    }

    pub fn conflict_fingerprint(&self) -> String {
        format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
            self.namespace.stable_key(),
            self.event_id,
            self.event_type.as_str(),
            self.role.as_str(),
            self.content,
            self.occurred_at
        )
    }
}
