# 附件上传 & 交付物下载 全面审查报告

> 审查日期：2026-06-18
> 分支：feat/agent-commerce-new-flow
> 审查范围：buyer 附件添加（创建时 / 中途补充）、provider 交付物发送、buyer 交付物接收下载、争议证据上传/下载

---

## 一、架构总览

### 存储布局

| 类型 | 路径 | 用途 |
|------|------|------|
| 附件（buyer 上传） | `~/.onchainos/task/<jobId>/attachments/` | 原文件副本，本地暂存 |
| 交付物（buyer/provider） | `~/.onchainos/deliverables/<role>/<jobId>/` | 持久存储，含 manifest.json |
| 争议证据 | 临时目录，上传后不保留本地 | 直接 POST multipart 到 API |

### 文件传输加密

所有跨 XMTP 的文件传输使用 `okx-a2a file upload/download`，由 daemon 完成 AES-256-GCM 加密。
返回 6 个元数据字段：`fileKey`, `digest`, `salt`, `nonce`, `secret`, `filename`。
接收方需全部 6 个字段才能解密。

---

## 二、上传路径（5 条路径）

| # | 路径 | 入口 | 加密 | 文件大小校验 | 代码位置 |
|---|------|------|------|------------|---------|
| 1 | `task-attach`（中途补充） | `attachments.rs:18` | 否（仅本地存储，上传由 LLM 后续执行） | 100MB | `attachments.rs:35` |
| 2 | `create-task --file` | `create.rs` → `copy_attachments_to_job` | 否（仅本地存储） | ❌ 无校验 | `attachments.rs:96` |
| 3 | `draft create --file` | `draft.rs` → `copy_attachments_to_job` | 否（仅本地存储） | ❌ 无校验 | `attachments.rs:96` |
| 4 | `okx-a2a file upload`（XMTP 发送前） | LLM 调用 daemon | AES-256-GCM | daemon 侧校验 | `okx_a2a.rs:416` |
| 5 | `dispute upload` | `dispute_upload.rs:44` | 否（直接 multipart 上传到 API） | 100MB | `dispute_upload.rs:37` |

### 下载路径（3 条路径）

| # | 路径 | 入口 | 解密 | 代码位置 |
|---|------|------|------|---------|
| 1 | buyer 接收文件交付物 | `core.rs:233` `deliverable_received_cli` | AES-256-GCM（via daemon） | `core.rs:283` |
| 2 | buyer 接收文本交付物 | `core.rs:320` | 无加密（明文 via XMTP） | `core.rs:327` |
| 3 | evaluator 下载争议证据 | `info.rs:17` `handle_info` | 无加密（API 直接返回） | `info.rs:107` |

---

## 三、发现的问题

### 🔴 P0 — 严重风险

#### 1. `create-task --file` 和 `draft create --file` 没有文件大小校验

**位置**: `attachments.rs:96-114` (`copy_attachments_to_job`)

`task-attach` 有 100MB 限制（`attachments.rs:35`），但 `copy_attachments_to_job` 被 `create-task` 和 `draft create` 调用时，只校验文件存在性，**没有大小校验**。用户可以通过创建时附带一个 > 100MB 的文件绕过限制。

后续 `okx-a2a file upload`（daemon 端）可能会失败或超时，但本地已经拷贝了大文件。

**修复建议**: 在 `copy_attachments_to_job` 中加入与 `handle_task_attach` 相同的 `MAX_FILE_SIZE` 校验。

---

#### 2. 附件文件名冲突会静默覆盖

**位置**: `attachments.rs:51` 和 `attachments.rs:107`

```rust
let dest = dir.join(file_name);
std::fs::copy(src, &dest)?;
```

如果用户先后添加两个同名文件（如 `photo.jpg`），第二次会覆盖第一次。没有时间戳前缀或冲突检测。

对比交付物存储 `deliverables.rs:136` 使用了 `{timestamp}_{original_name}` 前缀来避免冲突。

**修复建议**: 附件存储也加时间戳前缀，或检测冲突后自动重命名（如 `photo_2.jpg`）。

---

#### 3. LLM 需精确转发 6 个加密字段，任一截断即解密失败

**位置**: `manage.rs:432-438`（attachment_added 事件），`buyer-sub-playbook.md:55`（deliverable_received）

加密元数据字段（`secret`、`digest`、`salt`、`nonce`）可达 40-200+ 字符的 base64/hex 字符串。当前架构要求 LLM：
1. 从 `okx-a2a file upload` 输出中提取全部 6 个字段
2. 原封不动粘贴到 `xmtp-send` 消息中
3. 接收方 LLM 再从 XMTP 消息中提取并放入 `--message` JSON

代码中有两处真实事故记录（`manage.rs:412` 和 `manage.rs:439`）：
- Minimax 模型跳过 upload 直接发送本地路径
- 某模型用 `...` 截断了 `secret` 字段

**交付物接收** `deliverable_received_cli`（`core.rs:277-280`）任一字段为空即 fallback 到纯 LLM 处理路径，但 fallback 路径也需要 LLM 提取同样的 6 个字段。

**风险评估**: 这是架构性风险——把加密字段传递交给 LLM 处理天然不可靠。目前已通过详细的 playbook + 真实事故警告缓解，但根本解法是让 CLI 在 Rust 层完成 upload → xmtp-send 的原子操作。

---

### 🟡 P1 — 功能缺陷

#### 4. 无 provider 时附件不会自动延迟转发

**位置**: `buyer-actions.md:39`，`manage.rs:446-452`

文档说："If no sub session exists…tell the user the file is saved and will be forwarded once a provider is matched."

但实际代码中，当 provider 后来匹配上时（`job_created` → 协商流程），`designated.rs:124-125` 的 `attachments_handled_in_rust` 分支：
- `true` → Rust 已处理（仅特定条件下生效）
- `false` → 依赖 LLM 执行 `list-attachments` + `file upload` 转发

**当 `attachments_handled_in_rust=false` 时**（`designated.rs:127-137`），LLM 需要主动执行附件上传。如果 LLM 跳过或遗漏这一步，附件就永远不会发给 provider。

**风险**: 延迟转发完全依赖 LLM 遵循 playbook Step 1.5，没有 CLI 层面的保障。

---

#### 5. `attachment_added` 事件无 Rust 快速路径

**位置**: `manage.rs:398-461`

对比 `deliverable_received` 有 `deliverable_received_cli` Rust 快速路径（`core.rs:233`），`attachment_added` 完全由 LLM 执行：
1. 解析 `[ATTACHMENT_ADDED]` 消息提取文件路径
2. 调用 `okx-a2a file upload`
3. 调用 `okx-a2a xmtp-send` 转发 6 个字段

这是问题 #3 的根源之一——如果 `attachment_added` 也有 Rust 快速路径（upload + xmtp-send 一步完成），就不会有 LLM 截断字段的风险。

---

#### 6. 争议证据加密不一致

**位置**: `dispute_upload.rs`

争议证据上传使用 **明文 multipart POST**（`dispute_upload.rs:222-265`），不经过 `okx-a2a file upload` 的 AES 加密流程。

而 buyer/provider 的附件和交付物传输都经过 AES-256-GCM 加密。

这意味着：
- 争议证据在传输和服务器存储时是明文（依赖 HTTPS + 服务器端安全）
- evaluator 下载证据也是明文直接获取（`info.rs:88-98`）

**评估**: 设计上可能是有意为之（服务器需要读取证据用于争议判定），但与其他文件传输的加密标准不一致，应在文档中明确说明。

---

### 🟢 P2 — 文档/一致性问题

#### 7. Skill 文档未记录 100MB 文件大小限制

**位置**: `buyer-actions.md §2`，`buyer-actions-publish.md`，`cli-reference.md`

代码中 `task-attach` 和 `deliverables.rs` 都有 100MB 限制，但所有 skill 文档均未提及。如果用户尝试添加大文件会得到不明确的错误。

---

#### 8. 附件存储路径与交付物存储路径不一致

| 类型 | 路径模式 |
|------|---------|
| 附件 | `~/.onchainos/task/<jobId>/attachments/` |
| 交付物 | `~/.onchainos/deliverables/<role>/<jobId>/` |

附件以 jobId 为根，不区分 role（因为只有 buyer 上传附件）。交付物以 role 为根，区分 buyer/provider。两者目录结构不一致，且附件没有 manifest.json 元数据追踪。

这不是 bug，但增加了维护成本和理解难度。

---

#### 9. `deliver` 命令的 `--file` 参数语义模糊

**位置**: `cli-reference.md:625-639`

文档说 provider 发送文件交付物需要：
1. 先 `okx-a2a file upload --file-path <path>` 加密上传
2. 再 `onchainos agent deliver <jobId> --file <path>`

但代码 `deliver.rs:131-152` 中 `--file` 是本地文件路径（用于 auto-save），不是 fileKey。文档描述为"bind the file_key reference"与实际代码不一致。

实际流程是：
1. Provider LLM 调用 `okx-a2a file upload` → 获得 6 个字段
2. Provider LLM 调用 `okx-a2a xmtp-send` → 发送 `[intent:deliver]` + 6 个字段给 buyer
3. Provider LLM 调用 `onchainos agent deliver <jobId> --file <local path>` → 链上提交 + 本地 auto-save

---

#### 10. `buyer-sub-playbook.md` 仍列 `expireConfig` 为锁定参数

**位置**: `buyer-sub-playbook.md:72`

```
**Locked parameters are immutable** — refuse provider modifications to description / amount / symbol / paymentMode / expireConfig.
```

虽然 `expireConfig` 现在由服务器管理而不再由客户端传递，但在这里作为"不可修改"参数仍然语义正确。不需要修改，但值得注意。

---

## 四、端到端流程验证

### 流程 A：创建任务时带附件

```
用户 → create-task --file photo.jpg
  ↓ copy_attachments_to_job → ~/.onchainos/task/{jobId}/attachments/photo.jpg  ❌ 无大小校验
  ↓ job_created 系统事件
  ↓ designated.rs: 协商开始
  ↓ Step 1.5: LLM 调 list-attachments → 发现 photo.jpg
  ↓ LLM 调 okx-a2a file upload → 6 个字段
  ↓ LLM 调 okx-a2a xmtp-send → 转发给 provider  ⚠️ 依赖 LLM 不截断字段
  ✓ Provider 收到加密文件
```

**风险点**: 无大小校验(#1)、LLM 截断字段(#3)、LLM 可能跳过 Step 1.5(#4)

### 流程 B：中途补充附件

```
用户 → 补充附件 photo2.jpg
  ↓ buyer-actions.md §2: 确认 jobId
  ↓ task-attach → 校验 status < 2 → 100MB 校验 → copy 到 attachments/  ✓
  ↓ 同名文件覆盖风险  ❌ (#2)
  ↓ okx-a2a session send "[ATTACHMENT_ADDED] <path>"
  ↓ Sub session 收到 → attachment_added 事件
  ↓ manage.rs: LLM 提取路径 → okx-a2a file upload → 6 字段 → xmtp-send
  ✓ Provider 收到
```

**风险点**: 同名覆盖(#2)、无 Rust 快速路径(#5)、LLM 截断字段(#3)

### 流程 C：Provider 交付文件

```
Provider → deliver --file result.pdf
  ↓ deliver.rs: 校验 status=1 (accepted)  ✓
  ↓ LLM 先 okx-a2a file upload → 6 字段 → xmtp-send [intent:deliver]
  ↓ CLI: POST submit + sign_uop_and_broadcast → 链上提交  ✓
  ↓ auto-save to ~/.onchainos/deliverables/provider/{jobId}/  ✓
  ↓ Buyer sub session 收到 [intent:deliver]
  ↓ deliverable_received_cli: 提取 6 字段 → file_download → handle_save  ✓ Rust 快速路径
  ↓ 保存到 ~/.onchainos/deliverables/buyer/{jobId}/  ✓
  ↓ LLM 发 user notify  ✓
```

**风险点**: provider 侧 LLM 也需要正确传递 6 个字段(#3)

### 流程 D：Buyer 接收文件交付物（Rust 快速路径）

```
Sub session 收到 [intent:deliver]
  ↓ buyer-sub-playbook §3.5 #2: LLM 提取 6 字段 + deliverableType
  ↓ next-action --message '{"event":"deliverable_received",...6 fields}'
  ↓ core.rs:233 deliverable_received_cli
  ↓ file_download(fileKey, agentId, digest, salt, nonce, secret, filename)  ✓ Rust 内完成
  ↓ handle_save → deliverables/buyer/{jobId}/  ✓ 带时间戳前缀，无冲突
  ↓ 返回 notify-only prompt → LLM 发 user notify  ✓
```

**风险点**: LLM 提取 6 字段放入 --message JSON 仍可能截断(#3)，但严重性低于附件路径——这里只需要放进 JSON，不需要拼接命令行字符串

---

## 五、修复优先级建议

| 优先级 | 问题 | 修复方式 | 工作量 |
|--------|------|---------|--------|
| P0 | #1 create/draft 无文件大小校验 | `copy_attachments_to_job` 加 MAX_FILE_SIZE 校验 | 小 |
| P0 | #2 同名附件静默覆盖 | 目标路径加时间戳前缀或冲突检测 | 小 |
| P1 | #5 attachment_added 无 Rust 快速路径 | 新增 `attachment_added_cli`，在 Rust 中完成 upload + xmtp-send | 中 |
| P1 | #4 无 provider 时延迟转发无保障 | 在 designated 协商启动时，Rust 层自动检查并转发待发附件 | 中 |
| P2 | #7 文档未记录 100MB 限制 | skill docs 补充 | 小 |
| P2 | #9 deliver --file 文档不准确 | 更新 cli-reference.md | 小 |
| 追踪 | #3 LLM 传递加密字段的架构风险 | 长期：attachment_added Rust 快速路径(#5) 可消除上传侧风险；下载侧已有快速路径 | — |
| 追踪 | #6 争议证据明文传输 | 确认设计意图，文档中明确说明 | 小 |
