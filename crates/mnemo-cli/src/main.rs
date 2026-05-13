#![forbid(unsafe_code)]

use std::env;
use std::io::Read;
use std::io::Write;
use std::net::TcpStream;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use mnemo_api::ServerConfig;
use mnemo_store::SQLITE_INIT_SQL;
use mnemo_store::init_sqlite_database;
use serde_json::Value;
use serde_json::json;

fn main() -> ExitCode {
    match run() {
        Ok(Some(output)) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Ok(None) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<Option<String>, String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("health") => health(&args[1..]),
        Some("serve") => serve(&args[1..]),
        Some("event") => event_command(&args[1..]),
        Some("db") => db_command(&args[1..]),
        Some("remember") => remember(&args[1..]),
        Some("context") => context(&args[1..]),
        Some("thread") => thread_command(&args[1..]),
        Some("policy") => policy_command(&args[1..]),
        Some("wrapup") => wrapup(&args[1..]),
        Some("job") => job_command(&args[1..]),
        Some("memory") => memory_command(&args[1..]),
        Some("summary") => summary_command(&args[1..]),
        Some("usage") => usage_command(&args[1..]),
        Some("conflict") => conflict_command(&args[1..]),
        Some("worker") => worker_command(&args[1..]),
        Some("forget") => forget(&args[1..]),
        Some("-h") | Some("--help") | None => {
            print_help();
            Ok(None)
        }
        Some(command) => Err(format!("unknown command: {command}")),
    }
}

fn health(args: &[String]) -> Result<Option<String>, String> {
    if has_flag(args, "--remote") {
        return http_request("GET", "/v1/health", None).map(Some);
    }
    Ok(Some(mnemo_api::health_response_json()))
}

fn db_command(args: &[String]) -> Result<Option<String>, String> {
    match args.first().map(String::as_str) {
        Some("schema") => {
            let dialect = flag_value(args, "--dialect")
                .or_else(|| flag_value(args, "--provider"))
                .unwrap_or_else(|| "sqlite".to_string());
            match dialect.as_str() {
                "sqlite" => Ok(Some(SQLITE_INIT_SQL.trim_end().to_string())),
                other => Err(format!("unsupported schema dialect: {other}")),
            }
        }
        Some("init") => {
            let dialect = flag_value(args, "--dialect")
                .or_else(|| flag_value(args, "--provider"))
                .unwrap_or_else(|| "sqlite".to_string());
            match dialect.as_str() {
                "sqlite" => {
                    let path = require_flag(args, "--path")?;
                    let user_version = init_sqlite_database(path.as_str())
                        .map_err(|err| format!("db init failed: {err}"))?;
                    Ok(Some(
                        json!({
                            "ok": true,
                            "dialect": "sqlite",
                            "path": path,
                            "user_version": user_version,
                        })
                        .to_string(),
                    ))
                }
                other => Err(format!("unsupported init dialect: {other}")),
            }
        }
        _ => Err(
            "usage: mnemo db schema [--dialect sqlite] | mnemo db init --path ./mnemo.db"
                .to_string(),
        ),
    }
}

fn serve(args: &[String]) -> Result<Option<String>, String> {
    let bind_address = flag_value(args, "--addr").unwrap_or_else(|| "127.0.0.1:8080".to_string());
    let bearer_token = env::var("MNEMO_TOKEN")
        .ok()
        .filter(|value| !value.is_empty());
    let data_path = flag_value(args, "--data").or_else(|| env::var("MNEMO_DATA_PATH").ok());
    let sqlite_path = flag_value(args, "--sqlite").or_else(|| env::var("MNEMO_SQLITE_PATH").ok());
    let wrapup_command =
        flag_value(args, "--wrapup-command").or_else(|| env::var("MNEMO_WRAPUP_COMMAND").ok());
    mnemo_api::serve_blocking(ServerConfig {
        bind_address,
        bearer_token,
        data_path,
        sqlite_path,
        wrapup_command,
    })
    .map_err(|err| format!("failed to serve: {err}"))?;
    Ok(None)
}

fn event_command(args: &[String]) -> Result<Option<String>, String> {
    match args.first().map(String::as_str) {
        Some("add") => {
            let mut body = serde_json::Map::new();
            if let Some(event_id) = flag_value(args, "--event-id") {
                body.insert("event_id".to_string(), json!(event_id));
            }
            body.insert("namespace".to_string(), namespace_json(args)?);
            body.insert(
                "type".to_string(),
                json!(flag_value(args, "--type").unwrap_or_else(|| "user_message".to_string())),
            );
            body.insert(
                "role".to_string(),
                json!(flag_value(args, "--role").unwrap_or_else(|| "user".to_string())),
            );
            body.insert(
                "content".to_string(),
                json!(require_flag(args, "--content")?),
            );
            body.insert(
                "occurred_at".to_string(),
                json!(flag_value(args, "--occurred-at").unwrap_or_else(unix_timestamp_string)),
            );
            body.insert(
                "memory_hints".to_string(),
                json!({
                    "eligible": has_flag(args, "--eligible") || has_flag(args, "--explicit-memory-intent"),
                    "explicit_memory_intent": has_flag(args, "--explicit-memory-intent"),
                    "external_context": has_flag(args, "--external-context"),
                }),
            );
            http_request("POST", "/v1/events", Some(Value::Object(body))).map(Some)
        }
        Some("batch") => {
            let path = require_flag(args, "--file")?;
            let raw = std::fs::read_to_string(&path)
                .map_err(|err| format!("failed to read {path}: {err}"))?;
            let value = serde_json::from_str::<Value>(&raw)
                .map_err(|err| format!("invalid batch json: {err}"))?;
            let body = if value.is_array() {
                json!({ "events": value })
            } else {
                value
            };
            http_request("POST", "/v1/events/batch", Some(body)).map(Some)
        }
        Some("search") => {
            let body = json!({
                "namespace": namespace_json(args)?,
                "q": flag_value(args, "--q"),
            });
            http_request("POST", "/v1/events/search", Some(body)).map(Some)
        }
        _ => Err("usage: mnemo event add|search ...".to_string()),
    }
}

fn remember(args: &[String]) -> Result<Option<String>, String> {
    let body = json!({
        "namespace": namespace_json(args)?,
        "content": require_flag(args, "--content")?,
        "memory_type": flag_value(args, "--memory-type").unwrap_or_else(|| "preference".to_string()),
        "importance": flag_value(args, "--importance").unwrap_or_else(|| "normal".to_string()),
        "conflict_key": flag_value(args, "--conflict-key"),
        "source_event_ids": flag_values(args, "--source-event-id"),
        "valid_from": flag_value(args, "--valid-from"),
    });
    http_request("POST", "/v1/memories", Some(body)).map(Some)
}

fn context(args: &[String]) -> Result<Option<String>, String> {
    let max_tokens = flag_value(args, "--max-tokens")
        .map(|value| {
            value
                .parse::<usize>()
                .map_err(|err| format!("invalid --max-tokens: {err}"))
        })
        .transpose()?
        .unwrap_or(1200);
    let body = json!({
        "namespace": namespace_json(args)?,
        "budget": {
            "max_tokens": max_tokens,
        },
    });
    http_request("POST", "/v1/context-pack", Some(body)).map(Some)
}

fn thread_command(args: &[String]) -> Result<Option<String>, String> {
    match (args.first().map(String::as_str), args.get(1).map(String::as_str)) {
        (Some("memory-mode"), Some("set")) => {
            let body = json!({
                "namespace": namespace_json(args)?,
                "mode": require_flag(args, "--mode")?,
            });
            http_request("POST", "/v1/threads/memory-mode", Some(body)).map(Some)
        }
        _ => Err("usage: mnemo thread memory-mode set --user u1 --thread t1 --mode enabled|disabled|polluted".to_string()),
    }
}

fn policy_command(args: &[String]) -> Result<Option<String>, String> {
    match args.first().map(String::as_str) {
        Some("get") => {
            let body = json!({ "namespace": namespace_json(args)? });
            http_request("GET", "/v1/namespaces/policy", Some(body)).map(Some)
        }
        Some("set") => {
            let mut body = serde_json::Map::new();
            body.insert("namespace".to_string(), namespace_json(args)?);
            insert_bool_flag(args, &mut body, "--auto-generate", "auto_generate_memories")?;
            insert_bool_flag(args, &mut body, "--auto-use", "auto_use_memories")?;
            insert_usize_flag(
                args,
                &mut body,
                "--context-pack-max-tokens",
                "context_pack_max_tokens",
            )?;
            insert_string_flag(
                args,
                &mut body,
                "--external-context-policy",
                "external_context_policy",
            );
            insert_string_flag(
                args,
                &mut body,
                "--conflict-resolution-mode",
                "conflict_resolution_mode",
            );
            insert_u64_flag(args, &mut body, "--max-unused-days", "max_unused_days")?;
            insert_u64_flag(
                args,
                &mut body,
                "--max-thread-age-days",
                "max_thread_age_days",
            )?;
            insert_u64_flag(
                args,
                &mut body,
                "--min-thread-idle-seconds",
                "min_thread_idle_seconds",
            )?;
            http_request("PUT", "/v1/namespaces/policy", Some(Value::Object(body))).map(Some)
        }
        _ => Err("usage: mnemo policy get|set --user u1 [--workspace w1]".to_string()),
    }
}

fn wrapup(args: &[String]) -> Result<Option<String>, String> {
    let body = json!({
        "namespace": namespace_json(args)?,
        "q": flag_value(args, "--q"),
        "generate_memories": !has_flag(args, "--no-generate-memories"),
        "async": has_flag(args, "--async"),
        "simulate_failure": has_flag(args, "--simulate-failure"),
        "failure_reason": flag_value(args, "--failure-reason"),
    });
    http_request("POST", "/v1/sessions/wrapup", Some(body)).map(Some)
}

fn job_command(args: &[String]) -> Result<Option<String>, String> {
    match args.first().map(String::as_str) {
        Some("get") => {
            let job_id = require_flag(args, "--id")?;
            http_request("GET", &format!("/v1/jobs/{job_id}"), None).map(Some)
        }
        Some("list") => http_request("GET", "/v1/jobs", None).map(Some),
        Some("retry") => {
            let body = json!({
                "job_id": require_flag(args, "--id")?,
                "q": flag_value(args, "--q"),
                "generate_memories": !has_flag(args, "--no-generate-memories"),
            });
            http_request("POST", "/v1/jobs/retry", Some(body)).map(Some)
        }
        _ => Err("usage: mnemo job get --id job-000001 | mnemo job list | mnemo job retry --id job-000001".to_string()),
    }
}

fn memory_command(args: &[String]) -> Result<Option<String>, String> {
    match args.first().map(String::as_str) {
        Some("get") => {
            let memory_id = require_flag(args, "--id")?;
            http_request("GET", &format!("/v1/memories/{memory_id}"), None).map(Some)
        }
        Some("search") => {
            let body = json!({
                "namespace": namespace_json(args)?,
                "q": flag_value(args, "--q"),
                "include_inactive": has_flag(args, "--include-inactive"),
            });
            http_request("POST", "/v1/memories/search", Some(body)).map(Some)
        }
        Some("patch") => {
            let memory_id = require_flag(args, "--id")?;
            let mut body = serde_json::Map::new();
            body.insert("namespace".to_string(), namespace_json(args)?);
            insert_string_flag(args, &mut body, "--content", "content");
            insert_string_flag(args, &mut body, "--memory-type", "memory_type");
            insert_string_flag(args, &mut body, "--status", "status");
            insert_string_flag(args, &mut body, "--importance", "importance");
            insert_string_flag(args, &mut body, "--conflict-key", "conflict_key");
            insert_string_flag(args, &mut body, "--valid-from", "valid_from");
            let source_event_ids = flag_values(args, "--source-event-id");
            if !source_event_ids.is_empty() {
                body.insert("source_event_ids".to_string(), json!(source_event_ids));
            }
            http_request(
                "PATCH",
                &format!("/v1/memories/{memory_id}"),
                Some(Value::Object(body)),
            )
            .map(Some)
        }
        _ => Err("usage: mnemo memory get|search ...".to_string()),
    }
}

fn summary_command(args: &[String]) -> Result<Option<String>, String> {
    match args.first().map(String::as_str) {
        Some("get") => {
            let summary_id = require_flag(args, "--id")?;
            http_request("GET", &format!("/v1/session-summaries/{summary_id}"), None).map(Some)
        }
        Some("search") => {
            let body = json!({
                "namespace": namespace_json(args)?,
                "q": flag_value(args, "--q"),
            });
            http_request("POST", "/v1/session-summaries/search", Some(body)).map(Some)
        }
        _ => Err("usage: mnemo summary get|search ...".to_string()),
    }
}

fn usage_command(args: &[String]) -> Result<Option<String>, String> {
    match args.first().map(String::as_str) {
        Some("report") => {
            let body = json!({
                "namespace": namespace_json(args)?,
                "context_pack_id": require_flag(args, "--context-pack-id")?,
                "signal": require_flag(args, "--signal")?,
                "memory_ids": flag_values(args, "--memory-id"),
                "notes": flag_value(args, "--notes"),
            });
            http_request("POST", "/v1/usage", Some(body)).map(Some)
        }
        Some("list") => {
            let body = json!({
                "namespace": namespace_json(args)?,
            });
            http_request("POST", "/v1/usage/search", Some(body)).map(Some)
        }
        _ => Err(
            "usage: mnemo usage report --context-pack-id ctx --signal positive | mnemo usage list --user u1"
                .to_string(),
        ),
    }
}

fn conflict_command(args: &[String]) -> Result<Option<String>, String> {
    match args.first().map(String::as_str) {
        Some("search") => {
            let body = json!({
                "namespace": namespace_json(args)?,
            });
            http_request("POST", "/v1/conflicts/search", Some(body)).map(Some)
        }
        Some("resolve") => {
            let body = json!({
                "namespace": namespace_json(args)?,
                "conflict_key": require_flag(args, "--conflict-key")?,
                "winner_memory_id": require_flag(args, "--winner-memory-id")?,
            });
            http_request("POST", "/v1/conflicts/resolve", Some(body)).map(Some)
        }
        _ => Err("usage: mnemo conflict search --user u1 | mnemo conflict resolve --conflict-key key --winner-memory-id mem-1".to_string()),
    }
}

fn worker_command(args: &[String]) -> Result<Option<String>, String> {
    match args.first().map(String::as_str) {
        Some("run-once") => worker_run_once_command(args),
        Some("run") => worker_run_loop(args),
        _ => Err("usage: mnemo worker run-once [--lease-seconds 300] | mnemo worker run [--max-jobs 10] [--idle-sleep-ms 1000] [--idle-exit-after 3] [--lease-seconds 300]".to_string()),
    }
}

fn worker_run_once_command(args: &[String]) -> Result<Option<String>, String> {
    let body = worker_request_body(args)?;
    http_request("POST", "/v1/worker/run-once", Some(body)).map(Some)
}

fn worker_run_loop(args: &[String]) -> Result<Option<String>, String> {
    let max_jobs = parse_optional_usize_flag(args, "--max-jobs")?;
    let idle_sleep_ms = parse_optional_u64_flag(args, "--idle-sleep-ms")?.unwrap_or(1000);
    let idle_exit_after = parse_optional_usize_flag(args, "--idle-exit-after")?;
    let request_body = worker_request_body(args)?;

    let mut processed_jobs = 0_usize;
    let mut failed_jobs = 0_usize;
    let mut idle_polls = 0_usize;
    let mut iterations = 0_usize;
    let mut last_job_id = None::<String>;

    if max_jobs == Some(0) {
        return Ok(Some(
            json!({
                "ok": true,
                "processed_jobs": processed_jobs,
                "failed_jobs": failed_jobs,
                "idle_polls": idle_polls,
                "iterations": iterations,
                "stopped_reason": "max_jobs",
                "last_job_id": last_job_id,
            })
            .to_string(),
        ));
    }

    loop {
        iterations += 1;
        let raw = http_request("POST", "/v1/worker/run-once", Some(request_body.clone()))?;
        let value = serde_json::from_str::<Value>(&raw)
            .map_err(|err| format!("invalid worker response json: {err}"))?;
        let processed = value
            .get("processed")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        if processed {
            idle_polls = 0;
            processed_jobs += 1;
            if let Some(job) = value.get("job").and_then(Value::as_object) {
                last_job_id = job
                    .get("job_id")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                if job.get("status").and_then(Value::as_str) == Some("failed") {
                    failed_jobs += 1;
                }
            }

            if max_jobs.is_some_and(|limit| processed_jobs >= limit) {
                return Ok(Some(
                    worker_run_summary(
                        processed_jobs,
                        failed_jobs,
                        idle_polls,
                        iterations,
                        "max_jobs",
                        last_job_id,
                    )
                    .to_string(),
                ));
            }
            continue;
        }

        idle_polls += 1;
        if idle_exit_after.is_some_and(|limit| idle_polls >= limit) {
            return Ok(Some(
                worker_run_summary(
                    processed_jobs,
                    failed_jobs,
                    idle_polls,
                    iterations,
                    "idle_exit_after",
                    last_job_id,
                )
                .to_string(),
            ));
        }

        thread::sleep(Duration::from_millis(idle_sleep_ms));
    }
}

fn worker_request_body(args: &[String]) -> Result<Value, String> {
    let mut body = serde_json::Map::new();
    if let Some(lease_seconds) = parse_optional_u64_flag(args, "--lease-seconds")? {
        body.insert("lease_seconds".to_string(), json!(lease_seconds));
    }
    Ok(Value::Object(body))
}

fn worker_run_summary(
    processed_jobs: usize,
    failed_jobs: usize,
    idle_polls: usize,
    iterations: usize,
    stopped_reason: &str,
    last_job_id: Option<String>,
) -> Value {
    json!({
        "ok": true,
        "processed_jobs": processed_jobs,
        "failed_jobs": failed_jobs,
        "idle_polls": idle_polls,
        "iterations": iterations,
        "stopped_reason": stopped_reason,
        "last_job_id": last_job_id,
    })
}

fn forget(args: &[String]) -> Result<Option<String>, String> {
    let memory_ids = flag_values(args, "--memory-id");
    if memory_ids.is_empty() {
        return Err("missing --memory-id".to_string());
    }
    let body = json!({
        "namespace": namespace_json(args)?,
        "memory_ids": memory_ids,
        "reason": flag_value(args, "--reason"),
    });
    http_request("POST", "/v1/forget", Some(body)).map(Some)
}

fn namespace_json(args: &[String]) -> Result<Value, String> {
    Ok(json!({
        "tenant_id": flag_value(args, "--tenant").unwrap_or_else(|| "default".to_string()),
        "user_id": require_flag(args, "--user")?,
        "workspace_id": flag_value(args, "--workspace"),
        "thread_id": flag_value(args, "--thread"),
        "agent_id": flag_value(args, "--agent"),
        "source": flag_value(args, "--source"),
    }))
}

fn http_request(method: &str, path: &str, body: Option<Value>) -> Result<String, String> {
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

fn insert_string_flag(
    args: &[String],
    body: &mut serde_json::Map<String, Value>,
    flag: &str,
    field: &str,
) {
    if let Some(value) = flag_value(args, flag) {
        body.insert(field.to_string(), json!(value));
    }
}

fn insert_bool_flag(
    args: &[String],
    body: &mut serde_json::Map<String, Value>,
    flag: &str,
    field: &str,
) -> Result<(), String> {
    if let Some(value) = flag_value(args, flag) {
        body.insert(field.to_string(), json!(parse_bool(&value)?));
    }
    Ok(())
}

fn insert_usize_flag(
    args: &[String],
    body: &mut serde_json::Map<String, Value>,
    flag: &str,
    field: &str,
) -> Result<(), String> {
    if let Some(value) = flag_value(args, flag) {
        body.insert(
            field.to_string(),
            json!(
                value
                    .parse::<usize>()
                    .map_err(|err| format!("invalid {flag}: {err}"))?
            ),
        );
    }
    Ok(())
}

fn insert_u64_flag(
    args: &[String],
    body: &mut serde_json::Map<String, Value>,
    flag: &str,
    field: &str,
) -> Result<(), String> {
    if let Some(value) = flag_value(args, flag) {
        body.insert(
            field.to_string(),
            json!(
                value
                    .parse::<u64>()
                    .map_err(|err| format!("invalid {flag}: {err}"))?
            ),
        );
    }
    Ok(())
}

fn parse_optional_usize_flag(args: &[String], name: &str) -> Result<Option<usize>, String> {
    flag_value(args, name)
        .map(|value| {
            value
                .parse::<usize>()
                .map_err(|err| format!("invalid {name}: {err}"))
        })
        .transpose()
}

fn parse_optional_u64_flag(args: &[String], name: &str) -> Result<Option<u64>, String> {
    flag_value(args, name)
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|err| format!("invalid {name}: {err}"))
        })
        .transpose()
}

fn parse_bool(value: &str) -> Result<bool, String> {
    match value {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        other => Err(format!("invalid boolean: {other}")),
    }
}

fn flag_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|window| window[0] == name)
        .map(|window| window[1].clone())
}

fn require_flag(args: &[String], name: &str) -> Result<String, String> {
    flag_value(args, name).ok_or_else(|| format!("missing {name}"))
}

fn flag_values(args: &[String], name: &str) -> Vec<String> {
    args.windows(2)
        .filter(|window| window[0] == name)
        .map(|window| window[1].clone())
        .collect()
}

fn has_flag(args: &[String], name: &str) -> bool {
    args.iter().any(|arg| arg == name)
}

fn unix_timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

fn print_help() {
    println!(
        r#"mnemo

USAGE:
  mnemo health [--remote]
  mnemo serve [--addr 127.0.0.1:8080] [--data ./mnemo-artifacts/store.json] [--sqlite ./mnemo.db] [--wrapup-command ./wrapup-provider]
  mnemo db schema [--dialect sqlite]
  mnemo db init --path ./mnemo.db
  mnemo event add --user u1 [--event-id evt-1] --content "..."
  mnemo event batch --file events.json
  mnemo event search --user u1 [--workspace w1] [--thread t1] [--q text]
  mnemo remember --user u1 --content "..." [--conflict-key key]
  mnemo context --user u1 [--workspace w1] [--max-tokens 1200]
  mnemo policy get --user u1 [--workspace w1]
  mnemo policy set --user u1 --auto-use false --context-pack-max-tokens 800
  mnemo thread memory-mode set --user u1 --thread t1 --mode enabled|disabled|polluted
  mnemo wrapup --user u1 --thread t1 [--source chat] [--async] [--simulate-failure]
  mnemo job get --id job-000001
  mnemo job retry --id job-000001
  mnemo worker run-once [--lease-seconds 300]
  mnemo worker run [--max-jobs 10] [--idle-sleep-ms 1000] [--idle-exit-after 3] [--lease-seconds 300]
  mnemo memory patch --user u1 --id mem-000001 --status inactive
  mnemo memory search --user u1 [--q text] [--include-inactive]
  mnemo usage report --user u1 --context-pack-id ctx --signal positive --memory-id mem-000001
  mnemo usage list --user u1
  mnemo conflict search --user u1
  mnemo conflict resolve --user u1 --conflict-key key --winner-memory-id mem-000001
  mnemo forget --user u1 --memory-id mem-000001

ENV:
  MNEMO_BASE_URL=http://127.0.0.1:8080
  MNEMO_TOKEN=optional-bearer-token
  MNEMO_DATA_PATH=./mnemo-artifacts/store.json
  MNEMO_SQLITE_PATH=./mnemo.db
  MNEMO_WRAPUP_COMMAND=./wrapup-provider"#
    );
}
