# notify-openclaw

Dev script — push arbitrary messages into a live openclaw agent session via
the gateway RPC (`sessions.send` / `chat.inject`).

Simulates what a "chain-event subscriber backend" would do in production:
watch on-chain events → push a structured notification into the agent's
session → agent reacts per skill logic.

## Usage

```bash
# 默认走 sessions.send —— 塞 user 消息 + 触发 AI 推理
node notify.js \
  --session-key 'agent:main:xmtp:group:okx-xmtp:my=0x...&to=0x...&job=101&gid=fef1...' \
  --message '[系统通知] provider_applied jobId=101 tokenAmount=100 tokenSymbol=USDT'

# 或走 chat.inject —— 塞 assistant 消息，不触发推理，只记录
node notify.js -k '<sessionKey>' -m '...' --method chat.inject --label system
```

## 参数

| 参数 | 必填 | 说明 |
|---|---|---|
| `--session-key` / `-k` | ✅ | 完整 sessionKey。从 openclaw UI 会话下拉复制；格式：`agent:main:xmtp:group:okx-xmtp:my=<addr>&to=<addr>&job=<id>&gid=<groupId>` |
| `--message` / `-m` | ✅ | 要灌进 session 的消息正文 |
| `--method` | | `sessions.send`（默认，触发 AI）或 `chat.inject`（仅记录，不触发） |
| `--label` | | 可选，仅 `chat.inject` 时生效，用于 TUI/Web 展示分类 |

## 两种 RPC 的区别

| RPC | 作用 | 何时用 |
|---|---|---|
| `sessions.send` | 塞一条 **user 消息** + **触发 AI 推理** | 模拟"系统通知送达、agent 需要响应"（典型场景） |
| `chat.inject` | 塞一条 **assistant 消息** + 持久化到 transcript，**不触发推理** | 只想给 transcript 加一条记录（如备注），不让 agent 反应 |

## 生产环境的对应物

在真实部署里，有个**后端服务监听链事件**，状态变化（`provider_applied` / `job_accepted` / ...）时调 gateway 的 `sessions.send` 往对应 agent session 塞通知。**本脚本替代它做手动测试**。

## Session key 怎么找

1. 打开 openclaw UI
2. 左上角 session 下拉里复制 agent 端对应的 key（`agent:main:xmtp:group:...`）

或者看 gateway log：
```
grep "sessionKey=\|session 创建完成" ~/.openclaw/logs/gateway.log
```

## 依赖

复用全局安装的 openclaw 包里的 `GatewayClient`（无需 npm install）。默认查：
- `/opt/homebrew/lib/node_modules/openclaw/dist/plugin-sdk/gateway-runtime.js`
- `/usr/local/lib/node_modules/openclaw/dist/plugin-sdk/gateway-runtime.js`
- 当前 `node_modules/openclaw/…`

如果都找不到，安装 openclaw CLI：`brew install openclaw` 或 npm install 到当前 repo。

## 排错

- **`gateway timeout`**：openclaw gateway 没在跑？`launchctl list | grep openclaw`
- **`gateway closed (4xxx)`**：权限不对？检查 `scopes` 是否匹配 gateway 配置
- **`session not found`**：sessionKey 不对、session 被删、或 gateway 重启过。先让 openclaw 重新建 session
