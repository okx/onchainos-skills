# xmtp-mock-seller

裸 XMTP 客户端，跟 `xmtp-mock-buyer` 完全对称——两个一起起来就能互发消息。
**不处理业务协议 header、不调 mock-api、不处理 TASK_\* 系统事件**；只验证 XMTP 通路本身。

## 环境变量

| 变量 | 必填 | 说明 |
|---|---|---|
| `XMTP_WALLET_KEYS` | ✅ | 私钥（16 进制 0x 前缀）。**要和 buyer 用不同的 key**，否则 XMTP 会当成同一个身份。 |
| `XMTP_ENV` | | `dev` / `production` / `local`。默认 `dev`。 |
| `TO` | | 对端 ETH 地址或 inboxId；提供时启动后主动建 DM。 |
| `INIT` | | 启动后要发的首条消息内容。需要配合 `TO`。 |

DB 文件放 `~/.xmtp-mock-seller/<inboxId>-<env>.db3`（与 buyer 隔离）。

## 两 mock 对接的跑法

**Terminal 1（buyer）**：
```bash
cd tools/xmtp-mock-buyer
XMTP_WALLET_KEYS=0xBUYER_KEY npm start
# 记下打印的 address，例如: 0xBuyerAddress…
```

**Terminal 2（seller）**：
```bash
cd tools/xmtp-mock-seller
XMTP_WALLET_KEYS=0xSELLER_KEY \
  TO=0xBuyerAddress从上一步复制 \
  INIT="hi buyer, this is seller" \
  npm start
```

**Terminal 1** 那端应当立刻看到：
```
[recv] conv=abcd1234… from=ef563210…
hi buyer, this is seller
```

然后 Terminal 1 敲一行回车就回到 Terminal 2。双向通。

## 注意事项

- `buyer` 和 `seller` **必须用不同私钥**；XMTP 按 inboxId 去重，同 key 会认为是同一个 app
- 首次连 dev 网络会 register 一次 identity（略慢 1-2s）
- 国内网络可能需要代理才能访问 XMTP dev 端点（`*.xmtp.network`）

## 不做的事（同 buyer）

- ❌ 解析 onchainos task 协议 header（`jobId:` / `来自:` / `类型:` / `会话:`）
- ❌ 调 mock-api、onchainos 后端
- ❌ 代签（onchainos agent xmtp-sign）—— 走本地私钥
- ❌ 多身份、群聊路由
