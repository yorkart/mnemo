use std::collections::HashMap;
use std::io::Read;
use std::io::Write;
use std::net::TcpStream;

use mnemo_core::MnemoError;
use mnemo_core::MnemoResult;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl HttpRequest {
    pub fn new(method: &str, path: &str, body: impl Into<Vec<u8>>) -> Self {
        Self {
            method: method.to_string(),
            path: path.to_string(),
            headers: HashMap::new(),
            body: body.into(),
        }
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers
            .insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        self
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn json_body<T: for<'de> Deserialize<'de>>(&self) -> MnemoResult<T> {
        serde_json::from_slice(&self.body)
            .map_err(|err| MnemoError::InvalidRequest(format!("invalid json body: {err}")))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: HttpStatus,
    pub body: String,
}

impl HttpResponse {
    pub fn ok(body: String) -> Self {
        Self {
            status: HttpStatus::Ok,
            body,
        }
    }

    pub fn from_error_for_request(error: MnemoError, request: &HttpRequest) -> Self {
        let status = HttpStatus::from_error(&error);
        Self {
            status,
            body: crate::responses::error_response_json_with_request_id(
                error.code(),
                error.message(),
                request.header("x-request-id"),
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpStatus {
    Ok,
    BadRequest,
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict,
    InternalServerError,
}

impl HttpStatus {
    pub fn from_error(error: &MnemoError) -> Self {
        match error {
            MnemoError::InvalidRequest(_) => Self::BadRequest,
            MnemoError::Unauthorized => Self::Unauthorized,
            MnemoError::Forbidden => Self::Forbidden,
            MnemoError::NotFound(_) => Self::NotFound,
            MnemoError::IdempotencyConflict(_) | MnemoError::EventConflict(_) => Self::Conflict,
            MnemoError::Internal(_) => Self::InternalServerError,
        }
    }

    pub fn status_line(self) -> &'static str {
        match self {
            Self::Ok => "200 OK",
            Self::BadRequest => "400 Bad Request",
            Self::Unauthorized => "401 Unauthorized",
            Self::Forbidden => "403 Forbidden",
            Self::NotFound => "404 Not Found",
            Self::Conflict => "409 Conflict",
            Self::InternalServerError => "500 Internal Server Error",
        }
    }
}

pub fn read_http_request(stream: &mut TcpStream) -> std::io::Result<HttpRequest> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    let mut headers_end = None;

    while headers_end.is_none() {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        headers_end = find_headers_end(&buffer);
        if buffer.len() > 1024 * 1024 {
            break;
        }
    }

    let headers_end = headers_end.unwrap_or(buffer.len());
    let header_text = String::from_utf8_lossy(&buffer[..headers_end]).to_string();
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    let mut headers = HashMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }

    let body_start = headers_end.saturating_add(4);
    let mut body = if body_start <= buffer.len() {
        buffer[body_start..].to_vec()
    } else {
        Vec::new()
    };
    let content_length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    while body.len() < content_length {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(content_length);

    Ok(HttpRequest {
        method,
        path,
        headers,
        body,
    })
}

fn find_headers_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

pub fn write_response(stream: &mut TcpStream, status: HttpStatus, body: &str) -> std::io::Result<()> {
    let status = status.status_line();
    write!(
        stream,
        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}
