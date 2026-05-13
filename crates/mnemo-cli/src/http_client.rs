use std::env;
use std::io::Read;
use std::io::Write;
use std::net::TcpStream;

use serde_json::Value;

pub fn http_request(method: &str, path: &str, body: Option<Value>) -> Result<String, String> {
    let base_url =
        env::var("MNEMO_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string());
    let (host, port, base_path) = parse_base_url(&base_url)?;
    let mut stream = TcpStream::connect((host.as_str(), port))
        .map_err(|err| format!("connect failed: {err}"))?;
    let body = body.map(|value| value.to_string()).unwrap_or_default();
    let full_path = format!("{base_path}{path}");
    let mut request = format!(
        "{method} {full_path} HTTP/1.1\r\nhost: {host}:{port}\r\naccept: application/json\r\nconnection: close\r\ncontent-length: {}\r\n",
        body.len()
    );
    if !body.is_empty() {
        request.push_str("content-type: application/json\r\n");
    }
    if let Ok(token) = env::var("MNEMO_TOKEN") {
        if !token.is_empty() {
            request.push_str("authorization: Bearer ");
            request.push_str(&token);
            request.push_str("\r\n");
        }
    }
    request.push_str("\r\n");
    request.push_str(&body);

    stream
        .write_all(request.as_bytes())
        .map_err(|err| format!("request write failed: {err}"))?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|err| format!("response read failed: {err}"))?;
    let (head, response_body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| "invalid http response".to_string())?;
    let status_code = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| "invalid http status".to_string())?;
    if !(200..300).contains(&status_code) {
        return Err(format!("http {status_code}: {response_body}"));
    }
    Ok(response_body.to_string())
}

fn parse_base_url(base_url: &str) -> Result<(String, u16, String), String> {
    let without_scheme = base_url
        .strip_prefix("http://")
        .ok_or_else(|| "MNEMO_BASE_URL must start with http://".to_string())?;
    let (authority, path) = without_scheme
        .split_once('/')
        .unwrap_or((without_scheme, ""));
    let (host, port) = authority.split_once(':').unwrap_or((authority, "80"));
    let port = port
        .parse::<u16>()
        .map_err(|err| format!("invalid MNEMO_BASE_URL port: {err}"))?;
    let base_path = if path.is_empty() {
        String::new()
    } else {
        format!("/{path}")
    };
    Ok((host.to_string(), port, base_path))
}
