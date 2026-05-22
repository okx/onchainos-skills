//! Shared token alias resolution + chain-aware address format validation.
//!
//! Every command that accepts a user-supplied token address (`swap`,
//! `wallet send --contract-token`, `strategy create-limit --from-token /
//! --to-token`) routes through this module so the three commands behave
//! identically: known aliases (`usdc`, `sol`, `native`, ...) resolve to a
//! canonical CA, full contract addresses pass through, and anything else
//! (typo / ticker / garbage) is rejected with a friendly message before
//! the request leaves the CLI.

use std::collections::HashMap;
use std::sync::LazyLock;

use anyhow::{bail, Result};

// ─────────────────────────────────────────────────────────────────────
// TOKEN_MAP — chain index → alias → canonical contract address
//
// Matching is case-insensitive. Special aliases:
//   - "native" → native token address per chain
//   - Error CA auto-correction (e.g. wSOL SPL address → native SOL address)
// ─────────────────────────────────────────────────────────────────────

static TOKEN_MAP: LazyLock<HashMap<&str, HashMap<&str, &str>>> = LazyLock::new(|| {
    HashMap::from([
        // Solana (501)
        ("501", HashMap::from([
            ("sol", "11111111111111111111111111111111"),
            ("native", "11111111111111111111111111111111"),
            ("usdc", "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"),
            ("usdt", "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"),
            // Error CA corrections: wSOL SPL token / typo
            ("so11111111111111111111111111111111111111112", "11111111111111111111111111111111"),
            ("so11111111111111111111111111111111111111111", "11111111111111111111111111111111"),
        ])),
        // Ethereum (1)
        ("1", HashMap::from([
            ("eth", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("native", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("usdc", "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"),
            ("usdt", "0xdac17f958d2ee523a2206206994597c13d831ec7"),
            ("wbtc", "0x2260fac5e5542a773aa44fbcfedf7c193bc2c599"),
            ("dai", "0x6b175474e89094c44da98b954eedeac495271d0f"),
            ("weth", "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"),
        ])),
        // Base (8453)
        ("8453", HashMap::from([
            ("eth", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("native", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("usdc", "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913"),
            ("weth", "0x4200000000000000000000000000000000000006"),
            ("usdbc", "0xd9aaec86b65d86f6a7b5b1b0c42ffa531710b6ca"),
        ])),
        // BSC (56)
        ("56", HashMap::from([
            ("bnb", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("native", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("usdt", "0x55d398326f99059ff775485246999027b3197955"),
            ("usdc", "0x8ac76a51cc950d9822d68b83fe1ad97b32cd580d"),
            ("wbnb", "0xbb4cdb9cbd36b01bd1cbaebf2de08d9173bc095c"),
            ("weth", "0x2170ed0880ac9a755fd29b2688956bd959f933f8"),
            ("btcb", "0x7130d2a12b9bcbfae4f2634d864a1ee1ce3ead9c"),
        ])),
        // Arbitrum (42161)
        ("42161", HashMap::from([
            ("eth", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("native", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("usdc", "0xaf88d065e77c8cc2239327c5edb3a432268e5831"),
            ("usdt", "0xfd086bc7cd5c481dcc9c85ebe478a1c0b69fcbb9"),
            ("weth", "0x82af49447d8a07e3bd95bd0d56f35241523fbab1"),
        ])),
        // Polygon (137)
        ("137", HashMap::from([
            ("matic", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("pol", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("native", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("usdc", "0x3c499c542cef5e3811e1192ce70d8cc03d5c3359"),
            ("usdt0", "0xc2132d05d31c914a87c6611c10748aeb04b58e8f"),
            ("weth", "0x7ceb23fd6bc0add59e62ac25578270cff1b9f619"),
            ("wmatic", "0x0d500b1d8e8ef31e21c99d1db9a6444d3adf1270"),
            ("wpol", "0x0d500b1d8e8ef31e21c99d1db9a6444d3adf1270"),
        ])),
        // Optimism (10)
        ("10", HashMap::from([
            ("eth", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("native", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("usdc", "0x0b2c639c533813f4aa9d7837caf62653d097ff85"),
            ("usdt", "0x94b008aa00579c1307b0ef2c499ad98a8ce58e58"),
            ("weth", "0x4200000000000000000000000000000000000006"),
            ("op", "0x4200000000000000000000000000000000000042"),
        ])),
        // Avalanche (43114)
        ("43114", HashMap::from([
            ("avax", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("native", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("usdc", "0xb97ef9ef8734c71904d8002f8b6bc66dd9c48a6e"),
            ("usdt", "0x9702230a8ea53601f5cd2dc00fdbc13d4df4a8c7"),
            ("wavax", "0xb31f66aa3c1e785363f0875a1b74e27b85fd66c7"),
            ("weth.e", "0x49d5c2bdffac6ce2bfdb6640f4f80f226bc10bab"),
        ])),
        // XLayer (196)
        ("196", HashMap::from([
            ("okb", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("native", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("usdc", "0x74b7f16337b8972027f6196a17a631ac6de26d22"),
            ("xlayer_usdt", "0x1e4a5963abfd975d8c9021ce480b42188849d41d"),
            ("usdt0", "0x779ded0c9e1022225f8e0630b35a9b54be713736"),
            ("usdt", "0x779ded0c9e1022225f8e0630b35a9b54be713736"),
            ("weth", "0x5a77f1443d16ee5761d310e38b62f77f726bc71c"),
            ("wokb", "0xe538905cf8410324e03a5a23c1c177a474d59b2b"),
        ])),
        // X Layer Testnet (1952)
        ("1952", HashMap::from([
            ("okb", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("native", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("usdc", "0xcb8bf24c6ce16ad21d707c9505421a17f2bec79d"),
            ("usdt", "0x9e29b3aada05bf2d2c827af80bd28dc0b9b4fb0c"),
            ("usdg", "0xa78e2baabaf5c4f36b7fc394725deb68d332eec1"),
        ])),
        // Linea (59144)
        ("59144", HashMap::from([
            ("eth", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("native", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("usdc", "0x176211869ca2b568f2a7d4ee941e073a821ee1ff"),
            ("usdt", "0xa219439258ca9da29e9cc4ce5596924745e12b93"),
            ("weth", "0xe5d7c2a44ffddf6b295a15c148167daaaf5cf34f"),
        ])),
        // Scroll (534352)
        ("534352", HashMap::from([
            ("eth", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("native", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("usdc", "0x06efdbff2a14a7c8e15944d1f4a48f9f95f663a4"),
            ("usdt", "0xf55bec9cafdbe8730f096aa55dad6d22d44099df"),
            ("weth", "0x5300000000000000000000000000000000000004"),
        ])),
        // zkSync (324)
        ("324", HashMap::from([
            ("eth", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("native", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("weth", "0x5aea5775959fbc2557cc8789bc1bf90a239d9a91"),
            ("usdt", "0x493257fd37edb34451f62edf8d2a0c418852ba4c"),
        ])),
        // Fantom (250)
        ("250", HashMap::from([
            ("ftm", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("native", "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            ("wftm", "0x21be370d5312f44cb42ce377bc9b8a0cef1a4c83"),
        ])),
        // Tron (195)
        ("195", HashMap::from([
            ("trx", "T9yD14Nj9j7xAB4dbGeiX9h8unkKHxuWwb"),
            ("native", "T9yD14Nj9j7xAB4dbGeiX9h8unkKHxuWwb"),
            ("usdt", "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t"),
            ("wtrx", "TNUC9Qb1rRpS5CbWLmNMxXBjyFoydXjWFR"),
            ("eth", "THb4CqiFdwNHsWsQCs4JhzwjMWys4aqCbF"),
        ])),
        // Sui (784)
        ("784", HashMap::from([
            ("sui", "0x2::sui::SUI"),
            ("native", "0x2::sui::SUI"),
            ("wusdc", "0x5d4b302506645c37ff133b98c4b50a5ae14841659738d6d733d59d0d217a93bf::coin::COIN"),
            ("wusdt", "0xc060006111016b8a020ad5b33834984a437aaa7d3c74c18e09a95d48aceab08c::coin::COIN"),
        ])),
    ])
});

/// Resolve a token address using the chain-specific mapping table.
/// Matching is case-insensitive. If no match is found, returns the original
/// value unchanged.
pub fn resolve_token_address(chain_index: &str, token: &str) -> String {
    let key = token.to_ascii_lowercase();
    if let Some(chain_map) = TOKEN_MAP.get(chain_index) {
        if let Some(&resolved) = chain_map.get(key.as_str()) {
            return resolved.to_string();
        }
    }
    token.to_string()
}

/// Validate that `token` looks like a contract address for the given chain.
/// Called after `resolve_token_address` so it inspects the actual address
/// (alias-resolved or user-supplied).
///
/// Note: chain_family() is a binary "solana" / "evm" function and classifies
/// Tron (195), TON (607), and Sui (784) as "evm" for historical reasons.
/// Those chains have their own address formats, so we skip format validation
/// for them and only check genuine Solana vs. EVM chains.
pub fn validate_address_for_chain(
    chain_index: &str,
    token: &str,
    label: &str,
) -> Result<()> {
    match chain_index {
        // Solana: must not be a 0x-prefixed EVM address, and must be 32-44 chars (base58).
        "501" => {
            if token.starts_with("0x") || token.starts_with("0X") {
                bail!(
                    "--{label} looks like an EVM address (0x…) but chain is Solana. \
                     Solana uses base58 addresses (e.g. EPjFWdd5...wyTDt1v). \
                     Did you mean to use a different chain?"
                );
            }
            if token.len() < 32 || token.len() > 44 {
                bail!(
                    "--{label} is not a valid Solana address: expected 32-44 base58 characters, got {} characters (\"{}\")",
                    token.len(), token
                );
            }
            // Base58 alphabet excludes: 0, O, I, l
            if !token
                .chars()
                .all(|c| c.is_ascii_alphanumeric() && !matches!(c, '0' | 'O' | 'I' | 'l'))
            {
                bail!(
                    "--{label} is not a valid Solana address: contains characters outside base58 alphabet (\"{}\")",
                    token
                );
            }
        }
        // Tron / TON / Sui — their native address formats differ from both EVM and Solana;
        // skip format validation and let the API handle address errors.
        "195" | "607" | "784" => {}
        // EVM chains: must start with 0x and be 42 characters long.
        _ => {
            if !token.starts_with("0x")
                && !token.starts_with("0X")
                && token.len() >= 32
                && token.len() <= 44
                && token.chars().all(|c| c.is_ascii_alphanumeric())
                && token.chars().any(|c| c.is_ascii_uppercase())
            {
                bail!(
                    "--{label} looks like a Solana/base58 address but chain is EVM (chainIndex={chain_index}). \
                     EVM addresses start with 0x (e.g. 0xa0b869...606eb48). \
                     Did you mean to use --chain solana?"
                );
            }
            // EVM addresses must be 0x/0X + 40 hex digits = 42 characters
            let is_valid_evm = (token.starts_with("0x") || token.starts_with("0X"))
                && token.len() == 42
                && token[2..].chars().all(|c| c.is_ascii_hexdigit());
            if !is_valid_evm {
                bail!(
                    "--{label} is not a valid EVM address: expected 0x + 40 hex digits, got \"{}\"",
                    token
                );
            }
        }
    }
    Ok(())
}

/// One-shot pipeline: alias → CA → format check. Returns the resolved
/// address ready to send to BE, or `Err` with a friendly message when the
/// input is neither a known alias nor an address-shaped string.
///
/// Use this from any command that accepts a user-supplied token address —
/// it is the single entry point that keeps swap / send / strategy in sync.
pub fn resolve_and_validate(chain_index: &str, raw: &str, label: &str) -> Result<String> {
    let resolved = resolve_token_address(chain_index, raw);
    validate_address_for_chain(chain_index, &resolved, label)?;
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── resolve_token_address ──────────────────────────────────────────

    #[test]
    fn resolve_known_alias_returns_canonical_ca() {
        assert_eq!(
            resolve_token_address("501", "usdc"),
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
        );
        assert_eq!(
            resolve_token_address("1", "USDT"),
            "0xdac17f958d2ee523a2206206994597c13d831ec7"
        );
        assert_eq!(
            resolve_token_address("501", "native"),
            "11111111111111111111111111111111"
        );
    }

    #[test]
    fn resolve_unknown_token_returns_input_unchanged() {
        assert_eq!(resolve_token_address("501", "aaa"), "aaa");
        assert_eq!(
            resolve_token_address("1", "0xdac17f958d2ee523a2206206994597c13d831ec7"),
            "0xdac17f958d2ee523a2206206994597c13d831ec7"
        );
    }

    #[test]
    fn resolve_is_case_insensitive() {
        assert_eq!(resolve_token_address("501", "USDC"), resolve_token_address("501", "usdc"));
        assert_eq!(resolve_token_address("1", "Usdt"), resolve_token_address("1", "usdt"));
    }

    // ── validate_address_for_chain — EVM ───────────────────────────────

    #[test]
    fn validate_evm_valid() {
        assert!(validate_address_for_chain(
            "1",
            "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
            "from"
        )
        .is_ok());
        assert!(validate_address_for_chain(
            "1",
            "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
            "wallet"
        )
        .is_ok());
        assert!(validate_address_for_chain(
            "56",
            "0x55d398326f99059ff775485246999027b3197955",
            "token"
        )
        .is_ok());
    }

    #[test]
    fn validate_evm_rejects_solana_address() {
        let sol_addr = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
        assert!(validate_address_for_chain("1", sol_addr, "from").is_err());
        assert!(validate_address_for_chain("56", sol_addr, "token").is_err());
        assert!(validate_address_for_chain("8453", sol_addr, "wallet").is_err());
    }

    #[test]
    fn validate_evm_rejects_short_address() {
        assert!(validate_address_for_chain("1", "0xabc123", "from").is_err());
        assert!(validate_address_for_chain("56", "0x1234", "token").is_err());
    }

    #[test]
    fn validate_evm_rejects_long_address() {
        assert!(validate_address_for_chain(
            "1",
            "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48a",
            "from"
        )
        .is_err());
    }

    #[test]
    fn validate_evm_rejects_ticker_and_garbage() {
        assert!(validate_address_for_chain("196", "WIF", "to").is_err());
        assert!(validate_address_for_chain("1", "USDC", "from").is_err());
        assert!(validate_address_for_chain("56", "BNB", "to").is_err());
        assert!(validate_address_for_chain("1", "hello", "from").is_err());
        assert!(validate_address_for_chain("1", "native", "to").is_err());
        assert!(validate_address_for_chain("196", "", "from").is_err());
        assert!(validate_address_for_chain("1", "12345", "to").is_err());
    }

    #[test]
    fn validate_evm_rejects_non_hex_42_chars() {
        assert!(validate_address_for_chain(
            "1",
            "0xGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG",
            "from"
        )
        .is_err());
    }

    // ── validate_address_for_chain — Solana ────────────────────────────

    #[test]
    fn validate_solana_valid() {
        assert!(validate_address_for_chain(
            "501",
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            "from"
        )
        .is_ok());
        assert!(validate_address_for_chain(
            "501",
            "11111111111111111111111111111111",
            "wallet"
        )
        .is_ok());
    }

    #[test]
    fn validate_solana_rejects_evm_address() {
        assert!(validate_address_for_chain(
            "501",
            "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
            "from"
        )
        .is_err());
        assert!(validate_address_for_chain(
            "501",
            "0x1234567890abcdef1234567890abcdef12345678",
            "wallet"
        )
        .is_err());
    }

    #[test]
    fn validate_solana_length_boundary() {
        let addr_32 = "1".repeat(32);
        assert!(validate_address_for_chain("501", &addr_32, "from").is_ok());
        let addr_44 = "A".repeat(44);
        assert!(validate_address_for_chain("501", &addr_44, "from").is_ok());
        let addr_31 = "1".repeat(31);
        assert!(validate_address_for_chain("501", &addr_31, "from").is_err());
        let addr_45 = "A".repeat(45);
        assert!(validate_address_for_chain("501", &addr_45, "from").is_err());
    }

    #[test]
    fn validate_solana_rejects_non_base58_chars() {
        let with_zero = format!("{}0", "A".repeat(31));
        assert!(validate_address_for_chain("501", &with_zero, "from").is_err());
        let with_o = format!("{}O", "A".repeat(31));
        assert!(validate_address_for_chain("501", &with_o, "from").is_err());
        let with_i_upper = format!("{}I", "A".repeat(31));
        assert!(validate_address_for_chain("501", &with_i_upper, "from").is_err());
        let with_l_lower = format!("{}l", "A".repeat(31));
        assert!(validate_address_for_chain("501", &with_l_lower, "from").is_err());
    }

    // ── validate_address_for_chain — Tron / Sui (skip) ─────────────────

    #[test]
    fn validate_tron_skips() {
        assert!(
            validate_address_for_chain("195", "T9yD14Nj9j7xAB4dbGeiX9h8unkKHxuWwb", "from").is_ok()
        );
        assert!(validate_address_for_chain("195", "0xabc123", "wallet").is_ok());
    }

    #[test]
    fn validate_sui_skips() {
        assert!(validate_address_for_chain("784", "0x2::sui::SUI", "from").is_ok());
    }

    // ── validate_address_for_chain — label propagates to error ─────────

    #[test]
    fn validate_error_includes_label() {
        let err = validate_address_for_chain(
            "1",
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            "wallet",
        )
        .unwrap_err();
        assert!(err.to_string().contains("--wallet"));
    }

    // ── resolve_and_validate (one-shot) ────────────────────────────────

    #[test]
    fn resolve_and_validate_alias_returns_ca() {
        let resolved =
            resolve_and_validate("501", "usdc", "to-token").expect("usdc alias resolves");
        assert_eq!(resolved, "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
    }

    #[test]
    fn resolve_and_validate_full_ca_passes_through() {
        let resolved = resolve_and_validate(
            "1",
            "0xdac17f958d2ee523a2206206994597c13d831ec7",
            "from-token",
        )
        .expect("CA passes through");
        assert_eq!(resolved, "0xdac17f958d2ee523a2206206994597c13d831ec7");
    }

    #[test]
    fn resolve_and_validate_rejects_garbage() {
        let err =
            resolve_and_validate("501", "aaa", "to-token").expect_err("aaa rejected");
        assert!(err.to_string().contains("to-token"));
        assert!(err.to_string().contains("not a valid Solana address"));
    }

    #[test]
    fn resolve_and_validate_rejects_unknown_symbol_on_evm() {
        let err = resolve_and_validate("1", "usdcc", "from-token")
            .expect_err("usdcc rejected (typo)");
        assert!(err.to_string().contains("from-token"));
    }
}
