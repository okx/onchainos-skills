# Onchain OS DEX Signal — WebSocket Protocol Reference

This document is for **developers and agents** who want to connect directly to the OKX DEX WebSocket
and subscribe to real-time data.

---

## Endpoint

```
wss://wsdex.okx.com:8443/ws/v5/dex
```

Uses TLS. Connect with any standard WebSocket client that supports TLS.

---

## Authentication

The OKX DEX WebSocket uses HMAC-SHA256 API key authentication, which is the same scheme
as the OKX REST API. Full documentation:
👉 https://web3.okx.com/onchainos/dev-docs/market/websocket-login

### Credentials

Obtain your API Key, Secret Key, and Passphrase from the
[OKX Developer Portal](https://web3.okx.com/onchain-os/dev-portal).

### Login Message

After connecting, send a login message before subscribing:

```json
{
  "op": "login",
  "args": [{
    "apiKey":     "<your_api_key>",
    "passphrase": "<your_passphrase>",
    "timestamp":  "<unix_seconds_as_string>",
    "sign":       "<base64_hmac_signature>"
  }]
}
```

**Signature algorithm**:

```
prehash = timestamp + "GET/users/self/verify"
sign    = Base64( HMAC-SHA256(secret_key, prehash) )
```

- `timestamp`: current Unix time in **seconds** (string)
- `secret_key`: your Secret Key (used as the HMAC key)
- `prehash`: string concatenation of timestamp and the literal `GET/users/self/verify`

**Example (Python)**:

```python
import hmac, hashlib, base64, time

def make_sign(secret_key: str) -> tuple[str, str]:
    ts = str(int(time.time()))
    prehash = ts + "GET/users/self/verify"
    sig = base64.b64encode(
        hmac.new(secret_key.encode(), prehash.encode(), hashlib.sha256).digest()
    ).decode()
    return ts, sig

ts, sign = make_sign("YOUR_SECRET_KEY")
login_msg = {
    "op": "login",
    "args": [{"apiKey": "YOUR_API_KEY", "passphrase": "YOUR_PASSPHRASE",
              "timestamp": ts, "sign": sign}]
}
```

**Example (JavaScript/Node)**:

```js
const crypto = require('crypto');

function makeSign(secretKey) {
  const ts = String(Math.floor(Date.now() / 1000));
  const prehash = ts + 'GET/users/self/verify';
  const sign = crypto.createHmac('sha256', secretKey)
    .update(prehash).digest('base64');
  return { ts, sign };
}
```

### Login ACK

The server responds with:

```json
{ "event": "login", "code": "0", "msg": "" }
```

`code` = `"0"` means success. Any other code means failure — check `msg` for details.
Wait for this ACK before sending subscribe messages. Recommended timeout: 10 seconds.

---

## Channels

### `kol_smartmoney-tracker-activity` (Public)

Aggregated real-time trade feed from KOL wallets and smart money tracked by OKX.
No wallet address parameter needed.

Subscribe arg:
```json
{ "channel": "kol_smartmoney-tracker-activity" }
```

### `address-tracker-activity` (Per-address)

Real-time trade feed for a custom wallet address.
Send **one subscription arg per address** (up to 20 addresses).

Subscribe arg:
```json
{ "channel": "address-tracker-activity", "walletAddress": "0xYourAddress" }
```

Supports both EVM addresses (`0x...`) and Solana addresses (base58).

---

## Subscribe Message

Send a single subscribe message containing all channel args:

```json
{
  "op": "subscribe",
  "args": [
    { "channel": "kol_smartmoney-tracker-activity" },
    { "channel": "address-tracker-activity", "walletAddress": "0xAAA..." },
    { "channel": "address-tracker-activity", "walletAddress": "0xBBB..." }
  ]
}
```

### Subscribe ACK

The server sends one ACK per subscription arg:

```json
{ "event": "subscribe", "arg": { "channel": "kol_smartmoney-tracker-activity" } }
```

Wait for N ACKs (one per arg) before considering the session active.
If any arg fails, you receive:

```json
{ "event": "error", "code": "...", "msg": "..." }
```

---

## Push Data Format

When a trade event occurs, the server pushes:

```json
{
  "arg": {
    "channel": "kol_smartmoney-tracker-activity"
  },
  "data": [
    {
      "walletAddress":        "0xabc...",
      "tokenSymbol":          "PEPE",
      "tokenContractAddress": "0x6982...",
      "chainIndex":           "1",
      "tokenPrice":           "0.00001234",
      "marketCap":            "520000000",
      "quoteTokenSymbol":     "USDT",
      "quoteTokenAmount":     "50000",
      "realizedPnlUsd":       "1200.5",
      "tradeType":            "1",
      "tradeTime":            "1742700000000",
      "trackerType":          [1, 2],
      "txHash":               "0xdeadbeef..."
    }
  ]
}
```

For `address-tracker-activity`, the `arg` also includes `walletAddress`:

```json
{
  "arg": {
    "channel": "address-tracker-activity",
    "walletAddress": "0xAAA..."
  },
  "data": [...]
}
```

### Trade Event Fields

| Field | Type | Description |
|---|---|---|
| `walletAddress` | String | Wallet that made the trade |
| `tokenSymbol` | String | Traded token symbol |
| `tokenContractAddress` | String | Token contract address |
| `chainIndex` | String | Chain: `"1"` = Ethereum, `"501"` = Solana, `"56"` = BSC, etc. |
| `tokenPrice` | String | Token price in USD at trade time |
| `marketCap` | String | Token market cap in USD |
| `quoteTokenSymbol` | String | Quote token (e.g. `"USDT"`, `"SOL"`, `"ETH"`) |
| `quoteTokenAmount` | String | Amount of quote token |
| `realizedPnlUsd` | String | Realized PnL for this trade (USD) |
| `tradeType` | String | `"1"` = Buy, `"2"` = Sell |
| `tradeTime` | String | Unix milliseconds |
| `trackerType` | Array\<Number\> | Wallet tags: `1` = Smart Money, `2` = KOL |
| `txHash` | String | Transaction hash (may be absent) |

---

## Heartbeat

Send `"ping"` as a plain text frame every **25 seconds**.
The server responds with `"pong"`. If no pong is received within 25 seconds, reconnect.

```
client → "ping"
server → "pong"
```

---

## Connection Lifecycle

```
1. connect (TLS WebSocket)
2. send login message
3. wait for login ACK  ← timeout 10s
4. send subscribe message
5. wait for N subscribe ACKs  ← timeout 10s
6. receive push data frames
7. send ping every 25s, expect pong
8. on disconnect: reconnect and repeat from step 1
```

### Service Upgrade Notice

Before a server upgrade, you may receive:

```json
{ "event": "notice", ... }
```

Treat this as a signal to gracefully disconnect and reconnect after a short delay.

---

## Reconnection Strategy

The server may disconnect clients during maintenance or network issues.
Recommended reconnect policy:
- Max attempts: 20
- Delay between attempts: 3 seconds
- On exhaustion: surface error to the user

After reconnecting, re-send the full login + subscribe sequence.

