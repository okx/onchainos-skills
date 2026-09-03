//! Internal Funding Target Resolver + `FundingBundle` composition (spec §2.5,
//! Appendix A, §7.1/§7.2).
//!
//! This module is the single scene-agnostic layer that resolves *the current
//! account's* receive address for a target chain and composes it with the
//! Common QR output ([`crate::qr::build_qr_output`]) into a [`FundingBundle`]
//! consumed by active receive and the shared recovery scenes (Wallet Send,
//! Swap, A2A payment, and task creation).
//!
//! `chain_name` is populated from ONE source — [`crate::chains::chain_display_name`]
//! — so the rendered value is identical across every scene, eliminating the prior
//! `XLayer`/`X Layer` split (canonical form: `X Layer`).
//!
//! Scope (Appendix A): this module contains **NO** business recovery state
//! machine, **NO** resume logic, and **NO** scene-specific formatting. The
//! standard blocked-result helper maps every scene into the same
//! `fundingTarget` / `qr` / `fundingNeed` payload. Loading/refreshing `wallets.json`
//! (`ensure_wallet_accounts_fresh`, spec §6.2) is the caller's responsibility —
//! these functions take an already-loaded [`WalletsJson`] and perform a pure,
//! hermetic composition (mirroring T4's pure-lookup resolver).

use anyhow::Result;
use num_bigint::BigUint;
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::path::Path;

use crate::chains::chain_display_name;
use crate::commands::agentic_wallet::account::{
    resolve_account_address_for_chain, resolve_active_account_id,
};
use crate::qr::{build_qr_output, QrOutput};
use crate::wallet_store::WalletsJson;

/// Composition of Funding Target Resolver output + Common QR output. Internal
/// only — not a new CLI output type; scenes project it into their own JSON.
#[derive(Debug, Clone, Serialize)]
pub struct FundingBundle {
    pub target: FundingTarget,
    pub qr: QrOutput,
}

/// The resolved funding target for the current account on a specific chain.
///
/// `chain_name` is the SINGLE canonical display value (serialized as `chainName`)
/// rendered identically by every scene.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FundingTarget {
    pub account_name: String,
    pub chain_index: String, // [UNIT: chain-index]
    /// Canonical chain display name; serialized as `chainName`; SINGLE source for
    /// all scenes (removes the `XLayer`/`X Layer` split — canonical form `X Layer`).
    pub chain_name: String,
    pub receive_address: String,
    /// Display hint (spec §2.5 LOW #6): `true` for X Layer (196) so scenes render
    /// the "X Layer gas-free" note. Projected into each scene as `gasFree`; does NOT
    /// alter the deposit address or QR payload.
    pub gas_free: bool,
    /// Safety flag (spec §2.5 LOW #7 / §5.2.2): a resolved target is always bound to
    /// one known network, so top-ups must arrive on that same network. Projected
    /// into each scene as `sameNetworkRequired`.
    pub same_network_required: bool,
}

/// Common facts every business supplies when an operation is blocked by a
/// funding shortfall. Business handlers own only their phase/reason and input
/// facts; this module owns the shared Funding payload and action contract.
pub struct FundingBlockedInput<'a> {
    pub phase: &'a str,
    pub reason: &'a str,
    pub asset: Option<&'a str>,
    pub token_address: &'a str,
    pub required: Option<&'a str>,
    pub balance: Option<&'a str>,
    pub business_payload: Value,
}

/// Build the standard structured result consumed by the shared Funding
/// Reference. The owning business may wrap this value in its existing success
/// or error envelope, but must not reconstruct `fundingTarget`, `qr`,
/// `fundingNeed`, or the `fund_account` action itself.
pub fn build_funding_blocked_result(
    bundle: &FundingBundle,
    input: FundingBlockedInput<'_>,
) -> Value {
    let shortfall = input
        .required
        .zip(input.balance)
        .and_then(|(required, balance)| readable_shortfall(required, balance));
    let mut payload = match input.business_payload {
        Value::Object(payload) => payload,
        _ => Map::new(),
    };
    payload.insert(
        "fundingTarget".to_string(),
        serde_json::to_value(&bundle.target).unwrap_or(Value::Null),
    );
    payload.insert(
        "qr".to_string(),
        serde_json::to_value(&bundle.qr).unwrap_or(Value::Null),
    );
    payload.insert(
        "fundingNeed".to_string(),
        json!({
            "asset": input.asset,
            "tokenAddress": input.token_address,
            "required": input.required,
            "balance": input.balance,
            "shortfall": shortfall,
        }),
    );

    json!({
        "phase": input.phase,
        "decision": "blocked",
        "reason": input.reason,
        "nextAction": [{
            "id": "fund_account",
            "recommend": true,
            "params": {},
        }],
        "payload": payload,
    })
}

/// Resolve the current (selected) account's [`FundingTarget`] for `chain_index`.
///
/// `receive_address` comes from T4's pure resolver
/// ([`resolve_account_address_for_chain`]); `chain_name` from the single canonical
/// source [`chain_display_name`]. Returns `Err` (surfaced verbatim from T4) when the
/// selected account has no address for the chain — callers get no partial target.
pub fn resolve_funding_target(wallets: &WalletsJson, chain_index: &str) -> Result<FundingTarget> {
    // Address first: propagate T4's resolution failure before composing anything.
    let receive_address = resolve_account_address_for_chain(wallets, chain_index)?;
    Ok(FundingTarget {
        account_name: selected_account_name(wallets),
        chain_index: chain_index.to_string(),
        chain_name: chain_display_name(chain_index).to_string(),
        receive_address,
        gas_free: chain_index == "196",
        same_network_required: true,
    })
}

/// Resolve the target then compose it with the Common QR output into a
/// [`FundingBundle`] (spec §2.5). On a resolution failure the whole call errors —
/// no partial bundle and no QR is built.
pub fn build_funding_bundle(
    wallets: &WalletsJson,
    chain_index: &str,
    image_dir: Option<&Path>,
) -> Result<FundingBundle> {
    let target = resolve_funding_target(wallets, chain_index)?;
    let qr = build_qr_output(&target.receive_address, image_dir);
    Ok(FundingBundle { target, qr })
}

/// Compose a Funding bundle for a caller that already resolved the receiving
/// address through another authoritative identity path (for example an Agent
/// Commerce agent ID). This keeps target normalization and QR construction in
/// the same common module instead of rebuilding them in each business.
pub fn build_funding_bundle_for_address(
    account_name: &str,
    chain_index: &str,
    receive_address: &str,
    image_dir: Option<&Path>,
) -> Result<FundingBundle> {
    if chain_index.trim().is_empty() {
        anyhow::bail!("funding chain index must not be blank");
    }
    if receive_address.trim().is_empty() {
        anyhow::bail!("funding receive address must not be blank");
    }
    let target = FundingTarget {
        account_name: account_name.to_string(),
        chain_index: chain_index.to_string(),
        chain_name: chain_display_name(chain_index).to_string(),
        receive_address: receive_address.to_string(),
        gas_free: chain_index == "196",
        same_network_required: true,
    };
    let qr = build_qr_output(receive_address, image_dir);
    Ok(FundingBundle { target, qr })
}

/// Return `required - available` for non-negative readable decimal amounts.
///
/// Funding scenes use this helper so the CLI, rather than a Skill or output
/// template, owns the balance fact. Scientific notation and signed values are
/// deliberately rejected; callers emit `null` when either upstream value is
/// unavailable or not a plain decimal.
pub fn readable_shortfall(required: &str, available: &str) -> Option<String> {
    fn parse(value: &str) -> Option<(BigUint, usize)> {
        let value = value.trim();
        let mut parts = value.split('.');
        let whole = parts.next()?;
        let fraction = parts.next().unwrap_or("");
        if parts.next().is_some()
            || (whole.is_empty() && fraction.is_empty())
            || !whole.chars().all(|c| c.is_ascii_digit())
            || !fraction.chars().all(|c| c.is_ascii_digit())
        {
            return None;
        }
        let digits = format!("{}{}", if whole.is_empty() { "0" } else { whole }, fraction);
        Some((BigUint::parse_bytes(digits.as_bytes(), 10)?, fraction.len()))
    }

    let (mut required, required_scale) = parse(required)?;
    let (mut available, available_scale) = parse(available)?;
    let scale = required_scale.max(available_scale);
    required *= BigUint::from(10u8).pow((scale - required_scale) as u32);
    available *= BigUint::from(10u8).pow((scale - available_scale) as u32);
    if required <= available {
        return Some("0".to_string());
    }

    let mut digits = (required - available).to_str_radix(10);
    if scale == 0 {
        return Some(digits);
    }
    if digits.len() <= scale {
        digits = format!("{}{}", "0".repeat(scale + 1 - digits.len()), digits);
    }
    let split = digits.len() - scale;
    let mut rendered = format!("{}.{}", &digits[..split], &digits[split..]);
    while rendered.ends_with('0') {
        rendered.pop();
    }
    if rendered.ends_with('.') {
        rendered.pop();
    }
    Some(rendered)
}

/// Display name of the selected account, best-effort (`""` when it can't be
/// resolved) — the funding target is still valid without a human account name.
fn selected_account_name(wallets: &WalletsJson) -> String {
    let Ok(account_id) = resolve_active_account_id(wallets) else {
        return String::new();
    };
    wallets
        .accounts
        .iter()
        .find(|a| a.account_id == account_id)
        .map(|a| a.account_name.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*; // brings WalletsJson (+ resolvers) from the parent module's imports
    use crate::wallet_store::{AccountInfo, AccountMapEntry, AddressInfo};
    use std::collections::HashMap;

    fn addr(chain_index: &str, address: &str) -> AddressInfo {
        AddressInfo {
            account_id: "account-1".to_string(),
            address: address.to_string(),
            chain_index: chain_index.to_string(),
            chain_name: String::new(),
            address_type: String::new(),
            chain_path: String::new(),
        }
    }

    /// Fixture whose selected account (`account-1`, named "Trading") owns the
    /// given address list — no disk, no network (hermetic, §13 sandbox rule).
    fn wallets_fixture(address_list: Vec<AddressInfo>) -> WalletsJson {
        let mut accounts_map = HashMap::new();
        accounts_map.insert("account-1".to_string(), AccountMapEntry { address_list });
        WalletsJson {
            selected_account_id: "account-1".to_string(),
            accounts: vec![AccountInfo {
                project_id: "project-1".to_string(),
                account_id: "account-1".to_string(),
                account_name: "Trading".to_string(),
                is_default: true,
            }],
            accounts_map,
            ..Default::default()
        }
    }

    /// Sandbox PNG dir under `cli/target/test_tmp/<name>` (never `tempfile::tempdir()`,
    /// §13 sandbox rule) so the image-notify path is hermetic.
    fn funding_test_dir(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join(name)
    }

    #[test]
    fn resolve_funding_target_x_layer_is_canonical_and_gas_free() {
        // chain 196 shares the common EVM address; chain_name is the canonical
        // "X Layer" (space) and gas_free is set for X Layer only.
        let wallets = wallets_fixture(vec![addr("1", "0xEvmShared"), addr("501", "SoLaNaAddr")]);

        let target = resolve_funding_target(&wallets, "196").unwrap();
        assert_eq!(target.chain_name, "X Layer");
        assert!(target.gas_free, "X Layer (196) must set gas_free");
        assert_eq!(target.chain_index, "196");
        assert_eq!(target.receive_address, "0xEvmShared");
        assert_eq!(target.account_name, "Trading");
        assert!(target.same_network_required);
    }

    #[test]
    fn resolve_funding_target_ethereum_is_canonical_and_not_gas_free() {
        let wallets = wallets_fixture(vec![addr("1", "0xEvmShared"), addr("501", "SoLaNaAddr")]);

        let target = resolve_funding_target(&wallets, "1").unwrap();
        assert_eq!(target.chain_name, "Ethereum");
        assert!(!target.gas_free, "Ethereum (1) must NOT be gas_free");
        assert_eq!(target.receive_address, "0xEvmShared");
    }

    #[test]
    fn funding_target_serializes_chain_name_as_chain_name_camel_case() {
        let wallets = wallets_fixture(vec![addr("196", "0xXLayerAddr")]);
        let target = resolve_funding_target(&wallets, "196").unwrap();

        let json = serde_json::to_value(&target).expect("FundingTarget serializes");
        // The single canonical source proven: chain_name → `chainName`.
        assert_eq!(json["chainName"], "X Layer");
        assert_eq!(json["chainIndex"], "196");
        assert_eq!(json["receiveAddress"], "0xXLayerAddr");
        assert_eq!(json["gasFree"], true);
        // snake_case keys must NOT leak into the serialized form.
        assert!(json.get("chain_name").is_none());
    }

    #[test]
    fn build_funding_bundle_composes_target_and_populated_qr() {
        let wallets = wallets_fixture(vec![addr("1", "0xEvmShared")]);
        let dir = funding_test_dir("funding_bundle_image");

        let bundle = build_funding_bundle(&wallets, "1", Some(&dir)).unwrap();

        // Target is composed verbatim from resolve_funding_target.
        assert_eq!(bundle.target.chain_name, "Ethereum");
        assert_eq!(bundle.target.receive_address, "0xEvmShared");

        // QrOutput is a real, populated Common QR (not a stub): requested_format is
        // always "auto", and a valid short address always yields a QR payload in
        // whichever display mode the runtime resolves to.
        assert_eq!(bundle.qr.requested_format, "auto");
        assert!(
            bundle.qr.terminal_qr.is_some() || bundle.qr.image_path.is_some(),
            "QrOutput must carry a QR payload (terminal or image)"
        );
        // When the image-notify mode wrote a PNG, it must live under our image_dir.
        if let Some(path) = bundle.qr.image_path.as_deref() {
            assert!(std::path::Path::new(path).starts_with(&dir));
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn readable_shortfall_uses_exact_decimal_arithmetic() {
        assert_eq!(
            readable_shortfall("10", "0.08504764").as_deref(),
            Some("9.91495236")
        );
        assert_eq!(readable_shortfall("1.20", "0.2").as_deref(), Some("1"));
        assert_eq!(readable_shortfall("1", "2").as_deref(), Some("0"));
        assert_eq!(readable_shortfall("1e3", "1"), None);
    }

    #[test]
    fn blocked_result_builds_the_common_funding_contract() {
        let wallets = wallets_fixture(vec![addr("196", "0xReceive")]);
        let bundle = build_funding_bundle(&wallets, "196", None).unwrap();
        let value = build_funding_blocked_result(
            &bundle,
            FundingBlockedInput {
                phase: "example_funding",
                reason: "asset_shortfall",
                asset: Some("USDC"),
                token_address: "0xToken",
                required: Some("10"),
                balance: Some("1"),
                business_payload: json!({"scene": "example_balance_shortfall"}),
            },
        );

        assert_eq!(value["phase"], "example_funding");
        assert_eq!(value["reason"], "asset_shortfall");
        assert_eq!(
            value["nextAction"],
            json!([{"id": "fund_account", "recommend": true, "params": {}}])
        );
        assert_eq!(value["payload"]["fundingNeed"]["shortfall"], "9");
        assert_eq!(
            value["payload"]["fundingTarget"]["receiveAddress"],
            "0xReceive"
        );
        assert!(value["payload"]["qr"].is_object());
    }

    #[test]
    fn resolved_address_bundle_uses_the_same_target_and_qr_rules() {
        let address = "0x1234567890abcdef1234567890abcdef12345678";
        let bundle = build_funding_bundle_for_address("Agent", "196", address, None)
            .expect("resolved address bundle");
        assert_eq!(bundle.target.account_name, "Agent");
        assert_eq!(bundle.target.chain_name, "X Layer");
        assert!(bundle.target.gas_free);
        assert_eq!(bundle.target.receive_address, address);
        assert_eq!(bundle.qr.requested_format, "auto");
    }

    #[test]
    fn missing_address_for_chain_errors_with_no_partial_bundle() {
        // Account has only an EVM address — Bitcoin (0) is not EVM-family and has
        // no dedicated entry, so T4 errs and no partial target/bundle is produced.
        let wallets = wallets_fixture(vec![addr("1", "0xEvmShared")]);

        assert!(resolve_funding_target(&wallets, "0").is_err());
        assert!(build_funding_bundle(&wallets, "0", None).is_err());
    }
}
