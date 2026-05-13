use std::error::Error;
use std::fmt;

pub type MnemoResult<T> = Result<T, MnemoError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MnemoError {
    InvalidRequest(String),
    Unauthorized,
    Forbidden,
    NotFound(String),
    IdempotencyConflict(String),
    EventConflict(String),
    Internal(String),
}

impl MnemoError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidRequest(_) => "invalid_request",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::NotFound(_) => "not_found",
            Self::IdempotencyConflict(_) => "idempotency_conflict",
            Self::EventConflict(_) => "event_conflict",
            Self::Internal(_) => "internal_error",
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::InvalidRequest(message)
            | Self::NotFound(message)
            | Self::IdempotencyConflict(message)
            | Self::EventConflict(message)
            | Self::Internal(message) => message.as_str(),
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
        }
    }
}

impl fmt::Display for MnemoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code(), self.message())
    }
}

impl Error for MnemoError {}
