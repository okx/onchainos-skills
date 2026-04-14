# onchainos CLI — DoH Proxy 集成设计

## 问题

OKX API 域名在中国大陆被两种方式封锁：
- **DNS 污染**：`web3.okx.com` 解析到 `169.254.0.2`（伪造的链路本地地址），TCP 超时
- **TLS RST**：`wsdex.okx.com` DNS 解析正常，但 TLS 握手被 GFW 重置

OKX 提供预编译的二进制 `okx-doh-resolver`，通过加密的 DNS-over-HTTPS 发现备用代理节点。该二进制内嵌了用于解密 DoH 响应的 RSA 私钥，此逻辑无法在 onchainos 中重新实现。

## 已确认的设计决策

| 决策 | 选择 | 原因 |
|------|------|------|
| 缓存路径 | `~/.onchainos/doh-cache.json` | 与 TS SDK 的 `~/.okx/` 分离 |
| 缓存格式 | Rust serde 原生格式 | 路径已分开，无需兼容 TS SDK |
| 二进制下载时机 | 懒加载 — 首次直连失败且本地无二进制时触发 | 不拖累任何不需要 DoH 的用户 |
| 二进制存储位置 | `~/.onchainos/bin/okx-doh-resolver` | 统一在 onchainos home 目录下 |
| HTTP 请求改写 | base_url 改为 `https://{node.host}`，使用 `reqwest::ClientBuilder::resolve(host, ip)` | TLS SNI 必须是 `node.host` 才能通过证书校验 |
| WS 请求改写 | 手动 TCP 连接 `node.ip` + TLS 握手 SNI 设为 `node.host` + tungstenite 握手 | `connect_async` 没有 `resolve()` 等价方法 |
| Client 重建 | DoH 节点变更时重建 `reqwest::Client` | `resolve()` 只能在构建时调用 |
| POST 重试 | 永不重试 | 价格可能变动；资金操作不能重复提交 |
| 覆盖范围 | HTTP (`web3.okx.com`) + WebSocket (`wsdex.okx.com`、`wsdexpre.okx.com:8443`) | 两者均已确认被封锁；pre 环境 WS 也覆盖 |
| 自定义 base_url | 完全跳过 DoH | 与 TS SDK 一致：用户配置的代理优先 |

## 架构

### 模块结构

```
src/doh/
  mod.rs          — pub 导出
  types.rs        — DohNode、DohCache、DohMode 等 serde 类型
  binary.rs       — 下载 + 执行 okx-doh-resolver
  cache.rs        — ~/.onchainos/doh-cache.json 读写
  manager.rs      — DohManager：状态机 + 对外接口
  ws.rs           — DoH 感知的 WebSocket 连接辅助
```

### 集成点

```
ApiClient (client.rs)          ──┐
WalletApiClient (wallet_api.rs) ─┤── DohManager（需要时通过 Arc 共享）
WS daemon (watch/daemon.rs)    ──┘
```

## 类型定义 (`types.rs`)

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DohNode {
    pub ip: String,
    pub host: String,
    pub ttl: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailedNode {
    pub ip: String,
    pub failed_at: u64, // unix 毫秒时间戳
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DohMode {
    Proxy,
    Direct,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DohCacheEntry {
    pub mode: DohMode,
    pub node: Option<DohNode>,
    pub failed_nodes: Vec<FailedNode>,
    pub updated_at: u64,
}

/// 缓存文件格式：域名 → 缓存条目
pub type DohCacheFile = HashMap<String, DohCacheEntry>;
```

## 二进制管理 (`binary.rs`)

两个职责：**下载** 和 **执行**。

### 二进制路径

`~/.onchainos/bin/okx-doh-resolver`，可通过 `OKX_DOH_BINARY_PATH` 环境变量覆盖。

### 平台映射

| Rust target | CDN platform 字符串 |
|-------------|-------------------|
| `aarch64-apple-darwin` | `darwin-arm64` |
| `x86_64-apple-darwin` | `darwin-x64` |
| `x86_64-unknown-linux-*` | `linux-x64` |
| `x86_64-pc-windows-*` | `win32-x64` |

### 下载流程

触发条件：直连失败 **且** 本地不存在二进制。

1. 检测平台
2. 依次尝试 3 个 CDN 源：
   - `https://static.okx.com/upgradeapp/doh/{platform}/okx-doh-resolver`
   - `https://pcdoh.qcxex.com/upgradeapp/doh/{platform}/okx-doh-resolver`
   - `https://static.coinall.ltd/upgradeapp/doh/{platform}/okx-doh-resolver`
3. 写入 `~/.onchainos/bin/okx-doh-resolver`
4. 设置可执行权限（Unix 上 0755）
5. Best-effort：任何失败返回 `None`，不 panic 不阻塞

### 执行接口

```rust
pub async fn exec_doh_binary(
    domain: &str,
    exclude: &[String],
) -> Option<DohNode>
```

- 调用：`okx-doh-resolver --domain {domain} [--exclude ip1,ip2]`
- 超时：30 秒
- 解析 stdout JSON：`{ "code": 0, "data": { "ip": "...", "host": "...", "ttl": ... } }`
- 任何错误（非零 code、超时、解析失败、二进制不存在）返回 `None`

## 缓存 (`cache.rs`)

### 文件位置

`~/.onchainos/doh-cache.json`

### 接口

```rust
pub fn read_cache(domain: &str) -> Option<DohCacheEntry>
pub fn write_cache(domain: &str, entry: &DohCacheEntry)
pub fn invalidate_cache(domain: &str)
```

### 写入策略

- 读取现有文件 → 合并域名条目 → 原子写入（`.tmp` + `rename`）
- 所有操作 best-effort：错误静默忽略（缓存未命中仅意味着多调一次二进制）

### 失败节点 TTL

- 失败节点 1 小时后过期（3,600,000 毫秒）
- 下次 `read_cache` 时清理过期节点

## 状态管理器 (`manager.rs`)

`DohManager` 是唯一对外接口。所有调用方（ApiClient、WalletApiClient、WS daemon）都通过它交互。

### 状态

```rust
pub struct DohManager {
    domain: String,              // 例如 "web3.okx.com"
    original_base_url: String,   // 例如 "https://web3.okx.com"
    mode: Option<DohMode>,       // 当前路由模式
    node: Option<DohNode>,       // 当前代理节点（mode=Proxy 时有值）
    resolved: bool,              // 是否已完成首次解析？
    retried: bool,               // 本轮是否已 failover 过？
}
```

### 公开 API

```rust
impl DohManager {
    pub fn new(domain: &str, base_url: &str) -> Self

    /// 首次请求前调用。读缓存，设置 mode。
    /// 缓存命中 → 设置 mode + node。缓存未命中 → 不做任何事（先尝试直连）。
    pub fn prepare(&mut self)

    /// 网络失败时调用。返回调用方是否应该重试。
    /// - 首次失败（无缓存）：直连失败 → 调用二进制 → 获取节点 → 返回 true
    /// - 首次失败（缓存=proxy，节点挂了）：排除该节点 → 重新解析 → 返回 true
    /// - 本轮已重试过：返回 false（避免无限循环）
    /// - GET 调用方收到 true 时重试；POST 调用方永不重试
    /// - `retried` 标志在成功切换节点后重置（支持 MCP 长驻进程）
    pub async fn handle_failure(&mut self) -> bool

    /// 返回请求应使用的 base_url（原始或代理）
    pub fn base_url(&self) -> &str

    /// 返回 reqwest ClientBuilder 的 resolve override，proxy 模式下将 node.host 映射到 node.ip:443
    pub fn resolve_override(&self) -> Option<(&str, std::net::SocketAddr)>

    /// 直连成功后调用（无缓存时）。
    /// 缓存 mode=Direct，后续请求完全跳过 DoH。
    pub fn cache_direct_if_needed(&self)
}
```

### 状态转换

```
                     ┌─────────────────────────────────────────┐
                     │           无缓存（初始状态）              │
                     └──────┬──────────────────┬───────────────┘
                            │                  │
                       直连成功             直连失败
                            │                  │
                            v                  v
                  ┌─────────────────┐  ┌───────────────────┐
                  │  缓存: Direct   │  │  调用二进制         │
                  │  （零开销）      │  │  → 获取代理节点     │
                  └─────────────────┘  └──────┬────────────┘
                                              │
                                              v
                                    ┌─────────────────────┐
                                    │  缓存: Proxy        │
                                    │  （用 node.host/ip） │
                                    └──────┬──────────────┘
                                           │
                                      代理节点失败
                                           │
                                           v
                                    ┌─────────────────────┐
                                    │  Failover            │
                                    │  排除失败节点         │
                                    │  重新调用二进制       │
                                    └──────┬──────────────┘
                                           │
                                     所有节点耗尽
                                           │
                                           v
                                    ┌─────────────────────┐
                                    │  兜底: 直连          │
                                    │  （best-effort）     │
                                    └─────────────────────┘
```

## HTTP 集成 (`client.rs`)

### ApiClient 改动

```rust
pub struct ApiClient {
    http: Client,
    base_url: String,          // 原始值，不变
    auth: AuthMode,
    doh: DohManager,           // 新增
}
```

### 请求流程 (get_with_headers / post_with_headers)

```
改动前:
  构建请求 → send → handle_response

改动后:
  doh.prepare()
  用 doh.base_url() 构建请求
  如果 doh.resolve_override() 有值 → 用 resolve() 重建 Client
  send
    → 成功 → doh.cache_direct_if_needed() → handle_response
    → 网络错误 →
        doh.handle_failure()
          → true + GET → 重建 Client → 重试一次
          → true + POST → 不重试，返回错误
          → false → 返回错误
```

### Client 重建

```rust
fn rebuild_http_client(&mut self) -> Result<()> {
    let mut builder = Client::builder()
        .timeout(std::time::Duration::from_secs(10));
    if let Some((host, addr)) = self.doh.resolve_override() {
        builder = builder.resolve(host, addr);
    }
    self.http = builder.build()?;
    Ok(())
}
```

### WalletApiClient

同样的模式。`DohManager` 用相同域名创建，保留 30 秒超时。

### 签名兼容性

HMAC 签名格式是 `timestamp + method + path + body` — 不包含 hostname。更改 base_url 不影响签名校验。

## WebSocket 集成 (`watch/daemon.rs`)

### 当前代码

```rust
const WS_URL_PROD: &str = "wss://wsdex.okx.com/ws/v6/dex";
let (mut ws, _) = connect_async(ws_url).await?;
```

### 改动后

```rust
let (mut ws, _) = doh_connect_ws(ws_url, &doh_manager).await?;
```

### `doh_connect_ws` 实现

位于 `src/doh/ws.rs`（新文件）：

```rust
pub async fn doh_connect_ws(
    url: &str,
    doh: &DohManager,
) -> Result<(WebSocketStream<...>, Response)>
```

逻辑：
1. `doh.resolve_override()` 返回 `None` → 标准 `connect_async(url)`
2. `doh.resolve_override()` 返回 `Some((host, addr))` →
   - `TcpStream::connect(addr)` — 直接连接代理 IP
   - 通过 `tokio-rustls` 进行 TLS 握手，SNI 设为 `node.host`
   - `tokio_tungstenite::client_async(url, tls_stream)` — 在已建立的 TLS 连接上进行 WS 握手

### 新依赖

`tokio-rustls` — 需要显式控制 TLS 层。`rustls` 版本必须与 `reqwest` 内部使用的版本匹配（通过 `rustls-tls` feature），避免重复 crate 版本。

### WS 重连

`daemon.rs` 中现有的重连循环已经会在失败时重试。重连前应调用 `doh.handle_failure()` 以在重试前可能切换节点。

## 错误处理

所有 DoH 操作遵循 **best-effort** 原则：

| 操作 | 失败时 |
|------|--------|
| 读缓存 | 返回 None，当作无缓存处理 |
| 写缓存 | 静默忽略，下次请求会再调一次二进制 |
| 下载二进制 | 返回 None，回退到直连 |
| 执行二进制 | 返回 None，回退到直连 |
| 代理连接 | handle_failure() → 尝试下一个节点或回退直连 |

DoH 相关的错误不应以致命错误的形式暴露给用户。最坏情况是回退到直连（和没有 DoH 时一样）。

## User-Agent

通过代理节点路由时，User-Agent 改为：
```
OKX/onchainos-cli/{version}
```
帮助 OKX 运维区分代理流量和直连流量。

## 环境变量

| 变量 | 用途 |
|------|------|
| `OKX_DOH_BINARY_PATH` | 覆盖二进制路径（用于测试） |

无需其他新环境变量。现有的 `OKX_BASE_URL` 覆盖优先 — 如果用户设置了自定义 base_url，DoH 完全跳过（与 TS SDK 行为一致）。

## 测试策略

### 单元测试

- `doh/cache.rs`：读写/失效、原子写入、失败节点过期清理
- `doh/binary.rs`：路径解析、环境变量覆盖、平台映射
- `doh/manager.rs`：状态转换（direct 缓存、proxy 缓存、failover、节点耗尽兜底）

### 集成测试

在中国大陆网络环境下手动验证：
- HTTP 直连失败 → DoH 介入 → 请求成功
- WS 直连失败 → DoH 介入 → WS 连接成功
- 二进制不存在 → 自动下载 → 正常工作
- 代理节点宕机 → failover 到下一个节点
- DoH 切换期间 POST 失败 → 不重试，返回错误

## 变更文件汇总

| 文件 | 变更 |
|------|------|
| `src/doh/mod.rs` | 新增 — 模块导出 |
| `src/doh/types.rs` | 新增 — serde 类型 |
| `src/doh/binary.rs` | 新增 — 下载 + 执行二进制 |
| `src/doh/cache.rs` | 新增 — 缓存读写 |
| `src/doh/manager.rs` | 新增 — DohManager 状态机 |
| `src/doh/ws.rs` | 新增 — DoH 感知的 WS 连接 |
| `src/client.rs` | 修改 — 添加 DohManager，重试逻辑 |
| `src/wallet_api.rs` | 修改 — 添加 DohManager |
| `src/watch/daemon.rs` | 修改 — 使用 doh_connect_ws |
| `src/main.rs` | 修改 — 注册 doh 模块 |
| `Cargo.toml` | 修改 — 添加 tokio-rustls 依赖 |
