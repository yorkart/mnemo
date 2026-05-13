use std::net::TcpListener;
use std::net::TcpStream;
use std::sync::Arc;

use mnemo_core::MnemoError;
use mnemo_core::MnemoResult;
use mnemo_store::InMemoryStore;
use mnemo_store::MnemoStore;
use mnemo_store::SqliteStore;

use crate::config::ServerConfig;
use crate::http::{HttpRequest, read_http_request, write_response};
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

    pub(crate) fn authorize(&self, request: &HttpRequest) -> MnemoResult<()> {
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
    let listener = TcpListener::bind(&config.bind_address)?;
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
    for stream in listener.incoming() {
        handle_stream(stream?, Arc::clone(&state))?;
    }
    Ok(())
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

fn handle_stream(mut stream: TcpStream, state: Arc<ApiState>) -> std::io::Result<()> {
    let request = read_http_request(&mut stream)?;
    let response = handle_request(&state, &request);
    write_response(&mut stream, response.status, &response.body)
}
