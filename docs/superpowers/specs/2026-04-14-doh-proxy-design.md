# onchainos CLI — DoH Proxy Integration Design

## Problem

OKX API domains are blocked in mainland China via two mechanisms:
- **DNS pollution**: `web3.okx.com` resolves to `169.254.0.2` (bogus link-local address), TCP times out
- **TLS RST**: `wsdex.okx.com` resolves correctly but TLS handshake is reset by GFW

OKX provides a pre-compiled binary `okx-doh-resolver` that discovers alternative proxy nodes via encrypted DNS-over-HTTPS. The binary contains embedded RSA private keys for decrypting DoH responses — this logic cannot be reimplemented in onchainos.

## Design Decisions (Confirmed)

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Cache path | `~/.onchainos/doh-cache.json` | Separate from TS SDK's `~/.okx/` |
| Cache format | Rust serde native | No need for TS SDK compatibility since paths are separate |
| Binary download timing | Lazy — on first direct-connect failure when binary absent | Don't slow down any user who doesn't need it |
| Binary storage | `~/.onchainos/bin/okx-doh-resolver` | Under onchainos home dir |
| Request rewrite (HTTP) | Change base_url to `https://{node.host}`, use `reqwest::ClientBuilder::resolve(host, ip)` | TLS SNI must be `node.host` for cert validation |
| Request rewrite (WS) | Manual TCP connect to `node.ip` + TLS with SNI `node.host` + tungstenite handshake | `connect_async` has no `resolve()` equivalent |
| Client rebuild | Rebuild `reqwest::Client` when DoH node changes | `resolve()` is only available at build time |
| POST retry | Never | Price may change; fund-moving operations must not be duplicated |
| Scope | HTTP (`web3.okx.com`) + WebSocket (`wsdex.okx.com`, `wsdexpre.okx.com:8443`) | Both confirmed blocked; pre-env WS also covered |
| Custom base_url | DoH skipped entirely | Same as TS SDK: user-configured proxy takes precedence |

## Architecture

### Module Structure

```
src/doh/
  mod.rs          — pub exports
  types.rs        — DohNode, DohCache, DohMode serde types
  binary.rs       — download + exec okx-doh-resolver
  cache.rs        — ~/.onchainos/doh-cache.json read/write
  manager.rs      — DohManager: state machine + public API
  ws.rs           — DoH-aware WebSocket connect helper
```

### Integration Points

```
ApiClient (client.rs)          ──┐
WalletApiClient (wallet_api.rs) ─┤── DohManager (shared via Arc if needed)
WS daemon (watch/daemon.rs)    ──┘
```

## Types (`types.rs`)

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
    pub failed_at: u64, // unix timestamp in milliseconds
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

/// Cache file: domain -> cache entry
pub type DohCacheFile = HashMap<String, DohCacheEntry>;
```

## Binary Management (`binary.rs`)

Two responsibilities: **download** and **exec**.

### Binary Path

`~/.onchainos/bin/okx-doh-resolver`, overridable via `OKX_DOH_BINARY_PATH` env var.

### Platform Mapping

| Rust target | CDN platform string |
|-------------|-------------------|
| `aarch64-apple-darwin` | `darwin-arm64` |
| `x86_64-apple-darwin` | `darwin-x64` |
| `x86_64-unknown-linux-*` | `linux-x64` |
| `x86_64-pc-windows-*` | `win32-x64` |

### Download Flow

Triggered only when: direct connection failed AND binary does not exist locally.

1. Detect platform
2. Try CDN sources in order:
   - `https://static.okx.com/upgradeapp/doh/{platform}/okx-doh-resolver`
   - `https://pcdoh.qcxex.com/upgradeapp/doh/{platform}/okx-doh-resolver`
   - `https://static.coinall.ltd/upgradeapp/doh/{platform}/okx-doh-resolver`
3. Write to `~/.onchainos/bin/okx-doh-resolver`
4. Set executable permission (0755 on Unix)
5. Best-effort: any failure returns `None`, does not panic or block

### Exec Interface

```rust
pub async fn exec_doh_binary(
    domain: &str,
    exclude: &[String],
) -> Option<DohNode>
```

- Calls: `okx-doh-resolver --domain {domain} [--exclude ip1,ip2]`
- Timeout: 30 seconds
- Parses stdout JSON: `{ "code": 0, "data": { "ip": "...", "host": "...", "ttl": ... } }`
- Returns `None` on any error (non-zero code, timeout, parse failure, binary not found)

## Cache (`cache.rs`)

### File Location

`~/.onchainos/doh-cache.json`

### Interface

```rust
pub fn read_cache(domain: &str) -> Option<DohCacheEntry>
pub fn write_cache(domain: &str, entry: &DohCacheEntry)
pub fn invalidate_cache(domain: &str)
```

### Write Strategy

- Read existing file → merge domain entry → atomic write (`.tmp` + `rename`)
- All operations best-effort: errors are silently ignored (cache miss just means one extra binary call)

### Failed Nodes TTL

- Failed nodes expire after 1 hour (3,600,000 ms)
- Expired nodes are cleaned up on next `read_cache`

## State Manager (`manager.rs`)

`DohManager` is the only public interface. All callers (ApiClient, WalletApiClient, WS daemon) interact through it.

### State

```rust
pub struct DohManager {
    domain: String,              // e.g. "web3.okx.com"
    original_base_url: String,   // e.g. "https://web3.okx.com"
    mode: Option<DohMode>,       // current routing mode
    node: Option<DohNode>,       // current proxy node (when mode=Proxy)
    resolved: bool,              // first resolution done?
    retried: bool,               // already did failover this cycle?
}
```

### Public API

```rust
impl DohManager {
    pub fn new(domain: &str, base_url: &str) -> Self

    /// Called before first request. Reads cache, sets mode.
    /// If cache hit → sets mode + node. If cache miss → no-op (try direct first).
    pub fn prepare(&mut self)

    /// Called on network failure. Returns whether caller should retry.
    /// - First failure (no cache): try direct connect failed → call binary → get node → return true
    /// - First failure (cache=proxy, node failed): exclude node → re-resolve → return true
    /// - Already retried this cycle: return false (avoid infinite loop)
    /// - GET callers retry on true; POST callers never retry
    /// - `retried` flag resets on successful node switch (supports MCP long-running processes)
    pub async fn handle_failure(&mut self) -> bool

    /// Returns the base_url to use (original or proxy)
    pub fn base_url(&self) -> &str

    /// Returns resolve override for reqwest ClientBuilder, if in proxy mode
    pub fn resolve_override(&self) -> Option<(&str, std::net::SocketAddr)>

    /// Called after successful direct connection (no cache).
    /// Caches mode=Direct so future requests skip DoH entirely.
    pub fn cache_direct_if_needed(&self)
}
```

### State Transitions

```
                     ┌─────────────────────────────────────────┐
                     │           No Cache (initial)            │
                     └──────┬──────────────────┬───────────────┘
                            │                  │
                   direct succeeds        direct fails
                            │                  │
                            v                  v
                  ┌─────────────────┐  ┌───────────────────┐
                  │  Cache: Direct  │  │  Call binary       │
                  │  (zero cost)    │  │  → get proxy node  │
                  └─────────────────┘  └──────┬────────────┘
                                              │
                                              v
                                    ┌─────────────────────┐
                                    │  Cache: Proxy       │
                                    │  (use node.host/ip) │
                                    └──────┬──────────────┘
                                           │
                                    proxy node fails
                                           │
                                           v
                                    ┌─────────────────────┐
                                    │  Failover            │
                                    │  exclude failed node │
                                    │  re-resolve binary   │
                                    └──────┬──────────────┘
                                           │
                                  all nodes exhausted
                                           │
                                           v
                                    ┌─────────────────────┐
                                    │  Fallback: direct   │
                                    │  (best-effort)      │
                                    └─────────────────────┘
```

## HTTP Integration (`client.rs`)

### ApiClient Changes

```rust
pub struct ApiClient {
    http: Client,
    base_url: String,          // original, never changes
    auth: AuthMode,
    doh: DohManager,           // new
}
```

### Request Flow (get_with_headers / post_with_headers)

```
Before:
  build request → send → handle_response

After:
  doh.prepare()
  build request with doh.base_url()
  if doh.resolve_override() → rebuild Client with resolve()
  send
    → success → doh.cache_direct_if_needed() → handle_response
    → network error →
        doh.handle_failure()
          → true + GET → rebuild Client → retry once
          → true + POST → don't retry, return error
          → false → return error
```

### Client Rebuild

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

Same pattern. `DohManager` created with same domain, 30s timeout preserved.

### Signature Compatibility

HMAC signature format is `timestamp + method + path + body` — no hostname. Changing the base_url does not affect signature validation.

## WebSocket Integration (`watch/daemon.rs`)

### Current Code

```rust
const WS_URL_PROD: &str = "wss://wsdex.okx.com/ws/v6/dex";
let (mut ws, _) = connect_async(ws_url).await?;
```

### After Integration

```rust
let (mut ws, _) = doh_connect_ws(ws_url, &doh_manager).await?;
```

### `doh_connect_ws` Implementation

Located in `src/doh/ws.rs` (new file):

```rust
pub async fn doh_connect_ws(
    url: &str,
    doh: &DohManager,
) -> Result<(WebSocketStream<...>, Response)>
```

Logic:
1. `doh.resolve_override()` returns `None` → standard `connect_async(url)`
2. `doh.resolve_override()` returns `Some((host, addr))` →
   - `TcpStream::connect(addr)` — connect to proxy IP directly
   - TLS handshake via `tokio-rustls` with SNI = `node.host`
   - `tokio_tungstenite::client_async(url, tls_stream)` — WS handshake over established TLS

### New Dependency

`tokio-rustls` — needed for explicit TLS control. The `rustls` version must match what `reqwest` uses internally (via `rustls-tls` feature) to avoid duplicate crate versions.

### WS Reconnect

The existing reconnect loop in `daemon.rs` already retries on failure. On reconnect, it should call `doh.handle_failure()` to potentially switch nodes before retrying.

## Error Handling

All DoH operations follow the **best-effort** principle:

| Operation | On Failure |
|-----------|-----------|
| Read cache | Return None, proceed as if no cache |
| Write cache | Silently ignore, next request calls binary again |
| Download binary | Return None, fall back to direct connect |
| Exec binary | Return None, fall back to direct connect |
| Proxy connect | handle_failure() → try next node or fall back to direct |

No DoH-related error should ever surface to the user as a fatal error. The worst case is falling back to direct connection (same as today without DoH).

## User-Agent

When routing through a proxy node, User-Agent changes to:
```
OKX/onchainos-cli/{version}
```
This helps OKX ops distinguish proxy traffic from direct traffic.

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `OKX_DOH_BINARY_PATH` | Override binary path (for testing) |

No other new env vars needed. Existing `OKX_BASE_URL` override takes precedence — if user sets a custom base_url, DoH is completely skipped (same as TS SDK behavior).

## Testing Strategy

### Unit Tests

- `doh/cache.rs`: read/write/invalidate, atomic write, failed node expiry cleanup
- `doh/binary.rs`: path resolution, env var override, platform mapping
- `doh/manager.rs`: state transitions (direct cache, proxy cache, failover, all-exhausted fallback)

### Integration Testing

Manual verification in mainland China environment:
- HTTP direct fails → DoH kicks in → request succeeds
- WS direct fails → DoH kicks in → WS connects
- Binary not present → auto-download → works
- Proxy node down → failover to next node
- POST fails during DoH switch → no retry, error returned

## Files Changed Summary

| File | Change |
|------|--------|
| `src/doh/mod.rs` | New — module exports |
| `src/doh/types.rs` | New — serde types |
| `src/doh/binary.rs` | New — download + exec binary |
| `src/doh/cache.rs` | New — cache read/write |
| `src/doh/manager.rs` | New — DohManager state machine |
| `src/doh/ws.rs` | New — DoH-aware WS connect |
| `src/client.rs` | Modified — add DohManager, retry logic |
| `src/wallet_api.rs` | Modified — add DohManager |
| `src/watch/daemon.rs` | Modified — use doh_connect_ws |
| `src/main.rs` | Modified — register doh module |
| `Cargo.toml` | Modified — add tokio-rustls dep |
