//! Defines Bitcoin outpoints and read-only continuation models.

use anyhow::{bail, Result};
use bitcoin::OutPoint;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::str::FromStr;

use crate::commands::agentic_wallet::shared::common::json::shell_arg;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct BtcOutPoint {
    pub tx_hash: String,
    pub vout_index: u32,
}

impl BtcOutPoint {
    /// Parses `txHash:voutIndex` input and returns a canonical Bitcoin outpoint.
    pub fn parse(value: &str) -> Result<Self> {
        let parsed = OutPoint::from_str(value)
            .map_err(|error| anyhow::anyhow!("invalid outpoint '{value}': {error}"))?;
        Ok(Self {
            tx_hash: parsed.txid.to_string(),
            vout_index: parsed.vout,
        })
    }

    /// Returns the outpoint in the CLI's canonical `txHash:voutIndex` form.
    pub fn canonical(&self) -> String {
        format!("{}:{}", self.tx_hash, self.vout_index)
    }

    /// Converts the outpoint into the string-valued fields expected by wallet APIs.
    pub fn to_api_value(&self) -> Value {
        serde_json::json!({
            "txHash": self.tx_hash,
            "voutIndex": self.vout_index.to_string(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadOnlyNextStep {
    CheckInscriptionStatus {
        tx_hash: Option<String>,
        order_id: Option<String>,
    },
    QueryUnavailableUtxos,
    ShowBitcoinAddress,
    RefreshBtcBalance,
    QueryBrc20TransferableUtxos {
        token_address: String,
    },
}

impl ReadOnlyNextStep {
    /// Converts one allowed read-only continuation into its response key and CLI command.
    fn build_command_entry(&self) -> Result<(&'static str, String)> {
        let pair = match self {
            Self::CheckInscriptionStatus { tx_hash, order_id } => (
                "checkInscriptionStatus",
                build_inscription_status_command(tx_hash.as_deref(), order_id.as_deref())?,
            ),
            Self::QueryUnavailableUtxos => (
                "queryUnavailableUtxos",
                "onchainos wallet utxo unavailable --chain bitcoin".to_string(),
            ),
            Self::ShowBitcoinAddress => (
                "showBitcoinAddress",
                "onchainos wallet addresses --chain bitcoin".to_string(),
            ),
            Self::RefreshBtcBalance => (
                "refreshBtcBalance",
                "onchainos wallet balance --chain bitcoin --force".to_string(),
            ),
            Self::QueryBrc20TransferableUtxos { token_address } => {
                if token_address.trim().is_empty() {
                    bail!("token address is required for a transferable BRC-20 UTXO query");
                }
                (
                    "queryBrc20TransferableUtxos",
                    format!(
                        "onchainos wallet utxo brc20-transferable --chain bitcoin --token-address {}",
                        shell_arg(token_address)
                    ),
                )
            }
        };
        ensure_read_only_command(&pair.1)?;
        Ok(pair)
    }
}

/// Builds a JSON object of validated read-only continuation commands.
pub fn next_steps(steps: impl IntoIterator<Item = ReadOnlyNextStep>) -> Result<Value> {
    let mut map = Map::new();
    for step in steps {
        let (action, command) = step.build_command_entry()?;
        map.insert(action.to_string(), Value::String(command));
    }
    Ok(Value::Object(map))
}

/// Builds an inscription-status command from a transaction hash or order ID.
fn build_inscription_status_command(
    tx_hash: Option<&str>,
    order_id: Option<&str>,
) -> Result<String> {
    match (
        tx_hash.filter(|v| !v.is_empty()),
        order_id.filter(|v| !v.is_empty()),
    ) {
        (Some(tx_hash), _) => Ok(format!(
            "onchainos wallet inscription status --chain bitcoin --tx-hash {}",
            shell_arg(tx_hash)
        )),
        (None, Some(order_id)) => Ok(format!(
            "onchainos wallet inscription status --chain bitcoin --order-id {}",
            shell_arg(order_id)
        )),
        _ => bail!("txHash or orderId is required for a status continuation"),
    }
}

/// Rejects any generated continuation command outside the read-only Bitcoin allowlist.
fn ensure_read_only_command(command: &str) -> Result<()> {
    const PREFIXES: &[&str] = &[
        "onchainos wallet inscription status --chain bitcoin ",
        "onchainos wallet utxo unavailable --chain bitcoin",
        "onchainos wallet utxo brc20-transferable --chain bitcoin ",
        "onchainos wallet addresses --chain bitcoin",
        "onchainos wallet balance --chain bitcoin",
    ];
    if PREFIXES.iter().any(|prefix| command.starts_with(prefix)) {
        return Ok(());
    }
    bail!("nextSteps rejected non-read-only command: {command}")
}

/// Recursively extracts, canonicalizes, and deduplicates outpoints from a JSON snapshot.
pub fn collect_outpoints(value: &Value) -> Vec<BtcOutPoint> {
    /// Visits nested response values and stores each valid outpoint by canonical key.
    fn visit(value: &Value, output: &mut BTreeMap<String, BtcOutPoint>) {
        match value {
            Value::Object(map) => {
                let tx_hash = ["txHash", "txhash", "txid", "txId"]
                    .iter()
                    .find_map(|key| map.get(*key).and_then(Value::as_str));
                let vout = ["voutIndex", "voutindex", "vout"]
                    .iter()
                    .find_map(|key| map.get(*key))
                    .and_then(|value| {
                        value
                            .as_u64()
                            .or_else(|| value.as_str().and_then(|raw| raw.parse().ok()))
                    });
                if let (Some(tx_hash), Some(vout)) = (tx_hash, vout) {
                    if let Ok(vout_index) = u32::try_from(vout) {
                        let point = BtcOutPoint {
                            tx_hash: tx_hash.to_string(),
                            vout_index,
                        };
                        output.insert(point.canonical(), point);
                    }
                }
                for nested in map.values() {
                    visit(nested, output);
                }
            }
            Value::Array(values) => {
                for nested in values {
                    visit(nested, output);
                }
            }
            _ => {}
        }
    }

    let mut output = BTreeMap::new();
    visit(value, &mut output);
    output.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn next_steps_only_emit_read_commands() {
        let steps = next_steps([
            ReadOnlyNextStep::QueryUnavailableUtxos,
            ReadOnlyNextStep::RefreshBtcBalance,
        ])
        .unwrap();
        assert!(steps["queryUnavailableUtxos"]
            .as_str()
            .unwrap()
            .contains("utxo unavailable --chain bitcoin"));
        assert!(steps["refreshBtcBalance"]
            .as_str()
            .unwrap()
            .contains("wallet balance"));
    }

    #[test]
    fn brc20_ready_next_step_queries_transferable_utxos() {
        let steps = next_steps([ReadOnlyNextStep::QueryBrc20TransferableUtxos {
            token_address: "btc-brc20-trac".to_string(),
        }])
        .unwrap();
        assert_eq!(
            steps["queryBrc20TransferableUtxos"],
            "onchainos wallet utxo brc20-transferable --chain bitcoin --token-address btc-brc20-trac"
        );
    }

    #[test]
    fn recursively_collects_unique_outpoints() {
        let points = collect_outpoints(&json!({
            "groups": [{"txHash": "4d3f6a7a45dbb9d3398a8f83c0219b6bedfdcd77d1de63cc09f9cfe360c553c0", "voutIndex": 1}],
            "again": {"txHash": "4d3f6a7a45dbb9d3398a8f83c0219b6bedfdcd77d1de63cc09f9cfe360c553c0", "vout": "1"}
        }));
        assert_eq!(points.len(), 1);
    }

    #[test]
    fn request_outpoint_serializes_vout_index_as_string() {
        let point = BtcOutPoint::parse(
            "4d3f6a7a45dbb9d3398a8f83c0219b6bedfdcd77d1de63cc09f9cfe360c553c0:7",
        )
        .unwrap();
        assert_eq!(point.to_api_value()["voutIndex"], "7");
    }
}
