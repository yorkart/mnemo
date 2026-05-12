use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use mnemo_adapters::SqliteStore;
use mnemo_application::MnemoApp;
use mnemo_domain::*;
use serde_json::Value;
use tower::ServiceExt;

use crate::auth::{AuthConfig, TokenScope, parse_token_scopes};
use crate::router::router_with_auth;

fn auth() -> AuthConfig {
    auth_with_permissions(vec!["memories:read"])
}

fn auth_with_permissions(permissions: Vec<&str>) -> AuthConfig {
    AuthConfig {
        token: Some("secret".to_string()),
        scopes: vec![TokenScope {
            tenant_id: Some("default".to_string()),
            user_id: Some("u1".to_string()),
            workspace_id: Some("*".to_string()),
            thread_id: Some("*".to_string()),
            permissions: permissions.into_iter().map(ToString::to_string).collect(),
        }],
    }
}

fn test_router() -> axum::Router {
    let store = SqliteStore::in_memory().expect("store");
    let app = MnemoApp::new(Arc::new(store), Arc::new(mnemo_adapters::NoopAuth), Arc::new(mnemo_adapters::NoopExtraction));
    router_with_auth(
        app,
        auth_with_permissions(vec!["memories:read", "memories:write"]),
    )
}

#[test]
fn namespace_scope_allows_matching_permission() {
    let namespace = Namespace {
        tenant_id: Some("default".to_string()),
        user_id: Some("u1".to_string()),
        workspace_id: Some("w1".to_string()),
        thread_id: Some("t1".to_string()),
        agent_id: None,
        source: None,
    };
    assert!(auth().check_namespace(&namespace, "memories:read").is_ok());
    assert!(
        auth()
            .check_namespace(&namespace, "memories:write")
            .is_err()
    );
}

#[test]
fn scope_expansion_requires_parent_permission() {
    let namespace = Namespace {
        tenant_id: Some("default".to_string()),
        user_id: Some("u1".to_string()),
        workspace_id: Some("w1".to_string()),
        thread_id: Some("t1".to_string()),
        agent_id: None,
        source: None,
    };
    let scope = QueryScope {
        include_thread: true,
        include_workspace: true,
        include_user: true,
        include_tenant: false,
    };
    assert!(
        auth()
            .check_scope(&namespace, Some(&scope), "memories:read")
            .is_err()
    );
}

#[test]
#[should_panic(expected = "invalid MNEMO_TOKEN_SCOPES JSON")]
fn malformed_token_scope_config_panics() {
    let _ = parse_token_scopes(Some("{".to_string()));
}

#[tokio::test]
async fn health_is_public_but_memory_requires_auth() {
    let app = test_router();
    let health = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK);

    let denied = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/memories")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"namespace":{"tenant_id":"default","user_id":"u1","workspace_id":"w1","thread_id":"t1"},"content":"x"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn request_context_headers_are_returned_on_success_and_error() {
    let app = test_router();
    let health = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/health")
                .header("x-request-id", "req-health")
                .header(
                    "traceparent",
                    "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-00",
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK);
    assert_eq!(health.headers().get("x-request-id").unwrap(), "req-health");
    assert_eq!(
        health.headers().get("traceparent").unwrap(),
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-00"
    );

    let denied = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/memories")
                .header("x-request-id", "req-denied")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"namespace":{"tenant_id":"default","user_id":"u1","workspace_id":"w1","thread_id":"t1"},"content":"x"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(denied.headers().get("x-request-id").unwrap(), "req-denied");
    let denied_body = to_bytes(denied.into_body(), usize::MAX).await.unwrap();
    let denied_json: Value = serde_json::from_slice(&denied_body).unwrap();
    assert_eq!(denied_json["error"]["request_id"], "req-denied");
}

#[tokio::test]
async fn route_scope_allows_matching_namespace_and_rejects_other_user() {
    let app = test_router();
    let allowed = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/memories")
                .header("authorization", "Bearer secret")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"namespace":{"tenant_id":"default","user_id":"u1","workspace_id":"w1","thread_id":"t1"},"content":"allowed"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(allowed.status(), StatusCode::OK);

    let denied = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/memories")
                .header("authorization", "Bearer secret")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"namespace":{"tenant_id":"default","user_id":"u2","workspace_id":"w1","thread_id":"t1"},"content":"denied"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn route_idempotency_key_replays_memory_create_response() {
    let app = test_router();
    let request = || {
        Request::builder()
            .method("POST")
            .uri("/v1/memories")
            .header("authorization", "Bearer secret")
            .header("content-type", "application/json")
            .header("Idempotency-Key", "idem-route")
            .body(Body::from(
                r#"{"namespace":{"tenant_id":"default","user_id":"u1","workspace_id":"w1","thread_id":"t1"},"content":"idempotent"}"#,
            ))
            .unwrap()
    };

    let first = app.clone().oneshot(request()).await.unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let first_body = to_bytes(first.into_body(), usize::MAX).await.unwrap();
    let first_json: Value = serde_json::from_slice(&first_body).unwrap();

    let second = app.oneshot(request()).await.unwrap();
    assert_eq!(second.status(), StatusCode::OK);
    let second_body = to_bytes(second.into_body(), usize::MAX).await.unwrap();
    let second_json: Value = serde_json::from_slice(&second_body).unwrap();

    assert_eq!(first_json["memory_id"], second_json["memory_id"]);
}
