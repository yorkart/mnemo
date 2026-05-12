use mnemo_domain::*;
use serde::Deserialize;

use crate::error::HttpError;

#[derive(Clone, Debug, Default)]
pub struct AuthConfig {
    pub token: Option<String>,
    pub scopes: Vec<TokenScope>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TokenScope {
    pub tenant_id: Option<String>,
    pub user_id: Option<String>,
    pub workspace_id: Option<String>,
    pub thread_id: Option<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
}

impl AuthConfig {
    pub fn from_env() -> Self {
        let token = std::env::var("MNEMO_TOKEN")
            .ok()
            .filter(|token| !token.is_empty());
        let scopes = parse_token_scopes(std::env::var("MNEMO_TOKEN_SCOPES").ok());
        Self { token, scopes }
    }

    pub(crate) fn check_namespace(
        &self,
        namespace: &Namespace,
        permission: &str,
    ) -> Result<(), HttpError> {
        if self.token.is_none() || self.scopes.is_empty() {
            return Ok(());
        }
        let namespace = namespace.canonical();
        if self
            .scopes
            .iter()
            .any(|scope| scope.matches(&namespace, permission))
        {
            Ok(())
        } else {
            Err(HttpError::forbidden())
        }
    }

    pub(crate) fn check_scope(
        &self,
        namespace: &Namespace,
        scope: Option<&QueryScope>,
        permission: &str,
    ) -> Result<(), HttpError> {
        if self.token.is_none() || self.scopes.is_empty() {
            return Ok(());
        }
        for namespace in expanded_namespaces(namespace, scope) {
            self.check_namespace(&namespace, permission)?;
        }
        Ok(())
    }
}

pub(crate) fn parse_token_scopes(raw: Option<String>) -> Vec<TokenScope> {
    let Some(raw) = raw.filter(|raw| !raw.trim().is_empty()) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<TokenScope>>(&raw)
        .unwrap_or_else(|error| panic!("invalid MNEMO_TOKEN_SCOPES JSON: {error}"))
}

impl TokenScope {
    fn matches(&self, namespace: &Namespace, permission: &str) -> bool {
        self.permissions.iter().any(|p| p == "*" || p == permission)
            && field_matches(&self.tenant_id, namespace.tenant_id.as_deref())
            && field_matches(&self.user_id, namespace.user_id.as_deref())
            && field_matches(&self.workspace_id, namespace.workspace_id.as_deref())
            && field_matches(&self.thread_id, namespace.thread_id.as_deref())
    }
}

fn field_matches(scope: &Option<String>, value: Option<&str>) -> bool {
    match scope.as_deref() {
        Some("*") => value.is_some(),
        Some(expected) => value == Some(expected),
        None => true, // None in scope means "no restriction" (wildcard)
    }
}

fn expanded_namespaces(namespace: &Namespace, scope: Option<&QueryScope>) -> Vec<Namespace> {
    let namespace = namespace.canonical();
    let Some(scope) = scope else {
        return vec![namespace];
    };
    let mut out = vec![namespace.clone()];
    if scope.include_workspace && namespace.workspace_id.is_some() {
        let mut parent = namespace.clone();
        parent.thread_id = None;
        out.push(parent);
    }
    if scope.include_user && namespace.user_id.is_some() {
        let mut parent = namespace.clone();
        parent.workspace_id = None;
        parent.thread_id = None;
        out.push(parent);
    }
    if scope.include_tenant {
        let mut parent = namespace;
        parent.user_id = None;
        parent.workspace_id = None;
        parent.thread_id = None;
        out.push(parent);
    }
    out
}
