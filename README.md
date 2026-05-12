# Mnemo

Mnemo 是一个面向 AI Agent 的协议优先记忆服务。它通过稳定的 HTTP 和 CLI
契约，管理原始事件、显式记忆、推断记忆、使用反馈、策略、任务以及派生产物。

当前实现状态：

- 持久化事实存储：SQLite。
- 派生产物存储：`mnemo-artifacts/` 下的本地 Markdown/JSON 文件。
- HTTP 接口：基于 Axum，路由统一位于 `/v1` 下。
- CLI 接口：`mnemo` 子命令支持 ingest、remember、query、context、wrapup、
  policy、search、jobs 和 forget。
- Worker 运行时：处理排队的 wrapup 任务、可选的 Codex 抽取，以及 outbox
  产物/索引刷新。
- 召回：本地混合 lexical/hash-vector 评分，并结合重要性和使用反馈加权。
- 时态记忆：支持 `valid_from`、`valid_until`、`conflict_key`、`supersedes`、
  `superseded_by`，以及 current/historical/all 查询模式。
- Phase 4 图谱支持：实体、关系、冲突建议、记忆详情元数据，以及 forget 级联清理。

## 快速开始

```bash
cargo run -p mnemo-cli --bin mnemo -- serve \
  --bind 127.0.0.1:8787 \
  --db ./mnemo.db \
  --artifact-dir ./mnemo-artifacts
```

健康检查：

```bash
curl --noproxy '*' http://127.0.0.1:8787/v1/health
```

创建一条显式记忆：

```bash
cargo run -p mnemo-cli --bin mnemo -- remember \
  --namespace default/u1/w1/t1 \
  --text "User prefers concise Chinese answers"
```

查询记忆：

```bash
cargo run -p mnemo-cli --bin mnemo -- query \
  --namespace default/u1/w1/t1 \
  --q "answer style" \
  --limit 5
```

构建 context pack：

```bash
cargo run -p mnemo-cli --bin mnemo -- context \
  --namespace default/u1/w1/t1 \
  --purpose agent_bootstrap \
  --max-tokens 1200
```

同步执行一次会话 wrapup：

```bash
cargo run -p mnemo-cli --bin mnemo -- wrapup \
  --namespace default/u1/w1/t1 \
  --wait
```

## 配置

环境变量：

| 变量 | 默认值 | 说明 |
|---|---:|---|
| `MNEMO_BIND` | `127.0.0.1:8787` | HTTP 绑定地址。 |
| `MNEMO_SQLITE_PATH` | `mnemo.db` | SQLite 数据库路径。 |
| `MNEMO_ARTIFACT_DIR` | `mnemo-artifacts` | 产物输出目录。设置为 `off` 或 `disabled` 可在 `SqliteStore::open` 中禁用默认产物输出。 |
| `MNEMO_TOKEN` | unset | 受保护路由所需的可选 bearer token。 |
| `MNEMO_TOKEN_SCOPES` | `[]` | 可选 JSON scope 列表，用于 namespace 和权限检查。 |
| `MNEMO_EXTRACTION_PROVIDER` | `codex_cli_v1` | Wrapup 抽取 provider。`local_rules_v1` 仅用于开发和测试。 |
| `MNEMO_CODEX_COMMAND` | `codex exec --skip-git-repo-check "$(cat)"` | Codex provider 使用的 shell 命令。抽取 prompt 会通过 stdin 传入。 |
| `MNEMO_CODEX_MODEL` | unset | 默认 Codex 命令的可选模型参数。 |
| `MNEMO_BASE_URL` | `http://127.0.0.1:8787` | CLI 目标 base URL。 |

Token scope 示例：

```json
[
  {
    "tenant_id": "default",
    "user_id": "u1",
    "workspace_id": "*",
    "thread_id": "*",
    "permissions": ["memories:read", "memories:write", "events:write"]
  }
]
```

如果未设置 `MNEMO_TOKEN`，受保护路由会在本地开发场景下开放。如果 scopes
为空，bearer token 只用于认证调用方，namespace scope 检查会被跳过。

## Namespace

大多数路由要求 namespace 至少包含 `tenant_id` 和 `user_id`。当 domain
canonicalization 发现 `tenant_id` 缺失时，会默认使用 `default`；但 HTTP
handler 对普通记忆操作仍然要求 user 级 namespace。

CLI namespace 格式：

```text
tenant/user/workspace/thread/agent/source
```

尾部字段可以省略。示例：

```text
default/u1
default/u1/w1
default/u1/w1/t1
```

HTTP JSON namespace 形态：

```json
{
  "tenant_id": "default",
  "user_id": "u1",
  "workspace_id": "w1",
  "thread_id": "t1",
  "agent_id": "agent-a",
  "source": "cli"
}
```

## HTTP 契约

所有成功 JSON 响应都包含：

```json
{ "ok": true }
```

所有错误响应都包含：

```json
{
  "ok": false,
  "error": {
    "code": "invalid_request",
    "message": "invalid request: ...",
    "request_id": "req-..."
  }
}
```

服务会在每个响应中把 `X-Request-ID` 回显为 `x-request-id`。如果请求没有携带
该 header，服务会生成 `req-<uuid>`。如果请求提供了 `traceparent`，服务也会回显。

Cursor 分页使用不透明字符串。当前本地 adapter 会为 jobs 和 search 端点生成
`page:<offset>` 形式的 cursor。

详细端点说明见 [docs/API.md](docs/API.md)。当前 OpenAPI 草案见
[docs/openapi-v1.yaml](docs/openapi-v1.yaml)。

## CLI 参考

```bash
cargo run -p mnemo-cli --bin mnemo -- health
cargo run -p mnemo-cli --bin mnemo -- ingest --namespace default/u1/w1/t1 --text "message"
cargo run -p mnemo-cli --bin mnemo -- ingest-file --namespace default/u1/w1/t1 --file transcript.txt
cargo run -p mnemo-cli --bin mnemo -- remember --namespace default/u1/w1 --text "preference"
cargo run -p mnemo-cli --bin mnemo -- query --namespace default/u1/w1/t1 --q "preference"
cargo run -p mnemo-cli --bin mnemo -- context --namespace default/u1/w1/t1
cargo run -p mnemo-cli --bin mnemo -- wrapup --namespace default/u1/w1/t1 --wait
cargo run -p mnemo-cli --bin mnemo -- jobs --namespace default/u1/w1/t1 --limit 20 --cursor page:20
cargo run -p mnemo-cli --bin mnemo -- job --namespace default/u1/w1/t1 <job_id>
cargo run -p mnemo-cli --bin mnemo -- status --namespace default/u1/w1/t1
cargo run -p mnemo-cli --bin mnemo -- policy get --namespace default/u1/w1/t1
cargo run -p mnemo-cli --bin mnemo -- policy set --namespace default/u1/w1/t1 --auto-organize true
cargo run -p mnemo-cli --bin mnemo -- memories search --namespace default/u1/w1/t1 --q "rust" --limit 20
cargo run -p mnemo-cli --bin mnemo -- memories get --namespace default/u1/w1/t1 <memory_id>
cargo run -p mnemo-cli --bin mnemo -- memories patch --namespace default/u1/w1/t1 <memory_id> --status disabled
cargo run -p mnemo-cli --bin mnemo -- events search --namespace default/u1/w1/t1 --q "meeting" --limit 20
cargo run -p mnemo-cli --bin mnemo -- forget --namespace default/u1/w1/t1 --memory-id <memory_id>
```

在 CLI 命令中添加 `--json` 可以打印原始 JSON 响应。

## 开发

```bash
cargo fmt
cargo check
cargo test
```

Workspace 按职责拆分：

- `mnemo-domain`：请求/响应 DTO 和 domain value objects。
- `mnemo-ports`：`DurableStore` trait 和存储错误。
- `mnemo-application`：use-case facade 和 context-pack 组装。
- `mnemo-adapters`：SQLite 持久化存储、本地召回、产物、worker 操作。
- `mnemo-http`：Axum 路由、认证、请求上下文、响应映射。
- `mnemo-cli`：Clap CLI 和 HTTP client。
- `mnemo-worker`：后台 job 和 outbox loop。
- `mnemo-config`：环境配置。
- `mnemo-telemetry`：tracing 初始化。

## 当前限制

- 召回使用本地 lexical/hash-vector 评分，不是生产级 vector DB。
- Wrapup 压缩默认使用 Codex CLI。`local_rules_v1` 仅作为显式开发/测试抽取器保留，
  不会在 Codex 失败时作为静默 fallback。
- 分页 cursor 对本地 adapter 是稳定的，但客户端应将其视为不透明值。
- OpenAPI 文件是当前契约草案快照；在生成外部 SDK 前，应从 typed schemas
  重新生成。
