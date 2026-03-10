# onchainos CLI → MCP Server 适配计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 将现有 `onchainos` Rust CLI 包装成 MCP Server，让 Claude 通过标准 MCP Tool Call 调用所有链上工具，替代当前 SKILL.md + Shell 方式，为后续授权体系奠定基础。

**Architecture:** Wrap 模式——新建 Node.js MCP Server，内部 `spawn onchainos` 执行命令、捕获 JSON 输出后返回给 Claude。CLI 本身不改动。Transport 使用 stdio（本地运行）。

**Tech Stack:** Node.js 20, TypeScript, `@modelcontextprotocol/sdk ^1.0`, `execa ^9`

---

## 背景

| 当前方式 | 适配后 |
|---------|--------|
| Claude 读 SKILL.md → 手拼 shell 命令 | Claude 直接 MCP Tool Call |
| 每次无状态，全量授权 | Server 持有凭证，可按工具分级授权 |
| API Key 需用户手动 export | MCP Server 配置时一次性设置 |

**本计划范围：** 只做适配层，共 34 个工具。不实现签名、授权流程、托管钱包。

---

## 文件结构

```
mcp/
├── package.json
├── tsconfig.json
└── src/
    ├── index.ts       ← MCP Server 入口（~50 行）
    ├── runner.ts      ← CLI 子进程执行器（~40 行）
    └── tools.ts       ← 34 个工具定义 + 分发（~350 行）
```

---

## Task 1: 初始化项目

**Files:** `mcp/package.json`, `mcp/tsconfig.json`

**Step 1: 创建 package.json**

```json
{
  "name": "@onchainos/mcp-server",
  "version": "0.1.0",
  "type": "module",
  "main": "dist/index.js",
  "bin": { "onchainos-mcp": "dist/index.js" },
  "scripts": {
    "build": "tsc",
    "start": "node dist/index.js"
  },
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

**Step 2: 创建 tsconfig.json**

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "Node16",
    "moduleResolution": "Node16",
    "outDir": "./dist",
    "rootDir": "./src",
    "strict": true,
    "esModuleInterop": true,
    "skipLibCheck": true
  },
  "include": ["src"]
}
```

**Step 3: 安装依赖**

```bash
cd mcp && npm install
```

Expected: `node_modules/` 生成，无报错。

**Step 4: Commit**

```bash
git add mcp/package.json mcp/tsconfig.json
git commit -m "feat(mcp): init Node.js MCP server project"
```

---

## Task 2: CLI 执行器

**Files:** `mcp/src/runner.ts`

```typescript
// mcp/src/runner.ts
import { execa } from 'execa';

/** CLI 统一输出结构（对应 Rust output.rs 的 JsonOutput） */
export interface CliResult {
  ok: boolean;
  data?: unknown;
  error?: string;
}

/** onchainos binary 路径优先级：环境变量 > ~/.local/bin > PATH */
function bin(): string {
  return process.env.ONCHAINOS_BIN
    ?? `${process.env.HOME}/.local/bin/onchainos`;
}

/**
 * 调用 onchainos CLI 并返回 JSON 结果
 * @example run(['market', 'price', '0xeeee...', '--chain', 'ethereum'])
 */
export async function run(args: string[]): Promise<CliResult> {
  try {
    const result = await execa(bin(), ['--output', 'json', ...args], {
      env: { ...process.env },
      timeout: 30_000,
      reject: false,
    });

    const raw = (result.stdout || result.stderr).trim();
    if (!raw) return { ok: false, error: 'empty CLI output' };

    return JSON.parse(raw) as CliResult;
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : String(e) };
  }
}
```

**Step: 验证执行器（需已安装 onchainos）**

```bash
cd mcp && npm run build
node -e "
import('./dist/runner.js').then(async ({ run }) => {
  const r = await run(['market', 'signal-chains']);
  console.log(JSON.stringify(r, null, 2));
});
"
```

Expected: `{"ok": true, "data": [...]}`

**Commit:**

```bash
git add mcp/src/runner.ts
git commit -m "feat(mcp): add CLI subprocess runner"
```

---

## Task 3: 工具定义与分发

**Files:** `mcp/src/tools.ts`

34 个工具按 5 个命令组定义。每个工具包含 `name`、`description`、`inputSchema`，以及对应的 CLI args 构造逻辑。

```typescript
// mcp/src/tools.ts
import type { Tool } from '@modelcontextprotocol/sdk/types.js';
import { run, type CliResult } from './runner.js';

// ─── 工具定义 ──────────────────────────────────────────────

export const TOOLS: Tool[] = [
  // ── Portfolio (4) ──
  {
    name: 'portfolio_chains',
    description: '获取 portfolio 功能支持的链列表',
    inputSchema: { type: 'object', properties: {} },
  },
  {
    name: 'portfolio_total_value',
    description: '查询钱包在指定链上的总资产价值（USD）',
    inputSchema: {
      type: 'object',
      required: ['address', 'chains'],
      properties: {
        address: { type: 'string', description: '钱包地址' },
        chains: { type: 'string', description: '逗号分隔链名，如 ethereum,solana' },
        asset_type: { type: 'string', description: '0=全部 1=代币 2=DeFi（可选）' },
        exclude_risk: { type: 'boolean', description: '排除风险代币（可选）' },
      },
    },
  },
  {
    name: 'portfolio_all_balances',
    description: '查询钱包在指定链上所有代币余额',
    inputSchema: {
      type: 'object',
      required: ['address', 'chains'],
      properties: {
        address: { type: 'string' },
        chains: { type: 'string', description: '逗号分隔链名' },
        exclude_risk: { type: 'string', description: '0=不排除 1=排除（可选）' },
      },
    },
  },
  {
    name: 'portfolio_token_balances',
    description: '查询钱包指定代币余额，tokens 格式: chainIndex:tokenAddress 逗号分隔',
    inputSchema: {
      type: 'object',
      required: ['address', 'tokens'],
      properties: {
        address: { type: 'string' },
        tokens: { type: 'string', description: '如 196:,1:0x6b17...' },
      },
    },
  },

  // ── Token (5) ──
  {
    name: 'token_search',
    description: '按名称/符号/地址搜索代币',
    inputSchema: {
      type: 'object',
      required: ['query'],
      properties: {
        query: { type: 'string' },
        chains: { type: 'string', description: '逗号分隔链名（可选，默认全链）' },
      },
    },
  },
  {
    name: 'token_info',
    description: '获取代币基础信息（名称、符号、小数位、Logo）',
    inputSchema: {
      type: 'object',
      required: ['address', 'chain'],
      properties: {
        address: { type: 'string' },
        chain: { type: 'string' },
      },
    },
  },
  {
    name: 'token_price_info',
    description: '获取代币价格详情（市值、流动性、24h 涨跌幅）',
    inputSchema: {
      type: 'object',
      required: ['address', 'chain'],
      properties: {
        address: { type: 'string' },
        chain: { type: 'string' },
      },
    },
  },
  {
    name: 'token_trending',
    description: '获取热门代币列表',
    inputSchema: {
      type: 'object',
      required: ['chains'],
      properties: {
        chains: { type: 'string' },
        sort_by: { type: 'string', description: '2=价格变化 5=交易量 6=市值（默认 5）' },
        time_frame: { type: 'string', description: '1=5min 2=1h 3=4h 4=24h（默认 4）' },
      },
    },
  },
  {
    name: 'token_holders',
    description: '获取代币 Top 20 持有人分布',
    inputSchema: {
      type: 'object',
      required: ['address', 'chain'],
      properties: {
        address: { type: 'string' },
        chain: { type: 'string' },
      },
    },
  },

  // ── Market (14) ──
  {
    name: 'market_price',
    description: '获取单个代币当前价格',
    inputSchema: {
      type: 'object',
      required: ['address', 'chain'],
      properties: { address: { type: 'string' }, chain: { type: 'string' } },
    },
  },
  {
    name: 'market_kline',
    description: '获取代币 K 线（OHLCV）数据',
    inputSchema: {
      type: 'object',
      required: ['address', 'chain'],
      properties: {
        address: { type: 'string' },
        chain: { type: 'string' },
        bar: { type: 'string', description: '1m 5m 15m 1H 4H 1D（默认 1H）' },
        limit: { type: 'number', description: '最多 300（默认 24）' },
      },
    },
  },
  {
    name: 'market_trades',
    description: '获取代币最近成交记录',
    inputSchema: {
      type: 'object',
      required: ['address', 'chain'],
      properties: { address: { type: 'string' }, chain: { type: 'string' } },
    },
  },
  {
    name: 'market_index',
    description: '获取代币聚合指数价格',
    inputSchema: {
      type: 'object',
      required: ['address', 'chain'],
      properties: { address: { type: 'string' }, chain: { type: 'string' } },
    },
  },
  {
    name: 'market_signal_chains',
    description: '获取支持智能钱包信号的链列表',
    inputSchema: { type: 'object', properties: {} },
  },
  {
    name: 'market_signal_list',
    description: '获取智能钱包/巨鲸/KOL 的买入信号',
    inputSchema: {
      type: 'object',
      required: ['chain'],
      properties: {
        chain: { type: 'string' },
        wallet_type: { type: 'string', description: 'smart_money|whale|kol（可选）' },
        min_amount_usd: { type: 'string', description: '最小金额 USD（可选）' },
        token_address: { type: 'string', description: '筛选特定代币（可选）' },
      },
    },
  },
  {
    name: 'market_memepump_chains',
    description: '获取 Meme 代币分析支持的链和协议',
    inputSchema: { type: 'object', properties: {} },
  },
  {
    name: 'market_memepump_tokens',
    description: '扫描 Meme 代币列表（支持按市值/交易量过滤）',
    inputSchema: {
      type: 'object',
      required: ['chain'],
      properties: {
        chain: { type: 'string' },
        sort_field: { type: 'string', description: '排序字段（可选）' },
        sort_order: { type: 'string', description: 'asc|desc（可选）' },
        min_market_cap_usd: { type: 'string', description: '（可选）' },
        max_market_cap_usd: { type: 'string', description: '（可选）' },
      },
    },
  },
  {
    name: 'market_memepump_token_details',
    description: '获取 Meme 代币交易数据、持有人、流动性详情',
    inputSchema: {
      type: 'object',
      required: ['address', 'chain'],
      properties: { address: { type: 'string' }, chain: { type: 'string' } },
    },
  },
  {
    name: 'market_memepump_token_dev_info',
    description: '获取 Meme 代币开发者历史（发币记录、Rug pull 历史）',
    inputSchema: {
      type: 'object',
      required: ['address', 'chain'],
      properties: { address: { type: 'string' }, chain: { type: 'string' } },
    },
  },
  {
    name: 'market_memepump_similar_tokens',
    description: '查找相似 Meme 代币',
    inputSchema: {
      type: 'object',
      required: ['address', 'chain'],
      properties: { address: { type: 'string' }, chain: { type: 'string' } },
    },
  },
  {
    name: 'market_memepump_bundle_info',
    description: '检测 Meme 代币的 Bundle/Sniper 买入情况',
    inputSchema: {
      type: 'object',
      required: ['address', 'chain'],
      properties: { address: { type: 'string' }, chain: { type: 'string' } },
    },
  },
  {
    name: 'market_memepump_aped_wallet',
    description: '查看与某代币共同投资的聪明钱包列表',
    inputSchema: {
      type: 'object',
      required: ['address', 'chain'],
      properties: { address: { type: 'string' }, chain: { type: 'string' } },
    },
  },

  // ── Swap (5) ──
  {
    name: 'swap_chains',
    description: '获取 DEX 聚合器支持的链',
    inputSchema: { type: 'object', properties: {} },
  },
  {
    name: 'swap_liquidity',
    description: '获取指定链上可用的 DEX 流动性来源',
    inputSchema: {
      type: 'object',
      required: ['chain'],
      properties: { chain: { type: 'string' } },
    },
  },
  {
    name: 'swap_approve',
    description: '生成 ERC-20 授权交易数据（swap 前调用）',
    inputSchema: {
      type: 'object',
      required: ['token', 'amount', 'chain'],
      properties: {
        token: { type: 'string', description: 'ERC-20 合约地址' },
        amount: { type: 'string', description: '授权金额（最小单位）' },
        chain: { type: 'string' },
      },
    },
  },
  {
    name: 'swap_quote',
    description: '获取 DEX 聚合最优报价（只查询，不执行）',
    inputSchema: {
      type: 'object',
      required: ['from', 'to', 'amount', 'chain'],
      properties: {
        from: { type: 'string', description: '卖出代币地址' },
        to: { type: 'string', description: '买入代币地址' },
        amount: { type: 'string', description: '卖出金额（最小单位）' },
        chain: { type: 'string' },
        swap_mode: { type: 'string', description: 'exactIn|exactOut（默认 exactIn）' },
      },
    },
  },
  {
    name: 'swap_build',
    description: '构建 Swap 未签名交易数据（用户需自行签名后用 gateway_broadcast 广播）',
    inputSchema: {
      type: 'object',
      required: ['from', 'to', 'amount', 'chain', 'wallet'],
      properties: {
        from: { type: 'string' },
        to: { type: 'string' },
        amount: { type: 'string' },
        chain: { type: 'string' },
        wallet: { type: 'string', description: '用户钱包地址' },
        slippage: { type: 'string', description: '滑点百分比（默认 0.5）' },
      },
    },
  },

  // ── Gateway (6) ──
  {
    name: 'gateway_chains',
    description: '获取交易网关支持的链',
    inputSchema: { type: 'object', properties: {} },
  },
  {
    name: 'gateway_gas',
    description: '查询当前 Gas 价格',
    inputSchema: {
      type: 'object',
      required: ['chain'],
      properties: { chain: { type: 'string' } },
    },
  },
  {
    name: 'gateway_gas_limit',
    description: '估算特定交易的 Gas 用量',
    inputSchema: {
      type: 'object',
      required: ['from', 'to', 'amount', 'chain'],
      properties: {
        from: { type: 'string' },
        to: { type: 'string' },
        amount: { type: 'string', description: '转账金额（wei）' },
        chain: { type: 'string' },
        data: { type: 'string', description: 'calldata 十六进制（可选）' },
      },
    },
  },
  {
    name: 'gateway_simulate',
    description: '模拟执行交易（不广播，检查是否会 revert）',
    inputSchema: {
      type: 'object',
      required: ['from', 'to', 'amount', 'data', 'chain'],
      properties: {
        from: { type: 'string' },
        to: { type: 'string' },
        amount: { type: 'string' },
        data: { type: 'string', description: 'calldata 十六进制' },
        chain: { type: 'string' },
      },
    },
  },
  {
    name: 'gateway_broadcast',
    description: '广播已签名交易到链上',
    inputSchema: {
      type: 'object',
      required: ['signed_tx', 'address', 'chain'],
      properties: {
        signed_tx: { type: 'string', description: '已签名交易（十六进制）' },
        address: { type: 'string', description: '发送方地址' },
        chain: { type: 'string' },
      },
    },
  },
  {
    name: 'gateway_orders',
    description: '查询广播订单状态（追踪交易是否上链）',
    inputSchema: {
      type: 'object',
      required: ['address', 'chain'],
      properties: {
        address: { type: 'string' },
        chain: { type: 'string' },
        order_id: { type: 'string', description: '订单 ID（可选，不传则返回所有）' },
      },
    },
  },
];

// ─── 工具分发 ──────────────────────────────────────────────

type Args = Record<string, unknown>;
type Content = { type: 'text'; text: string };

function text(result: CliResult): Content {
  return { type: 'text', text: JSON.stringify(result, null, 2) };
}

function opt(args: Args, key: string, flag: string): string[] {
  return args[key] != null ? [flag, String(args[key])] : [];
}

export async function dispatch(name: string, args: Args): Promise<Content> {
  switch (name) {
    // ── Portfolio ──
    case 'portfolio_chains':
      return text(await run(['portfolio', 'chains']));
    case 'portfolio_total_value':
      return text(await run([
        'portfolio', 'total-value',
        '--address', args.address as string,
        '--chains', args.chains as string,
        ...opt(args, 'asset_type', '--asset-type'),
        ...opt(args, 'exclude_risk', '--exclude-risk'),
      ]));
    case 'portfolio_all_balances':
      return text(await run([
        'portfolio', 'all-balances',
        '--address', args.address as string,
        '--chains', args.chains as string,
        ...opt(args, 'exclude_risk', '--exclude-risk'),
      ]));
    case 'portfolio_token_balances':
      return text(await run([
        'portfolio', 'token-balances',
        '--address', args.address as string,
        '--tokens', args.tokens as string,
      ]));

    // ── Token ──
    case 'token_search':
      return text(await run([
        'token', 'search', args.query as string,
        ...opt(args, 'chains', '--chains'),
      ]));
    case 'token_info':
      return text(await run(['token', 'info', args.address as string, '--chain', args.chain as string]));
    case 'token_price_info':
      return text(await run(['token', 'price-info', args.address as string, '--chain', args.chain as string]));
    case 'token_trending':
      return text(await run([
        'token', 'trending',
        '--chains', args.chains as string,
        ...opt(args, 'sort_by', '--sort-by'),
        ...opt(args, 'time_frame', '--time-frame'),
      ]));
    case 'token_holders':
      return text(await run(['token', 'holders', args.address as string, '--chain', args.chain as string]));

    // ── Market ──
    case 'market_price':
      return text(await run(['market', 'price', args.address as string, '--chain', args.chain as string]));
    case 'market_kline':
      return text(await run([
        'market', 'kline', args.address as string, '--chain', args.chain as string,
        ...opt(args, 'bar', '--bar'),
        ...opt(args, 'limit', '--limit'),
      ]));
    case 'market_trades':
      return text(await run(['market', 'trades', args.address as string, '--chain', args.chain as string]));
    case 'market_index':
      return text(await run(['market', 'index', args.address as string, '--chain', args.chain as string]));
    case 'market_signal_chains':
      return text(await run(['market', 'signal-chains']));
    case 'market_signal_list':
      return text(await run([
        'market', 'signal-list', '--chain', args.chain as string,
        ...opt(args, 'wallet_type', '--wallet-type'),
        ...opt(args, 'min_amount_usd', '--min-amount-usd'),
        ...opt(args, 'token_address', '--token-address'),
      ]));
    case 'market_memepump_chains':
      return text(await run(['market', 'memepump-chains']));
    case 'market_memepump_tokens':
      return text(await run([
        'market', 'memepump-tokens', '--chain', args.chain as string,
        ...opt(args, 'sort_field', '--sort-field'),
        ...opt(args, 'sort_order', '--sort-order'),
        ...opt(args, 'min_market_cap_usd', '--min-market-cap-usd'),
        ...opt(args, 'max_market_cap_usd', '--max-market-cap-usd'),
      ]));
    case 'market_memepump_token_details':
      return text(await run(['market', 'memepump-token-details', args.address as string, '--chain', args.chain as string]));
    case 'market_memepump_token_dev_info':
      return text(await run(['market', 'memepump-token-dev-info', args.address as string, '--chain', args.chain as string]));
    case 'market_memepump_similar_tokens':
      return text(await run(['market', 'memepump-similar-tokens', args.address as string, '--chain', args.chain as string]));
    case 'market_memepump_bundle_info':
      return text(await run(['market', 'memepump-token-bundle-info', args.address as string, '--chain', args.chain as string]));
    case 'market_memepump_aped_wallet':
      return text(await run(['market', 'memepump-aped-wallet', args.address as string, '--chain', args.chain as string]));

    // ── Swap ──
    case 'swap_chains':
      return text(await run(['swap', 'chains']));
    case 'swap_liquidity':
      return text(await run(['swap', 'liquidity', '--chain', args.chain as string]));
    case 'swap_approve':
      return text(await run([
        'swap', 'approve',
        '--token', args.token as string,
        '--amount', args.amount as string,
        '--chain', args.chain as string,
      ]));
    case 'swap_quote':
      return text(await run([
        'swap', 'quote',
        '--from', args.from as string,
        '--to', args.to as string,
        '--amount', args.amount as string,
        '--chain', args.chain as string,
        ...opt(args, 'swap_mode', '--swap-mode'),
      ]));
    case 'swap_build':
      return text(await run([
        'swap', 'swap',
        '--from', args.from as string,
        '--to', args.to as string,
        '--amount', args.amount as string,
        '--chain', args.chain as string,
        '--wallet', args.wallet as string,
        ...opt(args, 'slippage', '--slippage'),
      ]));

    // ── Gateway ──
    case 'gateway_chains':
      return text(await run(['gateway', 'chains']));
    case 'gateway_gas':
      return text(await run(['gateway', 'gas', '--chain', args.chain as string]));
    case 'gateway_gas_limit':
      return text(await run([
        'gateway', 'gas-limit',
        '--from', args.from as string,
        '--to', args.to as string,
        '--amount', args.amount as string,
        '--chain', args.chain as string,
        ...opt(args, 'data', '--data'),
      ]));
    case 'gateway_simulate':
      return text(await run([
        'gateway', 'simulate',
        '--from', args.from as string,
        '--to', args.to as string,
        '--amount', args.amount as string,
        '--data', args.data as string,
        '--chain', args.chain as string,
      ]));
    case 'gateway_broadcast':
      return text(await run([
        'gateway', 'broadcast',
        '--signed-tx', args.signed_tx as string,
        '--address', args.address as string,
        '--chain', args.chain as string,
      ]));
    case 'gateway_orders':
      return text(await run([
        'gateway', 'orders',
        '--address', args.address as string,
        '--chain', args.chain as string,
        ...opt(args, 'order_id', '--order-id'),
      ]));

    default:
      return text({ ok: false, error: `unknown tool: ${name}` });
  }
}
```

**Commit:**

```bash
git add mcp/src/tools.ts
git commit -m "feat(mcp): add 34 tool definitions and dispatch (portfolio/token/market/swap/gateway)"
```

---

## Task 4: MCP Server 入口

**Files:** `mcp/src/index.ts`

```typescript
#!/usr/bin/env node
// mcp/src/index.ts
import { Server } from '@modelcontextprotocol/sdk/server/index.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import {
  CallToolRequestSchema,
  ListToolsRequestSchema,
} from '@modelcontextprotocol/sdk/types.js';
import { TOOLS, dispatch } from './tools.js';

const server = new Server(
  { name: 'onchainos-mcp', version: '0.1.0' },
  { capabilities: { tools: {} } }
);

server.setRequestHandler(ListToolsRequestSchema, async () => ({ tools: TOOLS }));

server.setRequestHandler(CallToolRequestSchema, async (req) => {
  const { name, arguments: args = {} } = req.params;
  const content = await dispatch(name, args as Record<string, unknown>);
  return { content: [content] };
});

const transport = new StdioServerTransport();
await server.connect(transport);
```

**Step 1: 构建**

```bash
cd mcp && npm run build
```

Expected: `dist/` 生成，0 TypeScript 错误。

**Step 2: 用 MCP Inspector 验证**

```bash
npx @modelcontextprotocol/inspector node dist/index.js
```

打开 `http://localhost:5173`：
- List Tools → 应看到 34 个工具
- 调用 `market_signal_chains` → 应返回 `{"ok": true, "data": [...]}`
- 调用 `token_search` `{"query": "USDC"}` → 返回搜索结果

**Step 3: Commit**

```bash
git add mcp/src/index.ts
git commit -m "feat(mcp): add MCP server entry with stdio transport"
```

---

## Task 5: 接入 Claude Code

在项目根目录创建 `.mcp.json`：

```json
{
  "mcpServers": {
    "onchainos": {
      "command": "node",
      "args": ["./mcp/dist/index.js"],
      "env": {
        "OKX_API_KEY": "your-api-key",
        "OKX_SECRET_KEY": "your-secret-key",
        "OKX_PASSPHRASE": "your-passphrase"
      }
    }
  }
}
```

**验证：** 重启 Claude Code，问 "查 ETH 当前价格" → 应自动调用 `market_price` 工具。

**Commit:**

```bash
git add .mcp.json
git commit -m "feat(mcp): add Claude Code MCP config"
```

---

## 验证清单

- [ ] `npm run build` 零 TypeScript 报错
- [ ] MCP Inspector 列出 34 个工具
- [ ] `market_signal_chains` 返回 `{"ok": true, "data": [...]}`
- [ ] `token_search {"query": "USDC"}` 返回搜索结果
- [ ] `swap_quote` 返回报价（不执行交易）
- [ ] Claude Code 识别到全部 34 个 MCP 工具并能调用

---

## 后续计划（本次不实现）

1. **授权层**：区分只读工具（自动通过）和写入工具（`gateway_broadcast`/`swap_build` 需用户确认）
2. **签名集成**：接入钱包签名流程（WalletConnect / 本地密钥）
3. **HTTP Transport**：升级为 HTTP/SSE 支持远程部署和多用户会话
4. **凭证管理**：替换手动填写 API Key 为 OAuth 流程
