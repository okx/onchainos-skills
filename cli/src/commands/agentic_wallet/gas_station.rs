use anyhow::{bail, Result};
use serde_json::{json, Value};

use crate::commands::agentic_wallet::auth::{ensure_tokens_refreshed, format_api_error};
use crate::output;
use crate::wallet_api::{GasStationStatus, WalletApiClient};

use super::wallet::GasStationCommand;

pub async fn execute(cmd: GasStationCommand) -> Result<()> {
    match cmd {
        GasStationCommand::UpdateDefaultToken {
            chain,
            gas_token_address,
        } => {
            let data = fetch_update_default_token(&chain, &gas_token_address).await?;
            output::success(data);
            Ok(())
        }
        GasStationCommand::Enable { chain } => {
            let data = fetch_update(&chain, true).await?;
            output::success(data);
            Ok(())
        }
        GasStationCommand::Disable { chain } => {
            let data = fetch_update(&chain, false).await?;
            output::success(data);
            Ok(())
        }
        GasStationCommand::Status { chain, from } => cmd_status(&chain, from.as_deref()).await,
        GasStationCommand::Setup {
            chain,
            gas_token_address,
            relayer_id,
            from,
        } => cmd_setup(&chain, &gas_token_address, &relayer_id, from.as_deref()).await,
    }
}

/// `wallet gas-station status` — read-only Gas Station readiness probe.
///
/// Used by third-party plugin pre-flight: agent runs this before invoking a plugin's
/// on-chain command, branches on the returned `recommendation`. Never broadcasts.
///
/// Implementation: calls `pre_transaction_unsigned_info` with a zero-amount native
/// self-transfer probe. Backend's `gasStationStatus` enum + `gasStationTokenList` +
/// `defaultGasTokenAddress` carry everything we need to synthesize a `recommendation`.
/// Shared context built once for both `cmd_status` and `cmd_setup`. Extracts the
/// auth + chain + wallet + session resolution that both flows need before they
/// can call `pre_transaction_unsigned_info`.
struct GsContext {
    access_token: String,
    addr_info: crate::wallet_store::AddressInfo,
    chain_name: String,
    chain_index_resolved: String,
    chain_index_num: u64,
    session_cert: String,
}

async fn build_gs_context(chain: &str, from: Option<&str>) -> Result<GsContext> {
    let access_token = ensure_tokens_refreshed().await?;
    let chain_index_resolved = crate::chains::resolve_chain(chain).to_string();
    let chain_entry = super::chain::get_chain_by_real_chain_index(&chain_index_resolved)
        .await?
        .ok_or_else(|| anyhow::anyhow!("unsupported chain: {chain}"))?;
    let chain_name = chain_entry["chainName"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("chain entry missing chainName"))?
        .to_string();

    let wallets = crate::wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let (_account_id, addr_info) = super::transfer::resolve_address(&wallets, from, &chain_name)?;

    let session = crate::wallet_store::load_session()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let chain_index_num: u64 = addr_info.chain_index.parse().map_err(|_| {
        anyhow::anyhow!("chain id '{}' is not a valid number", addr_info.chain_index)
    })?;

    Ok(GsContext {
        access_token,
        addr_info,
        chain_name,
        chain_index_resolved,
        chain_index_num,
        session_cert: session.session_cert,
    })
}

/// Issue a Phase 1 diagnostic probe (zero-amount native self-transfer). Backend
/// returns gasStationStatus + gasStationTokenList without broadcasting.
async fn probe_phase1_diagnostic(
    client: &mut WalletApiClient,
    ctx: &GsContext,
) -> Result<crate::wallet_api::UnsignedInfoResponse> {
    client
        .pre_transaction_unsigned_info(
            &ctx.access_token,
            &ctx.addr_info.chain_path,
            ctx.chain_index_num,
            &ctx.addr_info.address,
            &ctx.addr_info.address,
            "0",
            None,
            &ctx.session_cert,
            Some("0x"),
            None, None, None, None, None,
            None,
            None, None, None,
        )
        .await
        .map_err(format_api_error)
}

async fn cmd_status(chain: &str, from: Option<&str>) -> Result<()> {
    let ctx = build_gs_context(chain, from).await?;
    let mut client = WalletApiClient::new()?;
    let probe = probe_phase1_diagnostic(&mut client, &ctx).await?;

    let recommendation = recommend_from_probe(&probe);
    let token_list_json: Vec<Value> = probe
        .gas_station_token_list
        .iter()
        .map(|t| {
            json!({
                "symbol": t.symbol,
                "feeTokenAddress": t.fee_token_address,
                "relayerId": t.relayer_id,
                "balance": t.balance,
                "serviceCharge": t.service_charge,
                "sufficient": t.sufficient,
            })
        })
        .collect();

    let gs_activated = matches!(probe.gs_status(), GasStationStatus::ReadyToUse);
    let default_gas_token = if probe.default_gas_token_address.is_empty() {
        Value::Null
    } else {
        Value::String(probe.default_gas_token_address.clone())
    };

    output::success(json!({
        "chainId": ctx.addr_info.chain_index,
        "chainName": ctx.chain_name,
        "fromAddress": ctx.addr_info.address,
        "gasStationActivated": gs_activated,
        "gasStationDefaultToken": default_gas_token,
        "gasStationStatus": probe.gas_station_status,
        "recommendation": recommendation,
        "hasPendingTx": probe.has_pending_tx,
        "insufficientAll": probe.insufficient_all,
        "tokenList": token_list_json,
    }));
    Ok(())
}

/// Map a Phase 1 probe response to a high-level recommendation enum.
fn recommend_from_probe(probe: &crate::wallet_api::UnsignedInfoResponse) -> &'static str {
    if probe.has_pending_tx {
        return "HAS_PENDING_TX";
    }
    if probe.insufficient_all {
        return "INSUFFICIENT_ALL";
    }
    match probe.gs_status() {
        GasStationStatus::ReadyToUse => "READY",
        GasStationStatus::NotApplicable => "READY",
        GasStationStatus::FirstTimePrompt => "ENABLE_GAS_STATION",
        GasStationStatus::PendingUpgrade => "PENDING_UPGRADE",
        GasStationStatus::ReenableOnly => "REENABLE_GAS_STATION",
        GasStationStatus::InsufficientAll => "INSUFFICIENT_ALL",
        GasStationStatus::HasPendingTx => "HAS_PENDING_TX",
        GasStationStatus::Unknown => {
            if probe.gas_station_used {
                "ENABLE_GAS_STATION"
            } else {
                "READY"
            }
        }
    }
}

/// `wallet gas-station setup` — standalone first-time activation.
///
/// Decoupled from `wallet send` so the agent can activate GS *before* invoking
/// a third-party plugin (which calls `wallet contract-call --force`). After
/// successful setup, subsequent contract-call/send transparently use GS — the
/// plugin needs no modification.
///
/// Implementation: wraps `wallet send` as a self-transfer carrier of the picked
/// gas token (1 minimal unit, idempotent) with `--enable-gas-station`. Backend
/// Phase 2 returns signing material for both 712 hash and (when needed)
/// `authHashFor7702`; CLI signs and broadcasts in one tx. Idempotent: same
/// default → `alreadyActivated=true`; different default → switches via
/// `update-default-token`.
async fn cmd_setup(
    chain: &str,
    gas_token_address: &str,
    relayer_id: &str,
    from: Option<&str>,
) -> Result<()> {
    let ctx = build_gs_context(chain, from).await?;
    let mut client = WalletApiClient::new()?;
    // Idempotency check: probe first; if GS already active with same default → short-circuit.
    let probe = probe_phase1_diagnostic(&mut client, &ctx).await?;

    let already_active_same_default = matches!(probe.gs_status(), GasStationStatus::ReadyToUse)
        && !probe.default_gas_token_address.is_empty()
        && probe
            .default_gas_token_address
            .eq_ignore_ascii_case(gas_token_address);
    if already_active_same_default {
        output::success(json!({
            "chainId": ctx.addr_info.chain_index,
            "chainName": ctx.chain_name,
            "gasStationActivated": true,
            "alreadyActivated": true,
            "defaultToken": {
                "feeTokenAddress": probe.default_gas_token_address,
            },
            "txHash": Value::Null,
            "needs7702Upgrade": false,
            "summary": "Gas Station already enabled with the requested default token. No action taken.",
        }));
        return Ok(());
    }

    // If GS active but with a different default → switch via update-default-token.
    if matches!(probe.gs_status(), GasStationStatus::ReadyToUse) {
        let _data = client
            .gas_station_update_default_token(
                &ctx.access_token,
                &ctx.addr_info.chain_index,
                gas_token_address,
                &ctx.addr_info.address,
            )
            .await
            .map_err(format_api_error)?;
        output::success(json!({
            "chainId": ctx.addr_info.chain_index,
            "chainName": ctx.chain_name,
            "gasStationActivated": true,
            "alreadyActivated": true,
            "defaultTokenSwitched": true,
            "defaultToken": { "feeTokenAddress": gas_token_address, "relayerId": relayer_id },
            "txHash": Value::Null,
            "needs7702Upgrade": false,
            "summary": "Gas Station was already enabled; only the default gas token was switched (server-side flag flip, no on-chain transaction).",
        }));
        return Ok(());
    }

    // Sanity check: only proceed if probe state is first-time-eligible.
    if !matches!(
        probe.gs_status(),
        GasStationStatus::FirstTimePrompt
            | GasStationStatus::PendingUpgrade
            | GasStationStatus::ReenableOnly
            | GasStationStatus::Unknown
    ) {
        bail!(
            "Cannot setup Gas Station: backend reports state '{}' which is not first-time-eligible. \
             Run `wallet gas-station status --chain {}` for diagnostics.",
            probe.gas_station_status,
            chain
        );
    }

    // Drive the carrier transfer through the existing send flow with --enable-gas-station.
    // Amount "1" is the minimal unit for an ERC-20 self-transfer (e.g. 0.000001 USDC for
    // a 6-decimal token). Net value change to the user = 0 (self → self); only the GS
    // service charge is consumed. cmd_send prints its own success JSON containing
    // `{ txHash, orderId, gasStationUsed, serviceCharge, serviceChargeSymbol }` — which IS
    // the setup result; no additional output wrapping is needed here.
    super::transfer::cmd_send(
        "1",
        &ctx.addr_info.address,
        &ctx.chain_index_resolved,
        from,
        Some(gas_token_address),
        true,
        Some(gas_token_address),
        Some(relayer_id),
        true,
    )
    .await
}

/// Public fetch function for MCP and CLI
pub async fn fetch_update_default_token(chain: &str, gas_token_address: &str) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let chain_index = crate::chains::resolve_chain(chain);

    // Resolve fromAddr from currently selected account for this chain
    let chain_entry = super::chain::get_chain_by_real_chain_index(&chain_index)
        .await?
        .ok_or_else(|| anyhow::anyhow!("unsupported chain: {chain}"))?;
    let chain_name = chain_entry["chainName"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("chain entry missing chainName"))?;
    let wallets = crate::wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let (_, addr_info) = super::transfer::resolve_address(&wallets, None, chain_name)?;
    let from_addr = addr_info.address;

    let mut client = WalletApiClient::new()?;
    let data = client
        .gas_station_update_default_token(&access_token, &chain_index, gas_token_address, &from_addr)
        .await
        .map_err(format_api_error)?;
    Ok(data)
}

/// Flip Gas Station DB flag for a chain (`enable=true` to enable / `false` to disable).
/// DB flag only, no on-chain action. On-chain 7702 delegation is preserved on disable,
/// so a later enable does NOT require a new 7702 upgrade (backend returns a msg if the
/// chain was never delegated to begin with).
pub async fn fetch_update(chain: &str, enable: bool) -> Result<Value> {
    let access_token = ensure_tokens_refreshed().await?;
    let chain_index = crate::chains::resolve_chain(chain);
    // Both enable and disable require fromAddr — backend contract is consistent across both.
    let chain_entry = super::chain::get_chain_by_real_chain_index(&chain_index)
        .await?
        .ok_or_else(|| anyhow::anyhow!("unsupported chain: {chain}"))?;
    let chain_name = chain_entry["chainName"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("chain entry missing chainName"))?;
    let wallets = crate::wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let (_, addr_info) = super::transfer::resolve_address(&wallets, None, chain_name)?;
    let from_addr = addr_info.address;

    let mut client = WalletApiClient::new()?;
    let data = client
        .gas_station_update(&access_token, &chain_index, enable, Some(&from_addr))
        .await
        .map_err(format_api_error)?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wallet_api::UnsignedInfoResponse;

    fn unsigned_with_status(status: &str) -> UnsignedInfoResponse {
        let mut r: UnsignedInfoResponse = serde_json::from_str("{}").unwrap();
        r.gas_station_status = status.to_string();
        r
    }

    #[test]
    fn recommend_ready_from_ready_to_use() {
        assert_eq!(
            recommend_from_probe(&unsigned_with_status("READY_TO_USE")),
            "READY"
        );
    }

    #[test]
    fn recommend_ready_from_not_applicable() {
        assert_eq!(
            recommend_from_probe(&unsigned_with_status("NOT_APPLICABLE")),
            "READY"
        );
    }

    #[test]
    fn recommend_enable_gas_station_from_first_time_prompt() {
        assert_eq!(
            recommend_from_probe(&unsigned_with_status("FIRST_TIME_PROMPT")),
            "ENABLE_GAS_STATION"
        );
    }

    #[test]
    fn recommend_pending_upgrade_passes_through() {
        assert_eq!(
            recommend_from_probe(&unsigned_with_status("PENDING_UPGRADE")),
            "PENDING_UPGRADE"
        );
    }

    #[test]
    fn recommend_reenable_from_reenable_only() {
        assert_eq!(
            recommend_from_probe(&unsigned_with_status("REENABLE_ONLY")),
            "REENABLE_GAS_STATION"
        );
    }

    #[test]
    fn recommend_insufficient_all_overrides_via_top_level_flag() {
        // top-level insufficient_all bool wins even when status enum is otherwise.
        let mut r: UnsignedInfoResponse =
            serde_json::from_str(r#"{"insufficientAll": true}"#).unwrap();
        r.gas_station_status = "FIRST_TIME_PROMPT".to_string();
        assert_eq!(recommend_from_probe(&r), "INSUFFICIENT_ALL");
    }

    #[test]
    fn recommend_has_pending_tx_overrides_via_top_level_flag() {
        let mut r: UnsignedInfoResponse =
            serde_json::from_str(r#"{"hasPendingTx": true}"#).unwrap();
        r.gas_station_status = "FIRST_TIME_PROMPT".to_string();
        assert_eq!(recommend_from_probe(&r), "HAS_PENDING_TX");
    }

    #[test]
    fn recommend_unknown_with_gas_station_used_means_enable() {
        let mut r: UnsignedInfoResponse =
            serde_json::from_str(r#"{"gasStationUsed": true}"#).unwrap();
        r.gas_station_status = "SOMETHING_NEW".to_string();
        assert_eq!(recommend_from_probe(&r), "ENABLE_GAS_STATION");
    }

    #[test]
    fn recommend_unknown_without_gas_station_used_falls_back_to_ready() {
        let r = unsigned_with_status("SOMETHING_NEW");
        assert_eq!(recommend_from_probe(&r), "READY");
    }
}
