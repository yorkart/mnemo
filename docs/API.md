# Mnemo API

Base URL: `http://127.0.0.1:8787`

## Common Headers

| Header | Direction | Description |
|---|---|---|
| `Authorization: Bearer <token>` | request | Required when `MNEMO_TOKEN` is set. |
| `X-Request-ID` | request/response | Optional request id. Echoed as `x-request-id`; generated when absent. |
| `traceparent` | request/response | Optional W3C trace context. Echoed when supplied. |
| `Idempotency-Key` | request | Supported by memory create, usage report, wrapup, and forget routes when body key is absent. |

## Response Envelope

Success:

```json
{
  "ok": true
}
```

Error:

```json
{
  "ok": false,
  "error": {
    "code": "invalid_request",
    "message": "invalid request: namespace.user_id is required",
    "request_id": "req-..."
  }
}
```

Error codes include `invalid_request`, `not_found`, `event_conflict`,
`idempotency_conflict`, `unauthorized`, `forbidden`, and `internal_error`.

## Namespace

Body routes carry namespace as JSON. Detail/list GET routes carry namespace in
query parameters.

```json
{
  "tenant_id": "default",
  "user_id": "u1",
  "workspace_id": "w1",
  "thread_id": "t1",
  "agent_id": "agent-a",
  "source": "api"
}
```

Normal user memory routes require `user_id`. `tenant_id` defaults to `default`
when canonicalized.

## Pagination

List/search endpoints return:

```json
{
  "page": {
    "limit": 20,
    "next_cursor": "page:20",
    "has_more": true
  }
}
```

Clients should treat `next_cursor` as opaque. The current local adapter emits
`page:<offset>`.

## Endpoints

### `GET /v1/health`

Public health check.

Response:

```json
{
  "ok": true,
  "service": "mnemo",
  "version": "0.1.0"
}
```

### `POST /v1/events`

Write one raw event. `event_id` is the event-level idempotency key.

```json
{
  "event_id": "evt-1",
  "namespace": { "tenant_id": "default", "user_id": "u1", "workspace_id": "w1", "thread_id": "t1" },
  "type": "message",
  "role": "user",
  "content": "remember that I prefer Rust examples",
  "memory_hints": { "eligible": true, "external_context": false },
  "metadata": {}
}
```

Response:

```json
{
  "ok": true,
  "event_id": "evt-1",
  "deduplicated": false,
  "status": "accepted"
}
```

### `POST /v1/events/batch`

Write multiple events under a shared namespace.

```json
{
  "namespace": { "tenant_id": "default", "user_id": "u1", "workspace_id": "w1", "thread_id": "t1" },
  "events": [
    { "event_id": "evt-1", "content": "hello", "role": "user" }
  ]
}
```

### `POST /v1/memories`

Create an explicit memory.

```json
{
  "namespace": { "tenant_id": "default", "user_id": "u1", "workspace_id": "w1" },
  "content": "User prefers concise Chinese answers",
  "memory_type": "preference",
  "importance": "high",
  "conflict_key": "user.answer_style",
  "metadata": {}
}
```

Response:

```json
{
  "ok": true,
  "memory_id": "mem-...",
  "origin": "explicit",
  "memory_type": "preference",
  "memory_status": "active",
  "write_status": "accepted"
}
```

### `POST /v1/query`

Hybrid memory retrieval for agent use.

```json
{
  "namespace": { "tenant_id": "default", "user_id": "u1", "workspace_id": "w1", "thread_id": "t1" },
  "query": "answer style",
  "temporal_scope": "current",
  "scope": {
    "include_thread": true,
    "include_workspace": true,
    "include_user": true,
    "include_tenant": false
  },
  "filters": {
    "memory_types": ["preference"],
    "entity_ids": [],
    "relation_types": []
  },
  "limit": 8,
  "response_format": "results_only"
}
```

`temporal_scope` supports `current`, `historical`, and `all`.

### `POST /v1/context-pack`

Build a budgeted context pack from current memories. Results are cached by
namespace, purpose, token budget, and context state version.

```json
{
  "namespace": { "tenant_id": "default", "user_id": "u1", "workspace_id": "w1", "thread_id": "t1" },
  "purpose": "agent_bootstrap",
  "budget": { "max_tokens": 1200 }
}
```

### `POST /v1/usage`

Record memory usage feedback.

```json
{
  "namespace": { "tenant_id": "default", "user_id": "u1", "workspace_id": "w1" },
  "response_id": "resp-1",
  "used_items": [
    { "memory_id": "mem-1", "usage": "cited" }
  ],
  "metadata": {}
}
```

### `POST /v1/sessions/wrapup`

Create a wrapup job. With `wait=true`, the current adapter processes it
synchronously before returning. Extraction defaults to `codex_cli_v1`, which runs
the configured Codex CLI command and parses JSON memory candidates from stdout.
`local_rules_v1` is only for explicit development/test use and is not used as a
fallback when Codex fails.

```json
{
  "namespace": { "tenant_id": "default", "user_id": "u1", "workspace_id": "w1", "thread_id": "t1" },
  "reason": "session_end",
  "wait": true,
  "timeout_ms": 30000,
  "extraction_instructions": "Only extract coding preferences and project decisions."
}
```

### `GET /v1/jobs`

List jobs for a namespace.

Query parameters: `tenant_id`, `user_id`, `workspace_id`, `thread_id`,
`agent_id`, `source`, `limit`, `cursor`.

### `GET /v1/jobs/{job_id}`

Read one job under the supplied namespace query parameters.

### `POST /v1/namespaces/status`

Return namespace event/memory/job counts and organization cursor state.

```json
{
  "namespace": { "tenant_id": "default", "user_id": "u1", "workspace_id": "w1", "thread_id": "t1" }
}
```

### `GET /v1/namespaces/policy`

Read namespace policy by query parameters.

### `PUT /v1/namespaces/policy`

Replace namespace policy.

```json
{
  "namespace": { "tenant_id": "default", "user_id": "u1", "workspace_id": "w1" },
  "policy": {
    "auto_organize": true,
    "conflict_mode": "suggest",
    "extraction_instructions": "Only extract technical preferences and project decisions."
  }
}
```

### `POST /v1/memories/search`

Administrative memory search with cursor pagination.

```json
{
  "namespace": { "tenant_id": "default", "user_id": "u1", "workspace_id": "w1" },
  "query": "rust",
  "filters": {},
  "pagination": { "limit": 20, "cursor": null }
}
```

### `GET /v1/memories/{memory_id}`

Read memory detail. Phase 4 graph/conflict detail is attached under
`metadata.phase4`.

### `PATCH /v1/memories/{memory_id}`

Patch memory content, importance, status, or metadata.

```json
{
  "content": "updated text",
  "importance": "high",
  "status": "active"
}
```

### `POST /v1/events/search`

Administrative event search with cursor pagination.

```json
{
  "namespace": { "tenant_id": "default", "user_id": "u1", "workspace_id": "w1", "thread_id": "t1" },
  "query": "meeting",
  "pagination": { "limit": 20, "cursor": null }
}
```

### `POST /v1/forget`

Forget one or more memories and enqueue derived cleanup.

```json
{
  "namespace": { "tenant_id": "default", "user_id": "u1", "workspace_id": "w1" },
  "target": { "memory_ids": ["mem-1"] },
  "mode": "soft_delete",
  "reason": "user_requested"
}
```

Supported local modes include `soft_delete`, `disable`, `hard_delete`, and
`anonymize`.
