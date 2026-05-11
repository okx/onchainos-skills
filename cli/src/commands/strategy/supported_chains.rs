//! Strategy-specific chain whitelist.
//!
//! Phase 1 BE only supports the 6 chains in `BaseOrderChainInfoEnum`
//! (`ChainInfoEnum`). We validate the user's `--chain-id` against this
//! whitelist BEFORE calling BE so the user gets a friendly error instead
//! of round-tripping to BE for code 10106 `CHAIN_NOT_SUPPORT_ERROR`.
//!
//! The global `cli/src/chains.rs` registry (16+ chains) is intentionally
//! NOT used here — it's superset of what strategy supports. Do not call
//! `chains::ensure_supported_chain` from strategy code.

use anyhow::{bail, Result};

/// Chain identifier (numeric chainIndex string) + canonical EN name.
/// Mirrors BE `BaseOrderChainInfoEnum`(`ChainInfoEnum`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrategyChain {
    pub chain_index: &'static str,
    pub name: &'static str,
}

/// 6 chains supported by Phase 1 strategy orders.
/// Source: BE `ChainInfoEnum.java` (Lark wiki / handed over via screenshot
/// 2026-05-08). Ordered by `chainIndex` ascending to match the SKILL.md
/// canonical listing.
pub const SUPPORTED_STRATEGY_CHAINS: &[StrategyChain] = &[
    StrategyChain { chain_index: "1",     name: "Ethereum" },
    StrategyChain { chain_index: "56",    name: "BSC"      },
    StrategyChain { chain_index: "196",   name: "X Layer"  },
    StrategyChain { chain_index: "501",   name: "Solana"   },
    StrategyChain { chain_index: "8453",  name: "Base"     },
    StrategyChain { chain_index: "42161", name: "Arbitrum" },
];

/// Validate `chain_index` (already resolved to numeric string) is one of
/// the 6 supported strategy chains. Returns `Err` with a friendly message
/// listing the allowed chains for any other input.
///
/// `raw_input` is the original user-typed value (alias or numeric) used
/// to make the error message clearer.
pub fn ensure_strategy_chain(chain_index: &str, raw_input: &str) -> Result<()> {
    if SUPPORTED_STRATEGY_CHAINS
        .iter()
        .any(|c| c.chain_index == chain_index)
    {
        return Ok(());
    }
    let supported_list = SUPPORTED_STRATEGY_CHAINS
        .iter()
        .map(|c| format!("{} ({})", c.name, c.chain_index))
        .collect::<Vec<_>>()
        .join(", ");
    bail!(
        "chain \"{raw_input}\" (resolved to chainIndex {chain_index}) is not supported \
         for strategy orders. Phase 1 supports: {supported_list}"
    );
}

/// Solana branch marker — strategy signing path uses ed25519-over-hex for
/// Solana and EIP-191 for everything else. Centralise the comparison so
/// other modules don't hardcode "501" string literals.
pub fn is_solana(chain_index: &str) -> bool {
    chain_index == "501"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_strategy_chain_accepts_all_six() {
        for c in SUPPORTED_STRATEGY_CHAINS {
            assert!(
                ensure_strategy_chain(c.chain_index, c.name).is_ok(),
                "expected {} to be supported",
                c.chain_index
            );
        }
    }

    #[test]
    fn ensure_strategy_chain_rejects_polygon() {
        let err = ensure_strategy_chain("137", "polygon").unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("polygon"), "msg should echo raw input: {msg}");
        assert!(msg.contains("137"), "msg should include resolved id: {msg}");
        assert!(msg.contains("Solana"), "msg should list supported chains: {msg}");
    }

    #[test]
    fn ensure_strategy_chain_rejects_optimism_and_linea() {
        // Optimism (10) and Linea (59144) are in the global registry but
        // not in strategy's whitelist.
        assert!(ensure_strategy_chain("10", "optimism").is_err());
        assert!(ensure_strategy_chain("59144", "linea").is_err());
    }

    #[test]
    fn is_solana_only_true_for_501() {
        assert!(is_solana("501"));
        assert!(!is_solana("1"));
        assert!(!is_solana("42161"));
        assert!(!is_solana(""));
    }
}
