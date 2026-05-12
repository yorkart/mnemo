use std::net::SocketAddr;

use axum::{
    Router,
    middleware,
    routing::{get, post},
};
use mnemo_application::MnemoApp;
use tower_http::trace::TraceLayer;

use crate::auth::AuthConfig;
use crate::handlers::*;

#[derive(Clone)]
pub struct HttpState {
    pub app: MnemoApp,
    pub auth: AuthConfig,
}

pub fn router(app: MnemoApp) -> Router {
    router_with_auth(app, AuthConfig::from_env())
}

pub fn router_with_auth(app: MnemoApp, auth: AuthConfig) -> Router {
    let state = HttpState {
        app,
        auth: auth.clone(),
    };
    let protected = Router::new()
        .route("/v1/events", post(ingest_event))
        .route("/v1/events/batch", post(ingest_events_batch))
        .route("/v1/memories", post(create_memory))
        .route("/v1/query", post(query_memories))
        .route("/v1/usage", post(record_usage))
        .route("/v1/context-pack", post(context_pack))
        .route("/v1/sessions/wrapup", post(wrapup))
        .route("/v1/jobs", get(list_jobs))
        .route("/v1/jobs/{job_id}", get(get_job))
        .route("/v1/namespaces/status", post(namespace_status))
        .route("/v1/namespaces/policy", get(get_policy).put(put_policy))
        .route("/v1/memories/search", post(search_memories))
        .route(
            "/v1/memories/{memory_id}",
            get(get_memory).patch(patch_memory),
        )
        .route("/v1/events/search", post(search_events))
        .route("/v1/forget", post(forget))
        .route_layer(middleware::from_fn_with_state(auth, auth_middleware));

    Router::new()
        .route("/v1/health", get(health))
        .merge(protected)
        .layer(middleware::from_fn(request_context_middleware))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

pub async fn serve(app: MnemoApp, bind: SocketAddr) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(bind).await?;
    axum::serve(listener, router(app)).await?;
    Ok(())
}
