# Deliverable Out-of-Order Fix: Remove Marker, Use Status + Temp File

## Background

Buyer 收到交付物涉及两个事件：
- `[intent:deliver]` — P2P XMTP 消息，含加密密钥/文本内容（**不可恢复，不会重发**）
- `job_submitted` — 链上系统事件（**可重放，watch 会重新投递**）

两个事件到达顺序不确定，且不同平台对并发消息的处理不同：
- **排队平台**（Claude Code）：第二条消息等第一条处理完
- **打断平台**（OpenClaw）：第二条消息可打断第一条处理

## Problem

当前用 `review_marker` 文件协调两个事件的先后顺序。存在致命场景：

**B1：打断平台上 deliver 先到，job_submitted 打断 skill loading**
- deliver 还没调 CLI，数据未持久化
- job_submitted 写 marker → "等待交付物"
- deliver 处理被丢弃 → 数据丢失 → **死锁**

## Solution

用链上 status + temp file 替代 marker，消除死锁，减少 I/O。

### Design Principles

1. **不可恢复数据尽早持久化** — [intent:deliver] 的原始 JSON 在 LLM 处理前落盘
2. **每个事件独立判断完整状态** — 不依赖对方事件是否已处理
3. **单一事实源** — 链上 status（API prefetch）替代本地 marker 文件

### Changes

#### 1. Remove marker mechanism (deliverables.rs)

Delete:
- `review_marker_path()`
- `write_review_marker()`
- `has_review_marker()`
- `delete_review_marker()`

#### 2. deliverable_received_cli (core.rs) — status-based review entry

After save, check `ctx.prefetched.status`:
- `status == Some(2)` (Submitted) → patch prefetched with saved deliverable → call `job_submitted_escrow()` directly
- Otherwise → notify + wait for job_submitted (same as before, minus marker check)

Remove: marker check at line 613-661 and marker check at line 267-282 in `deliverable_received()`.

#### 3. job_submitted check-freshness (mod.rs) — temp file recovery

Replace marker logic (lines 1705-1716) with temp file check:

```
if job_submitted:
  ① read_manifest → found → fill ctx.deliverable (unchanged)
  ② /tmp/a2a_deliver_<jobId>.json exists → parse + download + save → fill ctx.deliverable (NEW)
  ③ neither → output "wait" with context-persist hint (simplified from marker)
```

#### 4. job_submitted_escrow (core.rs) — simplify no-deliverable path

Remove marker read/write. When `p.deliverable.is_none()`:
- read_manifest fallback (unchanged)
- temp file fallback (NEW, same logic as check-freshness but as safety net)
- "wait" output (simplified)

#### 5. Prompt rule — context-based temp file persist

In buyer-sub-playbook.md and CLAUDE.md (for Claude Code), add:
> When processing `job_submitted`, if `[intent:deliver]` raw JSON exists in conversation context, Write it to `/tmp/a2a_deliver_<jobId>.json` before calling next-action.

This ensures on interrupt platforms, the deliver data is persisted from context before job_submitted's CLI runs.

### Coverage Matrix

| Scenario | Platform | First | Recovery | Result |
|----------|----------|-------|----------|--------|
| Q1 | Queue | deliver | manifest | OK |
| Q2 | Queue | submitted | **status=2** at deliver time | OK |
| I1 | Interrupt | deliver (interrupted) | **temp file** → CLI inline recovery | OK |
| I2 | Interrupt | deliver (CLI done) | manifest | OK |
| I3 | Interrupt | submitted (interrupted) | deliver priority + submitted re-delivery | OK |
| I4 | Interrupt | submitted (CLI done) | **status=2** at deliver time | OK |

### I/O Comparison

| Path | Before (marker) | After |
|------|----------------|-------|
| submitted normal (manifest hit) | read_manifest + stat marker + delete marker = 3 | read_manifest = 1 |
| submitted wait (no manifest) | read_manifest + stat marker + write marker = 3 | read_manifest + stat temp = 2 |
| submitted recovery (temp file) | N/A (deadlock) | read_manifest + read temp + download + save = 4 |
| deliver normal | save + stat marker = 2 | save = 1 |
| deliver + review merge | save + stat marker + delete marker = 3 | save + status check (in memory) = 1 |
