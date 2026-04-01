---
name: okx-dex-ws
description: "Use this skill when the user wants to write a WebSocket script/脚本, build a real-time monitoring bot/监控机器人, or subscribe to on-chain streaming data/实时数据推送 via OKX DEX WebSocket. Covers ALL OKX DEX WebSocket channels: token price monitoring/价格监控/实时行情, market cap & liquidity streaming/市值变化/流动性变化, candlestick/K线推送, token trade feed/代币交易流/每笔成交, smart money/KOL/大户 wallet tracking/聪明钱监控/追踪地址, buy signal alerts/信号/大户信号, meme token scanning/扫链/新盘提醒/打狗机器人, meme metric updates/巨鲸占比/bonding curve进度. This skill contains the proprietary OKX DEX WebSocket protocol (endpoint, auth, channel names, field schemas) required to write correct code."
license: MIT
metadata:
  author: okx
  version: "1.0.0"
  homepage: "https://web3.okx.com"
---

# OKX DEX WebSocket Protocol — Unified Skill

This skill provides the complete OKX DEX WebSocket protocol reference for writing real-time streaming scripts.

## Prerequisites

This skill requires co-installation with `okx-dex-market`, `okx-dex-signal`, and `okx-dex-trenches` (all included in the onchainos-skills repository).

## When to Use

Load this skill when the user asks to:
- Write a WebSocket script/脚本 for any OKX DEX real-time data
- Build a monitoring bot/机器人 for token prices, trades, signals, or meme tokens
- Subscribe to on-chain streaming data via WebSocket
- Implement real-time price alerts, trade feeds, wallet tracking, or meme scanners

## Channel Routing

Based on the user's intent, read the corresponding protocol reference file:

### Market Data (per-token streams)

**Use when**: price monitoring, candlestick/K-line streaming, token trade feed, market cap/liquidity updates

**Read**: `../okx-dex-market/references/ws-protocol.md`

Channels:
- **`price`** — real-time token price updates
- **`price-info`** — detailed price data with market cap, volume, liquidity, holder count (max 1 push/sec)
- **`dex-token-candle{period}`** — candlestick/K-line data (27 periods from 1s to 3M)
- **`trades`** — real-time trade feed for a token (every buy/sell)

### Signal & Wallet Tracking (per-wallet streams)

**Use when**: smart money/KOL/whale wallet monitoring, buy signal alerts, address tracking

**Read**: `../okx-dex-signal/references/ws-protocol.md`

Channels:
- **`dex-market-new-signal-openapi`** — aggregated buy signal alerts from smart money/KOL/whales (single-chain)
- **`kol_smartmoney-tracker-activity`** — KOL and smart money trade feed (no wallet address needed)
- **`address-tracker-activity`** — trade feed for custom wallet addresses (up to 200 per connection; create additional connections for more)

### Meme/Trenches (meme token streams)

**Use when**: new meme token scanning, meme metric updates, bonding curve tracking

**Read**: `../okx-dex-trenches/references/ws-protocol.md`

Channels:
- **`dex-market-memepump-new-token-openapi`** — new meme token launches (full snapshot)
- **`dex-market-memepump-update-metrics-openapi`** — incremental metric updates (market cap, volume, holders, bonding curve)

## Workflow

1. Identify the user's intent and select the correct channel group above
2. Read the corresponding `ws-protocol.md` file for the full protocol spec
3. Write the script using the endpoint, auth flow, channel names, and field schemas from that file
4. Include heartbeat (ping every 25s) and reconnection logic

## Common Protocol (all channels share)

- **Endpoint**: `wss://wsdex.okx.com/ws/v6/dex`
- **Auth**: HMAC-SHA256 login required before subscribing
- **Heartbeat**: send `"ping"` every 25s, expect `"pong"`
- **Subscribe**: `{"op": "subscribe", "args": [...]}`
- **Unsubscribe**: `{"op": "unsubscribe", "args": [...]}`
