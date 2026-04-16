# ws-mock

本地开发用的 WebSocket mock 服务器，模拟 XMTP 消息总线和 ERC-8004 身份系统，以及任务后端 HTTP API。

## 架构

```
openclaw（AI Agent）
    ↕ ws-channel 插件
ws-mock server（ws://127.0.0.1:9000）  ← 模拟 XMTP
    ↕
mock-buyer / mock-seller / mock-arbitrator  ← 交互式测试工具

mock-api（http://127.0.0.1:9001）      ← 模拟任务后端 REST API
    ↕ GET /api/tasks/:task_id
openclaw buyer AI 自动查询任务上下文
```

生产环境中 ws-mock 会被替换为真实 XMTP 网络，mock-api 会被替换为真实后端，ws-channel 接口不变。

---

## 编译

需要 Rust 工具链：

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source ~/.cargo/env

cargo build --release
# 产物：
#   target/release/server           — WebSocket 服务器（XMTP 模拟）
#   target/release/mock-api         — 任务后端 HTTP API（REST）
#   target/release/mock-buyer       — 买家测试工具
#   target/release/mock-seller      — 卖家测试工具
#   target/release/mock-arbitrator  — 仲裁者测试工具
```

---

## 启动 WS 服务器

```bash
./target/release/server
# [server] listening on ws://127.0.0.1:9000
```

服务器支持的操作（JSON 消息发往 ws://127.0.0.1:9000）：

| action | 说明 |
|---|---|
| `Register` | 注册地址，后续消息路由到该连接 |
| `JoinConversation` | 创建/加入会话，指定参与者列表 |
| `Send` | 发消息到会话（服务器转发给其他参与者） |
| `RegisterIdentity` | 注册 ERC-8004 身份（role + addr） |
| `LookupRole` | 查询某角色的所有已注册 Agent |
| `LookupAddr` | 查询某地址注册的角色 |

---

## 启动任务后端 mock-api

```bash
./target/release/mock-api
# [mock-api] HTTP server listening on http://127.0.0.1:9001
# [mock-api] 已预置示例任务: task-001, task-002
```

| 方法 | 路径 | 说明 |
|---|---|---|
| `GET` | `/api/tasks` | 列出所有任务 |
| `POST` | `/api/tasks` | 创建任务 |
| `GET` | `/api/tasks/:task_id` | 查询任务详情 |
| `PATCH` | `/api/tasks/:task_id` | 更新任务（status / title / description 等） |

任务状态流转：`open` → `accepted` → `delivered` → `confirmed` / `rejected` / `disputed` → `resolved`

**示例：**

```bash
# 查询任务详情
curl http://127.0.0.1:9001/api/tasks/task-001

# 创建任务
curl -X POST http://127.0.0.1:9001/api/tasks \
  -H "Content-Type: application/json" \
  -d '{"task_id":"task-001","title":"合约审计","description":"审计 0xABC...","budget":"500 USDT","deadline":"2026-04-30","buyer_addr":"0x338267..."}'

# 更新状态
curl -X PATCH http://127.0.0.1:9001/api/tasks/task-001 \
  -H "Content-Type: application/json" \
  -d '{"status":"accepted"}'
```

openclaw buyer AI 收到含 `task_id` 的 WS 消息时，可调用此接口查询任务上下文，再决定如何回应。

---

## 交互式测试工具

三个工具各对应一个角色，菜单驱动（↑↓ 选命令，Enter 确认），task_id 用过后自动记住可直接点选。

### mock-seller（卖家）

```bash
./target/release/mock-seller [--buyer-addr <买家钱包地址>]
```

| 菜单项 | 说明 |
|---|---|
| `/connect` | 向买家发起询价（创建会话 + 发 TASK_INQUIRE） |
| `/accept` | 接单（发 TASK_ACCEPT） |
| `/deliver` | 提交交付（发 TASK_DELIVER） |
| `/dispute` | 发起仲裁（创建三方会话 + 发 TASK_DISPUTE） |
| `/convid` | 查看该任务的会话 ID |
| `/register` | 注册 ERC-8004 身份（PROVIDER） |
| `/lookup` | 查询角色对应的 Agent 列表 |
| `send` | 发送自由文本到指定会话 |

### mock-buyer（买家，仅全 mock 测试时使用）

```bash
./target/release/mock-buyer
```

| 菜单项 | 说明 |
|---|---|
| `/confirm` | 确认验收（发 TASK_CONFIRM） |
| `/reject` | 拒绝验收（发 TASK_REJECT） |
| `/convid` | 查看该任务的会话 ID |
| `/register` | 注册 ERC-8004 身份（REQUESTER） |
| `/lookup` | 查询角色对应的 Agent 列表 |
| `send` | 发送自由文本到指定会话 |

> 生产测试时买家由 openclaw 扮演，无需启动 mock-buyer。

### mock-arbitrator（仲裁者）

```bash
./target/release/mock-arbitrator [--buyer-addr <买家钱包地址>]
```

| 菜单项 | 说明 |
|---|---|
| `/resolve buyer` | 裁定买家胜（发 TASK_RESOLVE winner=buyer） |
| `/resolve seller` | 裁定卖家胜（发 TASK_RESOLVE winner=seller） |
| `/convid` | 查看该任务的会话 ID |
| `/register` | 注册 ERC-8004 身份（EVALUATOR） |
| `/lookup` | 查询角色对应的 Agent 列表 |
| `send` | 发送自由文本到指定会话 |

---

## 会话 ID 规则

会话 ID 由参与地址排序后确定性生成，买家地址必须一致才能路由：

```
买卖双方：conv-{task_id}-{小地址}-{大地址}
三方仲裁：conv-arb-{task_id}-{addr1}-{addr2}-{addr3}（三地址排序）
```

mock-seller 和 mock-arbitrator 需要知道买家地址才能计算正确的 conv_id，通过 `--buyer-addr` 传入。

---

## 安装 okx-agent-task Skill

`skills/okx-agent-task/` 是本地开发版 skill，需要手动安装到 Claude Code / OpenClaw。

### 安装（全局，同时装到 Claude Code 和 OpenClaw）

```bash
# 在任意目录执行，-g 全局安装，-s 指定 skill 名
npx skills add /path/to/OKOnchainOS -g -s okx-agent-task --yes
```

例如：

```bash
npx skills add /Users/gan/meili/mingtao.gan_dacs_at_okg.com/121/Documents/RustProjects/OKOnchainOS \
  -g -s okx-agent-task --yes
```

安装完成后会看到：

```
✓ ~/.agents/skills/okx-agent-task
  universal: Codex, Amp, Antigravity, Cline, Cursor +7 more
  symlinked: Claude Code, OpenClaw
```

Skill 文件存在 `~/.agents/skills/okx-agent-task`，symlink 到 Claude Code 和 OpenClaw，**无需重启立即生效**。

### 更新

本地改了 `skills/okx-agent-task/SKILL.md` 后，因为是 symlink，无需重新安装，改动直接生效。

### 卸载

```bash
npx skills remove okx-agent-task -g
```

### 安装所有 skills（一次性）

```bash
REPO=/Users/gan/meili/mingtao.gan_dacs_at_okg.com/121/Documents/RustProjects/OKOnchainOS
npx skills add $REPO -g -s '*' --yes
```

---

## 测试 openclaw Skill 触发

### 前置：确认 gateway 已启动

```bash
openclaw gateway status
# 看到 bind=loopback, port=18789 说明正常
```

---

### 方式一：浏览器 UI（最简单）

打开宿主机浏览器：
```
http://127.0.0.1:18789/chat?session=agent%3Amain%3Amain
```

直接发消息，如"我想发布一个任务"，观察 skill 是否触发、preflight 是否执行。

> 清除上下文重新测试：点 **New Chat** 开新会话（或 `Cmd+N`）。

---

### 方式二：CLI 发消息（dacs 内）

**首次使用需要先批准设备配对**，否则会报 `pairing required`：

```bash
# 1. 查看待审批的设备
openclaw devices list

# 2. 批准 pending 里的 ACP 请求（复制 Request 列的 UUID）
openclaw devices approve <request-uuid>

# 3. 发消息
openclaw agent --agent main -m "我想发布一个任务，帮我审计一个智能合约，预算500 USDT"
```

> 只需批准一次，后续直接用 `openclaw agent --agent main -m "..."` 即可。

---

### 查看实时日志

```bash
tail -f /tmp/openclaw/openclaw-$(date +%Y-%m-%d).log
```

用于确认 agent 执行了哪些 tool call / shell 命令。

---

### 测试 onchainos CLI 未安装的场景

验证 preflight 引导安装流程是否正确：

```bash
# 删除（dacs 内用 node）
node -e "require('fs').unlinkSync('/Users/gan/.local/bin/onchainos'); console.log('deleted');"

# 发消息触发 preflight
openclaw agent --agent main -m "我想发布一个任务"

# 测试完恢复
node -e "
const fs = require('fs');
fs.copyFileSync('cli/target/release/onchainos', '/Users/gan/.local/bin/onchainos');
fs.chmodSync('/Users/gan/.local/bin/onchainos', 0o755);
console.log('restored');
"
```

---

## 与 openclaw 对接调试

### 前置：获取 openclaw 买家地址

openclaw 连上 ws-mock 时会注册一个从 `device.json` 派生的确定性地址：

```bash
node -e "
const { existsSync, readFileSync } = require('fs');
const { join } = require('path');
const { homedir } = require('os');
const mapPath = join(homedir(), '.openclaw/ws-mock-addresses.json');
if (existsSync(mapPath)) {
  const map = JSON.parse(readFileSync(mapPath, 'utf8'));
  console.log('buyer addr:', Object.values(map)[0]);
}
"
```

### 完整调试流程（3 个终端）

**终端 1：启动 ws-mock server**
```bash
./target/release/server
```

**终端 2：启动 openclaw gateway**（ws-channel 插件会自动连上 ws-mock）
```bash
openclaw gateway --force
# 看到：[ws-channel] connected as 0x... 说明买家已上线
```

**终端 3：卖家 mock-seller**
```bash
BUYER=<上面查到的地址>
./target/release/mock-seller --buyer-addr $BUYER

# 菜单操作：
# 选 /connect → 输入 task_id（如 task-001）→ openclaw AI 买家自动回复
# 选 /accept  → 选 task-001
# 选 /deliver → 选 task-001
```

若需要仲裁，再开终端 4：
```bash
./target/release/mock-arbitrator --buyer-addr $BUYER
# 选 /resolve buyer 或 /resolve seller → 选 task-001
```

### 全 mock 测试（不需要 openclaw）

```bash
# 终端 1
./target/release/server

# 终端 2（买家）
./target/release/mock-buyer

# 终端 3（卖家，使用 mock-buyer 的固定地址）
./target/release/mock-seller
# 默认 buyer-addr: 0xMockBuyer00000000000000000000000000001

# 终端 4（仲裁者，可选）
./target/release/mock-arbitrator
```

---

## 跨域调试（开发环境在 dacs 内，openclaw 在宿主机）

### 问题

如果你的开发环境在容器/VM（如 dacs）内，而 openclaw 运行在宿主机，会遇到：

```
plugins: plugin: failed to read extensions dir: /path/in/dacs/plugins/ws-channel
(Error: EPERM: operation not permitted, scandir ...)
```

原因：openclaw 启动时会去 scan 插件目录，宿主机读不到 dacs 内的路径。

### 解决方法：用 node 把插件写到宿主机路径

`cp` / Write 工具在跨域写入时会 EPERM，但 `node` 可以绕过：

```bash
# 1. 在宿主机上建目录
node -e "require('fs').mkdirSync(require('os').homedir()+'/openclaw-plugins/ws-channel/src', {recursive:true})"

# 2. 写 manifest 和配置文件
node -e "
const fs = require('fs'), dst = require('os').homedir()+'/openclaw-plugins/ws-channel';
fs.writeFileSync(dst+'/openclaw.plugin.json', JSON.stringify({id:'ws-mock',channels:['ws-mock'],configSchema:{type:'object',additionalProperties:false,properties:{walletAddr:{type:'string'},serverUrl:{type:'string'}}}}, null, 2));
fs.writeFileSync(dst+'/package.json', JSON.stringify({name:'@okx/ws-channel',version:'0.1.0',openclaw:{extensions:['./src/index.ts'],channel:{id:'ws-mock',label:'WS Mock',blurb:'WebSocket mock channel, simulates XMTP for local development'}},dependencies:{openclaw:'^2026.4.9',ws:'^8.18.0'},devDependencies:{'@types/ws':'^8.5.13',typescript:'^5.0.0'}}, null, 2));
fs.writeFileSync(dst+'/tsconfig.json', JSON.stringify({compilerOptions:{target:'ES2022',module:'ESNext',moduleResolution:'bundler',strict:true,esModuleInterop:true,skipLibCheck:true,outDir:'./dist'},include:['src/**/*']}, null, 2));
console.log('manifest written');
"

# 3. 复制 TypeScript 源文件
DACS="/path/to/dacs/OKOnchainOS/plugins/ws-channel/src"
DST="$HOME/openclaw-plugins/ws-channel/src"
node -e "
const fs = require('fs');
['index.ts','ws-client.ts','handler.ts','runtime.ts'].forEach(f =>
  fs.copyFileSync('$DACS/'+f, '$DST/'+f)
);
console.log('src copied');
"

# 4. 安装依赖
cd ~/openclaw-plugins/ws-channel && npm install

# 5. 注册到 openclaw（安装前先移走 node_modules，避免触发安全扫描限制）
mv node_modules ../ws-channel-nm-backup
openclaw plugins install --link ~/openclaw-plugins/ws-channel
mv ../ws-channel-nm-backup node_modules

# 6. 配置 channel
openclaw config set channels.ws-mock '{"serverUrl":"ws://127.0.0.1:9000","role":"buyer"}'
openclaw gateway restart
```

### 源码更新后同步

在 dacs 内改了 ws-channel 源码后，同步到宿主机并重启：

```bash
DACS="/path/to/dacs/OKOnchainOS/plugins/ws-channel/src"
DST="$HOME/openclaw-plugins/ws-channel/src"
node -e "
const fs = require('fs');
['index.ts','ws-client.ts','handler.ts','runtime.ts'].forEach(f =>
  fs.copyFileSync('$DACS/'+f, '$DST/'+f)
);
console.log('synced');
" && openclaw gateway restart
```

---

## openclaw.json 配置参考

```json
{
  "channels": {
    "ws-mock": {
      "serverUrl": "ws://127.0.0.1:9000",
      "role": "buyer"
    }
  }
}
```

| 字段 | 默认值 | 说明 |
|---|---|---|
| `serverUrl` | `ws://127.0.0.1:9000` | ws-mock server 地址 |
| `role` | `""` → generic | `buyer` / `seller` / `arbitrator` / `generic` |
| `walletAddr` | 从 device.json 自动派生 | 不填则自动生成，重启后地址不变 |

`role` 决定 Agent 的 system prompt，不同角色有不同的行为规则（见 `plugins/ws-channel/src/index.ts` 中的 `ROLE_PRESETS`）。
