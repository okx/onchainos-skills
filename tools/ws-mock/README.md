# ws-mock

本地开发用的 WebSocket mock 服务器，模拟 XMTP 消息总线和 ERC-8004 身份系统。

## 架构

```
openclaw（AI Agent）
    ↕ ws-channel 插件
ws-mock server（本文件所在）   ← 模拟 XMTP
    ↕
mock-agent CLI（交互式测试工具）
```

生产环境中 ws-mock 会被替换为真实 XMTP 网络，ws-channel 接口不变。

---

## 编译

需要 Rust 工具链：

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source ~/.cargo/env

cargo build --release
# 产物：
#   target/release/server      — WebSocket 服务器
#   target/release/mock-agent  — 交互式测试工具
```

---

## 启动服务器

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

## 交互式测试（mock-agent）

mock-agent 是一个交互式 CLI，让你手动扮演买家/卖家/仲裁者发消息。

### 基本用法

```bash
# 不指定角色，启动后交互选择
./target/release/mock-agent

# 指定角色
./target/release/mock-agent --role seller
./target/release/mock-agent --role buyer
./target/release/mock-agent --role arbitrator

# 指定买家地址（对接真实 openclaw Agent 时必须）
./target/release/mock-agent --role seller \
  --buyer-addr 0x338267a9ca27a594e7f4c3977e894754ff0625b6
```

### 常用命令

```
/connect <task_id>          卖家发起会话并发询价
/accept  <task_id>          卖家接单
/deliver <task_id>          卖家提交交付
/dispute <task_id> [原因]   卖家发起仲裁
/confirm <task_id>          买家确认验收
/reject  <task_id> [原因]   买家拒绝验收
/resolve <task_id> seller   仲裁者裁定卖家胜
/resolve <task_id> buyer    仲裁者裁定买家胜
/convid  <task_id>          查看该任务的会话 ID
/register [role]            注册当前 Agent 的 ERC-8004 身份
/lookup  <role>             查询角色对应的 Agent 列表
send <convId> <内容>        发送自由文本到指定会话
help                        查看帮助
```

---

## 与 openclaw 对接调试

### 前置：获取 openclaw 买家地址

openclaw 连上 ws-mock 时会注册一个从 `device.json` 派生的确定性地址：

```bash
node -e "
const { createHash } = require('crypto');
const { readFileSync, existsSync } = require('fs');
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

**终端 3：卖家 mock-agent**
```bash
BUYER=<上面查到的地址>
./target/release/mock-agent --role seller --buyer-addr $BUYER

# 然后在 mock-agent 里执行：
/connect task-001    # 发起询价，openclaw AI 买家会自动回复
/accept  task-001    # 接单
/deliver task-001    # 提交交付
```

若需要仲裁，再开终端 4 运行仲裁者：
```bash
./target/release/mock-agent --role arbitrator --buyer-addr $BUYER
# 然后：
/resolve task-001 seller
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

# 2. 写 manifest 文件
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

# 5. 注册到 openclaw（从宿主机路径 link）
openclaw plugins install --link ~/openclaw-plugins/ws-channel
openclaw config set channels.ws-mock '{"serverUrl":"ws://127.0.0.1:9000","role":"buyer"}'
openclaw gateway restart
```

### 验证安装成功

```bash
openclaw health
# 应看到：WS Mock: configured（不再出现 EPERM 错误）
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
