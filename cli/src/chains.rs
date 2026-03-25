use anyhow::Result;

/// All known chain indices produced by [`resolve_chain`].
/// Used by callers that need to reject unrecognised chains early.
pub const SUPPORTED_CHAIN_INDICES: &[&str] = &[
    "1", "10", "56", "137", "195", "196", "250", "324", "501", "534352", "607", "784", "8453",
    "42161", "43114", "59144",
];

/// Validate that `chain_index` is a known chain. Returns an error that
/// includes the original user input (`raw_input`) for a friendlier message.
pub fn ensure_supported_chain(chain_index: &str, raw_input: &str) -> Result<()> {
    if !SUPPORTED_CHAIN_INDICES.contains(&chain_index) {
        anyhow::bail!(
            "unsupported chain: \"{raw_input}\" (resolved to \"{chain_index}\"). \
             Use `onchainos swap chains` to list supported chains."
        );
    }
    Ok(())
}

/// Resolve a chain name to its OKX chainIndex string.
/// Accepts both names ("ethereum", "solana") and raw chain IDs ("1", "501").
/// Returns an owned String since the input may need case conversion.
pub fn resolve_chain(name: &str) -> String {
    match name.to_lowercase().as_str() {
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
        // If already a numeric chain ID, pass through
        _ => name.to_string(),
    }
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
