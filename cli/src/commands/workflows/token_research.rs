/// Token Research
///
/// Step 1: delegates to token::fetch_report() — the PRD §3.1 composite command
///   (token info + price-info + advanced-info + security scan in one call)
///   PRD: single sub-call failure → field null, rest continues
///   PRD: all Step 1 calls fail    → return error
/// Step 2 (parallel): holders + cluster overview + top traders + signal list
///   cluster-overview may 500 for brand-new tokens → treated as null, skipped gracefully
/// Step 3 (parallel, conditional): launchpad enrichment only when protocolId non-empty
///   if advanced-info itself failed (null), protocolId absent → Step 3 skipped safely
use anyhow::Result;
use serde_json::{json, Value};

use crate::chains;
use crate::client::ApiClient;
use crate::commands::{memepump, signal, token};
use crate::output;

use super::{ok_or_null, Context};

pub(crate) async fn fetch_and_assemble(
    client: &mut ApiClient,
    address: &str,
    chain_index: &str,
) -> Result<Value> {
    // ── Step 1: core data via token report composite command ──────────
    let report = token::fetch_report(client, address, chain_index).await?;

    // Extract individual values for Step 3 condition check and assemble()
    let info = report["info"].clone();
    let price = report["priceInfo"].clone();
    let advanced = report["advancedInfo"].clone();
    let security = report["security"].clone();

    // ── Step 2: on-chain structure (parallel) ───────────────────────
    let (mut c1, mut c2, mut c3) = (client.clone(), client.clone(), client.clone());
    let addr = address.to_string();
    let (holders, cluster, top_traders, signals) = tokio::join!(
        token::fetch_holders(client, address, chain_index, None, Some("100"), None),
        token::fetch_cluster_by_address(
            &mut c1,
            "/api/v6/dex/market/token/cluster/overview",
            address,
            chain_index,
        ),
        token::fetch_top_trader(&mut c2, address, chain_index, None, Some("20"), None),
        signal::fetch_list(
            &mut c3,
            chain_index,
            None,
            None,
            None,
            None,
            None,
            Some(addr),
            None,
            None,
            None,
            None,
            None,
            None,
        ),
    );
    let holders = ok_or_null(holders);
    let cluster = ok_or_null(cluster);
    let top_traders = ok_or_null(top_traders);
    let signals = ok_or_null(signals);

    // ── Step 3: launchpad supplement (parallel, conditional) ─────────
    let launchpad = if is_launchpad_token(&advanced) {
        let (mut c4, mut c5, mut c6) = (client.clone(), client.clone(), client.clone());
        let (details, dev_info, bundle_info, similar) = tokio::join!(
            memepump::fetch_by_address(
                client,
                "/api/v6/dex/market/memepump/tokenDetails",
                address,
                chain_index,
            ),
            memepump::fetch_by_address(
                &mut c4,
                "/api/v6/dex/market/memepump/tokenDevInfo",
                address,
                chain_index,
            ),
            memepump::fetch_by_address(
                &mut c5,
                "/api/v6/dex/market/memepump/tokenBundleInfo",
                address,
                chain_index,
            ),
            memepump::fetch_by_address(
                &mut c6,
                "/api/v6/dex/market/memepump/similarToken",
                address,
                chain_index,
            ),
        );
        json!({
            "tokenDetails":  ok_or_null(details),
            "devInfo":       ok_or_null(dev_info),
            "bundleInfo":    ok_or_null(bundle_info),
            "similarTokens": ok_or_null(similar),
        })
    } else {
        Value::Null
    };

    assemble(
        address,
        chain_index,
        info,
        price,
        advanced,
        security,
        holders,
        cluster,
        top_traders,
        signals,
        launchpad,
    )
}

/// Search tokens by symbol/name and return the top 5 matches for user selection.
/// Output includes a numbered list so the calling agent can present choices.
pub async fn search_and_select(
    client: &mut ApiClient,
    query: &str,
    chain_index: &str,
) -> Result<Value> {
    let results = token::fetch_search(client, query, chain_index, Some("5"), None).await?;
    let items = results.as_array().cloned().unwrap_or_default();

    if items.is_empty() {
        anyhow::bail!(
            "token-research: no tokens found for query '{}' on chain {}",
            query,
            chain_index
        );
    }

    let candidates: Vec<Value> = items
        .into_iter()
        .enumerate()
        .map(|(i, t)| {
            json!({
                "index": i + 1,
                "symbol": t.get("tokenSymbol").or_else(|| t.get("symbol")).cloned().unwrap_or(Value::Null),
                "name": t.get("tokenName").or_else(|| t.get("name")).cloned().unwrap_or(Value::Null),
                "address": t.get("tokenContractAddress").or_else(|| t.get("address")).cloned().unwrap_or(Value::Null),
                "chain": t.get("chainIndex").or_else(|| t.get("chain")).cloned().unwrap_or(Value::Null),
                "price": t.get("price").cloned().unwrap_or(Value::Null),
                "marketCap": t.get("marketCap").cloned().unwrap_or(Value::Null),
                "logoUrl": t.get("logoUrl").cloned().unwrap_or(Value::Null),
            })
        })
        .collect();

    Ok(json!({
        "workflow": "token-research",
        "step": "select-token",
        "query": query,
        "message": "Multiple tokens found. Please select one by number (1-5) to continue the full research workflow.",
        "candidates": candidates,
    }))
}

pub async fn run(
    ctx: &Context,
    address: Option<&str>,
    query: Option<&str>,
    chain: Option<String>,
) -> Result<()> {
    anyhow::ensure!(
        address.is_some() || query.is_some(),
        "token-research requires --address or --query"
    );

    let mut client = ctx.client_async().await?;
    let chain_index = chain
        .as_deref()
        .map(|c| chains::resolve_chain(c).to_string())
        .unwrap_or_else(|| ctx.chain_index_or("solana"));

    // If query is provided (symbol/name), do a search and return candidates for selection
    if let Some(q) = query {
        if address.is_none() {
            let result = search_and_select(&mut client, q, &chain_index).await?;
            output::success(result);
            return Ok(());
        }
    }

    // Direct address path — run the full workflow
    let addr = address.unwrap();
    let result = fetch_and_assemble(&mut client, addr, &chain_index).await?;
    output::success(result);
    Ok(())
}

/// Pure assembly function — applies all PRD logic on pre-fetched data.
/// Testable without any HTTP calls.
///
/// PRD rules applied here:
/// - all Step 1 fields null → propagate error (全部失败 → 返回错误)
/// - individual nulls → preserved in output (单个失败 → 对应字段 null)
/// - launchpad: null when Step 3 was skipped; object when it ran
#[allow(clippy::too_many_arguments)]
pub(crate) fn assemble(
    address: &str,
    chain_index: &str,
    // Step 1
    info: Value,
    price: Value,
    advanced: Value,
    security: Value,
    // Step 2
    holders: Value,
    cluster: Value,
    top_traders: Value,
    signals: Value,
    // Step 3 (pre-computed: null when skipped)
    launchpad: Value,
) -> Result<Value> {
    // PRD: all Step 1 core calls failed → return error.
    //
    // Note: unreachable via fetch_and_assemble — token::fetch_report already
    // bails with the same all-fail error, so by the time we get here all four
    // fields cannot be null together. Kept as a defence-in-depth check for
    // unit tests and any direct callers of assemble().
    if all_null(&[&info, &price, &advanced, &security]) {
        anyhow::bail!(
            "token-research: all Step 1 sub-calls failed for address {} on chain {}",
            address,
            chain_index
        );
    }

    Ok(json!({
        "workflow": "token-research",
        "address":  address,
        "chain":    chain_index,
        "core": {
            "info":     info,
            "price":    price,
            "contract": advanced,
            "security": security,
        },
        "structure": {
            "holders":    holders,
            "cluster":    cluster,
            "topTraders": top_traders,
            "signals":    signals,
        },
        "launchpad": launchpad,
    }))
}

/// Returns true when the token originates from a launchpad (protocolId present and non-empty).
/// Safe when `advanced` is null — returns false rather than panicking.
pub(crate) fn is_launchpad_token(advanced: &Value) -> bool {
    advanced["protocolId"]
        .as_str()
        .map(|s| !s.is_empty())
        .unwrap_or(false)
}

/// Returns true when every value in `values` is JSON null.
pub(crate) fn all_null(values: &[&Value]) -> bool {
    values.iter().all(|v| v.is_null())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── helpers ───────────────────────────────────────────────────────

    fn some_data() -> Value {
        json!({ "key": "value" })
    }

    fn null() -> Value {
        Value::Null
    }

    fn full_assemble(
        info: Value,
        price: Value,
        advanced: Value,
        security: Value,
        launchpad: Value,
    ) -> Result<Value> {
        assemble(
            "0xTOKEN",
            "501",
            info,
            price,
            advanced,
            security,
            some_data(), // holders
            some_data(), // cluster
            some_data(), // top_traders
            some_data(), // signals
            launchpad,
        )
    }

    // ── PRD: all Step 1 fail → error ─────────────────────────────────

    #[test]
    fn all_step1_null_returns_error() {
        let result = full_assemble(null(), null(), null(), null(), null());
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("all Step 1 sub-calls failed"));
        assert!(msg.contains("0xTOKEN"));
    }

    #[test]
    fn error_message_includes_chain() {
        let result = full_assemble(null(), null(), null(), null(), null());
        assert!(result.unwrap_err().to_string().contains("501"));
    }

    // ── PRD: single Step 1 fail → field null, rest present ───────────

    #[test]
    fn info_null_others_present_returns_ok() {
        let result = full_assemble(null(), some_data(), some_data(), some_data(), null());
        assert!(result.is_ok());
    }

    #[test]
    fn price_null_others_present_returns_ok() {
        let result = full_assemble(some_data(), null(), some_data(), some_data(), null());
        assert!(result.is_ok());
    }

    #[test]
    fn advanced_null_others_present_returns_ok() {
        let result = full_assemble(some_data(), some_data(), null(), some_data(), null());
        assert!(result.is_ok());
    }

    #[test]
    fn security_null_others_present_returns_ok() {
        let result = full_assemble(some_data(), some_data(), some_data(), null(), null());
        assert!(result.is_ok());
    }

    #[test]
    fn null_fields_preserved_in_core_output() {
        let out = full_assemble(null(), some_data(), some_data(), some_data(), null()).unwrap();
        assert!(out["core"]["info"].is_null());
        assert!(!out["core"]["price"].is_null());
    }

    #[test]
    fn security_only_present_prevents_all_null_error() {
        // security is non-null even when info/price/advanced all fail
        let result = full_assemble(null(), null(), null(), some_data(), null());
        assert!(result.is_ok());
    }

    // ── PRD: Step 3 conditional on protocolId ─────────────────────────

    #[test]
    fn launchpad_null_when_step3_skipped() {
        let out =
            full_assemble(some_data(), some_data(), some_data(), some_data(), null()).unwrap();
        assert!(out["launchpad"].is_null());
    }

    #[test]
    fn launchpad_data_present_when_step3_ran() {
        let lp = json!({
            "tokenDetails": {"bonding": "80%"},
            "devInfo":      {"rugCount": 0},
            "bundleInfo":   {"bundleRate": "5%"},
            "similarTokens": [],
        });
        let out = full_assemble(
            some_data(),
            some_data(),
            some_data(),
            some_data(),
            lp.clone(),
        )
        .unwrap();
        assert_eq!(out["launchpad"]["devInfo"]["rugCount"], 0);
    }

    #[test]
    fn launchpad_null_when_advanced_itself_failed() {
        // advanced-info call failed → advanced is null → is_launchpad_token returns false
        // Step 3 should be skipped; launchpad null passed in from run()
        let out = full_assemble(some_data(), some_data(), null(), some_data(), null()).unwrap();
        assert!(out["launchpad"].is_null());
    }

    // ── Output structure matches PRD spec ─────────────────────────────

    #[test]
    fn output_has_workflow_discriminator() {
        let out =
            full_assemble(some_data(), some_data(), some_data(), some_data(), null()).unwrap();
        assert_eq!(out["workflow"], "token-research");
    }

    #[test]
    fn output_has_address_and_chain() {
        let out =
            full_assemble(some_data(), some_data(), some_data(), some_data(), null()).unwrap();
        assert_eq!(out["address"], "0xTOKEN");
        assert_eq!(out["chain"], "501");
    }

    #[test]
    fn core_has_all_required_fields() {
        let out =
            full_assemble(some_data(), some_data(), some_data(), some_data(), null()).unwrap();
        assert!(!out["core"]["info"].is_null());
        assert!(!out["core"]["price"].is_null());
        assert!(!out["core"]["contract"].is_null());
        assert!(!out["core"]["security"].is_null());
    }

    #[test]
    fn structure_has_all_required_fields() {
        let out =
            full_assemble(some_data(), some_data(), some_data(), some_data(), null()).unwrap();
        let s = &out["structure"];
        assert!(!s["holders"].is_null());
        assert!(!s["cluster"].is_null());
        assert!(!s["topTraders"].is_null());
        assert!(!s["signals"].is_null());
    }

    #[test]
    fn cluster_null_in_structure_preserved() {
        // cluster-overview 500 on new token → null passed in
        let result = assemble(
            "0xNEW",
            "501",
            some_data(),
            some_data(),
            some_data(),
            some_data(),
            some_data(),
            null(),
            some_data(),
            some_data(), // cluster = null
            null(),
        );
        let out = result.unwrap();
        assert!(out["structure"]["cluster"].is_null());
        assert!(!out["structure"]["holders"].is_null());
    }

    // ── is_launchpad_token ────────────────────────────────────────────

    #[test]
    fn launchpad_token_with_non_empty_protocol_id() {
        assert!(is_launchpad_token(&json!({ "protocolId": "120596" })));
    }

    #[test]
    fn launchpad_token_with_empty_protocol_id() {
        assert!(!is_launchpad_token(&json!({ "protocolId": "" })));
    }

    #[test]
    fn launchpad_token_missing_protocol_id_field() {
        assert!(!is_launchpad_token(&json!({ "name": "BONK" })));
    }

    #[test]
    fn launchpad_token_advanced_is_null() {
        assert!(!is_launchpad_token(&Value::Null));
    }

    #[test]
    fn launchpad_token_advanced_is_empty_object() {
        assert!(!is_launchpad_token(&json!({})));
    }

    #[test]
    fn launchpad_token_protocol_id_non_string_type() {
        assert!(!is_launchpad_token(&json!({ "protocolId": 120596 })));
    }

    // ── all_null ──────────────────────────────────────────────────────

    #[test]
    fn all_null_when_every_value_is_null() {
        assert!(all_null(&[&null(), &null(), &null()]));
    }

    #[test]
    fn all_null_false_when_one_value_present() {
        assert!(!all_null(&[&null(), &some_data(), &null()]));
    }

    #[test]
    fn all_null_false_for_empty_object() {
        assert!(!all_null(&[&json!({})]));
    }

    #[test]
    fn all_null_false_for_empty_array() {
        assert!(!all_null(&[&json!([])]));
    }
}
