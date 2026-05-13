use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use serde_json::{Value, json};

pub fn flag_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|window| window[0] == name)
        .map(|window| window[1].clone())
}

pub fn require_flag(args: &[String], name: &str) -> Result<String, String> {
    flag_value(args, name).ok_or_else(|| format!("missing {name}"))
}

pub fn flag_values(args: &[String], name: &str) -> Vec<String> {
    args.windows(2)
        .filter(|window| window[0] == name)
        .map(|window| window[1].clone())
        .collect()
}

pub fn has_flag(args: &[String], name: &str) -> bool {
    args.iter().any(|arg| arg == name)
}

pub fn namespace_json(args: &[String]) -> Result<Value, String> {
    Ok(json!({
        "tenant_id": flag_value(args, "--tenant").unwrap_or_else(|| "default".to_string()),
        "user_id": require_flag(args, "--user")?,
        "workspace_id": flag_value(args, "--workspace"),
        "thread_id": flag_value(args, "--thread"),
        "agent_id": flag_value(args, "--agent"),
        "source": flag_value(args, "--source"),
    }))
}

pub fn unix_timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

pub fn insert_string_flag(
    args: &[String],
    body: &mut serde_json::Map<String, Value>,
    flag: &str,
    field: &str,
) {
    if let Some(value) = flag_value(args, flag) {
        body.insert(field.to_string(), json!(value));
    }
}

pub fn insert_bool_flag(
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

pub fn insert_usize_flag(
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

pub fn insert_u64_flag(
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

pub fn parse_optional_usize_flag(args: &[String], name: &str) -> Result<Option<usize>, String> {
    flag_value(args, name)
        .map(|value| {
            value
                .parse::<usize>()
                .map_err(|err| format!("invalid {name}: {err}"))
        })
        .transpose()
}

pub fn parse_optional_u64_flag(args: &[String], name: &str) -> Result<Option<u64>, String> {
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
