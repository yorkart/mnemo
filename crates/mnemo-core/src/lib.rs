#![forbid(unsafe_code)]

mod normalize;

pub mod error;
pub mod namespace;
pub mod event;
pub mod memory;
pub mod policy;
pub mod context_pack;
pub mod job;
pub mod session;
pub mod usage;
pub mod health;

pub use error::{MnemoError, MnemoResult};
pub use namespace::Namespace;
pub use event::{Event, EventRole, EventType, MemoryHints};
pub use memory::{Memory, MemoryImportance, MemoryOrigin, MemoryPatch, MemoryStatus, ThreadMemoryMode};
pub use policy::NamespacePolicy;
pub use context_pack::{ContextPackCacheEntry, ContextPackItem};
pub use job::{Job, JobStatus};
pub use session::SessionSummary;
pub use usage::{ForgetTombstone, UsageReport, UsageSignal};
pub use health::HealthStatus;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_defaults_tenant_and_requires_user() {
        let ns = Namespace::new(" u1 ").expect("namespace");
        assert_eq!(ns.tenant_id(), "default");
        assert_eq!(ns.user_id(), "u1");
        assert!(Namespace::new(" ").is_err());
    }

    #[test]
    fn memory_mode_controls_only_inferred_generation() {
        assert!(ThreadMemoryMode::Enabled.allows_inferred_memory());
        assert!(!ThreadMemoryMode::Disabled.allows_inferred_memory());
        assert!(!ThreadMemoryMode::Polluted.allows_inferred_memory());
    }
}
