# onchainos MCP Server：Wrap 模式 vs 重写模式

## 概述

将 onchainos 适配为 MCP Server 有两条路：

| | Wrap 模式 | 重写模式 |
|--|-----------|---------|
| **做什么** | MCP Server 调用现有 `onchainos` binary | MCP Server 直接调用 OKX Web3 API |
| **实现量** | 小（只做适配层） | 大（重新实现所有 API 调用） |
| **依赖** | 需要本地安装 `onchainos` CLI | 只需 Node.js 运行时 |
| **进程开销** | 每次 Tool Call 启动一个子进程 | 无额外进程，直接发 HTTP |
| **扩展授权** | 授权逻辑只能加在 MCP 层外面 | 授权逻辑可以嵌入 API 调用流程 |
| **分发** | 两个产物（MCP Server + CLI binary） | 一个产物（MCP Server） |
| **适合场景** | 快速验证、个人使用 | 生产部署、多用户、需要授权 |

---

## 架构对比

**Wrap 模式：**
```
Claude
  └─ MCP Tool Call
       └─ onchainos MCP Server (Node.js 常驻)
            └─ spawn onchainos binary  ← 每次调用 fork 一个进程
                 └─ HTTP → OKX Web3 API
```

**重写模式：**
```
Claude
  └─ MCP Tool Call
       └─ onchainos MCP Server (Node.js 常驻)
            └─ HTTP → OKX Web3 API  ← 直接调用，无额外进程
```

---

## Demo：Wrap 模式

用 3 个文件实现一个最小可运行的 MCP Server，包含 2 个工具（`market_price` + `token_search`）。

### 文件结构

```
demo-wrap/
├── package.json
├── tsconfig.json
└── src/
    └── index.ts
```

### package.json

```json
{
  "name": "onchainos-mcp-wrap-demo",
  "version": "0.1.0",
  "type": "module",
  "scripts": { "build": "tsc", "start": "node dist/index.js" },
  "dependencies": {
    "@modelcontextprotocol/sdk": "^1.0.4",
    "execa": "^9.3.0"
  },
  "devDependencies": {
    "typescript": "^5.4.0",
    "@types/node": "^20.0.0"
  }
}
```

### tsconfig.json

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "Node16",
    "moduleResolution": "Node16",
    "outDir": "./dist",
    "rootDir": "./src",
    "strict": true
  }
}
```

### src/index.ts

```typescript
#!/usr/bin/env node
import { Server } from '@modelcontextprotocol/sdk/server/index.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { CallToolRequestSchema, ListToolsRequestSchema } from '@modelcontextprotocol/sdk/types.js';
import { execa } from 'execa';

// ── 工具定义 ──────────────────────────────────────────────

const TOOLS = [
  {
    name: 'market_price',
    description: '获取代币当前价格',
    inputSchema: {
      type: 'object',
      required: ['address', 'chain'],
      properties: {
        address: { type: 'string', description: '代币合约地址' },
        chain: { type: 'string', description: '链名，如 ethereum' },
      },
    },
  },
  {
    name: 'token_search',
    description: '搜索代币',
    inputSchema: {
      type: 'object',
      required: ['query'],
      properties: {
        query: { type: 'string', description: '代币名称/符号/地址' },
        chains: { type: 'string', description: '逗号分隔链名（可选）' },
      },
    },
  },
];

// ── CLI 执行器 ────────────────────────────────────────────

const BIN = process.env.ONCHAINOS_BIN ?? `${process.env.HOME}/.local/bin/onchainos`;

async function runCli(args: string[]): Promise<unknown> {
  const result = await execa(BIN, ['--output', 'json', ...args], {
    env: { ...process.env },
    timeout: 30_000,
    reject: false,
  });
  const raw = (result.stdout || result.stderr).trim();
  return raw ? JSON.parse(raw) : { ok: false, error: 'empty output' };
}

// ── MCP Server ────────────────────────────────────────────

const server = new Server(
  { name: 'onchainos-wrap-demo', version: '0.1.0' },
  { capabilities: { tools: {} } }
);

server.setRequestHandler(ListToolsRequestSchema, async () => ({ tools: TOOLS }));

server.setRequestHandler(CallToolRequestSchema, async (req) => {
  const { name, arguments: args = {} } = req.params;
  const a = args as Record<string, string>;
  let result: unknown;

  if (name === 'market_price') {
    result = await runCli(['market', 'price', a.address, '--chain', a.chain]);
  } else if (name === 'token_search') {
    const extra = a.chains ? ['--chains', a.chains] : [];
    result = await runCli(['token', 'search', a.query, ...extra]);
  } else {
    result = { ok: false, error: `unknown tool: ${name}` };
  }

  return { content: [{ type: 'text', text: JSON.stringify(result, null, 2) }] };
});

await server.connect(new StdioServerTransport());
```

### 运行验证

```bash
cd demo-wrap && npm install && npm run build

# 用 MCP Inspector 测试
npx @modelcontextprotocol/inspector node dist/index.js
# 打开 http://localhost:5173 → List Tools → 调用 market_price
```

---

## Demo：重写模式

同样实现 `market_price` + `token_search`，但直接调用 OKX Web3 API，不依赖 `onchainos` binary。

### src/index.ts

```typescript
#!/usr/bin/env node
import { Server } from '@modelcontextprotocol/sdk/server/index.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { CallToolRequestSchema, ListToolsRequestSchema } from '@modelcontextprotocol/sdk/types.js';
import { createHmac } from 'node:crypto';

// ── OKX API 客户端 ─────────────────────────────────────────

const BASE_URL = process.env.OKX_BASE_URL ?? 'https://web3.okx.com';
const API_KEY  = process.env.OKX_API_KEY     ?? '03f0b376-251c-4618-862e-ae92929e0416';
const SECRET   = process.env.OKX_SECRET_KEY  ?? '652ECE8FF13210065B0851FFDA9191F7';
const PASS     = process.env.OKX_PASSPHRASE  ?? 'onchainOS#666';

// OKX chainIndex 映射（只列出常用链）
const CHAIN_INDEX: Record<string, string> = {
  ethereum: '1', eth: '1',
  solana: '501',  sol: '501',
  bsc: '56',      bnb: '56',
  base: '8453',
  arbitrum: '42161', arb: '42161',
  polygon: '137', matic: '137',
  xlayer: '196',  okb: '196',
};

function resolveChain(name: string): string {
  return CHAIN_INDEX[name.toLowerCase()] ?? name;
}

function sign(timestamp: string, method: string, path: string, body = ''): string {
  const prehash = timestamp + method + path + body;
  return createHmac('sha256', SECRET).update(prehash).digest('base64');
}

async function apiGet(path: string, query: Record<string, string>): Promise<unknown> {
  const qs = new URLSearchParams(
    Object.entries(query).filter(([, v]) => v !== '')
  ).toString();
  const fullPath = qs ? `${path}?${qs}` : path;
  const timestamp = new Date().toISOString().replace(/(\.\d{3})Z$/, '$1+00:00');

  const resp = await fetch(`${BASE_URL}${fullPath}`, {
    headers: {
      'OK-ACCESS-KEY':        API_KEY,
      'OK-ACCESS-SIGN':       sign(timestamp, 'GET', fullPath),
      'OK-ACCESS-PASSPHRASE': PASS,
      'OK-ACCESS-TIMESTAMP':  timestamp,
      'Content-Type':         'application/json',
    },
  });

  const json = await resp.json() as { code: string; msg: string; data: unknown };
  if (json.code !== '0') throw new Error(`OKX API error ${json.code}: ${json.msg}`);
  return json.data;
}

// ── 工具定义 ──────────────────────────────────────────────

const TOOLS = [
  {
    name: 'market_price',
    description: '获取代币当前价格',
    inputSchema: {
      type: 'object',
      required: ['address', 'chain'],
      properties: {
        address: { type: 'string', description: '代币合约地址' },
        chain: { type: 'string', description: '链名，如 ethereum' },
      },
    },
  },
  {
    name: 'token_search',
    description: '搜索代币',
    inputSchema: {
      type: 'object',
      required: ['query'],
      properties: {
        query: { type: 'string' },
        chains: { type: 'string', description: '逗号分隔链名（可选）' },
      },
    },
  },
];

// ── MCP Server ────────────────────────────────────────────

const server = new Server(
  { name: 'onchainos-rewrite-demo', version: '0.1.0' },
  { capabilities: { tools: {} } }
);

server.setRequestHandler(ListToolsRequestSchema, async () => ({ tools: TOOLS }));

server.setRequestHandler(CallToolRequestSchema, async (req) => {
  const { name, arguments: args = {} } = req.params;
  const a = args as Record<string, string>;
  let data: unknown;

  try {
    if (name === 'market_price') {
      data = await apiGet('/api/v6/dex/market/price', {
        chainIndex: resolveChain(a.chain),
        tokenContractAddress: a.address,
      });
    } else if (name === 'token_search') {
      const chainIndexes = a.chains
        ? a.chains.split(',').map(resolveChain).join(',')
        : '';
      data = await apiGet('/api/v6/dex/market/token/search', {
        chains: chainIndexes,
        search: a.query,
      });
    } else {
      throw new Error(`unknown tool: ${name}`);
    }
    return { content: [{ type: 'text', text: JSON.stringify({ ok: true, data }, null, 2) }] };
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    return { content: [{ type: 'text', text: JSON.stringify({ ok: false, error: msg }) }] };
  }
});

await server.connect(new StdioServerTransport());
```

### 运行验证

```bash
cd demo-rewrite && npm install && npm run build

# 无需安装 onchainos binary
npx @modelcontextprotocol/inspector node dist/index.js
```

---

## 优缺点汇总

### Wrap 模式

**优点**
- 实现量极小：只写适配层，所有 API 逻辑复用 Rust CLI
- Rust CLI 已经过测试，稳定可靠
- 新增 API 时只需升级 CLI，MCP Server 代码不动

**缺点**
- 需要本地安装 `onchainos` binary，分发时带两个产物
- 每次 Tool Call 启动子进程，多出 20~50ms 延迟
- 授权逻辑只能加在 MCP 层（无法在 API 调用中间插入权限检查）
- 高并发场景下进程数线性增长，内存压力大

**适合**
本地开发、个人使用、快速验证 MCP 架构可行性

---

### 重写模式

**优点**
- 单一产物，用户只需 Node.js，无需安装 CLI
- 无进程启动开销，直接 HTTP，延迟低
- 可以在 API 调用前后注入任意逻辑（授权检查、签名请求、审计日志）
- 适合服务化部署（多用户、HTTP transport、会话管理）

**缺点**
- 需要重新实现全部 34 个 API 调用（HMAC 签名、chain mapping、参数序列化）
- 两套代码需同步维护（Rust CLI + Node.js MCP Server）
- OKX API 变更时需同时更新两处

**适合**
生产部署、需要授权流程、需要签名集成、SaaS 场景

---

## 打包与发布的影响

当前 CI（`release.yml`）只打包 `cli/` 目录的 Rust binary，产物为 9 个平台：

```
onchainos-aarch64-apple-darwin         macOS Apple Silicon
onchainos-x86_64-apple-darwin          macOS Intel
onchainos-x86_64-unknown-linux-gnu     Linux x64
onchainos-i686-unknown-linux-gnu       Linux x86
onchainos-aarch64-unknown-linux-gnu    Linux ARM64
onchainos-armv7-unknown-linux-gnueabihf  Linux ARMv7
onchainos-x86_64-pc-windows-msvc       Windows x64
onchainos-i686-pc-windows-msvc         Windows x86
onchainos-aarch64-pc-windows-msvc      Windows ARM64
checksums.txt
```

### Wrap 模式 — 打包不变

MCP Server 是 Node.js，不需要编译为 native binary。

- 原有 9 平台 Rust 构建 **一行不改**
- 新增一步 `npm publish @onchainos/mcp-server` 即可
- 用户安装流程变为两步：

```bash
# 1. 还是要装 onchainos binary（现有流程不变）
curl -sSfL https://raw.githubusercontent.com/okx/onchainos-skills/master/install.sh | sh

# 2. 额外装 MCP Server
npx @onchainos/mcp-server
```

### 重写模式 — Rust 打包可以全删

MCP Server 直接调 OKX API，不依赖 binary。

- `release.yml` 里 9 个平台的 build matrix **可以全部删除**
- 用户只需：`npx @onchainos/mcp-server`
- 但现有 CLI 用户（脚本、自动化工具）会断掉，需单独决策是否继续维护

### 汇总

| | Wrap 模式 | 重写模式 |
|--|-----------|---------|
| Rust binary 继续打包 | **是**，9 平台不变 | **否**，可全删 |
| 新增打包内容 | npm 包 | npm 包 |
| 现有 CLI 用户影响 | 无 | CLI 停止维护 |
| `release.yml` 改动量 | 小（加 npm publish） | 大（删 build matrix） |

---

## 结论与建议

```
现在（快速验证）         →  Wrap 模式
     ↓
确认 MCP 架构可行        →  逐步迁移到重写模式
     ↓
需要授权/签名/多用户     →  完全切换重写模式
```

**推荐路径：**

1. 先用 Wrap 模式跑通完整的 34 个工具，验证 Claude ↔ MCP ↔ onchainos 全链路
2. 同时将重写模式的 `apiGet` / `sign` 基础设施抽出来，作为下一步的基础
3. 当授权需求明确后，再按工具组逐步从 Wrap 切换到直接 API 调用
4. 重写模式稳定后，停止 Rust 构建，统一通过 npm 分发

这样既不浪费现有 Rust CLI 的工作量，又为重写模式铺路。
