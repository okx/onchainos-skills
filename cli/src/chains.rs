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
