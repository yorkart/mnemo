use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use mnemo_ports::StoreError;
use serde_json::json;

tokio::task_local! {
    pub(crate) static REQUEST_ID: String;
}

#[derive(Debug)]
pub struct HttpError(pub(crate) StoreError);

impl From<StoreError> for HttpError {
    fn from(value: StoreError) -> Self {
        Self(value)
    }
}

impl HttpError {
    pub(crate) fn unauthorized() -> Self {
        Self(StoreError::Unauthorized)
    }

    pub(crate) fn forbidden() -> Self {
        Self(StoreError::Forbidden(String::new()))
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let status = match self.0 {
            StoreError::InvalidRequest(_) => StatusCode::BAD_REQUEST,
            StoreError::NotFound => StatusCode::NOT_FOUND,
            StoreError::Conflict(_) => StatusCode::CONFLICT,
            StoreError::IdempotencyConflict(_) => StatusCode::CONFLICT,
            StoreError::Unauthorized => StatusCode::UNAUTHORIZED,
            StoreError::Forbidden(_) => StatusCode::FORBIDDEN,
            StoreError::InvalidTransaction => StatusCode::INTERNAL_SERVER_ERROR,
            StoreError::Storage(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = json!({
            "ok": false,
            "error": {
                "code": self.0.code(),
                "message": self.0.to_string(),
                "request_id": REQUEST_ID.try_with(Clone::clone).ok()
            }
        });
        (status, Json(body)).into_response()
    }
}
