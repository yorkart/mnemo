# Mnemo 架构文档

本文档描述 Mnemo 当前代码实现的架构边界、模块职责、核心数据流和扩展点。Mnemo
的定位是一个面向 AI Agent 的本地优先记忆服务：底层用结构化存储保证可治理性，
对外通过 HTTP/CLI 提供稳定协议，并派生 Markdown/JSON 产物供人工查看和后续集成。

## 总体目标

Mnemo 解决的是 Agent 长期记忆的旁路管理问题：

- 接收会话事件、显式记忆和使用反馈。
- 通过 wrapup 任务调用 Codex/模型从事件中抽取长期记忆。
- 按 tenant/user/workspace/thread/agent/source 维度隔离和召回记忆。
- 支持记忆时效、冲突键、替代关系、遗忘、策略和审计。
- 为 Agent 生成可注入 prompt 的 context pack。
- 在本地生成 Markdown/JSON 派生产物，便于人工审阅和调试。

## 分层结构

```text
┌───────────────────────────────────────────────────────────┐
│ CLI / HTTP                                                 │
│ mnemo-cli, mnemo-http                                     │
└───────────────────────┬───────────────────────────────────┘
                        │
┌───────────────────────▼───────────────────────────────────┐
│ Application Layer                                          │
│ mnemo-application: 鉴权、用例编排、context-pack 组装         │
└───────────────────────┬───────────────────────────────────┘
                        │
┌───────────────────────▼───────────────────────────────────┐
│ Ports                                                      │
│ mnemo-ports: Store/Auth/Extraction 等 trait 边界            │
└───────────────────────┬───────────────────────────────────┘
                        │
┌───────────────────────▼───────────────────────────────────┐
│ Adapters                                                   │
│ mnemo-adapters: SQLite、Codex CLI 抽取、本地产物生成         │
└───────────────────────┬───────────────────────────────────┘
                        │
┌───────────────────────▼───────────────────────────────────┐
│ Durable State / Artifacts                                  │
│ mnemo.db + mnemo-artifacts/                                │
└───────────────────────────────────────────────────────────┘
```

## Crate 职责

| Crate | 职责 |
|---|---|
| `mnemo-domain` | 请求/响应 DTO、namespace、memory、query、context-pack、usage、job、policy、forget 等领域结构。 |
| `mnemo-ports` | 抽象端口：认证、事务、事件存储、记忆存储、任务、策略、usage、outbox、图谱、索引、产物、抽取 provider。 |
| `mnemo-application` | 用例编排层：参数校验、权限检查、事务边界调用、query/context-pack 组装、job/policy/forget 操作。 |
| `mnemo-adapters` | SQLite 实现、默认 auth/extraction adapter、Codex CLI 抽取、本地规则抽取、索引、ranking、outbox、Markdown/JSON 产物刷新。 |
| `mnemo-http` | Axum HTTP 路由、bearer token 中间件、namespace scope 检查、统一 JSON 响应和 request id。 |
| `mnemo-cli` | Clap CLI；既能启动服务，也能通过 HTTP 调用 health/ingest/remember/query/context/wrapup 等接口。 |
| `mnemo-worker` | 后台 worker runtime；周期性处理 wrapup jobs 和 outbox tasks。 |
| `mnemo-config` | 从环境变量读取 bind、SQLite 路径和 artifact 路径。 |
| `mnemo-telemetry` | tracing 初始化。 |

## 运行形态

### HTTP 服务

`mnemo-cli serve` 会创建 `SqliteStore`、`MnemoApp` 和 Axum router，然后监听
`MNEMO_BIND` 指定的地址。主要路由位于 `/v1` 下：

- `/v1/events`
- `/v1/events/batch`
- `/v1/memories`
- `/v1/query`
- `/v1/context-pack`
- `/v1/usage`
- `/v1/sessions/wrapup`
- `/v1/jobs`
- `/v1/namespaces/status`
- `/v1/namespaces/policy`
- `/v1/memories/search`
- `/v1/events/search`
- `/v1/forget`

### CLI 客户端

`mnemo-cli` 默认通过 `MNEMO_BASE_URL` 访问本地 HTTP 服务。常用命令包括：

- `ingest` / `ingest-file`
- `remember`
- `query`
- `context`
- `usage report`
- `wrapup`
- `jobs` / `job`
- `status`
- `policy get/set`
- `memories search/get/patch`
- `events search`
- `forget`

### Worker

`mnemo-worker` 的 `WorkerRuntime::run_once()` 会依次调用：

1. `MnemoApp::run_jobs_once(20)`：处理 queued wrapup jobs。
2. `MnemoApp::run_outbox_once(100)`：处理索引刷新和 artifact 刷新。

`run_loop()` 目前以 1 秒间隔轮询。

## 核心数据模型

### Namespace

Namespace 是所有数据隔离和权限判断的基础：

```text
tenant_id / user_id / workspace_id / thread_id / agent_id / source
```

`tenant_id` 缺省为 `default`。普通记忆操作要求至少有 `user_id`。召回时可以通过
`QueryScope` 向上扩展：

- thread scope
- workspace scope
- user scope
- tenant scope

### Event

Event 是原始会话输入。字段包括：

- `event_id`
- `namespace`
- `type`
- `role`
- `content`
- `occurred_at`
- `memory_hints`
- `metadata`

`memory_hints.eligible=false` 的事件不会进入 wrapup 抽取；`external_context=true`
的事件也会被排除，避免把外部检索内容错误写入长期记忆。

### Memory

Memory 是系统长期保存的事实或偏好。关键字段：

- `memory_id`
- `content`
- `origin`：`explicit` 或 `inferred`
- `memory_type`：如 `fact`、`preference`、`instruction`、`project_context`
- `importance`
- `status`
- `source_event_id`
- `conflict_key`
- `valid_from` / `valid_until`
- `supersedes` / `superseded_by`
- `metadata`

`conflict_key` 用于表达同一槽位的记忆，例如用户语言偏好、称呼、项目约定等。

## 持久化存储

当前唯一事实存储是 SQLite。主要表：

| 表 | 用途 |
|---|---|
| `events` | 原始事件流。 |
| `memories` | 结构化长期记忆。 |
| `memory_index` | 本地检索索引，包含 terms 和 hash embedding。 |
| `usage_feedback` | 原始使用反馈。 |
| `usage_aggregate` | 按 memory 聚合后的使用统计，用于召回加权。 |
| `jobs` | wrapup/forget 等任务记录。 |
| `policies` | namespace 级策略。 |
| `outbox_tasks` | 异步刷新索引和 artifact 的任务队列。 |
| `context_pack_cache` | context-pack 缓存。 |
| `namespace_cursors` | 已组织事件游标。 |
| `forgotten_tombstones` | 遗忘操作 tombstone。 |
| `idempotency` | 幂等请求记录。 |
| `entities` / `memory_entities` / `relations` | Phase 4 图谱索引。 |
| `conflict_suggestions` | 冲突建议。 |
| `audit_log` | 审计日志。 |

## 派生产物

当启用 `MNEMO_ARTIFACT_DIR` 时，outbox 会刷新本地 Markdown/JSON 文件。默认根目录是：

```text
mnemo-artifacts/
```

namespace 对应的产物目录：

```text
mnemo-artifacts/
  namespaces/
    <safe namespace key>/
      profile.md
      index.json
      periods/
        YYYY-MM.md
      threads/
        <thread_id>.md
      context-packs/
```

这些文件不是事实源，而是 SQLite 中 events/memories 的派生视图。删除或重建 artifact
不应影响真实记忆数据。

## 主要数据流

### 1. 事件写入

```text
Client → HTTP/CLI → MnemoApp::ingest_event → EventStore::append_event
       → events 表
       → outbox(index_event / refresh_artifact)
```

事件写入通过 `(namespace_key, event_id)` 去重。同一个 `event_id` 如果内容一致，会返回
deduplicated；如果内容不同，会返回 conflict。

### 2. 显式记忆写入

```text
Client → /v1/memories 或 mnemo remember
       → MnemoApp::create_memory
       → memories 表
       → conflict / graph / index / audit
       → outbox(index_memory / refresh_artifact)
```

显式记忆支持 idempotency key。写入后会更新本地索引、图谱关系和冲突建议。

### 3. Wrapup 压缩抽取

```text
Client → /v1/sessions/wrapup
       → jobs 表创建 wrapup job
       → worker 或 wait=true 同步处理
       → load eligible events
       → ExtractionProviderConfig
          ├─ codex_cli_v1
          └─ local_rules_v1
       → memories 表写入 inferred memories
       → namespace cursor 前移
       → outbox 刷新索引和 artifact
```

默认抽取 provider 是 `codex_cli_v1`。它会构造 prompt，通过 stdin 传给
`MNEMO_CODEX_COMMAND`，默认命令是：

```bash
codex exec --skip-git-repo-check "$(cat)"
```

当前 Codex 抽取 prompt 仍写在
`crates/mnemo-adapters/src/lib.rs` 的 `render_codex_extraction_prompt` 中，尚未拆成独立模板文件。

`local_rules_v1` 只用于开发和测试，不作为 Codex 失败后的静默 fallback。

### 4. 查询召回

```text
Client → /v1/query
       → scope expansion
       → temporal/filter/graph filtering
       → rank_memories
       → QueryResponse
```

当前召回使用本地 hybrid 策略：

- lexical matching
- hash embedding cosine similarity
- importance score
- usage score
- RRF 风格融合排序

返回结果带有 `provenance.signals`，便于调试每条记忆的排序来源。

### 5. Context Pack

```text
Client → /v1/context-pack
       → context_state_version
       → cache lookup
       → query current memories across thread/workspace/user scopes
       → ranking
       → budgeted prompt text
       → context_pack_cache
```

Context pack 当前输出的是简单文本列表，例如：

```text
- memory_id=... type=... trust=memory_data score=...: ...
```

同时返回结构化 `items`，用于 citation、usage feedback 和 debug。`guidance` 明确要求
Agent 将记忆内容视为 untrusted memory data，而不是系统指令。

### 6. 使用反馈

```text
Client → /v1/usage
       → usage_feedback
       → usage_aggregate
       → context_state_version 变化
       → 后续 query/context-pack 排名受影响
```

`usage` 为 `rejected`、`unused`、`irrelevant`、`bad` 时记为负反馈，其余默认记为正反馈。

### 7. 遗忘

```text
Client → /v1/forget
       → forgotten_tombstones
       → memories status/content 变更
       → forget job completed
       → outbox(forget_cascade)
       → 删除索引/图谱派生数据并刷新 artifact
```

当前支持的模式包括：

- `soft_delete`
- `disable`
- `hard_delete`
- `anonymize`

## 鉴权和权限

HTTP 层通过 `MNEMO_TOKEN` 和 `MNEMO_TOKEN_SCOPES` 做 bearer token 和 scope 检查。
权限字符串包括：

- `events:write`
- `events:read`
- `memories:write`
- `memories:read`
- `memories:delete`
- `context_pack:read`
- `usage:write`
- `jobs:write`
- `jobs:read`
- `policy:read`
- `policy:write`

`mnemo-application` 也保留了 `AuthPort` 抽象，便于未来在非 HTTP 嵌入场景中直接做
token 校验。当前 HTTP 路径已经在 middleware/handler 层完成认证，因此调用
`MnemoApp` 时传入的 token 为 `None`。

## 配置项

| 配置 | 默认值 | 说明 |
|---|---:|---|
| `MNEMO_BIND` | `127.0.0.1:8787` | HTTP 监听地址。 |
| `MNEMO_SQLITE_PATH` | `mnemo.db` | SQLite 文件路径。 |
| `MNEMO_ARTIFACT_DIR` | `mnemo-artifacts` | 派生产物目录；设为 `off` 或 `disabled` 可禁用。 |
| `MNEMO_TOKEN` | unset | HTTP bearer token。 |
| `MNEMO_TOKEN_SCOPES` | `[]` | namespace/permission scope JSON。 |
| `MNEMO_EXTRACTION_PROVIDER` | `codex_cli_v1` | wrapup 抽取 provider。 |
| `MNEMO_CODEX_COMMAND` | `codex exec --skip-git-repo-check "$(cat)"` | Codex CLI 调用命令。 |
| `MNEMO_CODEX_MODEL` | unset | 追加到默认 Codex 命令的模型参数。 |
| `MNEMO_BASE_URL` | `http://127.0.0.1:8787` | CLI 访问的服务地址。 |

## 扩展点

### 抽取 Provider

抽象位于 `mnemo-ports::ExtractionProvider`，当前 SQLite adapter 内部实际使用
`ExtractionProviderConfig`：

- `codex_cli_v1`
- `local_rules_v1`

后续可以将 provider 彻底从 adapter 中拆出，让 wrapup job 通过 port 调用独立
LLM provider。

### 存储 Adapter

`mnemo-ports::Store` 是当前组合 supertrait。理论上可以新增 Postgres、RocksDB 或
远端服务 adapter。需要实现事件、记忆、任务、usage、索引、outbox、图谱、策略等
trait。

### 召回和 Context Pack

当前召回是本地 deterministic hybrid ranking。后续可以在不改外部 API 的前提下增加：

- LLM rerank / model-select。
- 模型友好的 context-pack 渲染。
- `MEMORY.md + topic.md` 风格 Markdown 投影。
- 生产级 embedding/vector DB。

### Artifact

当前 artifact 是 namespace 级 profile/period/thread 文件。后续可以扩展成：

- per-topic markdown。
- 用户画像和项目画像分层。
- 与 Codex hook 直接兼容的 `MEMORY.md`。

## 当前实现边界

- Codex 抽取 prompt 仍硬编码在 adapter 文件中，建议后续迁移到独立模板。
- Context pack 当前是简单 records 拼接，尚未做模型语言化重写。
- 本地 hash embedding 不是生产级语义向量。
- SQLite adapter 中仍保留一部分兼容旧接口的 inherent methods，port trait 和旧方法之间还没有完全收敛。
- `SqliteTransaction` 在 port 抽象上是轻量占位；SQLite 写路径主要在 adapter 内部使用真实 rusqlite transaction。
- `local_rules_v1` 仅适合开发/测试，不应作为生产抽取逻辑。

## 推荐演进顺序

1. 将 `render_codex_extraction_prompt` 抽到独立模板文件，补测试快照。
2. 增加模型友好的 context-pack 渲染层，保留 `items` 作为结构化引用。
3. 增加 Codex hook 示例：session start 调 context-pack，turn/session end 写 events 和 wrapup。
4. 完成 `Store` trait 与 SQLite adapter 的事务边界收敛。
5. 增加可选 LLM rerank/model-select，提高召回效果。
6. 扩展 Markdown artifact 为用户画像、项目画像、topic memory 三类产物。
