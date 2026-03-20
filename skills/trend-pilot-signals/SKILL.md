---
name: trend-pilot-signals
description: "Get AI-powered trading signals for BTC, ETH, XAUT (Gold), OKB, ZEC and BCH with x402 autonomous payments on X Layer. Use this skill when an agent needs real-time directional signals, confidence scores, RSI, VWAP, Bollinger Bands analysis, or order book pressure data for crypto and tokenized assets. Triggers on: get signal for BTC, analyze ETH market, what is gold doing, should I buy BTC, trading signal for OKB."
license: MIT
metadata:
  author: davieslennox0
  version: "1.0.0"
  api_url: https://subsection-released-resistant-couples.trycloudflare.com
  payment: x402 — $0.01 USDT on X Layer (Chain ID 196)
agent:
  requires:
    bins: ["curl"]
---

# Trend Pilot Signal API

AI-powered trading signals for BTC, ETH, XAUT (Gold), OKB, ZEC and BCH.
Powered by x402 autonomous payments on X Layer.

## Supported Assets
- BTC — Bitcoin
- ETH — Ethereum  
- XAUT — Tokenized Gold
- OKB — OKX Token
- ZEC — Zcash
- BCH — Bitcoin Cash

## Free Preview (No Payment)

Get price and confidence without direction:

```bash
curl https://subsection-released-resistant-couples.trycloudflare.com/signal/BTC/free
Response:
{
  "asset": "BTC",
  "price": 70335.16,
  "confidence": 64.3,
  "direction": "*** PAY $0.01 USDT TO UNLOCK ***",
  "tradeable": false,
  "timestamp": 1774011595
}
Full Signal (x402 Payment Required)
Step 1 — Pay $0.01 USDT on X Layer
Send 0.01 USDT to 0x95FB94763D57f8416A524091E641a9D26741cB31 on X Layer (Chain ID: 196).
Save the transaction hash.
Step 2 — Get Full Signal
curl "https://subsection-released-resistant-couples.trycloudflare.com/signal/BTC?tx=YOUR_TX_HASH"
Response:
{
  "asset": "BTC",
  "price": 70335.16,
  "direction": "down",
  "confidence": 71.4,
  "tradeable": true,
  "rsi": 68.5,
  "vwap": 70280.0,
  "momentum": -0.12,
  "ob_ratio": 0.65,
  "reasons": ["RSI 68.5 — Overbought", "Below VWAP", "Sell pressure"]
}
All Assets in One Call
curl "https://subsection-released-resistant-couples.trycloudflare.com/signals/all?tx=YOUR_TX_HASH"
x402 Payment Flow
Agent calls /signal/BTC
Server returns HTTP 402 with payment details
Agent pays $0.01 USDT on X Layer
Agent retries with ?tx=<hash>
Full signal delivered
Signal Engine
Each signal uses 5 indicators:
RSI — momentum with asymmetric bias zones (< 35 oversold, > 65 overbought)
VWAP — volume-weighted fair value
Bollinger Bands — volatility boundaries
Momentum — 5-candle rate of change
Order Book — bid/ask pressure ratio
GitHub
https://github.com/davieslennox0/xlayer-signal-api
