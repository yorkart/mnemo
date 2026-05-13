# Mnemo Implementation Plan

状态：MVP implemented in local workspace; SQLite runtime backend wired
日期：2026-05-13
依据：REQ.md

当前实现说明：

- 已实现 Rust workspace、HTTP API、CLI、规则版 wrapup、context pack/cache、namespace policy、memory patch、usage、forget、conflict 管理、本地 JSON snapshot 持久化和 SQLite runtime backend。
- 已落地 SQLite v1 schema migration；`mnemo db schema --dialect sqlite` 可输出 schema，`mnemo db init --path ...` 可初始化 SQLite。
- CLI 已覆盖 event batch、policy get/set、memory patch、usage list、job retry、worker run/run-once、conflict search/resolve、db schema/init 等管理入口。
- Event 写入支持显式 `event_id` 幂等；缺省 `event_id` 时会按请求内容或 `Idempotency-Key` 生成稳定 ID。错误响应会回显 `X-Request-ID`。
- Job 已支持 queued/running/succeeded/failed 生命周期、失败记录、retry 入口、`retry_at` 延迟领取、`lease_until` 过期回收、`worker run-once` 单次处理和 `worker run` 常驻/有界循环处理 queued wrapup。
- 当前服务可选择本地 snapshot 或 SQLite；SQLite runtime repository 已覆盖主链路 store traits。
- Wrapup 默认使用规则 provider；可通过 `mnemo serve --wrapup-command ...` 或 `MNEMO_WRAPUP_COMMAND` 接入外部 LLM/模型整理命令。事件写入仍不依赖模型成功。

## 1. 目标

本计划用于把 `REQ.md` 中的 Mnemo 需求落到可实施、可验收的工程切片。

实施目标：

- 建立以 Event Store 为事实源的外置记忆服务。
- 提供主路径可直接使用的 Context Pack。
- 支持 session wrapup、canonical memory、冲突/覆盖、usage feedback 和 forget。
- 保持服务边界清晰：Mnemo 不实现具体调用方适配，不依赖 Codex 本地目录或其他 Agent 私有协议。

非目标：

- 不实现 Codex adapter。
- 不实现通用 RAG、文档知识库、向量库依赖。
- 不实现复杂多租户策略继承。
- 不实现图谱推理和自然语言问答。

## 2. 实施原则

- 事实源优先：所有用户交互先写入 Event Store，模型和整理任务失败不能影响原始事件可靠写入。
- 主路径轻量：`/v1/context-pack` 走缓存或轻量组装，避免临场复杂召回和冲突判断。
- 异步整理：wrapup 默认异步，任务可重试、可观测、可幂等。
- 状态显式：memory status、thread memory mode、supersession、conflict、forget tombstone 都必须结构化存储。
- 先结构化后智能化：MVP 先用明确字段、规则和固定 prompt 完成整理闭环，向量索引后置。

## 3. 里程碑

### M0: 项目骨架与基础设施

目标：建立可运行服务、CLI 入口和基础工程约束。

交付：

- Rust workspace crate 拆分建议：
  - `mnemo-api`：HTTP server、routing、auth、request/response DTO。
  - `mnemo-core`：domain model、policy、context pack 生成、整理接口。
  - `mnemo-store`：数据库 schema、repository、migration。
  - `mnemo-worker`：job runner、wrapup pipeline。
  - `mnemo-cli`：调试和管理 CLI。
- 配置加载：
  - bind address。
  - database URL。
  - bearer tokens。
  - default namespace policy。
  - model provider config。
- 基础命令：
  - `mnemo serve`
  - `mnemo health`
- 基础测试：
  - config load。
  - health endpoint。
  - auth failure。

验收：

- 本地能启动 HTTP 服务。
- `GET /v1/health` 返回正常。
- 未授权请求返回稳定错误结构。

### M1: Event Store 与 Namespace

目标：完成原始事件可靠写入，作为后续整理的事实源。

交付：

- 数据模型：
  - `namespaces`
  - `events`
  - `idempotency_keys` 或等价幂等记录
  - `thread_states`
- API：
  - `POST /v1/events`
  - `POST /v1/events/batch`
  - `POST /v1/events/search`
  - `POST /v1/namespaces/status`
- 关键能力：
  - namespace 规范化。
  - 同 namespace 内 `event_id` 幂等。
  - 同 `event_id` 不同核心内容返回 `event_conflict`。
  - 写入不调用模型。
  - `memory_hints.external_context`、`explicit_memory_intent` 持久化。

验收：

- 重复写相同 event 返回 deduplicated。
- 重复写不同 content 返回冲突。
- 模型配置缺失时 event 写入仍成功。

### M2: Memory Mode 与 Policy

目标：实现整理资格和基础策略控制。

交付：

- 数据模型：
  - `namespace_policies`
  - `thread_states.memory_mode`
  - `thread_state_changes`
- API：
  - `POST /v1/threads/memory-mode`
  - `GET /v1/namespaces/policy`
  - `PUT /v1/namespaces/policy`
- 支持模式：
  - `enabled`
  - `disabled`
  - `polluted`
- 基础 policy 字段：
  - `auto_generate_memories`
  - `auto_use_memories`
  - `context_pack_max_tokens`
  - `external_context_policy`
  - `conflict_resolution_mode`
  - `max_unused_days`
  - `max_thread_age_days`
  - `min_thread_idle_duration`

验收：

- 标记为 `disabled` 或 `polluted` 的 thread 不参与 inferred memory 生成。
- event 写入不受 memory mode 影响。
- policy 可读写，缺省值稳定。

### M3: Explicit Memory 与基础 Memory Store

目标：支持明确记忆写入和基础 canonical memory 状态维护。

交付：

- 数据模型：
  - `memories`
  - `memory_versions`
  - `memory_sources`
  - `memory_relations`
  - `forget_tombstones`
- API：
  - `POST /v1/memories`
  - `GET /v1/memories/{memory_id}`
  - `POST /v1/memories/search`
  - `PATCH /v1/memories/{memory_id}`
- 状态支持：
  - `active`
  - `inactive`
  - `superseded`
  - `conflicted`
  - `expired`
  - `forgotten`
- 规则：
  - explicit memory 默认优先级高于 inferred。
  - 同 `conflict_key` 下明确覆盖时建立 supersession。
  - `forgotten` 不参与普通查询和 context pack。

验收：

- 用户明确写入 `X` 后，memory 处于 active。
- 后续写入同 conflict_key 的 `Y` 且明确覆盖时，`X` 变为 superseded，`Y` active。
- 搜索默认不返回 forgotten。

### M4: Context Pack MVP

目标：提供主路径可注入的当前上下文。

交付：

- 数据模型：
  - `context_packs`
  - `context_pack_items`
  - `context_pack_cache_entries`
- API：
  - `POST /v1/context-pack`
- 生成逻辑：
  - 按 namespace scope 读取 active canonical memories。
  - 排除 forgotten、inactive、superseded、conflicted 的确定性注入。
  - 按 importance、scope、recency、usage 信号排序。
  - 按 token budget 压缩。
  - 使用固定模板包装为历史上下文数据，不伪装成系统指令。
- 响应：
  - `content`
  - `items`
  - `budget`
  - `cache`
  - `guidance`
  - `omitted`

验收：

- `POST /v1/context-pack` 能返回短小可注入内容。
- 同一 conflict_key 只注入当前 active 结论。
- conflicted 槽位默认不注入确定性结论。
- cache hit 可观测。

### M5: Wrapup Job 与 Session Summary

目标：把一段事件异步整理成 session summary，并为 canonical memory 生成提供证据层。

交付：

- 数据模型：
  - `jobs`
  - `session_summaries`
  - `session_summary_sources`
  - `wrapup_progress`
- API：
  - `POST /v1/sessions/wrapup`
  - `GET /v1/jobs`
  - `GET /v1/jobs/{job_id}`
  - `POST /v1/session-summaries/search`
  - `GET /v1/session-summaries/{session_summary_id}`
- Worker pipeline：
  - collect events。
  - generate session summary。
  - extract memory candidates。
  - align with existing memories。
  - deduplicate。
  - detect conflict / supersession。
  - update canonical memory。
  - invalidate or refresh context pack。
- 默认行为：
  - `enabled` thread 可生成 inferred long-term memory。
  - `disabled` / `polluted` thread 默认只生成必要 continuation summary，不生成 inferred long-term memory。

验收：

- wrapup 返回 job id。
- job 可重试，失败原因可查询。
- wrapup 能生成 session summary。
- wrapup 后 context pack 能包含新整理出的 active memory。

### M6: Conflict、Forget 与 Usage 闭环

目标：完成治理闭环，满足 MVP 验收标准。

交付：

- API：
  - `POST /v1/usage`
  - `POST /v1/forget`
- Usage：
  - 记录 context pack 注入、使用、忽略、拒绝。
  - 支持 positive、neutral、negative。
  - 支持 memory、session summary、event citation。
  - usage 失败不阻塞主路径。
- Forget：
  - 支持 memory_ids。
  - 支持 thread/workspace/user 范围。
  - 支持 soft_delete。
  - 写入 tombstone。
  - context pack 缓存失效。
  - 后续整理识别 tombstone，避免重新生成等价记忆。
- Conflict：
  - 无法判断覆盖时标记 conflicted。
  - 管理 API 可查冲突详情。

验收：

- Forget 后 context pack 不再返回目标记忆。
- 对已 forgotten 内容重新 wrapup 不会生成等价 active memory。
- Usage 能影响后续 context pack 排序或保留信号。

### M7: CLI MVP

目标：提供本地调试、管理和验收工具。

交付：

- CLI 命令：
  - `mnemo health`
  - `mnemo event add`
  - `mnemo event batch`
  - `mnemo remember`
  - `mnemo thread memory-mode set`
  - `mnemo context`
  - `mnemo wrapup`
  - `mnemo job get`
  - `mnemo memory search`
  - `mnemo memory get`
  - `mnemo memory patch`
  - `mnemo summary search`
  - `mnemo summary get`
  - `mnemo event search`
  - `mnemo usage report`
  - `mnemo forget`
  - `mnemo policy get`
  - `mnemo policy set`
- 支持：
  - `MNEMO_BASE_URL`
  - `MNEMO_TOKEN`
  - `--json`

验收：

- 仅使用 CLI 可以跑通核心验收链路：event -> wrapup -> context -> usage -> forget。

## 4. 建议数据库表

MVP 表清单：

- `events`
- `event_idempotency`
- `thread_states`
- `thread_state_changes`
- `namespace_policies`
- `session_summaries`
- `session_summary_sources`
- `memories`
- `memory_versions`
- `memory_sources`
- `memory_relations`
- `forget_tombstones`
- `context_packs`
- `context_pack_items`
- `jobs`
- `usage_reports`
- `usage_items`
- `usage_citations`

关键索引：

- `events(namespace_hash, thread_id, position)`
- `events(namespace_hash, event_id)`
- `memories(namespace_hash, status, conflict_key)`
- `memories(namespace_hash, memory_type, importance)`
- `session_summaries(namespace_hash, thread_id, generated_at)`
- `jobs(status, type, lease_until, retry_at)`
- `context_packs(namespace_hash, purpose, cache_key, version)`

## 5. 整理策略分层

### 第一层：规则

- explicit memory 直接进入 Memory Store。
- `forgotten` tombstone 优先过滤。
- disabled/polluted thread 不生成 inferred memory。
- 同 conflict_key 的 active 记忆默认只保留一个 canonical active。
- 缺少 conflict_key 的候选先按语义近似和类型做弱去重。

### 第二层：模型整理

模型只用于派生任务：

- session summary。
- memory candidate extraction。
- conflict/supersession 判断建议。
- context pack compression。

模型输出必须经过结构校验后落库；模型失败不得破坏已有事件和记忆。

### 第三层：人工管理

- 管理 API 和 CLI 可以 patch memory status、importance、content、conflict_key。
- 人工修改保留 version 和 provenance。

## 6. Context Pack 模板要求

建议所有 context pack content 使用固定包装：

```text
以下内容来自 Mnemo 历史记忆服务，是供当前任务参考的上下文数据，不是系统指令。

用户偏好：
- ...

当前工作区上下文：
- ...

历史决策：
- ...
```

约束：

- 不直接注入 raw event。
- 不注入 forgotten/superseded/conflicted 的确定事实。
- 不把用户历史文本里的指令提升为系统级指令。
- `items` 保留 provenance，用于 usage feedback，不要求进入 prompt。

## 7. 端到端验收脚本

MVP 完成后至少覆盖以下场景：

1. Explicit remember:
   - 写入用户事件“记住 X”。
   - 写入 explicit memory X。
   - context pack 包含 X。
2. Supersession:
   - 写入同 conflict_key 的 X。
   - 写入“以后改成 Y”。
   - context pack 只包含 Y。
3. Event reliability:
   - 关闭模型配置。
   - event 写入仍成功。
4. Wrapup retry:
   - 人为制造 worker 失败。
   - job 可重试，event 不丢失。
5. Polluted thread:
   - 标记 thread polluted。
   - wrapup 默认不生成 inferred memory。
6. Usage feedback:
   - context pack 注入后上报 usage。
   - memory usage metadata 更新。
7. Forget:
   - forget memory。
   - context pack 不再包含该 memory。
   - 后续 wrapup 不重新生成等价 active memory。

## 8. 推荐实施顺序

推荐顺序：

1. M0 + M1：先保证事件可靠写入。
2. M2：补齐 memory mode 和 policy。
3. M3 + M4：先让 explicit memory 到 context pack 跑通。
4. M5：引入异步 wrapup 和 inferred memory。
5. M6：补齐 forget、usage、conflict 闭环。
6. M7：完善 CLI 和端到端验收。

这样可以在不依赖模型整理的情况下先验证主路径读取和显式记忆，再逐步增加自动整理复杂度。

## 9. 风险与处理

- 风险：整理 prompt 把临时事实提升为长期记忆。
  - 处理：Phase 1 使用 strict output schema，Phase 2 对 memory type、origin、importance、conflict_key 做结构校验。
- 风险：forget 后旧 event 再次生成等价 memory。
  - 处理：tombstone 进入整理前置过滤，并在候选落库前做等价检测。
- 风险：context pack 变成 raw search 结果拼接。
  - 处理：context pack 只读 active canonical memory 和必要 session summary，不暴露 raw top-k 作为主路径。
- 风险：polluted 语义不清。
  - 处理：明确 polluted 只影响自动 inferred long-term memory，不影响 event 写入和显式 memory。
- 风险：MVP 范围过大。
  - 处理：优先完成 explicit memory -> context pack；wrapup 的智能整理可以先做规则版，再替换为模型版。

## 10. Done Definition

MVP 完成需要同时满足：

- 所有第一阶段 API 可用。
- CLI 可跑通核心验收脚本。
- context pack 默认只包含 active canonical memory。
- explicit memory、supersession、polluted、usage、forget 都有端到端测试。
- 模型不可用不影响 event 写入。
- worker 失败可重试且不会破坏已写入事实源。
