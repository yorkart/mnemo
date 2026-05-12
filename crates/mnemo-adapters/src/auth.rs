use async_trait::async_trait;
use mnemo_domain::*;
use mnemo_ports::{
    AuthError, AuthPort, ExtractionProvider, ExtractionResult, StorageError, TokenInfo,
    WrapupEvent as PortWrapupEvent,
};

/// A no-op auth implementation that permits all operations.
/// Use when auth is handled externally (e.g., by HTTP middleware).
pub struct NoopAuth;

#[async_trait]
impl AuthPort for NoopAuth {
    async fn validate_token(&self, _token: &str) -> Result<TokenInfo, AuthError> {
        Ok(TokenInfo::default())
    }

    fn check_namespace(
        &self,
        _token: &TokenInfo,
        _namespace: &Namespace,
        _permission: &str,
    ) -> Result<(), AuthError> {
        Ok(())
    }

    fn check_scope(
        &self,
        _token: &TokenInfo,
        _namespace: &Namespace,
        _scope: Option<&QueryScope>,
        _permission: &str,
    ) -> Result<(), AuthError> {
        Ok(())
    }
}

/// A no-op extraction provider. The real extraction logic lives in
/// SqliteStore's internal wrapup job processing for now.
pub struct NoopExtraction;

#[async_trait]
impl ExtractionProvider for NoopExtraction {
    async fn extract(&self, _events: &[PortWrapupEvent]) -> Result<ExtractionResult, StorageError> {
        Ok(ExtractionResult {
            provider_name: "noop".to_string(),
            candidates: Vec::new(),
            warning: None,
        })
    }

    fn label(&self) -> &str {
        "noop"
    }
}
