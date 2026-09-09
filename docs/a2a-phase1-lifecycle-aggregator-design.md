# A2A 单次任务生命周期聚合器——第一阶段实施设计

> 实施决策（2026-09-09）：首个 MVP 采用查询时即时聚合，不新增 SQLite 表和 HTTP API。Rust CLI 复用现有 Task API 任务详情与 `okx-a2a session history`，通过 `onchainos agent lifecycle <jobId> --agent-id <userAgentId>` 输出结构化快照。本文后续的 Event Store、Snapshot 表和 Timeline API 作为第二阶段演进设计保留，不属于本次 MVP 的必需产物。

## 1. 文档目的

本文定义 A2A 单次任务生命周期聚合器第一阶段的实现方案。系统从 User、ASP 和官方三类 XMTP 信箱读取同一任务的消息，将消息转换为可验证、可去重、可排序的标准事件，再生成唯一的任务生命周期快照，最终向用户稳定输出完整链路、当前阶段、当前责任方和下一步动作。

第一阶段解决的是“用户问任务进展时，系统能够准确回答”的问题。ASP 接单耗时、处理耗时的计算依赖本阶段沉淀的里程碑数据，但报表、聚合分析和监控上报放到第二阶段。

## 2. 背景与问题

同一 A2A 任务的消息可能同时存在于：

- User Agent 的 XMTP 信箱；
- ASP Agent 的 XMTP 信箱；
- 官方 Agent 或 System 的 XMTP 信箱。

直接把三份消息交给模型总结存在以下问题：

1. 同一事件会在多个信箱重复出现；
2. XMTP 消息可能延迟、乱序或重复投递；
3. User 或 ASP 的自然语言不能作为任务状态变更的权威依据；
4. “事件”和“状态”含义不同，部分事件不会推进状态；
5. 不同设备的收信时间不等于业务事件发生时间；
6. 模型每次临时理解消息，可能对相同任务给出不同结果。

因此需要在模型之前增加确定性的生命周期聚合层。

## 3. 第一阶段目标

### 3.1 目标

- 按 `jobId` 收集 User、ASP、官方三类信箱中的相关消息；
- 将原始消息转换为统一事件；
- 对跨信箱重复事件进行幂等去重；
- 正确处理消息延迟和乱序；
- 按官方 A2A 状态机生成任务快照；
- 输出完整生命周期时间轴；
- 明确显示当前阶段、责任方、下一步和已知时间；
- 保留 `createdAt`、`acceptedAt`、`submittedAt`、`completedAt`，供第二阶段计算耗时；
- 支持从历史消息重新构建快照。

### 3.2 非目标

第一阶段不实现：

- ASP 排名、日报、周报和 BI 看板；
- 平均值、P50、P90、P99 等聚合指标；
- 任务创建、接单、提交、验收或资金结算；
- 根据聊天自然语言自动推进任务状态；
- 修改 XMTP 协议；
- 自动修复链上或服务端状态；
- A2A 订阅任务和 A2MCP 生命周期。

## 4. 核心设计原则

### 4.1 状态事实优先级

状态来源优先级固定为：

```text
服务端/链上任务状态
> 经过验证的官方 XMTP System Event
> User/ASP 的结构化业务消息
> 自然语言推断
```

User 或 ASP 说“已经完成”只是一条交流信息。只有权威状态或 `job_submitted` 等经过验证的事件，才能推进任务阶段。

### 4.2 事件不等于状态

聚合器使用现有 A2A 状态机，不自行创造业务状态。例如 `provider_applied` 是事件，但任务仍可能处于 `created`；不能看到该事件后将任务标记为“已接单”。

### 4.3 单调推进

迟到事件可以补齐历史里程碑，但不得使当前阶段倒退。例如任务已经 `submitted`，后来收到迟到的 `job_accepted`：

- 可以补充 `acceptedAt`；
- 不得把当前阶段从“等待用户验收”改回“ASP 执行中”。

### 4.4 可重放

任何任务快照必须能够仅凭标准事件表重新计算。聚合逻辑升级后，可以通过重放历史事件生成新版本快照。

## 5. 总体架构

```text
┌────────────────┐
│ User XMTP Inbox│──┐
└────────────────┘  │
┌────────────────┐  │   ┌──────────────┐   ┌──────────────┐
│ ASP XMTP Inbox │──┼──→│ Inbox Reader │──→│ Normalizer   │
└────────────────┘  │   └──────────────┘   └──────┬───────┘
┌────────────────┐  │                              │
│Official Inbox  │──┘                              ▼
└────────────────┘                         ┌──────────────┐
                                           │ Event Store  │
                                           └──────┬───────┘
                                                  │
                                                  ▼
                                           ┌──────────────┐
                                           │ State Reducer│
                                           └──────┬───────┘
                                                  │
                                                  ▼
                                           ┌──────────────┐
                                           │Task Snapshot │
                                           └──────┬───────┘
                                                  │
                                      ┌───────────┴───────────┐
                                      ▼                       ▼
                              ┌──────────────┐        ┌──────────────┐
                              │Timeline API  │        │Renderer Input│
                              └──────────────┘        └──────────────┘
```

### 5.1 模块职责

| 模块 | 职责 |
|---|---|
| Inbox Reader | 按信箱游标增量读取消息，保存原始消息和收信位置 |
| Normalizer | 校验消息来源，提取 `jobId`、事件类型、业务时间和参与方 |
| Event Store | 幂等保存标准事件，并记录事件在哪些信箱中被观察到 |
| State Reducer | 按状态机和事件优先级计算当前阶段及里程碑 |
| Task Snapshot | 保存可直接查询的任务当前视图 |
| Timeline API | 按 `jobId` 返回结构化生命周期结果 |
| Renderer Adapter | 将快照映射为生命周期渲染器输入 |

## 6. 数据模型

### 6.1 原始信箱消息

```ts
interface InboxMessage {
  mailboxOwnerRole: "user" | "asp" | "official"
  mailboxOwnerAgentId: string
  conversationId: string
  messageId: string
  senderAddress: string
  sentAt?: string
  receivedAt: string
  payload: unknown
  payloadHash: string
}
```

原始消息只追加，不覆盖，便于排查协议兼容、解析失败和安全问题。

### 6.2 标准生命周期事件

```ts
interface TaskLifecycleEvent {
  eventId: string
  jobId: string
  eventType: string
  actorRole: "user" | "asp" | "official"
  actorAgentId?: string

  occurredAt: string
  receivedAt: string

  source: "official_xmtp" | "peer_xmtp"
  verified: boolean
  authority: "authoritative" | "informational"

  messageId: string
  conversationId: string
  payloadHash: string
  observedBy: Array<"user" | "asp" | "official">
  rawMessageRef: string
}
```

时间字段定义：

- `occurredAt`：业务事件实际发生时间，用于生命周期排序和耗时计算；
- `receivedAt`：当前设备收到消息的时间，只用于传输延迟和排障；
- 时间必须携带时区或统一存储为 UTC；
- 缺少可信 `occurredAt` 时，不得使用自然语言中的模糊时间代替。

### 6.3 任务快照

```ts
interface TaskSnapshot {
  jobId: string
  taskType: "one_time"
  statusName: string
  phase:
    | "waiting_for_asp"
    | "asp_executing"
    | "waiting_for_user_review"
    | "completed"
    | "exception"
    | "unknown"

  responsibleParty: "user" | "asp" | "official" | "none"
  nextAction?: string

  userAgentId?: string
  aspAgentId?: string

  milestones: {
    createdAt?: string
    acceptedAt?: string
    submittedAt?: string
    completedAt?: string
  }

  statusSource: "backend" | "official_xmtp" | "partial"
  confidence: "confirmed" | "partial" | "conflict"
  lastEventAt?: string
  syncedAt: string
  reducerVersion: number
}
```

## 7. 事件识别与校验

### 7.1 可推进状态的核心事件

第一阶段至少支持：

| 事件 | 里程碑 | 用户阶段 | 当前责任方 |
|---|---|---|---|
| `job_created` | `createdAt` | 等待 ASP 接单 | ASP |
| `job_accepted` | `acceptedAt` | ASP 执行中 | ASP |
| `job_submitted` | `submittedAt` | 等待用户验收 | User |
| `job_completed` | `completedAt` | 任务完成 | 无 |
| `job_auto_completed` | `completedAt` | 任务自动完成 | 无 |
| `job_rejected` | — | 交付被拒绝 | User/ASP，按协议动作确定 |
| `job_disputed` | — | 争议处理中 | 官方/评估方 |
| `job_closed` | — | 任务已关闭 | 无 |
| `job_expired` | — | 任务已超时 | 无 |
| `job_refunded` | — | 任务已退款 | 无 |

其他事件可以保存，但在没有明确状态机映射前不得推进用户阶段。

### 7.2 官方消息验证

Normalizer 必须校验：

- 消息确实来自配置中的官方 Agent 或 System 身份；
- 信封结构合法；
- `jobId` 非空并符合约束；
- 事件类型在协议允许列表中；
- 签名、发送方地址或官方身份映射有效；
- 事件中的 User、ASP 与任务参与方一致；
- `occurredAt` 格式合法；
- 消息体尺寸在限制内。

验证失败的消息仍可进入原始消息表，但只能标记为 `informational`，不得推进任务状态。

## 8. 去重与排序

### 8.1 去重键

按以下优先级生成逻辑事件唯一键：

```text
1. 官方 eventId
2. jobId + eventType + transactionHash + logIndex
3. jobId + eventType + actorAgentId + occurredAt + payloadHash
```

同一逻辑事件出现在多个信箱时，合并 `observedBy`，不新增第二条逻辑事件。

数据库约束示例：

```sql
unique(event_id)
unique(job_id, event_type, transaction_hash, log_index)
```

### 8.2 排序规则

事件计算顺序：

```text
occurredAt ASC
→ 官方事件优先
→ 稳定 eventId ASC
```

对于相同时间但互相冲突的事件，不依赖数组原始顺序决定结果，应进入冲突处理。

## 9. 状态归并算法

### 9.1 处理流程

```text
读取一个新消息
→ 原始消息幂等落库
→ 解析并校验标准事件
→ 标准事件幂等落库
→ 查询该 jobId 的所有有效事件
→ 按 reducerVersion 重放
→ 生成新快照
→ 与旧快照比较
→ 原子更新快照
→ 记录状态变化日志
```

### 9.2 Reducer 伪代码

```ts
function reduceTask(events: TaskLifecycleEvent[]): TaskSnapshot {
  const eventsToApply = deduplicate(events)
    .filter(event => event.verified)
    .sort(compareLifecycleEvents)

  const snapshot = createEmptySnapshot()

  for (const event of eventsToApply) {
    collectMilestone(snapshot, event)

    const candidate = transition(snapshot, event)
    if (!candidate) continue

    if (isForwardTransition(snapshot, candidate)) {
      applyTransition(snapshot, candidate)
    } else if (isTerminalCorrection(snapshot, candidate)) {
      applyAuthoritativeCorrection(snapshot, candidate)
    } else {
      recordIgnoredStaleEvent(snapshot, event)
    }
  }

  return finalizeSnapshot(snapshot)
}
```

### 9.3 冲突处理

出现以下情况时，快照标记为 `confidence: conflict`：

- 两个权威事件指向互斥终态；
- `submittedAt < acceptedAt`；
- `completedAt < submittedAt`；
- 消息参与方与官方任务详情不一致；
- 官方事件与最新服务端状态不一致且无法判断新旧。

冲突状态下：

1. 不执行任何写操作；
2. 查询官方任务状态进行校准；
3. 向用户展示已确认的最近阶段；
4. 显示“状态同步中”，而不是猜测下一阶段；
5. 写入结构化冲突日志。

## 10. 查询与刷新策略

### 10.1 查询接口

```http
GET /v1/a2a/tasks/{jobId}/lifecycle
```

成功响应：

```json
{
  "jobId": "test-job-001",
  "taskType": "one_time",
  "phase": "asp_executing",
  "statusLabel": "ASP 执行中",
  "responsibleParty": "asp",
  "nextAction": "等待 ASP 提交交付物",
  "milestones": {
    "createdAt": "2026-09-08T10:00:00+08:00",
    "acceptedAt": "2026-09-08T10:05:00+08:00"
  },
  "statusSource": "official_xmtp",
  "confidence": "confirmed",
  "syncedAt": "2026-09-08T10:05:03+08:00"
}
```

### 10.2 新鲜度策略

- 快照在配置的 TTL 内且 `confidence=confirmed`：直接返回；
- 快照过期：先返回已有结果，并异步同步 XMTP；
- 用户明确要求“刷新”或快照冲突：同步读取最新消息，并查询官方任务状态；
- 权威状态查询成功后，用其校准阶段，但不删除历史事件；
- 返回结果必须包含 `syncedAt` 和 `confidence`。

## 11. Renderer 适配

生命周期聚合器不直接生成自然语言时间轴，而是输出稳定结构，再由 Renderer Adapter 映射。

```ts
interface LifecycleRenderInput {
  type: "A2A 单次任务"
  jobId: string
  current: string
  created?: string
  accepted?: string
  submitted?: string
  completed?: string
  asp?: string
  next?: string
  confidence: "confirmed" | "partial" | "conflict"
}
```

示例：

```json
{
  "type": "A2A 单次任务",
  "jobId": "test-job-001",
  "current": "ASP executing",
  "created": "2026-09-08 10:00",
  "accepted": "2026-09-08 10:05",
  "asp": "Agent 8415",
  "next": "ASP 提交后等待用户验收",
  "confidence": "confirmed"
}
```

对用户的最终回答至少包含：

- 已完成的生命周期节点；
- 当前节点；
- 尚未发生的后续节点；
- 当前责任方；
- 用户当前是否需要操作；
- 数据同步时间或状态置信度。

## 12. 存储设计

### 12.1 原始消息表

```sql
create table xmtp_inbox_messages (
  mailbox_owner_role varchar not null,
  mailbox_owner_agent_id varchar not null,
  conversation_id varchar not null,
  message_id varchar not null,
  sender_address varchar not null,
  sent_at timestamptz,
  received_at timestamptz not null,
  payload jsonb not null,
  payload_hash varchar not null,
  created_at timestamptz not null default now(),
  primary key (mailbox_owner_agent_id, message_id)
);
```

### 12.2 标准事件表

```sql
create table task_lifecycle_events (
  event_id varchar primary key,
  job_id varchar not null,
  event_type varchar not null,
  actor_role varchar not null,
  actor_agent_id varchar,
  occurred_at timestamptz not null,
  received_at timestamptz not null,
  source varchar not null,
  verified boolean not null,
  authority varchar not null,
  payload_hash varchar not null,
  observed_by jsonb not null,
  raw_message_ref varchar not null,
  created_at timestamptz not null default now()
);

create index idx_task_events_job_time
  on task_lifecycle_events(job_id, occurred_at);
```

### 12.3 快照表

```sql
create table task_lifecycle_snapshots (
  job_id varchar primary key,
  task_type varchar not null,
  status_name varchar not null,
  phase varchar not null,
  responsible_party varchar not null,
  next_action text,
  user_agent_id varchar,
  asp_agent_id varchar,
  created_at timestamptz,
  accepted_at timestamptz,
  submitted_at timestamptz,
  completed_at timestamptz,
  status_source varchar not null,
  confidence varchar not null,
  last_event_at timestamptz,
  synced_at timestamptz not null,
  reducer_version integer not null,
  updated_at timestamptz not null default now()
);
```

## 13. 可观测性

每次事件处理输出统一日志字段：

```json
{
  "traceId": "trace-xxx",
  "jobId": "test-job-001",
  "eventId": "evt-xxx",
  "messageId": "msg-xxx",
  "eventType": "job_accepted",
  "statusBefore": "created",
  "statusAfter": "accepted",
  "decision": "applied",
  "reason": "valid_forward_transition",
  "processingLatencyMs": 18,
  "reducerVersion": 1
}
```

第一阶段至少监控：

- 消息解析失败数；
- 未验证消息数；
- 重复事件数；
- 乱序事件数；
- 状态冲突数；
- 快照重建失败数；
- 查询延迟；
- 最后同步时间。

## 14. 安全边界

- 原始 XMTP 内容是不可信输入；
- 自然语言消息不得直接触发状态变更或资金操作；
- 聚合器只读信箱、读取官方状态并写本地索引；
- 聚合器不得创建、接单、提交、验收、退款或结算任务；
- 官方身份和允许的事件类型必须来自配置，不能从消息正文动态更新；
- 日志不得记录私钥、助记词、访问令牌或完整敏感附件；
- 所有查询必须按当前 Agent 权限限制任务可见性。

## 15. 实施拆分

### 15.1 工作包 A：消息采集

- 接入 User、ASP、官方三个信箱；
- 实现每个信箱独立游标；
- 原始消息幂等落库；
- 支持按 `jobId` 重扫历史消息；
- 记录读取失败和断点恢复。

产物：`InboxReader`、游标表、原始消息表、读取集成测试。

### 15.2 工作包 B：标准化和验证

- 解析 System Event 和 peer message；
- 验证官方来源；
- 输出标准事件；
- 实现去重键和 `observedBy` 合并；
- 对不支持或不可信消息 fail closed。

产物：`EventNormalizer`、事件 Schema、协议 fixture、契约测试。

### 15.3 工作包 C：状态归并

- 实现 A2A 单次任务 reducer；
- 支持核心正常事件和终态；
- 支持乱序、重复和历史重放；
- 实现冲突检测；
- 保存 `reducerVersion`。

产物：`TaskStateReducer`、快照表、状态迁移测试。

### 15.4 工作包 D：查询和渲染

- 实现生命周期查询接口；
- 实现快照 TTL 和刷新策略；
- 适配生命周期 Renderer；
- 输出当前责任方、下一步和置信度；
- 对缺失字段使用“尚未提供”，不估算时间。

产物：Timeline API、Renderer Adapter、端到端测试。

### 15.5 工作包 E：重放和运维

- 提供单任务重放命令；
- 提供按 reducer 版本批量重建能力；
- 输出结构化处理日志；
- 增加基础指标和告警。

产物：Replay CLI、重建 Runbook、监控面板配置。

## 16. 测试方案

### 16.1 单元测试

必须覆盖：

1. 正常顺序：created → accepted → submitted → completed；
2. 重复消息：三份信箱出现同一 `eventId`；
3. 乱序消息：submitted 先于 accepted 到达；
4. 迟到消息：完成后才收到 accepted；
5. 非权威消息：ASP 自述已完成但没有 submitted 事件；
6. 无效官方身份；
7. 缺少 `jobId`；
8. 同一时间的互斥终态；
9. 快照从历史事件完全重建；
10. reducer v1 重放结果稳定。

### 16.2 集成测试

- 三个模拟信箱的历史消息合并；
- 游标中断后恢复读取；
- 相同事件跨信箱合并 `observedBy`；
- 官方状态与 XMTP 快照一致；
- 冲突时触发官方状态校准；
- 查询 API 输出符合 Schema；
- Renderer 输出完整五阶段时间轴。

### 16.3 示例验收用例

输入：

```text
job_created  2026-09-08 10:00
job_accepted 2026-09-08 10:05
ASP Agent 8415
预计提交时间 2026-09-09 18:00
```

期望快照：

```json
{
  "jobId": "test-job-001",
  "phase": "asp_executing",
  "responsibleParty": "asp",
  "milestones": {
    "createdAt": "2026-09-08 10:00",
    "acceptedAt": "2026-09-08 10:05"
  },
  "confidence": "confirmed"
}
```

期望用户语义：任务当前处于第 3/5 阶段，ASP 正在执行；用户目前无需操作；ASP 提交后进入用户验收，验收通过后结算。

## 17. 第一阶段验收标准

满足以下条件即可验收：

- 给定 `jobId`，能够从三类信箱恢复该任务的完整已知事件；
- 同一官方事件出现三次只产生一条逻辑事件；
- 重复消费不会重复推进状态；
- 乱序投递不会造成状态倒退；
- 非权威自然语言不能推进状态；
- 正常单次任务能够稳定输出创建、接单、执行、验收、完成五阶段时间轴；
- 输出明确标识当前阶段、责任方、下一步和同步时间；
- 冲突时不猜测，能够标记冲突并触发官方状态校准；
- 历史事件重放后快照结果一致；
- 快照包含第二阶段所需的四个核心里程碑时间；
- 聚合器全程不执行任务或资金相关写操作。

## 18. 第二阶段接口预留

第一阶段只保存里程碑，不生成正式绩效统计。第二阶段可基于快照计算：

```text
接单耗时 = acceptedAt - createdAt
任务处理耗时 = submittedAt - acceptedAt
用户验收耗时 = completedAt - submittedAt
任务总耗时 = completedAt - createdAt
```

为了区分平台通知延迟和 ASP 真实响应时间，协议后续应补充可信的 `aspNotifiedAt`：

```text
ASP 响应耗时 = acceptedAt - aspNotifiedAt
通知延迟 = aspNotifiedAt - createdAt
```

## 19. 推荐实施顺序

```text
第 1 步：冻结第一阶段 Event Schema 和状态映射
第 2 步：实现三信箱 Reader 与原始消息落库
第 3 步：实现 Normalizer、来源验证和跨信箱去重
第 4 步：实现 Reducer、Snapshot 和历史重放
第 5 步：实现 Timeline API 与 Renderer Adapter
第 6 步：补齐重复、乱序、冲突和恢复测试
第 7 步：用真实 A2A 单次任务做只读 E2E 验证
```

第一阶段的核心交付判断是：同一个 `jobId` 无论消息从哪个信箱、以什么顺序到达，系统都能得到同一个可解释、可重放的任务快照，并稳定地告诉用户“已经发生什么、现在由谁处理、接下来会发生什么”。
