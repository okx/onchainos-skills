use anyhow::Result;

/// All known chain indices produced by [`resolve_chain`].
/// Used by callers that need to reject unrecognised chains early.
pub const SUPPORTED_CHAIN_INDICES: &[&str] = &[
    "1", "10", "56", "137", "195", "196", "250", "324", "501", "534352", "607", "784", "8453",
    "42161", "43114", "59144",
];

/// Validate that `chain_index` is a known chain. Returns an error that
/// includes the original user input (`raw_input`) for a friendlier message.
///
/// Resolution order:
/// 1. Dynamic — trust whatever's in `chain_cache.json` (no TTL, no network).
///    New chains pushed by the backend become valid here without a CLI release.
/// 2. Hardcoded — `SUPPORTED_CHAIN_INDICES` whitelist for offline / cold-start.
pub fn ensure_supported_chain(chain_index: &str, raw_input: &str) -> Result<()> {
    if let Ok(cache) = crate::wallet_store::load_chain_cache() {
        if cache
            .chains
            .iter()
            .any(|c| chain_index_of(c).as_deref() == Some(chain_index))
        {
            return Ok(());
        }
    }
    if SUPPORTED_CHAIN_INDICES.contains(&chain_index) {
        return Ok(());
    }
    anyhow::bail!(
        "unsupported chain: \"{raw_input}\" (resolved to \"{chain_index}\"). \
         Use `onchainos swap chains` to list supported chains."
    );
}

/// Resolve a chain name to its OKX chainIndex string.
/// Accepts both names ("ethereum", "solana") and raw chain IDs ("1", "501").
/// Returns an owned String since the input may need case conversion.
///
/// Resolution order:
/// 1. Dynamic — match `chainName` (case-insensitive) in `chain_cache.json`.
/// 2. Hardcoded — alias table for offline / cold-start and common shorthands.
/// 3. Pass-through — input is likely a numeric chain ID, return as-is.
pub fn resolve_chain(name: &str) -> String {
    let lower = name.to_lowercase();

    if let Ok(cache) = crate::wallet_store::load_chain_cache() {
        for c in &cache.chains {
            let cn = c
                .get("chainName")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if cn.to_lowercase() == lower {
                if let Some(idx) = chain_index_of(c) {
                    return idx;
                }
            }
        }
    }

    match lower.as_str() {
        "ethereum" | "eth" => "1".to_string(),
        "solana" | "sol" => "501".to_string(),
        "bsc" | "bnb" => "56".to_string(),
        "polygon" | "matic" => "137".to_string(),
        "arbitrum" | "arb" => "42161".to_string(),
        "base" => "8453".to_string(),
        "xlayer" | "okb" => "196".to_string(),
        "avalanche" | "avax" => "43114".to_string(),
        "optimism" | "op" => "10".to_string(),
        "fantom" | "ftm" => "250".to_string(),
        "sui" => "784".to_string(),
        "tron" | "trx" => "195".to_string(),
        "ton" => "607".to_string(),
        "linea" => "59144".to_string(),
        "scroll" => "534352".to_string(),
        "zksync" => "324".to_string(),
        "tempo" => "4217".to_string(),
        _ => name.to_string(),
    }
}

/// Extract `chainIndex` from a chain entry, accepting either string or numeric serialization.
fn chain_index_of(c: &serde_json::Value) -> Option<String> {
    c.get("chainIndex").and_then(|v| {
        v.as_str()
            .map(|s| s.to_string())
            .or_else(|| v.as_i64().map(|n| n.to_string()))
    })
}

/// Resolve comma-separated chain names to comma-separated chainIndex values.
pub fn resolve_chains(names: &str) -> String {
    names
        .split(',')
        .map(|s| resolve_chain(s.trim()))
        .collect::<Vec<_>>()
        .join(",")
}

/// Determine chain family from chain index.
pub fn chain_family(chain_index: &str) -> &str {
    match chain_index {
        "501" => "solana",
        _ => "evm",
    }
}

/// Full display name for a given chainIndex, used in user-facing strings.
/// Returns the raw chain_index for unknown chains.
pub fn chain_display_name(chain_index: &str) -> &str {
    match chain_index {
        "1" => "Ethereum",
        "10" => "Optimism",
        "56" => "BNB Chain",
        "137" => "Polygon",
        "195" => "Tron",
        "196" => "X Layer",
        "250" => "Fantom",
        "324" => "zkSync",
        "501" => "Solana",
        "534352" => "Scroll",
        "607" => "TON",
        "784" => "Sui",
        "8453" => "Base",
        "42161" => "Arbitrum One",
        "43114" => "Avalanche",
        "59144" => "Linea",
        _ => chain_index,
    }
}

/// Native token symbol for a given chainIndex, used in user-facing strings.
/// Falls back to "native token" for unknown chains.
pub fn native_token_symbol(chain_index: &str) -> &str {
    match chain_index {
        "1" | "10" | "324" | "534352" | "8453" | "42161" | "59144" => "ETH",
        "56" => "BNB",
        "137" => "MATIC",
        "195" => "TRX",
        "196" => "OKB",
        "250" => "FTM",
        "43114" => "AVAX",
        "501" => "SOL",
        "607" => "TON",
        "784" => "SUI",
        _ => "native token",
    }
}

/// Native token address for a given chainIndex.
pub fn native_token_address(chain_index: &str) -> &str {
    match chain_index {
        "501" => "11111111111111111111111111111111",
        "784" => "0x2::sui::SUI",
        "195" => "T9yD14Nj9j7xAB4dbGeiX9h8unkKHxuWwb",
        "607" => "EQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAM9c",
        // EVM chains (Ethereum, BSC, Polygon, Arbitrum, Base, etc.)
        _ => "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_display_name_covers_gas_station_chains() {
        assert_eq!(chain_display_name("1"), "Ethereum");
        assert_eq!(chain_display_name("10"), "Optimism");
        assert_eq!(chain_display_name("56"), "BNB Chain");
        assert_eq!(chain_display_name("137"), "Polygon");
        assert_eq!(chain_display_name("8453"), "Base");
        assert_eq!(chain_display_name("42161"), "Arbitrum One");
        assert_eq!(chain_display_name("59144"), "Linea");
        assert_eq!(chain_display_name("534352"), "Scroll");
    }

    #[test]
    fn chain_display_name_falls_back_to_raw_index() {
        // Unknown chain → return the raw chain_index so output is at least informative.
        assert_eq!(chain_display_name("99999"), "99999");
        assert_eq!(chain_display_name(""), "");
    }

    #[test]
    fn native_token_symbol_maps_gas_station_chains() {
        assert_eq!(native_token_symbol("1"), "ETH");
        assert_eq!(native_token_symbol("10"), "ETH");
        assert_eq!(native_token_symbol("8453"), "ETH");
        assert_eq!(native_token_symbol("42161"), "ETH");
        assert_eq!(native_token_symbol("59144"), "ETH");
        assert_eq!(native_token_symbol("534352"), "ETH");
        assert_eq!(native_token_symbol("56"), "BNB");
        assert_eq!(native_token_symbol("137"), "MATIC");
    }

    #[test]
    fn native_token_symbol_non_gas_chains() {
        assert_eq!(native_token_symbol("501"), "SOL");
        assert_eq!(native_token_symbol("196"), "OKB");
        assert_eq!(native_token_symbol("43114"), "AVAX");
    }

    #[test]
    fn native_token_symbol_unknown_fallback() {
        assert_eq!(native_token_symbol("99999"), "native token");
        assert_eq!(native_token_symbol(""), "native token");
    }

    #[test]
    fn resolve_chain_accepts_names_and_numeric_ids() {
        assert_eq!(resolve_chain("ethereum"), "1");
        assert_eq!(resolve_chain("ETH"), "1"); // case-insensitive
        assert_eq!(resolve_chain("bsc"), "56");
        assert_eq!(resolve_chain("8453"), "8453"); // numeric passthrough
        assert_eq!(resolve_chain("unknown-chain"), "unknown-chain"); // passthrough
    }
}
