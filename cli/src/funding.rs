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
//! standard blocked-result helper maps every caller into the same
//! `fundingTarget` / `qr` / `fundingNeed` payload. That result enters the shared
//! Funding Reference immediately; no intermediate user-selected action is
//! required. The public wallet-backed
//! builder refreshes account/address facts before composing the result; the
//! pure resolver remains internal for deterministic tests.

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
use crate::wallet_api::WalletApiClient;
use crate::wallet_store;
use crate::wallet_store::WalletsJson;

/// Composition of Funding Target Resolver output + Common QR output. Internal
/// only — the public Funding builder projects it into the common JSON contract.
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

pub const FUNDING_REQUIRED_PHASE: &str = "funding_required";
pub const FUNDING_OPERATION_TRANSFER: &str = "transfer";
pub const FUNDING_OPERATION_SWAP: &str = "swap";
pub const FUNDING_OPERATION_A2A_PAYMENT: &str = "a2a_payment";
pub const FUNDING_OPERATION_TASK_CREATION: &str = "task_creation";

/// Fixed public input for any operation blocked by a funding shortfall.
///
/// `operation` is an optional presentation key consumed by the shared Funding
/// template. It is not business state and does not authorize resuming work.
pub struct FundingBlockedInput<'a> {
    pub asset: &'a str,
    pub token_address: &'a str,
    pub required: &'a str,
    pub balance: Option<&'a str>,
    pub operation: Option<&'a str>,
    /// Optional diagnostic facts supplied by the business. They are rendered
    /// only when present and never participate in routing or recovery.
    pub error_code: Option<&'a str>,
    pub error_message: Option<&'a str>,
}

/// Pure composition helper used by the public one-call builders and unit tests.
/// Business modules must use [`build_funding_bundle`] or
/// [`build_funding_bundle_for_address`] instead of reconstructing this contract.
pub(crate) fn build_funding_blocked_result(
    bundle: &FundingBundle,
    input: FundingBlockedInput<'_>,
) -> Value {
    let shortfall = input
        .balance
        .and_then(|balance| readable_shortfall(input.required, balance));
    let mut funding_need = json!({
        "asset": input.asset,
        "tokenAddress": input.token_address,
        "required": input.required,
        "balance": input.balance,
    });
    if let Some(shortfall) = shortfall {
        funding_need["shortfall"] = Value::String(shortfall);
    }
    let mut payload = json!({
        "fundingTarget": serde_json::to_value(&bundle.target).unwrap_or(Value::Null),
        "qr": serde_json::to_value(&bundle.qr).unwrap_or(Value::Null),
        "fundingNeed": funding_need,
    });
    if let Some(operation) = input.operation {
        payload["operation"] = Value::String(operation.to_string());
    }
    let mut error = Map::new();
    if let Some(code) = input.error_code.filter(|value| !value.trim().is_empty()) {
        error.insert("code".to_string(), Value::String(code.to_string()));
    }
    if let Some(message) = input
        .error_message
        .filter(|value| !value.trim().is_empty())
    {
        error.insert("message".to_string(), Value::String(message.to_string()));
    }
    if !error.is_empty() {
        payload["error"] = Value::Object(error);
    }

    json!({
        "phase": FUNDING_REQUIRED_PHASE,
        "decision": "blocked",
        "reason": "insufficient_balance",
        "nextAction": [],
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
fn build_funding_bundle_from_wallets(
    wallets: &WalletsJson,
    chain_index: &str,
    image_dir: Option<&Path>,
) -> Result<FundingBundle> {
    let target = resolve_funding_target(wallets, chain_index)?;
    let qr = build_qr_output(&target.receive_address, image_dir);
    Ok(FundingBundle { target, qr })
}

/// Resolve a Funding bundle for the selected wallet account using freshly
/// queried account/address facts. Kept crate-visible for the post-funding check,
/// which needs target + QR without creating another blocked Funding result.
pub(crate) async fn resolve_current_funding_bundle(
    chain_index: &str,
    image_dir: Option<&Path>,
) -> Result<FundingBundle> {
    let access_token =
        crate::commands::agentic_wallet::auth::ensure_tokens_refreshed().await?;
    let mut wallets = wallet_store::load_wallets()?.ok_or_else(|| {
        anyhow::anyhow!(crate::commands::agentic_wallet::common::ERR_NOT_LOGGED_IN)
    })?;
    let mut client = WalletApiClient::new()?;
    crate::commands::agentic_wallet::balance::refresh_wallet_accounts_strict(
        &mut client,
        &access_token,
        &mut wallets,
    )
    .await?;
    build_funding_bundle_from_wallets(&wallets, chain_index, image_dir)
}

/// One-call business integration for a wallet-backed insufficient-balance
/// branch. The business supplies only its fixed facts; this function owns fresh
/// address resolution, QR generation, shortfall calculation, and the stable
/// result that enters the shared Funding Reference immediately.
pub async fn build_funding_bundle(
    chain_index: &str,
    input: FundingBlockedInput<'_>,
) -> Result<Value> {
    validate_funding_blocked_input(&input)?;
    let bundle = resolve_current_funding_bundle(chain_index, None).await?;
    Ok(build_funding_blocked_result(&bundle, input))
}

fn compose_funding_bundle_for_address(
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

/// One-call integration for a business whose authoritative identity path has
/// already resolved the receiving address (for example Agent Commerce).
pub fn build_funding_bundle_for_address(
    account_name: &str,
    chain_index: &str,
    receive_address: &str,
    input: FundingBlockedInput<'_>,
) -> Result<Value> {
    validate_funding_blocked_input(&input)?;
    let bundle = compose_funding_bundle_for_address(
        account_name,
        chain_index,
        receive_address,
        None,
    )?;
    Ok(build_funding_blocked_result(&bundle, input))
}

fn validate_funding_blocked_input(input: &FundingBlockedInput<'_>) -> Result<()> {
    if input.asset.trim().is_empty() {
        anyhow::bail!("funding asset must not be blank");
    }
    if input.operation.is_some_and(|operation| operation.trim().is_empty()) {
        anyhow::bail!("funding operation must not be blank when provided");
    }
    let required = readable_shortfall(input.required, "0")
        .ok_or_else(|| anyhow::anyhow!("funding required amount must be a plain non-negative decimal"))?;
    if required == "0" {
        anyhow::bail!("funding required amount must be greater than zero");
    }
    if let Some(balance) = input.balance {
        let shortfall = readable_shortfall(input.required, balance)
            .ok_or_else(|| anyhow::anyhow!("funding balance must be a plain non-negative decimal"))?;
        if shortfall == "0" {
            anyhow::bail!("funding bundle requires an actual balance shortfall");
        }
    }
    Ok(())
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

        let bundle = build_funding_bundle_from_wallets(&wallets, "1", Some(&dir)).unwrap();

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
        let bundle = build_funding_bundle_from_wallets(&wallets, "196", None).unwrap();
        let value = build_funding_blocked_result(
            &bundle,
            FundingBlockedInput {
                asset: "USDC",
                token_address: "0xToken",
                required: "10",
                balance: Some("1"),
                operation: Some("example_operation"),
                error_code: Some("E_BALANCE"),
                error_message: Some("Insufficient funds"),
            },
        );

        assert_eq!(value["phase"], FUNDING_REQUIRED_PHASE);
        assert_eq!(value["reason"], "insufficient_balance");
        assert_eq!(value["nextAction"], json!([]));
        assert_eq!(value["payload"]["operation"], "example_operation");
        assert_eq!(value["payload"]["error"]["code"], "E_BALANCE");
        assert_eq!(
            value["payload"]["error"]["message"],
            "Insufficient funds"
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
        let value = build_funding_bundle_for_address(
            "Agent",
            "196",
            address,
            FundingBlockedInput {
                asset: "USDT",
                token_address: "0xToken",
                required: "10",
                balance: Some("1"),
                operation: None,
                error_code: None,
                error_message: None,
            },
        )
        .expect("resolved address funding result");
        assert_eq!(value["payload"]["fundingTarget"]["accountName"], "Agent");
        assert_eq!(value["payload"]["fundingTarget"]["chainName"], "X Layer");
        assert_eq!(value["payload"]["fundingTarget"]["gasFree"], true);
        assert_eq!(value["payload"]["fundingTarget"]["receiveAddress"], address);
        assert_eq!(value["payload"]["qr"]["requestedFormat"], "auto");
        assert!(value["payload"].get("operation").is_none());
        assert!(value["payload"].get("error").is_none());
    }

    #[test]
    fn missing_address_for_chain_errors_with_no_partial_bundle() {
        // Account has only an EVM address — Bitcoin (0) is not EVM-family and has
        // no dedicated entry, so T4 errs and no partial target/bundle is produced.
        let wallets = wallets_fixture(vec![addr("1", "0xEvmShared")]);

        assert!(resolve_funding_target(&wallets, "0").is_err());
        assert!(build_funding_bundle_from_wallets(&wallets, "0", None).is_err());
    }
}
