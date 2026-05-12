use std::sync::Arc;

use anyhow::Context;
use mnemo_adapters::{NoopAuth, NoopExtraction, SqliteStore};
use mnemo_application::MnemoApp;
use mnemo_config::MnemoConfig;
use mnemo_domain::*;
use serde_json::{Value, json};

use crate::cli::*;
use crate::client::HttpClient;
use crate::helpers::*;

pub async fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Command::Serve {
            bind,
            db,
            artifact_dir,
        } => {
            let mut config = MnemoConfig::from_env();
            if let Some(bind) = bind {
                config.bind = bind;
            }
            if let Some(db) = db {
                config.sqlite_path = db;
            }
            if let Some(artifact_dir) = artifact_dir {
                config.artifact_dir = artifact_dir;
            }
            let store = SqliteStore::open_with_artifact_dir(
                config.sqlite_path.to_string_lossy().as_ref(),
                Some(config.artifact_dir.clone()),
            )?;
            let app = MnemoApp::new(Arc::new(store), Arc::new(NoopAuth), Arc::new(NoopExtraction));
            let worker = mnemo_worker::WorkerRuntime::new(app.clone());
            tokio::spawn(worker.run_loop());
            tracing::info!("serving mnemo on {}", config.bind);
            mnemo_http::serve(app, config.bind).await?;
        }
        Command::Health { json } => {
            let client = HttpClient::new(cli.base_url, cli.token)?;
            let value = client.get("/v1/health").await?;
            print_value(value, json, "mnemo ok")?;
        }
        Command::Ingest {
            namespace,
            text,
            event_id,
            role,
            json,
        } => {
            let client = HttpClient::new(cli.base_url, cli.token)?;
            let request = EventInput {
                event_id: event_id.unwrap_or_else(|| format!("cli-{}", uuid::Uuid::new_v4())),
                namespace: parse_namespace(&namespace),
                event_type: Some("message".to_string()),
                role: Some(role),
                content: text,
                occurred_at: None,
                memory_hints: Some(MemoryHints {
                    eligible: true,
                    external_context: false,
                }),
                metadata: Value::Null,
            };
            let value = client.post("/v1/events", &request, None).await?;
            print_value(value, json, "event accepted")?;
        }
        Command::IngestFile {
            namespace,
            file,
            event_id,
            role,
            json,
        } => {
            let client = HttpClient::new(cli.base_url, cli.token)?;
            let text = std::fs::read_to_string(&file)
                .with_context(|| format!("read {}", file.display()))?;
            let request = EventInput {
                event_id: event_id.unwrap_or_else(|| format!("cli-file-{}", uuid::Uuid::new_v4())),
                namespace: parse_namespace(&namespace),
                event_type: Some("file".to_string()),
                role: Some(role),
                content: text,
                occurred_at: None,
                memory_hints: Some(MemoryHints {
                    eligible: true,
                    external_context: false,
                }),
                metadata: json!({ "file": file.display().to_string() }),
            };
            let value = client.post("/v1/events", &request, None).await?;
            print_value(value, json, "file event accepted")?;
        }
        Command::Remember {
            namespace,
            text,
            memory_type,
            importance,
            idempotency_key,
            json,
        } => {
            let client = HttpClient::new(cli.base_url, cli.token)?;
            let request = MemoryCreateRequest {
                namespace: parse_namespace(&namespace),
                content: text,
                memory_type,
                importance,
                source_event_id: None,
                conflict_key: None,
                valid_from: None,
                valid_until: None,
                supersedes: Vec::new(),
                metadata: Value::Null,
                idempotency_key: idempotency_key.clone(),
            };
            let value = client
                .post("/v1/memories", &request, idempotency_key.as_deref())
                .await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else {
                println!("{}", value["memory_id"].as_str().unwrap_or("accepted"));
            }
        }
        Command::Query {
            namespace,
            q,
            limit,
            json,
        } => {
            let client = HttpClient::new(cli.base_url, cli.token)?;
            let request = QueryRequest {
                namespace: parse_namespace(&namespace),
                query: q,
                as_of: None,
                temporal_scope: "current".to_string(),
                filters: QueryFilters::default(),
                scope: None,
                limit,
                response_format: "results_only".to_string(),
            };
            let value = client.post("/v1/query", &request, None).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else if let Some(results) = value["results"].as_array() {
                for result in results {
                    println!(
                        "{} {}",
                        result["memory_id"].as_str().unwrap_or("-"),
                        result["content"].as_str().unwrap_or("")
                    );
                }
            }
        }
        Command::Context {
            namespace,
            purpose,
            max_tokens,
            json,
        } => {
            let client = HttpClient::new(cli.base_url, cli.token)?;
            let request = ContextPackRequest {
                namespace: parse_namespace(&namespace),
                purpose,
                budget: TokenBudget { max_tokens },
            };
            let value = client.post("/v1/context-pack", &request, None).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else {
                println!("{}", value["content"].as_str().unwrap_or(""));
            }
        }
        Command::Usage { command } => match command {
            UsageCommand::Report {
                namespace,
                memory_id,
                usage,
                response_id,
                idempotency_key,
                json,
            } => {
                let client = HttpClient::new(cli.base_url, cli.token)?;
                let request = UsageRequest {
                    namespace: parse_namespace(&namespace),
                    idempotency_key: idempotency_key.clone(),
                    usage_batch_id: None,
                    response_id,
                    source_requests: Vec::new(),
                    used_items: vec![json!({
                        "type": "memory",
                        "memory_id": memory_id,
                        "usage": usage
                    })],
                    metadata: Value::Null,
                };
                let value = client
                    .post("/v1/usage", &request, idempotency_key.as_deref())
                    .await?;
                print_value(value, json, "usage accepted")?;
            }
        },
        Command::Wrapup {
            namespace,
            reason,
            wait,
            timeout_ms,
            idempotency_key,
            extraction_instructions,
            json,
        } => {
            let client = HttpClient::new(cli.base_url, cli.token)?;
            let request = WrapupRequest {
                namespace: parse_namespace(&namespace),
                idempotency_key: idempotency_key.clone(),
                reason,
                event_range: None,
                wait,
                timeout_ms,
                extraction_instructions,
            };
            let value = client
                .post("/v1/sessions/wrapup", &request, idempotency_key.as_deref())
                .await?;
            print_value(value, json, "wrapup accepted")?;
        }
        Command::Status { namespace, json } => {
            let client = HttpClient::new(cli.base_url, cli.token)?;
            let value = client
                .post(
                    "/v1/namespaces/status",
                    &NamespaceStatusRequest {
                        namespace: parse_namespace(&namespace),
                    },
                    None,
                )
                .await?;
            print_value(value, json, "status ok")?;
        }
        Command::Job {
            namespace,
            job_id,
            json,
        } => {
            let client = HttpClient::new(cli.base_url, cli.token)?;
            let ns = parse_namespace(&namespace).canonical();
            let value = client
                .get(&format!("/v1/jobs/{job_id}?{}", namespace_query(&ns, &[])))
                .await?;
            print_value(value, json, "job ok")?;
        }
        Command::Jobs {
            namespace,
            limit,
            cursor,
            json,
        } => {
            let client = HttpClient::new(cli.base_url, cli.token)?;
            let ns = parse_namespace(&namespace).canonical();
            let mut query = vec![("limit", limit.to_string())];
            if let Some(cursor) = cursor {
                query.push(("cursor", cursor));
            }
            let path = format!("/v1/jobs?{}", namespace_query(&ns, &query));
            let value = client.get(&path).await?;
            print_value(value, json, "jobs ok")?;
        }
        Command::Policy { command } => match command {
            PolicyCommand::Get { namespace, json } => {
                let client = HttpClient::new(cli.base_url, cli.token)?;
                let ns = parse_namespace(&namespace).canonical();
                let value = client
                    .get(&format!(
                        "/v1/namespaces/policy?{}",
                        namespace_query(&ns, &[])
                    ))
                    .await?;
                print_value(value, json, "policy ok")?;
            }
            PolicyCommand::Set {
                namespace,
                policy_json,
                auto_organize,
                json,
            } => {
                let client = HttpClient::new(cli.base_url, cli.token)?;
                let mut policy = if let Some(policy_json) = policy_json {
                    serde_json::from_str::<Value>(&policy_json).context("parse --policy-json")?
                } else {
                    json!({})
                };
                if let Some(auto_organize) = auto_organize {
                    policy["auto_organize"] = Value::Bool(auto_organize);
                }
                let request = PolicyRequest {
                    namespace: parse_namespace(&namespace),
                    policy,
                };
                let value = client.put("/v1/namespaces/policy", &request).await?;
                print_value(value, json, "policy updated")?;
            }
        },
        Command::Memories { command } => match command {
            MemoriesCommand::Search {
                namespace,
                q,
                limit,
                cursor,
                json,
            } => {
                let client = HttpClient::new(cli.base_url, cli.token)?;
                let request = SearchRequest {
                    namespace: parse_namespace(&namespace),
                    query: q,
                    filters: QueryFilters::default(),
                    pagination: PageRequest { limit, cursor },
                };
                let value = client.post("/v1/memories/search", &request, None).await?;
                print_value(value, json, "memories ok")?;
            }
            MemoriesCommand::Get {
                namespace,
                memory_id,
                json,
            } => {
                let client = HttpClient::new(cli.base_url, cli.token)?;
                let ns = parse_namespace(&namespace).canonical();
                let value = client
                    .get(&format!(
                        "/v1/memories/{memory_id}?{}",
                        namespace_query(&ns, &[])
                    ))
                    .await?;
                print_value(value, json, "memory ok")?;
            }
            MemoriesCommand::Patch {
                namespace,
                memory_id,
                content,
                importance,
                status,
                json,
            } => {
                let client = HttpClient::new(cli.base_url, cli.token)?;
                let ns = parse_namespace(&namespace).canonical();
                let patch = MemoryPatch {
                    namespace: None,
                    content,
                    importance,
                    status,
                    metadata: None,
                };
                let value = client
                    .patch(
                        &format!("/v1/memories/{memory_id}?{}", namespace_query(&ns, &[])),
                        &patch,
                    )
                    .await?;
                print_value(value, json, "memory updated")?;
            }
        },
        Command::Events { command } => match command {
            EventsCommand::Search {
                namespace,
                q,
                limit,
                cursor,
                json,
            } => {
                let client = HttpClient::new(cli.base_url, cli.token)?;
                let request = SearchRequest {
                    namespace: parse_namespace(&namespace),
                    query: q,
                    filters: QueryFilters::default(),
                    pagination: PageRequest { limit, cursor },
                };
                let value = client.post("/v1/events/search", &request, None).await?;
                print_value(value, json, "events ok")?;
            }
        },
        Command::Forget {
            namespace,
            memory_id,
            mode,
            reason,
            idempotency_key,
            json,
        } => {
            let client = HttpClient::new(cli.base_url, cli.token)?;
            let request = ForgetRequest {
                namespace: parse_namespace(&namespace),
                idempotency_key: idempotency_key.clone(),
                target: ForgetTarget {
                    memory_ids: vec![memory_id],
                },
                mode,
                reason,
            };
            let value = client
                .post("/v1/forget", &request, idempotency_key.as_deref())
                .await?;
            print_value(value, json, "forget accepted")?;
        }
    }
    Ok(())
}
