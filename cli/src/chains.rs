use anyhow::Result;

// ---------------------------------------------------------------------------
// Permit2 — canonical Uniswap Permit2 + x402 proxy addresses.
//
// Both are CREATE2 vanity deployments by the Arachnid factory, so they have
// the same address on every EVM chain. Hardcoding is intentional: any change
// here breaks all signature verification across the system.
// ---------------------------------------------------------------------------

/// Uniswap canonical Permit2 contract address (same on all EVM chains).
///
/// Buyers must `IERC20(token).approve(PERMIT2_ADDRESS, type(uint256).max)`
/// once per token before their first Permit2 payment.
pub const PERMIT2_ADDRESS: &str = "0x000000000022D473030F116dDEE9F6B43aC78BA3";

/// x402 exact-scheme Permit2 proxy address (same on all EVM chains).
///
/// The buyer's Permit2 signature names this proxy as the spender. The
/// facilitator calls `proxy.settle(...)`, which then invokes
/// `PERMIT2.permitWitnessTransferFrom(...)` to transfer tokens.
pub const X402_EXACT_PERMIT2_PROXY: &str = "0x402085c248EeA27D92E8b30b2C58ed07f9E20001";

/// x402 upto-scheme Permit2 proxy address (same on all EVM chains).
///
/// Like the exact-scheme proxy but enforces two additional on-chain
/// invariants: `msg.sender == witness.facilitator` (the facilitator
/// binding) and `settlementAmount <= permit.permitted.amount` (the cap).
/// Buyer Permit2 signatures for upto use this proxy as `spender`.
pub const X402_UPTO_PERMIT2_PROXY: &str = "0x4020e7393B728A3939659E5732F87fdd8e680002";

// ---------------------------------------------------------------------------
// EVM RPC endpoints — used by Permit2 allowance pre-check.
//
// Only X Layer is wired up in this iteration; new chains land here as the
// product expands. Until then `rpc_url_for_chain` returns None and the buyer
// flow refuses to attempt allowance pre-check on unsupported chains.
// ---------------------------------------------------------------------------

/// X Layer mainnet public RPC endpoint.
pub const XLAYER_RPC_URL: &str = "https://rpc.xlayer.tech";

/// Look up the public EVM RPC endpoint for a given chain index, if supported.
///
/// Returns `None` for chains where we haven't wired up an endpoint yet.
/// Callers in the x402 Permit2 flow should treat `None` as "cannot pre-check
/// allowance on this chain — fall back to letting settle revert on chain or
/// route through a backend allowance API".
pub fn rpc_url_for_chain(chain_index: &str) -> Option<&'static str> {
    match chain_index {
        "196" => Some(XLAYER_RPC_URL),
        _ => None,
    }
}

/// All known chain indices produced by [`resolve_chain`].
/// Used by callers that need to reject unrecognised chains early.
pub const SUPPORTED_CHAIN_INDICES: &[&str] = &[
    "1", "10", "56", "137", "195", "196", "250", "324", "501", "534352", "607", "784", "1952",
    "8453", "42161", "43114", "59144",
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
        "xlayer_test" => "1952".to_string(),
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

/// Known testnet chainIndices in the static registry, used only when the
/// dynamic chain cache does not classify the chain. Everything else recognised
/// by the registry is mainnet.
pub const TESTNET_CHAIN_INDICES: &[&str] = &["1952"];

/// Whether `chain_index` is a mainnet chain, per the chain registry.
///
/// Resolution order mirrors [`ensure_supported_chain`] / [`resolve_chain`]:
/// 1. Dynamic — a matching `chain_cache.json` entry classifies the chain (an
///    explicit `isTestnet` / `testnet` boolean wins; otherwise a `chainName`
///    containing "test" marks it a testnet).
/// 2. Static — the known-testnet set, then the `SUPPORTED_CHAIN_INDICES`
///    mainnet allowlist.
/// 3. Unknown — chains absent from both the cache and the static registry are
///    treated as NON-mainnet, so an unrecognised testnet (e.g. Sepolia) is never
///    mis-ranked ahead of a known mainnet by the mainnet-first ordering.
///    (The previous implementation used a testnet blacklist that defaulted every
///    unknown chain to mainnet — the exact bug this corrects.)
pub fn is_mainnet_chain(chain_index: &str) -> bool {
    if let Ok(cache) = crate::wallet_store::load_chain_cache() {
        if let Some(entry) = cache
            .chains
            .iter()
            .find(|c| chain_index_of(c).as_deref() == Some(chain_index))
        {
            return chain_entry_is_mainnet(entry);
        }
    }
    if TESTNET_CHAIN_INDICES.contains(&chain_index) {
        return false;
    }
    SUPPORTED_CHAIN_INDICES.contains(&chain_index)
}

/// Classify a dynamic chain-cache entry as mainnet. An explicit boolean flag
/// (`isTestnet` / `testnet`) is authoritative when the backend supplies one;
/// otherwise fall back to a `chainName` "test" heuristic (mainnet unless the
/// name signals a testnet).
fn chain_entry_is_mainnet(entry: &serde_json::Value) -> bool {
    for key in ["isTestnet", "testnet"] {
        if let Some(is_testnet) = entry.get(key).and_then(|v| v.as_bool()) {
            return !is_testnet;
        }
    }
    let name = entry
        .get("chainName")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    !name.to_lowercase().contains("test")
}

/// Resolve comma-separated chain names to comma-separated chainIndex values.
pub fn resolve_chains(names: &str) -> String {
    names
        .split(',')
        .map(|s| resolve_chain(s.trim()))
        .collect::<Vec<_>>()
        .join(",")
}

/// Loose bucketing for display / formatting: Solana split out, everything
/// else (incl. Tron / Sui / TON) falls into `"evm"`.
pub fn chain_family(chain_index: &str) -> &str {
    match chain_index {
        "501" => "solana",
        _ => "evm",
    }
}

/// `true` for chains where batch unsignedInfo may collapse to a single tx
/// (X Layer EIP-5792 smart-account semantics). Today: 196 / 1952.
///
/// Legal response length:
/// - merging chain (this fn = true) → `1` or `request_len`
/// - non-merging EVM                → exactly `request_len`
pub fn merges_batch_unsignedinfo(chain_index: &str) -> bool {
    matches!(chain_index, "196" | "1952")
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
        "1952" => "X Layer Testnet",
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
        "196" | "1952" => "OKB",
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

    #[test]
    fn merges_batch_unsignedinfo_only_xlayer() {
        // Backend contract: only X Layer (mainnet + pre-prod testnet) merges
        // batch unsignedInfo elements. Add new merging chains here as the
        // backend onboards them — and update the cmd_execute_batch response
        // length validator at the same time.
        assert!(merges_batch_unsignedinfo("196"));
        assert!(merges_batch_unsignedinfo("1952"));
        // Every other EVM chain must NOT be on this list — they all return
        // response.len() == request.len() unconditionally.
        for ci in ["1", "10", "56", "137", "8453", "42161", "59144", "534352"] {
            assert!(
                !merges_batch_unsignedinfo(ci),
                "chain {ci} must NOT be in the merging set"
            );
        }
        // Non-EVM and unknown chains: not applicable, but defensively false.
        assert!(!merges_batch_unsignedinfo("501"));
        assert!(!merges_batch_unsignedinfo("99999"));
        assert!(!merges_batch_unsignedinfo(""));
    }

    #[test]
    fn permit2_canonical_address_unchanged() {
        // Any change here breaks signature verification across the entire
        // x402 Permit2 system — both buyer and facilitator recompute the
        // EIP-712 hash using this address as `domain.verifyingContract`.
        assert_eq!(PERMIT2_ADDRESS, "0x000000000022D473030F116dDEE9F6B43aC78BA3");
    }

    #[test]
    fn x402_exact_permit2_proxy_unchanged() {
        // The proxy address is the buyer-signed `spender`. If it drifts from
        // the on-chain deployment, every Permit2 signature fails verification.
        assert_eq!(
            X402_EXACT_PERMIT2_PROXY,
            "0x402085c248EeA27D92E8b30b2C58ed07f9E20001"
        );
    }

    #[test]
    fn x402_upto_permit2_proxy_unchanged() {
        // The upto proxy is a different deployment from the exact proxy
        // (different ABI: settle takes an extra settlementAmount arg).
        // Same drift risk — pin it explicitly.
        assert_eq!(
            X402_UPTO_PERMIT2_PROXY,
            "0x4020e7393B728A3939659E5732F87fdd8e680002"
        );
    }

    #[test]
    fn rpc_url_for_chain_xlayer_only() {
        // Only X Layer is wired up for now — verify both that it works and
        // that we don't accidentally claim to support other chains.
        assert_eq!(rpc_url_for_chain("196"), Some(XLAYER_RPC_URL));
        assert_eq!(rpc_url_for_chain("8453"), None); // Base
        assert_eq!(rpc_url_for_chain("1"), None); // Ethereum
        assert_eq!(rpc_url_for_chain("1952"), None); // X Layer testnet — explicitly not wired
        assert_eq!(rpc_url_for_chain(""), None);
    }

    #[test]
    fn chain_family_loose_bucketing_documents_drift_risk() {
        // chain_family is loose — only Solana is split. Documenting current
        // behavior so a change here is intentional, not accidental.
        assert_eq!(chain_family("501"), "solana");
        assert_eq!(chain_family("1"), "evm");
        assert_eq!(chain_family("195"), "evm"); // Tron buckets to evm by default — loose bucketing only
        assert_eq!(chain_family("unknown"), "evm");
    }

    #[test]
    fn ensure_supported_chain_accepts_xlayer_testnet_offline() {
        // No chain_cache.json is written by this test, so load_chain_cache()
        // returns Ok(default) with an empty `chains` vec; the dynamic branch
        // finds nothing, then SUPPORTED_CHAIN_INDICES covers "1952".
        assert!(ensure_supported_chain("1952", "xlayer_test").is_ok());
    }

    #[test]
    fn resolve_chain_maps_xlayer_test_alias_to_1952() {
        // Regression: the `xlayer_test` alias must resolve to 1952 so the
        // documented `--chain xlayer_test` works offline. Without it the input
        // falls through to itself and ensure_supported_chain rejects it.
        assert_eq!(resolve_chain("xlayer_test"), "1952");
        assert_eq!(resolve_chain("XLAYER_TEST"), "1952"); // case-insensitive
        // And the resolved index must pass the supported-chain gate offline.
        assert!(ensure_supported_chain(&resolve_chain("xlayer_test"), "xlayer_test").is_ok());
    }

    #[test]
    fn xlayer_testnet_display_and_symbol_resolved() {
        assert_eq!(chain_display_name("1952"), "X Layer Testnet");
        assert_eq!(native_token_symbol("1952"), "OKB");
    }

    #[test]
    fn resolve_chains_handles_xlayer_test_in_list() {
        // The comma-separated resolver must also honor the alias.
        assert_eq!(resolve_chains("ethereum,xlayer_test,arbitrum"), "1,1952,42161");
    }

    #[test]
    fn is_mainnet_chain_uses_registry_not_blacklist() {
        // Known mainnet chains in SUPPORTED_CHAIN_INDICES → mainnet.
        assert!(is_mainnet_chain("1"));
        assert!(is_mainnet_chain("8453"));
        assert!(is_mainnet_chain("196"));
        // Known testnet → not mainnet.
        assert!(!is_mainnet_chain("1952"));
        // Regression guard: an unrecognised chain (e.g. Sepolia = 11155111) must NOT
        // be assumed mainnet — the old blacklist defaulted unknowns to mainnet.
        assert!(!is_mainnet_chain("11155111"));
        assert!(!is_mainnet_chain("99999"));
        assert!(!is_mainnet_chain(""));
    }

    #[test]
    fn chain_entry_is_mainnet_honors_flag_then_name() {
        // Explicit boolean flag wins.
        assert!(!chain_entry_is_mainnet(&serde_json::json!({
            "chainIndex": "8453", "chainName": "Base", "isTestnet": true
        })));
        assert!(chain_entry_is_mainnet(&serde_json::json!({
            "chainIndex": "8453", "chainName": "Base", "isTestnet": false
        })));
        // No flag → name heuristic ("test" ⇒ testnet).
        assert!(!chain_entry_is_mainnet(&serde_json::json!({
            "chainIndex": "1952", "chainName": "X Layer Testnet"
        })));
        assert!(chain_entry_is_mainnet(&serde_json::json!({
            "chainIndex": "1", "chainName": "Ethereum"
        })));
    }
}
