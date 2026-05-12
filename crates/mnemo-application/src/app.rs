use std::sync::Arc;

use mnemo_domain::*;
use mnemo_ports::{AuthPort, ExtractionProvider, StorageError, Store};

#[derive(Clone)]
pub struct MnemoApp {
    pub(crate) stores: Arc<dyn Store>,
    pub(crate) auth: Arc<dyn AuthPort>,
    #[allow(dead_code)]
    pub(crate) extraction: Arc<dyn ExtractionProvider>,
}

impl MnemoApp {
    pub fn new(
        stores: Arc<dyn Store>,
        auth: Arc<dyn AuthPort>,
        extraction: Arc<dyn ExtractionProvider>,
    ) -> Self {
        Self {
            stores,
            auth,
            extraction,
        }
    }

    pub(crate) async fn check_auth(
        &self,
        token: Option<&str>,
        namespace: &Namespace,
        permission: &str,
    ) -> Result<(), StorageError> {
        if let Some(token) = token {
            let info = self.auth.validate_token(token).await?;
            self.auth.check_namespace(&info, namespace, permission)?;
        }
        Ok(())
    }

    pub(crate) async fn check_auth_scope(
        &self,
        token: Option<&str>,
        namespace: &Namespace,
        scope: Option<&QueryScope>,
        permission: &str,
    ) -> Result<(), StorageError> {
        if let Some(token) = token {
            let info = self.auth.validate_token(token).await?;
            self.auth.check_scope(&info, namespace, scope, permission)?;
        }
        Ok(())
    }
}

pub(crate) fn require_user_namespace(namespace: &Namespace) -> Result<(), StorageError> {
    if namespace.user_id.is_none() {
        return Err(StorageError::InvalidRequest(
            "namespace.user_id is required".to_string(),
        ));
    }
    Ok(())
}
