use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use mnemo_core::MnemoError;
use mnemo_store::InMemoryStore;
use mnemo_store::MnemoStore;
use mnemo_store::SqliteStore;

use crate::config::ServerConfig;
use crate::http::{HttpRequest, HttpStatus};
use crate::router::handle_request;
use crate::wrapup::{CommandWrapupProvider, RuleWrapupProvider, WrapupProvider};

pub struct ApiState {
    store: Box<dyn MnemoStore>,
    wrapup_provider: Box<dyn WrapupProvider>,
    bearer_token: Option<String>,
}

impl ApiState {
    pub fn new<S>(store: S, bearer_token: Option<String>) -> Self
    where
        S: MnemoStore + 'static,
    {
        Self {
            store: Box::new(store),
            wrapup_provider: Box::new(RuleWrapupProvider),
            bearer_token,
        }
    }

    pub(crate) fn with_wrapup_provider<S, P>(
        store: S,
        bearer_token: Option<String>,
        wrapup_provider: P,
    ) -> Self
    where
        S: MnemoStore + 'static,
        P: WrapupProvider + 'static,
    {
        Self {
            store: Box::new(store),
            wrapup_provider: Box::new(wrapup_provider),
            bearer_token,
        }
    }

    pub(crate) fn authorize(&self, request: &HttpRequest) -> Result<(), MnemoError> {
        let Some(expected) = self.bearer_token.as_deref() else {
            return Ok(());
        };
        let Some(actual) = request.header("authorization") else {
            return Err(MnemoError::Unauthorized);
        };
        let Some(token) = actual.strip_prefix("Bearer ") else {
            return Err(MnemoError::Unauthorized);
        };
        if token == expected {
            Ok(())
        } else {
            Err(MnemoError::Unauthorized)
        }
    }

    pub(crate) fn store(&self) -> &dyn MnemoStore {
        self.store.as_ref()
    }

    pub(crate) fn wrapup_provider(&self) -> &dyn WrapupProvider {
        self.wrapup_provider.as_ref()
    }
}

pub fn serve_blocking(config: ServerConfig) -> std::io::Result<()> {
    let wrapup_command = config.wrapup_command.clone();
    let state = if let Some(path) = config.sqlite_path {
        let store = SqliteStore::open(path).map_err(std::io::Error::other)?;
        Arc::new(api_state_for_store(
            store,
            config.bearer_token,
            wrapup_command,
        )?)
    } else {
        let store = match config.data_path {
            Some(path) => InMemoryStore::open(path).map_err(std::io::Error::other)?,
            None => InMemoryStore::new(),
        };
        Arc::new(api_state_for_store(
            store,
            config.bearer_token,
            wrapup_command,
        )?)
    };

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(std::io::Error::other)?;

    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(&config.bind_address).await?;
        let app = Router::new()
            .route("/{*path}", any(axum_handler))
            .with_state(state);
        axum::serve(listener, app)
            .await
            .map_err(std::io::Error::other)
    })
}

fn api_state_for_store<S>(
    store: S,
    bearer_token: Option<String>,
    wrapup_command: Option<String>,
) -> std::io::Result<ApiState>
where
    S: MnemoStore + 'static,
{
    Ok(match wrapup_command {
        Some(command) if !command.trim().is_empty() => ApiState::with_wrapup_provider(
            store,
            bearer_token,
            CommandWrapupProvider::new(command).map_err(std::io::Error::other)?,
        ),
        _ => ApiState::new(store, bearer_token),
    })
}

async fn axum_handler(
    State(state): State<Arc<ApiState>>,
    request: axum::extract::Request,
) -> Response {
    let (parts, body) = request.into_parts();
    let body_bytes = match axum::body::to_bytes(body, usize::MAX).await {
        Ok(bytes) => bytes,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                crate::responses::error_response_json(
                    "bad_request",
                    &format!("invalid request body: {error}"),
                ),
            )
                .into_response();
        }
    };

    let mut http_request =
        HttpRequest::new(parts.method.as_str(), parts.uri.path(), body_bytes.to_vec());
    for (name, value) in &parts.headers {
        if let Ok(value) = value.to_str() {
            http_request = http_request.with_header(name.as_str(), value);
        }
    }

    let response = handle_request(&state, &http_request);
    (status_to_axum(response.status), response.body).into_response()
}

fn status_to_axum(status: HttpStatus) -> StatusCode {
    match status {
        HttpStatus::Ok => StatusCode::OK,
        HttpStatus::BadRequest => StatusCode::BAD_REQUEST,
        HttpStatus::Unauthorized => StatusCode::UNAUTHORIZED,
        HttpStatus::Forbidden => StatusCode::FORBIDDEN,
        HttpStatus::NotFound => StatusCode::NOT_FOUND,
        HttpStatus::Conflict => StatusCode::CONFLICT,
        HttpStatus::InternalServerError => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
