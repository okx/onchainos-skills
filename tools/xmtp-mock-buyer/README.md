# xmtp-mock-buyer

裸 XMTP 客户端 —— 连 dev 网络，收发纯文本消息。对端默认是 openclaw（XMTP 插件侧）。
**不处理业务协议 header、不调 mock-api、不处理 TASK_* 系统事件**；只验证 XMTP 通路本身。

## 环境变量

| 变量 | 必填 | 说明 |
|---|---|---|
| `XMTP_WALLET_KEYS` | ✅ | 私钥（16 进制 0x 前缀）。逗号分隔时只取第一个。 |
| `XMTP_ENV` | | `dev` / `production` / `local`。默认 `dev`。 |
| `TO` | | 对端 ETH 地址或 inboxId；提供时启动后主动建 DM。 |
| `INIT` | | 启动后要发的首条消息内容。需要配合 `TO`。 |

DB 文件放 `~/.xmtp-mock-buyer/<inboxId>-<env>.db3`（XMTP SDK 要求）。

## 用法

### 安装

```bash
cd tools/xmtp-mock-buyer
npm install
```

> 首次会下 `@xmtp/agent-sdk` 的 libxmtp native binding（rust 原生库，macOS/Linux 都有 prebuilt，国内网络可能慢）。

### 只监听（等对方先来消息）

```bash
XMTP_WALLET_KEYS=0xabc123... npm start
```

启动后打印 `inboxId` 和 `address`，把 `address` 告诉 openclaw 那端，让它向这个地址发消息。

### 主动发起（给 openclaw 先推一条）

```bash
XMTP_WALLET_KEYS=0xabc123... \
TO=0xOpenclawAgentAddress... \
INIT="hi from mock buyer" \
npm start
```

### 终端交互

启动后终端里每敲一行回车就发给**当前活跃会话**（最近一次收到消息的会话 / 或 `TO` 建的那个）。

```
[mock-buyer] > hello again
[send] conv=abcd1234… → "hello again"
```

## 常见问题

- **SDK install 失败** — 多半是 native binding 下不动，试 `npm config set registry https://registry.npmmirror.com` 再装
- **`inboxId = unknown`** — `agent.client` 可能未 ready；看 logger，必要时延后一次 `await`
- **消息看不见** — 先看对面 inboxId 注册没（dev 网络第一次用要 sign 一次做 identity register）

## 不做的事

- ❌ 解析 onchainos task 协议 header（`jobId:` / `来自:` / `类型:` / `会话:`）
- ❌ 调 mock-api、onchainos 后端
- ❌ 代签（onchainos agent xmtp-sign）—— 走本地私钥
- ❌ 多身份、群聊路由 —— 当前只支持单 key 单 DM
