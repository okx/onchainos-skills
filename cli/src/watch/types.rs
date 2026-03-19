use serde::{Deserialize, Serialize};

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
    #[serde(rename = "trackerType", default, skip_serializing_if = "Option::is_none")]
    pub tracker_type: Option<Vec<u8>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChannelVisibility {
    Public,
    Private,
}

pub struct ChannelDef {
    pub name: &'static str,
    pub visibility: ChannelVisibility,
}

/// All known channels. Used as the default when --channel is not specified.
pub const ALL_CHANNELS: &[ChannelDef] = &[
    ChannelDef { name: "kol_smartmoney-tracker-activity", visibility: ChannelVisibility::Public },
];

/// Persisted subscription config for a watch session.
#[derive(Debug, Serialize, Deserialize)]
pub struct WatchConfig {
    pub channels: Vec<String>,
    /// Wallet addresses for `address-tracker-activity` channel.
    /// Each address becomes a separate subscription arg: { "channel": "address-tracker-activity", "walletAddress": "0x..." }
    #[serde(default)]
    pub wallet_addresses: Vec<String>,
    pub env: WatchEnv,
    pub created_at: u64,
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
            "stopped" => DaemonState::Stopped,
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
