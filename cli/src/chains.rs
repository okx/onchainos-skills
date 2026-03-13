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

/// Convert OKX chain index to EVM chain ID (for transaction signing).
pub fn evm_chain_id(chain_index: &str) -> Option<u64> {
    match chain_index {
        "1" => Some(1),           // Ethereum
        "56" => Some(56),         // BSC
        "137" => Some(137),       // Polygon
        "42161" => Some(42161),   // Arbitrum
        "8453" => Some(8453),     // Base
        "196" => Some(196),       // XLayer
        "43114" => Some(43114),   // Avalanche
        "10" => Some(10),         // Optimism
        "250" => Some(250),       // Fantom
        "59144" => Some(59144),   // Linea
        "534352" => Some(534352), // Scroll
        "324" => Some(324),       // zkSync
        _ => None,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── resolve_chain ────────────────────────────────────────────────────────

    #[test]
    fn resolve_chain_full_names() {
        assert_eq!(resolve_chain("ethereum"), "1");
        assert_eq!(resolve_chain("solana"), "501");
        assert_eq!(resolve_chain("bsc"), "56");
        assert_eq!(resolve_chain("polygon"), "137");
        assert_eq!(resolve_chain("arbitrum"), "42161");
        assert_eq!(resolve_chain("base"), "8453");
        assert_eq!(resolve_chain("xlayer"), "196");
        assert_eq!(resolve_chain("avalanche"), "43114");
        assert_eq!(resolve_chain("optimism"), "10");
        assert_eq!(resolve_chain("fantom"), "250");
        assert_eq!(resolve_chain("sui"), "784");
        assert_eq!(resolve_chain("tron"), "195");
        assert_eq!(resolve_chain("ton"), "607");
        assert_eq!(resolve_chain("linea"), "59144");
        assert_eq!(resolve_chain("scroll"), "534352");
        assert_eq!(resolve_chain("zksync"), "324");
    }

    #[test]
    fn resolve_chain_aliases() {
        assert_eq!(resolve_chain("eth"), "1");
        assert_eq!(resolve_chain("sol"), "501");
        assert_eq!(resolve_chain("bnb"), "56");
        assert_eq!(resolve_chain("matic"), "137");
        assert_eq!(resolve_chain("arb"), "42161");
        assert_eq!(resolve_chain("okb"), "196");
        assert_eq!(resolve_chain("avax"), "43114");
        assert_eq!(resolve_chain("op"), "10");
        assert_eq!(resolve_chain("ftm"), "250");
        assert_eq!(resolve_chain("trx"), "195");
    }

    #[test]
    fn resolve_chain_case_insensitive() {
        assert_eq!(resolve_chain("ETH"), "1");
        assert_eq!(resolve_chain("Ethereum"), "1");
        assert_eq!(resolve_chain("SOLANA"), "501");
    }

    #[test]
    fn resolve_chain_numeric_passthrough() {
        assert_eq!(resolve_chain("1"), "1");
        assert_eq!(resolve_chain("501"), "501");
        assert_eq!(resolve_chain("99999"), "99999");
    }

    #[test]
    fn resolve_chain_unknown_passthrough() {
        assert_eq!(resolve_chain("unknown-chain"), "unknown-chain");
    }

    // ── resolve_chains ───────────────────────────────────────────────────────

    #[test]
    fn resolve_chains_single() {
        assert_eq!(resolve_chains("ethereum"), "1");
    }

    #[test]
    fn resolve_chains_multiple() {
        assert_eq!(resolve_chains("eth,sol,bsc"), "1,501,56");
    }

    #[test]
    fn resolve_chains_trims_whitespace() {
        assert_eq!(resolve_chains("eth, sol, base"), "1,501,8453");
    }

    // ── evm_chain_id ─────────────────────────────────────────────────────────

    #[test]
    fn evm_chain_id_known_chains() {
        assert_eq!(evm_chain_id("1"), Some(1));
        assert_eq!(evm_chain_id("56"), Some(56));
        assert_eq!(evm_chain_id("137"), Some(137));
        assert_eq!(evm_chain_id("42161"), Some(42161));
        assert_eq!(evm_chain_id("8453"), Some(8453));
        assert_eq!(evm_chain_id("196"), Some(196));
        assert_eq!(evm_chain_id("43114"), Some(43114));
        assert_eq!(evm_chain_id("10"), Some(10));
        assert_eq!(evm_chain_id("250"), Some(250));
        assert_eq!(evm_chain_id("59144"), Some(59144));
        assert_eq!(evm_chain_id("534352"), Some(534352));
        assert_eq!(evm_chain_id("324"), Some(324));
    }

    #[test]
    fn evm_chain_id_non_evm_returns_none() {
        assert_eq!(evm_chain_id("501"), None); // Solana
        assert_eq!(evm_chain_id("784"), None); // Sui
        assert_eq!(evm_chain_id("195"), None); // Tron
        assert_eq!(evm_chain_id("607"), None); // Ton
        assert_eq!(evm_chain_id("99999"), None);
    }

    // ── chain_family ─────────────────────────────────────────────────────────

    #[test]
    fn chain_family_solana() {
        assert_eq!(chain_family("501"), "solana");
    }

    #[test]
    fn chain_family_evm_chains() {
        assert_eq!(chain_family("1"), "evm");
        assert_eq!(chain_family("56"), "evm");
        assert_eq!(chain_family("137"), "evm");
        assert_eq!(chain_family("8453"), "evm");
        assert_eq!(chain_family("784"), "evm"); // Sui treated as evm (not explicitly handled)
    }

    // ── native_token_address ─────────────────────────────────────────────────

    #[test]
    fn native_token_address_solana() {
        assert_eq!(
            native_token_address("501"),
            "11111111111111111111111111111111"
        );
    }

    #[test]
    fn native_token_address_sui() {
        assert_eq!(native_token_address("784"), "0x2::sui::SUI");
    }

    #[test]
    fn native_token_address_tron() {
        assert_eq!(
            native_token_address("195"),
            "T9yD14Nj9j7xAB4dbGeiX9h8unkKHxuWwb"
        );
    }

    #[test]
    fn native_token_address_ton() {
        assert_eq!(
            native_token_address("607"),
            "EQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAM9c"
        );
    }

    #[test]
    fn native_token_address_evm_chains() {
        let evm_addr = "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
        assert_eq!(native_token_address("1"), evm_addr);
        assert_eq!(native_token_address("56"), evm_addr);
        assert_eq!(native_token_address("137"), evm_addr);
        assert_eq!(native_token_address("8453"), evm_addr);
    }
}
