use mnemo_domain::Namespace;
use serde_json::Value;

pub fn print_value(value: Value, as_json: bool, fallback: &str) -> anyhow::Result<()> {
    if as_json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!("{fallback}");
    }
    Ok(())
}

pub fn parse_namespace(input: &str) -> Namespace {
    // Slash format: tenant_id/user_id/workspace_id/thread_id
    // agent_id and source are NOT part of the slash format per spec;
    // they must be passed via separate CLI flags.
    let parts = input.split('/').collect::<Vec<_>>();
    Namespace {
        tenant_id: parts
            .first()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()),
        user_id: parts
            .get(1)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()),
        workspace_id: parts
            .get(2)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()),
        thread_id: parts
            .get(3)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()),
        agent_id: None,
        source: None,
    }
}

pub fn namespace_query(namespace: &Namespace, extra: &[(&str, String)]) -> String {
    let mut pairs = vec![
        ("tenant_id", namespace.tenant_id.clone().unwrap_or_default()),
        ("user_id", namespace.user_id.clone().unwrap_or_default()),
        (
            "workspace_id",
            namespace.workspace_id.clone().unwrap_or_default(),
        ),
        ("thread_id", namespace.thread_id.clone().unwrap_or_default()),
        ("agent_id", namespace.agent_id.clone().unwrap_or_default()),
        ("source", namespace.source.clone().unwrap_or_default()),
    ];
    pairs.extend(extra.iter().map(|(key, value)| (*key, value.clone())));
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in pairs {
        serializer.append_pair(key, &value);
    }
    serializer.finish()
}
