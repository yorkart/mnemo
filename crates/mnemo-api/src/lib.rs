#![forbid(unsafe_code)]

mod config;
mod http;
mod requests;
mod responses;
mod router;
mod server;
mod util;
mod wrapup;

pub use config::ServerConfig;
pub use http::{HttpRequest, HttpResponse, HttpStatus};
pub use responses::{error_response_json, error_response_json_with_request_id, health_response_json};
pub use router::handle_request;
pub use server::{ApiState, serve_blocking};

#[cfg(test)]
mod tests {
    use mnemo_store::InMemoryStore;
    use mnemo_store::SqliteStore;
    use serde_json::json;

    use crate::http::{HttpRequest, HttpStatus};
    use crate::responses::{error_response_json, health_response_json};
    use crate::router::handle_request;
    use crate::server::ApiState;
    use crate::wrapup::CommandWrapupProvider;

    #[test]
    fn health_json_is_stable() {
        assert_eq!(
            health_response_json(),
            r#"{"ok":true,"service":"mnemo","status":"ok"}"#
        );
    }

    #[test]
    fn error_json_escapes_strings() {
        assert_eq!(
            error_response_json("bad\ncode", "quote: \""),
            r#"{"error":{"code":"bad\ncode","message":"quote: \""},"ok":false}"#
        );
    }

    #[test]
    fn error_response_includes_request_id_header() {
        let state = ApiState::new(InMemoryStore::new(), None);
        let request = HttpRequest::new("POST", "/v1/events", b"{}".to_vec())
            .with_header("X-Request-ID", "req-123");

        let response = handle_request(&state, &request);

        assert_eq!(response.status, HttpStatus::BadRequest);
        assert!(response.body.contains(r#""request_id":"req-123""#));
    }

    #[test]
    fn post_event_accepts_and_deduplicates() {
        let state = ApiState::new(InMemoryStore::new(), None);
        let body = event_body("evt-1", "remember apples");
        let request = HttpRequest::new("POST", "/v1/events", body.as_bytes().to_vec());

        let first = handle_request(&state, &request);
        let second = handle_request(&state, &request);

        assert_eq!(first.status, HttpStatus::Ok);
        assert!(first.body.contains(r#""deduplicated":false"#));
        assert_eq!(second.status, HttpStatus::Ok);
        assert!(second.body.contains(r#""deduplicated":true"#));
    }

    #[test]
    fn post_event_generates_stable_event_id_when_missing() {
        let state = ApiState::new(InMemoryStore::new(), None);
        let body = json!({
            "namespace": {
                "tenant_id": "default",
                "user_id": "u1",
                "thread_id": "t1"
            },
            "type": "user_message",
            "role": "user",
            "content": "remember apples",
            "occurred_at": "2026-05-13T10:00:00+08:00"
        })
        .to_string();
        let request = HttpRequest::new("POST", "/v1/events", body.into_bytes());

        let first = handle_request(&state, &request);
        let second = handle_request(&state, &request);

        assert_eq!(first.status, HttpStatus::Ok);
        assert!(first.body.contains(r#""event_id":"evt-auto-"#));
        assert!(first.body.contains(r#""deduplicated":false"#));
        assert_eq!(second.status, HttpStatus::Ok);
        assert!(second.body.contains(r#""deduplicated":true"#));
    }

    #[test]
    fn post_event_can_use_idempotency_key_as_generated_event_id() {
        let state = ApiState::new(InMemoryStore::new(), None);
        let body = json!({
            "namespace": {
                "tenant_id": "default",
                "user_id": "u1",
                "thread_id": "t1"
            },
            "type": "user_message",
            "role": "user",
            "content": "remember apples",
            "occurred_at": "2026-05-13T10:00:00+08:00"
        })
        .to_string();
        let request = HttpRequest::new("POST", "/v1/events", body.into_bytes())
            .with_header("Idempotency-Key", "turn-1");

        let response = handle_request(&state, &request);

        assert_eq!(response.status, HttpStatus::Ok);
        assert!(response.body.contains(r#""event_id":"evt-idem-"#));
    }

    #[test]
    fn post_event_conflict_returns_409() {
        let state = ApiState::new(InMemoryStore::new(), None);
        let first = HttpRequest::new(
            "POST",
            "/v1/events",
            event_body("evt-1", "remember apples").into_bytes(),
        );
        let second = HttpRequest::new(
            "POST",
            "/v1/events",
            event_body("evt-1", "remember watermelon").into_bytes(),
        );

        assert_eq!(handle_request(&state, &first).status, HttpStatus::Ok);
        let response = handle_request(&state, &second);

        assert_eq!(response.status, HttpStatus::Conflict);
        assert!(response.body.contains(r#""code":"event_conflict""#));
    }

    #[test]
    fn post_event_requires_bearer_token_when_configured() {
        let state = ApiState::new(InMemoryStore::new(), Some("secret".to_string()));
        let request = HttpRequest::new(
            "POST",
            "/v1/events",
            event_body("evt-1", "remember apples").into_bytes(),
        );

        let unauthorized = handle_request(&state, &request);
        let authorized = handle_request(
            &state,
            &request
                .clone()
                .with_header("authorization", "Bearer secret"),
        );

        assert_eq!(unauthorized.status, HttpStatus::Unauthorized);
        assert_eq!(authorized.status, HttpStatus::Ok);
    }

    #[test]
    fn post_thread_memory_mode_accepts_polluted() {
        let state = ApiState::new(InMemoryStore::new(), None);
        let body = r#"{
            "namespace": {
                "tenant_id": "default",
                "user_id": "u1",
                "workspace_id": "w1",
                "thread_id": "t1"
            },
            "mode": "polluted"
        }"#;

        let response = handle_request(
            &state,
            &HttpRequest::new("POST", "/v1/threads/memory-mode", body.as_bytes().to_vec()),
        );

        assert_eq!(response.status, HttpStatus::Ok);
        assert!(response.body.contains(r#""thread_id":"t1""#));
        assert!(response.body.contains(r#""mode":"polluted""#));
    }

    #[test]
    fn event_search_returns_matching_events() {
        let state = ApiState::new(InMemoryStore::new(), None);
        let write = HttpRequest::new(
            "POST",
            "/v1/events",
            event_body("evt-1", "remember apples").into_bytes(),
        );
        assert_eq!(handle_request(&state, &write).status, HttpStatus::Ok);
        let search = HttpRequest::new(
            "POST",
            "/v1/events/search",
            r#"{
                "namespace": {
                    "tenant_id": "default",
                    "user_id": "u1",
                    "workspace_id": "w1",
                    "thread_id": "t1",
                    "source": "chat"
                },
                "q": "apples"
            }"#
            .as_bytes()
            .to_vec(),
        );

        let response = handle_request(&state, &search);

        assert_eq!(response.status, HttpStatus::Ok);
        assert!(response.body.contains(r#""event_id":"evt-1""#));
    }

    #[test]
    fn namespace_status_reports_thread_memory_mode() {
        let state = ApiState::new(InMemoryStore::new(), None);
        let set_mode = HttpRequest::new(
            "POST",
            "/v1/threads/memory-mode",
            r#"{
                "namespace": {
                    "tenant_id": "default",
                    "user_id": "u1",
                    "workspace_id": "w1",
                    "thread_id": "t1"
                },
                "mode": "disabled"
            }"#
            .as_bytes()
            .to_vec(),
        );
        assert_eq!(handle_request(&state, &set_mode).status, HttpStatus::Ok);
        let status = HttpRequest::new(
            "POST",
            "/v1/namespaces/status",
            r#"{
                "namespace": {
                    "tenant_id": "default",
                    "user_id": "u1",
                    "workspace_id": "w1",
                    "thread_id": "t1"
                }
            }"#
            .as_bytes()
            .to_vec(),
        );

        let response = handle_request(&state, &status);

        assert_eq!(response.status, HttpStatus::Ok);
        assert!(response.body.contains(r#""memory_mode":"disabled""#));
        assert!(response.body.contains(r#""allows_inferred_memory":false"#));
    }

    #[test]
    fn memory_write_supersedes_same_conflict_key() {
        let state = ApiState::new(InMemoryStore::new(), None);
        let first = HttpRequest::new(
            "POST",
            "/v1/memories",
            memory_body("favorite fruit is watermelon").into_bytes(),
        );
        let second = HttpRequest::new(
            "POST",
            "/v1/memories",
            memory_body("favorite fruit is apple").into_bytes(),
        );

        let first_response = handle_request(&state, &first);
        let second_response = handle_request(&state, &second);

        assert_eq!(first_response.status, HttpStatus::Ok);
        assert_eq!(second_response.status, HttpStatus::Ok);
        assert!(
            second_response
                .body
                .contains(r#""superseded":["mem-000001"]"#)
        );
    }

    #[test]
    fn context_pack_uses_only_active_memory() {
        let state = ApiState::new(InMemoryStore::new(), None);
        let first = HttpRequest::new(
            "POST",
            "/v1/memories",
            memory_body("favorite fruit is watermelon").into_bytes(),
        );
        let second = HttpRequest::new(
            "POST",
            "/v1/memories",
            memory_body("favorite fruit is apple").into_bytes(),
        );
        assert_eq!(handle_request(&state, &first).status, HttpStatus::Ok);
        assert_eq!(handle_request(&state, &second).status, HttpStatus::Ok);

        let context = HttpRequest::new(
            "POST",
            "/v1/context-pack",
            r#"{
                "namespace": {
                    "tenant_id": "default",
                    "user_id": "u1",
                    "workspace_id": "w1"
                },
                "budget": {
                    "max_tokens": 1200
                }
            }"#
            .as_bytes()
            .to_vec(),
        );
        let response = handle_request(&state, &context);

        assert_eq!(response.status, HttpStatus::Ok);
        assert!(response.body.contains("favorite fruit is apple"));
        assert!(!response.body.contains("favorite fruit is watermelon"));
        assert!(
            response
                .body
                .contains(r#""treat_as":"context_data_not_instructions""#)
        );
    }

    #[test]
    fn context_pack_reports_cache_hit_on_repeat_request() {
        let state = ApiState::new(InMemoryStore::new(), None);
        let write = HttpRequest::new(
            "POST",
            "/v1/memories",
            memory_body("favorite fruit is apple").into_bytes(),
        );
        assert_eq!(handle_request(&state, &write).status, HttpStatus::Ok);
        let context = HttpRequest::new(
            "POST",
            "/v1/context-pack",
            r#"{
                "namespace": {
                    "tenant_id": "default",
                    "user_id": "u1",
                    "workspace_id": "w1"
                },
                "budget": {
                    "max_tokens": 1200
                }
            }"#
            .as_bytes()
            .to_vec(),
        );

        let first = handle_request(&state, &context);
        let second = handle_request(&state, &context);

        assert_eq!(first.status, HttpStatus::Ok);
        assert_eq!(second.status, HttpStatus::Ok);
        assert!(first.body.contains(r#""hit":false"#));
        assert!(second.body.contains(r#""hit":true"#));
    }

    #[test]
    fn sqlite_backend_supports_wrapup_context_and_cache() {
        let state = ApiState::new(SqliteStore::open_in_memory().expect("sqlite"), None);
        let event = HttpRequest::new(
            "POST",
            "/v1/events",
            event_body("evt-1", "偏好使用简体中文回答技术问题").into_bytes(),
        );
        assert_eq!(handle_request(&state, &event).status, HttpStatus::Ok);

        let wrapup = HttpRequest::new(
            "POST",
            "/v1/sessions/wrapup",
            r#"{
                "namespace": {
                    "tenant_id": "default",
                    "user_id": "u1",
                    "workspace_id": "w1",
                    "thread_id": "t1",
                    "source": "chat"
                }
            }"#
            .as_bytes()
            .to_vec(),
        );
        let wrapup_response = handle_request(&state, &wrapup);
        assert_eq!(wrapup_response.status, HttpStatus::Ok);
        assert!(
            wrapup_response
                .body
                .contains(r#""created_memory_ids":["mem-000001"]"#)
        );

        let context = HttpRequest::new(
            "POST",
            "/v1/context-pack",
            r#"{
                "namespace": {
                    "tenant_id": "default",
                    "user_id": "u1",
                    "workspace_id": "w1",
                    "thread_id": "t1",
                    "source": "chat"
                },
                "budget": {
                    "max_tokens": 1200
                }
            }"#
            .as_bytes()
            .to_vec(),
        );
        let first_context = handle_request(&state, &context);
        let second_context = handle_request(&state, &context);

        assert_eq!(first_context.status, HttpStatus::Ok);
        assert_eq!(second_context.status, HttpStatus::Ok);
        assert!(first_context.body.contains("偏好使用简体中文回答技术问题"));
        assert!(first_context.body.contains(r#""hit":false"#));
        assert!(second_context.body.contains(r#""hit":true"#));
    }

    #[test]
    fn command_wrapup_provider_generates_summary_and_memory() {
        let script = std::env::temp_dir().join(format!(
            "mnemo-wrapup-provider-{}.sh",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::write(
            &script,
            r#"#!/bin/sh
cat >/dev/null
printf '%s\n' '{"summary":"command provider summary","memories":[{"content":"command provider memory","memory_type":"preference","importance":"high","conflict_key":"provider.memory","source_event_ids":["evt-1"]}]}'
"#,
        )
        .expect("write script");
        let provider =
            CommandWrapupProvider::new(format!("/bin/sh {}", script.display())).expect("provider");
        let state = ApiState::with_wrapup_provider(InMemoryStore::new(), None, provider);

        let event = HttpRequest::new(
            "POST",
            "/v1/events",
            event_body("evt-1", "raw event content").into_bytes(),
        );
        assert_eq!(handle_request(&state, &event).status, HttpStatus::Ok);

        let wrapup = HttpRequest::new(
            "POST",
            "/v1/sessions/wrapup",
            r#"{
                "namespace": {
                    "tenant_id": "default",
                    "user_id": "u1",
                    "workspace_id": "w1",
                    "thread_id": "t1",
                    "source": "chat"
                }
            }"#
            .as_bytes()
            .to_vec(),
        );
        let wrapup_response = handle_request(&state, &wrapup);

        assert_eq!(wrapup_response.status, HttpStatus::Ok);
        assert!(wrapup_response.body.contains("command provider summary"));
        assert!(
            wrapup_response
                .body
                .contains(r#""created_memory_ids":["mem-000001"]"#)
        );

        let search = HttpRequest::new(
            "POST",
            "/v1/memories/search",
            r#"{
                "namespace": {
                    "tenant_id": "default",
                    "user_id": "u1",
                    "workspace_id": "w1",
                    "thread_id": "t1",
                    "source": "chat"
                },
                "q": "command provider memory"
            }"#
            .as_bytes()
            .to_vec(),
        );
        let search_response = handle_request(&state, &search);

        assert_eq!(search_response.status, HttpStatus::Ok);
        assert!(search_response.body.contains("command provider memory"));
        assert!(search_response.body.contains(r#""importance":"high""#));

        let _ = std::fs::remove_file(script);
    }

    #[test]
    fn wrapup_promotes_explicit_memory_intent_when_enabled() {
        let state = ApiState::new(InMemoryStore::new(), None);
        let write = HttpRequest::new(
            "POST",
            "/v1/events",
            event_body("evt-1", "用户偏好使用简体中文回答技术问题").into_bytes(),
        );
        assert_eq!(handle_request(&state, &write).status, HttpStatus::Ok);

        let wrapup = HttpRequest::new(
            "POST",
            "/v1/sessions/wrapup",
            namespace_body_with_source().into_bytes(),
        );
        let wrapup_response = handle_request(&state, &wrapup);

        assert_eq!(wrapup_response.status, HttpStatus::Ok);
        assert!(wrapup_response.body.contains(r#""status":"succeeded""#));
        assert!(
            wrapup_response
                .body
                .contains(r#""created_memory_ids":["mem-000001"]"#)
        );

        let context = HttpRequest::new(
            "POST",
            "/v1/context-pack",
            r#"{
                "namespace": {
                    "tenant_id": "default",
                    "user_id": "u1",
                    "workspace_id": "w1",
                    "thread_id": "t1",
                    "source": "chat"
                }
            }"#
            .as_bytes()
            .to_vec(),
        );
        let context_response = handle_request(&state, &context);
        assert_eq!(context_response.status, HttpStatus::Ok);
        assert!(
            context_response
                .body
                .contains("用户偏好使用简体中文回答技术问题")
        );
    }

    #[test]
    fn failed_wrapup_job_can_be_retried() {
        let state = ApiState::new(InMemoryStore::new(), None);
        let write = HttpRequest::new(
            "POST",
            "/v1/events",
            event_body("evt-1", "用户偏好使用简体中文回答技术问题").into_bytes(),
        );
        assert_eq!(handle_request(&state, &write).status, HttpStatus::Ok);
        let failed_wrapup = HttpRequest::new(
            "POST",
            "/v1/sessions/wrapup",
            r#"{
                "namespace": {
                    "tenant_id": "default",
                    "user_id": "u1",
                    "workspace_id": "w1",
                    "thread_id": "t1",
                    "source": "chat"
                },
                "simulate_failure": true,
                "failure_reason": "worker unavailable"
            }"#
            .as_bytes()
            .to_vec(),
        );
        let failed_response = handle_request(&state, &failed_wrapup);
        assert_eq!(failed_response.status, HttpStatus::Ok);
        assert!(failed_response.body.contains(r#""status":"failed""#));
        assert!(failed_response.body.contains("worker unavailable"));

        let retry = HttpRequest::new(
            "POST",
            "/v1/jobs/retry",
            r#"{
                "job_id": "job-000001"
            }"#
            .as_bytes()
            .to_vec(),
        );
        let retry_response = handle_request(&state, &retry);
        assert_eq!(retry_response.status, HttpStatus::Ok);
        assert!(retry_response.body.contains(r#""job_id":"job-000002""#));
        assert!(retry_response.body.contains(r#""status":"succeeded""#));
        assert!(
            retry_response
                .body
                .contains(r#""created_memory_ids":["mem-000001"]"#)
        );
    }

    #[test]
    fn async_wrapup_is_processed_by_worker_run_once() {
        let state = ApiState::new(InMemoryStore::new(), None);
        let write = HttpRequest::new(
            "POST",
            "/v1/events",
            event_body("evt-1", "用户偏好使用简体中文回答技术问题").into_bytes(),
        );
        assert_eq!(handle_request(&state, &write).status, HttpStatus::Ok);

        let queued_wrapup = HttpRequest::new(
            "POST",
            "/v1/sessions/wrapup",
            r#"{
                "namespace": {
                    "tenant_id": "default",
                    "user_id": "u1",
                    "workspace_id": "w1",
                    "thread_id": "t1",
                    "source": "chat"
                },
                "async": true
            }"#
            .as_bytes()
            .to_vec(),
        );
        let queued_response = handle_request(&state, &queued_wrapup);
        assert_eq!(queued_response.status, HttpStatus::Ok);
        assert!(queued_response.body.contains(r#""status":"queued""#));
        assert!(queued_response.body.contains(r#""attempts":0"#));

        let worker = HttpRequest::new("POST", "/v1/worker/run-once", b"{}".to_vec());
        let worker_response = handle_request(&state, &worker);
        assert_eq!(worker_response.status, HttpStatus::Ok);
        assert!(worker_response.body.contains(r#""processed":true"#));
        assert!(worker_response.body.contains(r#""status":"succeeded""#));
        assert!(worker_response.body.contains(r#""attempts":1"#));
        assert!(
            worker_response
                .body
                .contains(r#""created_memory_ids":["mem-000001"]"#)
        );

        let no_job_response = handle_request(&state, &worker);
        assert_eq!(no_job_response.status, HttpStatus::Ok);
        assert!(no_job_response.body.contains(r#""processed":false"#));
    }

    #[test]
    fn polluted_wrapup_skips_inferred_memory() {
        let state = ApiState::new(InMemoryStore::new(), None);
        let set_mode = HttpRequest::new(
            "POST",
            "/v1/threads/memory-mode",
            r#"{
                "namespace": {
                    "tenant_id": "default",
                    "user_id": "u1",
                    "workspace_id": "w1",
                    "thread_id": "t1"
                },
                "mode": "polluted"
            }"#
            .as_bytes()
            .to_vec(),
        );
        assert_eq!(handle_request(&state, &set_mode).status, HttpStatus::Ok);
        let write = HttpRequest::new(
            "POST",
            "/v1/events",
            event_body("evt-1", "这条 polluted 事件不应推导成长记忆").into_bytes(),
        );
        assert_eq!(handle_request(&state, &write).status, HttpStatus::Ok);

        let wrapup = HttpRequest::new(
            "POST",
            "/v1/sessions/wrapup",
            namespace_body_with_source().into_bytes(),
        );
        let response = handle_request(&state, &wrapup);

        assert_eq!(response.status, HttpStatus::Ok);
        assert!(response.body.contains(r#""inferred_memory_allowed":false"#));
        assert!(response.body.contains(r#""created_memory_ids":[]"#));
    }

    #[test]
    fn usage_feedback_changes_context_pack_order() {
        let state = ApiState::new(InMemoryStore::new(), None);
        let first = HttpRequest::new(
            "POST",
            "/v1/memories",
            memory_body_with_conflict("first preference", "pref.first").into_bytes(),
        );
        let second = HttpRequest::new(
            "POST",
            "/v1/memories",
            memory_body_with_conflict("second preference", "pref.second").into_bytes(),
        );
        assert_eq!(handle_request(&state, &first).status, HttpStatus::Ok);
        assert_eq!(handle_request(&state, &second).status, HttpStatus::Ok);

        let usage = HttpRequest::new(
            "POST",
            "/v1/usage",
            r#"{
                "namespace": {
                    "tenant_id": "default",
                    "user_id": "u1",
                    "workspace_id": "w1"
                },
                "context_pack_id": "ctx-in-memory",
                "signal": "positive",
                "memory_ids": ["mem-000002"]
            }"#
            .as_bytes()
            .to_vec(),
        );
        assert_eq!(handle_request(&state, &usage).status, HttpStatus::Ok);

        let context = HttpRequest::new(
            "POST",
            "/v1/context-pack",
            r#"{
                "namespace": {
                    "tenant_id": "default",
                    "user_id": "u1",
                    "workspace_id": "w1"
                }
            }"#
            .as_bytes()
            .to_vec(),
        );
        let response = handle_request(&state, &context);
        let first_index = response.body.find("first preference").expect("first");
        let second_index = response.body.find("second preference").expect("second");
        assert!(second_index < first_index);
    }

    #[test]
    fn forget_removes_memory_from_context_pack() {
        let state = ApiState::new(InMemoryStore::new(), None);
        let write = HttpRequest::new(
            "POST",
            "/v1/memories",
            memory_body("favorite fruit is watermelon").into_bytes(),
        );
        assert_eq!(handle_request(&state, &write).status, HttpStatus::Ok);

        let forget = HttpRequest::new(
            "POST",
            "/v1/forget",
            r#"{
                "namespace": {
                    "tenant_id": "default",
                    "user_id": "u1",
                    "workspace_id": "w1"
                },
                "memory_ids": ["mem-000001"],
                "reason": "user requested forget"
            }"#
            .as_bytes()
            .to_vec(),
        );
        let forget_response = handle_request(&state, &forget);
        assert_eq!(forget_response.status, HttpStatus::Ok);
        assert!(
            forget_response
                .body
                .contains(r#""forgotten_memory_ids":["mem-000001"]"#)
        );

        let context = HttpRequest::new(
            "POST",
            "/v1/context-pack",
            r#"{
                "namespace": {
                    "tenant_id": "default",
                    "user_id": "u1",
                    "workspace_id": "w1"
                }
            }"#
            .as_bytes()
            .to_vec(),
        );
        let context_response = handle_request(&state, &context);
        assert_eq!(context_response.status, HttpStatus::Ok);
        assert!(
            !context_response
                .body
                .contains("favorite fruit is watermelon")
        );
    }

    #[test]
    fn policy_can_disable_context_pack_memory_use() {
        let state = ApiState::new(InMemoryStore::new(), None);
        let write = HttpRequest::new(
            "POST",
            "/v1/memories",
            memory_body("favorite fruit is watermelon").into_bytes(),
        );
        assert_eq!(handle_request(&state, &write).status, HttpStatus::Ok);

        let set_policy = HttpRequest::new(
            "PUT",
            "/v1/namespaces/policy",
            r#"{
                "namespace": {
                    "tenant_id": "default",
                    "user_id": "u1",
                    "workspace_id": "w1"
                },
                "auto_use_memories": false,
                "context_pack_max_tokens": 64
            }"#
            .as_bytes()
            .to_vec(),
        );
        let policy_response = handle_request(&state, &set_policy);
        assert_eq!(policy_response.status, HttpStatus::Ok);
        assert!(
            policy_response
                .body
                .contains(r#""auto_use_memories":false"#)
        );

        let get_policy = HttpRequest::new(
            "GET",
            "/v1/namespaces/policy",
            r#"{
                "namespace": {
                    "tenant_id": "default",
                    "user_id": "u1",
                    "workspace_id": "w1"
                }
            }"#
            .as_bytes()
            .to_vec(),
        );
        let get_response = handle_request(&state, &get_policy);
        assert_eq!(get_response.status, HttpStatus::Ok);
        assert!(
            get_response
                .body
                .contains(r#""context_pack_max_tokens":64"#)
        );

        let context = HttpRequest::new(
            "POST",
            "/v1/context-pack",
            r#"{
                "namespace": {
                    "tenant_id": "default",
                    "user_id": "u1",
                    "workspace_id": "w1"
                }
            }"#
            .as_bytes()
            .to_vec(),
        );
        let context_response = handle_request(&state, &context);
        assert_eq!(context_response.status, HttpStatus::Ok);
        assert!(
            !context_response
                .body
                .contains("favorite fruit is watermelon")
        );
        assert!(context_response.body.contains(r#""max_tokens":64"#));
    }

    #[test]
    fn policy_can_disable_wrapup_memory_generation() {
        let state = ApiState::new(InMemoryStore::new(), None);
        let set_policy = HttpRequest::new(
            "PUT",
            "/v1/namespaces/policy",
            r#"{
                "namespace": {
                    "tenant_id": "default",
                    "user_id": "u1",
                    "workspace_id": "w1",
                    "thread_id": "t1",
                    "source": "chat"
                },
                "auto_generate_memories": false
            }"#
            .as_bytes()
            .to_vec(),
        );
        assert_eq!(handle_request(&state, &set_policy).status, HttpStatus::Ok);
        let write = HttpRequest::new(
            "POST",
            "/v1/events",
            event_body("evt-1", "用户偏好使用简体中文回答技术问题").into_bytes(),
        );
        assert_eq!(handle_request(&state, &write).status, HttpStatus::Ok);

        let wrapup = HttpRequest::new(
            "POST",
            "/v1/sessions/wrapup",
            namespace_body_with_source().into_bytes(),
        );
        let response = handle_request(&state, &wrapup);
        assert_eq!(response.status, HttpStatus::Ok);
        assert!(response.body.contains(r#""inferred_memory_allowed":false"#));
        assert!(response.body.contains(r#""created_memory_ids":[]"#));
    }

    #[test]
    fn memory_patch_can_deactivate_memory() {
        let state = ApiState::new(InMemoryStore::new(), None);
        let write = HttpRequest::new(
            "POST",
            "/v1/memories",
            memory_body("favorite fruit is watermelon").into_bytes(),
        );
        assert_eq!(handle_request(&state, &write).status, HttpStatus::Ok);

        let patch = HttpRequest::new(
            "PATCH",
            "/v1/memories/mem-000001",
            r#"{
                "namespace": {
                    "tenant_id": "default",
                    "user_id": "u1",
                    "workspace_id": "w1"
                },
                "status": "inactive"
            }"#
            .as_bytes()
            .to_vec(),
        );
        let patch_response = handle_request(&state, &patch);
        assert_eq!(patch_response.status, HttpStatus::Ok);
        assert!(patch_response.body.contains(r#""status":"inactive""#));

        let context = HttpRequest::new(
            "POST",
            "/v1/context-pack",
            r#"{
                "namespace": {
                    "tenant_id": "default",
                    "user_id": "u1",
                    "workspace_id": "w1"
                }
            }"#
            .as_bytes()
            .to_vec(),
        );
        let context_response = handle_request(&state, &context);
        assert_eq!(context_response.status, HttpStatus::Ok);
        assert!(
            !context_response
                .body
                .contains("favorite fruit is watermelon")
        );
    }

    #[test]
    fn conflicts_can_be_searched_and_resolved() {
        let state = ApiState::new(InMemoryStore::new(), None);
        let first = HttpRequest::new(
            "POST",
            "/v1/memories",
            memory_body_with_conflict("first preference", "pref.language").into_bytes(),
        );
        let second = HttpRequest::new(
            "POST",
            "/v1/memories",
            memory_body_with_conflict("second preference", "pref.language").into_bytes(),
        );
        assert_eq!(handle_request(&state, &first).status, HttpStatus::Ok);
        assert_eq!(handle_request(&state, &second).status, HttpStatus::Ok);
        let mark_conflicted = HttpRequest::new(
            "PATCH",
            "/v1/memories/mem-000002",
            r#"{
                "namespace": {
                    "tenant_id": "default",
                    "user_id": "u1",
                    "workspace_id": "w1"
                },
                "status": "conflicted"
            }"#
            .as_bytes()
            .to_vec(),
        );
        assert_eq!(
            handle_request(&state, &mark_conflicted).status,
            HttpStatus::Ok
        );
        let reactivate_first = HttpRequest::new(
            "PATCH",
            "/v1/memories/mem-000001",
            r#"{
                "namespace": {
                    "tenant_id": "default",
                    "user_id": "u1",
                    "workspace_id": "w1"
                },
                "status": "active"
            }"#
            .as_bytes()
            .to_vec(),
        );
        assert_eq!(
            handle_request(&state, &reactivate_first).status,
            HttpStatus::Ok
        );

        let search = HttpRequest::new(
            "POST",
            "/v1/conflicts/search",
            r#"{
                "namespace": {
                    "tenant_id": "default",
                    "user_id": "u1",
                    "workspace_id": "w1"
                }
            }"#
            .as_bytes()
            .to_vec(),
        );
        let search_response = handle_request(&state, &search);
        assert_eq!(search_response.status, HttpStatus::Ok);
        assert!(
            search_response
                .body
                .contains(r#""conflict_key":"pref.language""#)
        );

        let resolve = HttpRequest::new(
            "POST",
            "/v1/conflicts/resolve",
            r#"{
                "namespace": {
                    "tenant_id": "default",
                    "user_id": "u1",
                    "workspace_id": "w1"
                },
                "conflict_key": "pref.language",
                "winner_memory_id": "mem-000002"
            }"#
            .as_bytes()
            .to_vec(),
        );
        let resolve_response = handle_request(&state, &resolve);
        assert_eq!(resolve_response.status, HttpStatus::Ok);
        assert!(
            resolve_response
                .body
                .contains(r#""memory_id":"mem-000002""#)
        );
        assert!(
            resolve_response
                .body
                .contains(r#""superseded_memory_ids":["mem-000001"]"#)
        );
    }

    #[test]
    fn usage_search_filters_by_namespace() {
        let state = ApiState::new(InMemoryStore::new(), None);
        let usage = HttpRequest::new(
            "POST",
            "/v1/usage",
            r#"{
                "namespace": {
                    "tenant_id": "default",
                    "user_id": "u1",
                    "workspace_id": "w1"
                },
                "context_pack_id": "ctx-in-memory",
                "signal": "positive",
                "memory_ids": ["mem-000001"]
            }"#
            .as_bytes()
            .to_vec(),
        );
        assert_eq!(handle_request(&state, &usage).status, HttpStatus::Ok);

        let search = HttpRequest::new(
            "POST",
            "/v1/usage/search",
            r#"{
                "namespace": {
                    "tenant_id": "default",
                    "user_id": "u1",
                    "workspace_id": "w1"
                }
            }"#
            .as_bytes()
            .to_vec(),
        );
        let response = handle_request(&state, &search);
        assert_eq!(response.status, HttpStatus::Ok);
        assert!(response.body.contains(r#""usage_id":"usage-000001""#));
    }

    fn event_body(event_id: &str, content: &str) -> String {
        format!(
            r#"{{
                "event_id": "{event_id}",
                "namespace": {{
                    "tenant_id": "default",
                    "user_id": "u1",
                    "workspace_id": "w1",
                    "thread_id": "t1",
                    "source": "chat"
                }},
                "type": "user_message",
                "role": "user",
                "content": "{content}",
                "occurred_at": "2026-05-13T10:00:00+08:00",
                "memory_hints": {{
                    "eligible": true,
                    "explicit_memory_intent": true,
                    "external_context": false
                }}
            }}"#
        )
    }

    fn memory_body(content: &str) -> String {
        memory_body_with_conflict(content, "user.favorite_fruit")
    }

    fn memory_body_with_conflict(content: &str, conflict_key: &str) -> String {
        format!(
            r#"{{
                "namespace": {{
                    "tenant_id": "default",
                    "user_id": "u1",
                    "workspace_id": "w1"
                }},
                "content": "{content}",
                "memory_type": "preference",
                "importance": "high",
                "conflict_key": "{conflict_key}",
                "source_event_ids": ["evt-1"],
                "valid_from": "2026-05-13T10:00:00+08:00"
            }}"#
        )
    }

    fn namespace_body_with_source() -> String {
        r#"{
            "namespace": {
                "tenant_id": "default",
                "user_id": "u1",
                "workspace_id": "w1",
                "thread_id": "t1",
                "source": "chat"
            }
        }"#
        .to_string()
    }
}
