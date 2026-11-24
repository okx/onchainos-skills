//! Strategy chain whitelist (Phase 1: 6 chains per BE `ChainInfoEnum`).
//! Validated pre-BE so unsupported chains fail fast instead of round-tripping 10106.
//! Do NOT fall back to the global `crate::chains` registry — its superset
//! includes chains BE rejects.

use anyhow::{bail, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrategyChain {
    pub chain_index: &'static str,
    pub name: &'static str,
}

/// Ascending by chainIndex (matches SKILL.md listing).
pub const SUPPORTED_STRATEGY_CHAINS: &[StrategyChain] = &[
    StrategyChain { chain_index: "1",     name: "Ethereum" },
    StrategyChain { chain_index: "56",    name: "BSC"      },
    StrategyChain { chain_index: "196",   name: "X Layer"  },
    StrategyChain { chain_index: "501",   name: "Solana"   },
    StrategyChain { chain_index: "8453",  name: "Base"     },
    StrategyChain { chain_index: "42161", name: "Arbitrum" },
];

/// Bail if `chain_index` isn't in `SUPPORTED_STRATEGY_CHAINS`. `raw_input`
/// is the user-typed value (echoed in the error message for clarity).
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

/// Solana branch for the signing path (ed25519-over-hex vs EIP-191).
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
