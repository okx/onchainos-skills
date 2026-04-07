use serde::{Deserialize, Serialize};

// ── Channel classification ──────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChannelPattern {
    /// No extra subscribe params (e.g. `kol_smartmoney-tracker-activity`)
    Global,
    /// One subscribe arg per wallet address (`address-tracker-activity`)
    PerWallet,
    /// One subscribe arg per token pair: chainIndex + tokenContractAddress
    /// (`price`, `price-info`, `trades`, `dex-token-candle*`)
    PerToken,
    /// One subscribe arg per chain index
    /// (`dex-market-new-signal-openapi`, `dex-market-memepump-*`)
    PerChain,
}

pub fn channel_pattern(ch: &str) -> ChannelPattern {
    match ch {
        "address-tracker-activity" => ChannelPattern::PerWallet,
        "price" | "price-info" | "trades" => ChannelPattern::PerToken,
        c if c.starts_with("dex-token-candle") => ChannelPattern::PerToken,
        "dex-market-new-signal-openapi"
        | "dex-market-memepump-new-token-openapi"
        | "dex-market-memepump-update-metrics-openapi" => ChannelPattern::PerChain,
        _ => ChannelPattern::Global,
    }
}

/// Returns true if the channel is a tracker channel (uses TradeEvent schema).
pub fn is_tracker_channel(ch: &str) -> bool {
    ch == "kol_smartmoney-tracker-activity" || ch == "address-tracker-activity"
}

// ── Channel registry ────────────────────────────────────────────────────────

pub struct ChannelInfo {
    pub name: &'static str,
    pub group: &'static str,
    pub pattern: ChannelPattern,
    pub description: &'static str,
    pub params_hint: &'static str,
    pub example: &'static str,
}

pub const ALL_CHANNELS: &[ChannelInfo] = &[
    // signal
    ChannelInfo {
        name: "kol_smartmoney-tracker-activity",
        group: "signal",
        pattern: ChannelPattern::Global,
        description: "KOL and smart money aggregated trade feed",
        params_hint: "(none)",
        example: "onchainos ws start --channel kol_smartmoney-tracker-activity",
    },
    ChannelInfo {
        name: "address-tracker-activity",
        group: "signal",
        pattern: ChannelPattern::PerWallet,
        description: "Trade feed for custom wallet addresses (up to 200)",
        params_hint: "--wallet-addresses addr1,addr2,...",
        example: "onchainos ws start --channel address-tracker-activity --wallet-addresses 0xAAA,0xBBB",
    },
    ChannelInfo {
        name: "dex-market-new-signal-openapi",
        group: "signal",
        pattern: ChannelPattern::PerChain,
        description: "Aggregated buy signal alerts from smart money/KOL/whale",
        params_hint: "--chain-index 1,501",
        example: "onchainos ws start --channel dex-market-new-signal-openapi --chain-index 1,501",
    },
    // market
    ChannelInfo {
        name: "price",
        group: "market",
        pattern: ChannelPattern::PerToken,
        description: "Real-time token price updates",
        params_hint: "--token-pair chainIndex:tokenAddress",
        example: "onchainos ws start --channel price --token-pair 1:0xdac17f958d2ee523a2206206994597c13d831ec7",
    },
    ChannelInfo {
        name: "dex-token-candle{period}",
        group: "market",
        pattern: ChannelPattern::PerToken,
        description: "Candlestick/K-line data (replace {period} with 1s,1m,5m,15m,1H,4H,1D, etc.)",
        params_hint: "--token-pair chainIndex:tokenAddress",
        example: "onchainos ws start --channel dex-token-candle1m --token-pair 1:0xdac17f958d2ee523a2206206994597c13d831ec7",
    },
    // token
    ChannelInfo {
        name: "price-info",
        group: "token",
        pattern: ChannelPattern::PerToken,
        description: "Detailed price with market cap, volume, liquidity, holders",
        params_hint: "--token-pair chainIndex:tokenAddress",
        example: "onchainos ws start --channel price-info --token-pair 1:0xdac17f958d2ee523a2206206994597c13d831ec7",
    },
    ChannelInfo {
        name: "trades",
        group: "token",
        pattern: ChannelPattern::PerToken,
        description: "Real-time trade feed for a token (every buy/sell)",
        params_hint: "--token-pair chainIndex:tokenAddress",
        example: "onchainos ws start --channel trades --token-pair 1:0xdac17f958d2ee523a2206206994597c13d831ec7",
    },
    // trenches
    ChannelInfo {
        name: "dex-market-memepump-new-token-openapi",
        group: "trenches",
        pattern: ChannelPattern::PerChain,
        description: "New meme token launches",
        params_hint: "--chain-index 501",
        example: "onchainos ws start --channel dex-market-memepump-new-token-openapi --chain-index 501",
    },
    ChannelInfo {
        name: "dex-market-memepump-update-metrics-openapi",
        group: "trenches",
        pattern: ChannelPattern::PerChain,
        description: "Meme token metric updates (market cap, volume, bonding curve)",
        params_hint: "--chain-index 501",
        example: "onchainos ws start --channel dex-market-memepump-update-metrics-openapi --chain-index 501",
    },
];

// ── Token pair for per-token channels ───────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct TokenPair {
    pub chain_index: String,
    pub token_contract_address: String,
}

// ── Trade event (tracker channels only) ─────────────────────────────────────

/// A single trade event stored in events.jsonl (one JSON object per line).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TradeEvent {
    pub wallet_address: String,
    pub quote_token_symbol: String,
    pub quote_token_amount: String,
    #[serde(rename = "tokenSymbol")]
    pub token_symbol: String,
    #[serde(rename = "tokenContractAddress")]
    pub token_contract_address: String,
    #[serde(rename = "chainIndex")]
    pub chain_index: String,
    #[serde(rename = "tokenPrice")]
    pub token_price: String,
    pub market_cap: String,
    pub realized_pnl_usd: String,
    /// "1" = buy, "2" = sell
    pub trade_type: String,
    pub trade_time: String,
    /// Tracker types: 1=smart_money, 2=kol
    #[serde(
        rename = "trackerType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub tracker_type: Option<Vec<u8>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<String>,
}

/// Default channels subscribed when `--channel` is not specified.
pub const DEFAULT_CHANNELS: &[&str] = &["kol_smartmoney-tracker-activity"];

/// Persisted subscription config for a watch session.
#[derive(Debug, Serialize, Deserialize)]
pub struct WatchConfig {
    pub channels: Vec<String>,
    /// Wallet addresses for `address-tracker-activity` channel (PerWallet).
    #[serde(default)]
    pub wallet_addresses: Vec<String>,
    /// Token pairs for per-token channels: price, price-info, trades, candle (PerToken).
    #[serde(default)]
    pub token_pairs: Vec<TokenPair>,
    /// Chain indexes for per-chain channels: signal, memepump (PerChain).
    #[serde(default)]
    pub chain_indexes: Vec<String>,
    pub env: WatchEnv,
    pub created_at: u64,
    /// Auto-stop if no poll within this duration (ms). 0 = disabled.
    #[serde(default = "default_idle_timeout_ms")]
    pub idle_timeout_ms: u64,
}

fn default_idle_timeout_ms() -> u64 {
    30 * 60 * 1000 // 30 minutes
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum WatchEnv {
    Pre,
    Prod,
}

/// Daemon status written to the status file every 10s.
#[derive(Debug, Clone, PartialEq)]
pub enum DaemonState {
    Running,
    Disconnected(String),
    Reconnecting,
    Stopped,
    Crashed,
}

impl DaemonState {
    /// Parse from status file content: "{state}|{timestamp_ms}[|{reason}]"
    pub fn from_status_line(line: &str, now_ms: u64) -> Self {
        let parts: Vec<&str> = line.trim().splitn(3, '|').collect();
        if parts.len() < 2 {
            return DaemonState::Crashed;
        }
        // "stopped" is a terminal state — not subject to staleness check
        if parts[0] == "stopped" {
            return DaemonState::Stopped;
        }
        let ts: u64 = parts[1].parse().unwrap_or(0);
        if now_ms.saturating_sub(ts) > 60_000 {
            return DaemonState::Crashed;
        }
        match parts[0] {
            "running" => DaemonState::Running,
            "disconnected" => {
                let reason = parts.get(2).unwrap_or(&"unknown").to_string();
                DaemonState::Disconnected(reason)
            }
            "reconnecting" => DaemonState::Reconnecting,
            _ => DaemonState::Crashed,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            DaemonState::Running => "running",
            DaemonState::Disconnected(_) => "disconnected",
            DaemonState::Reconnecting => "reconnecting",
            DaemonState::Stopped => "stopped",
            DaemonState::Crashed => "crashed",
        }
    }
}
