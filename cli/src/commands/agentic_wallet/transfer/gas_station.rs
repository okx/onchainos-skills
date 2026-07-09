//! Gas Station two-phase flow for `wallet send`.
//!
//! Extracted from `transfer/mod.rs` (move-only). The gas-station second-phase
//! entry point (`gas_station_send`), the two broadcast flows
//! (7702-upgrade vs. subsequent), the terminal-state emitters, the Confirming /
//! SetupRequired prompt builders, and the Phase 1 diagnostic classifier all
//! live here. `cmd_send` / `sign_and_broadcast` in the parent module call these
//! via the `use gas_station::*;` glob re-export.
//!
//! NOTE: this is `transfer/gas_station.rs` — the per-`wallet send` execution
//! flow. It is distinct from the sibling `agentic_wallet/gas_station.rs`, which
//! implements the `wallet gas-station` management subcommands.

use anyhow::{bail, Context, Result};
use base64::Engine;
use serde_json::{json, Value};

use crate::commands::agentic_wallet::auth::format_api_error;
use crate::output;

use super::super::common::handle_confirming_error;
use super::{resolve_address, TxParams};

/// Gas Station second-phase: user selected token, call unsignedInfo with gasTokenAddress
#[allow(clippy::too_many_arguments)]
pub(super) async fn gas_station_send(
    amt: &str,
    recipient: &str,
    chain: &str,
    from: Option<&str>,
    contract_token: Option<&str>,
    force: bool,
    gas_token_address: Option<&str>,
    relayer_id: Option<&str>,
    enable_gas_station: bool,
) -> Result<()> {
    let access_token =
        crate::commands::agentic_wallet::auth::ensure_tokens_refreshed().await?;
    let wallets = crate::wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!(super::super::common::ERR_NOT_LOGGED_IN))?;
    let chain_entry = super::super::chain::get_chain_by_real_chain_index(chain)
        .await?
        .ok_or_else(|| anyhow::anyhow!("unsupported chain: {}", chain))?;
    let chain_name = chain_entry["chainName"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing chainName"))?;
    let (_account_id, addr_info) = resolve_address(&wallets, from, chain_name)?;
    let chain_index_num: u64 = addr_info.chain_index.parse().unwrap_or(1);

    let session = crate::wallet_store::load_session()?
        .ok_or_else(|| anyhow::anyhow!(super::super::common::ERR_NOT_LOGGED_IN))?;

    let mut client = crate::wallet_api::WalletApiClient::new()?;
    let unsigned = client
        .pre_transaction_unsigned_info(
            &access_token,
            &addr_info.chain_path,
            chain_index_num,
            &addr_info.address,
            recipient,
            amt,
            contract_token,
            &session.session_cert,
            None, None, None, None, None, None, None,
            if enable_gas_station { Some(true) } else { None },
            gas_token_address,
            relayer_id,
        )
        .await
        .map_err(format_api_error)?;

    // Tx type not eligible for Gas Station — bail only when no signable payload was returned
    // (checked before the gasStationUsed bail so the message is actionable).
    if unsigned.gs_status() == crate::wallet_api::GasStationStatus::NotSupportIntention
        && !unsigned.has_sign_material()
    {
        return Err(gs_not_supported_err(&addr_info.address));
    }

    if !unsigned.gas_station_used {
        bail!("Gas Station not activated by backend for this transaction");
    }

    // Terminal diagnostic states — never broadcast.
    if unsigned.has_pending_tx {
        return emit_gs_pending_tx_state();
    }
    if unsigned.insufficient_all {
        return emit_gs_insufficient_all_state(&unsigned, &addr_info.address);
    }

    let execute_ok = match &unsigned.execute_result {
        Value::Bool(b) => *b,
        Value::Null => true,
        _ => true,
    };
    if !execute_ok {
        let err_msg = if unsigned.execute_error_msg.is_empty() {
            "transaction simulation failed".to_string()
        } else {
            unsigned.execute_error_msg.clone()
        };
        bail!("transaction simulation failed: {}", err_msg);
    }

    // Guard: only sign + broadcast when the backend actually returned signing
    // material (READY_TO_USE). A FIRST_TIME_PROMPT / no-material response means
    // Gas Station is not yet activated for this (account, chain); broadcasting an
    // empty msgForSign makes the backend TEE reject with code 81358 ("empty
    // signedTx"). Keep this set in sync with the branches in `gs_build_msg_for_sign`
    // — the guard must pass iff the signer can produce a `sessionSignature`.
    let has_sign_material = !unsigned.unsigned_tx_hash.is_empty()
        || !unsigned.hash.is_empty()
        || !unsigned.eip712_message_hash.is_empty();

    if !has_sign_material {
        if gas_token_address.is_some() {
            // A specific token was pinned but the backend still withheld signing
            // material — activation did not complete. Surface clearly; never broadcast.
            bail!(
                "Gas Station returned no signing material despite a pinned token \
                 (status: {}). Activation did not complete; retry or pick another token.",
                unsigned.gas_station_status
            );
        }
        // Auto-select (`--enable-gas-station` with no token): the backend cannot
        // activate first-time Gas Station without an explicit token. Route back
        // through the confirm / nextStep flow instead of broadcasting empty.
        return match classify_gs_phase1(&unsigned) {
            GsPhase1Decision::FirstTime => Err(build_gs_first_time_prompt(&addr_info, &unsigned)),
            GsPhase1Decision::Reenable => Err(build_gs_reenable_prompt(&addr_info, &unsigned)),
            GsPhase1Decision::AutoPick {
                fee_token_address,
                relayer_id,
                needs_enable,
            } => {
                // CLI picks a sufficient token and re-runs; the next unsignedInfo
                // call activates Gas Station and returns the signing material.
                Box::pin(gas_station_send(
                    amt,
                    recipient,
                    chain,
                    from,
                    contract_token,
                    force,
                    Some(&fee_token_address),
                    Some(&relayer_id),
                    needs_enable,
                ))
                .await
            }
            GsPhase1Decision::NeedsUserPick => Err(build_gs_token_selection_prompt(&unsigned)),
        };
    }

    let resp = gas_station_sign_and_broadcast(
        &mut client,
        &access_token,
        &_account_id,
        &addr_info,
        &session,
        &unsigned,
        force,
        recipient,
        amt,
        contract_token,
    )
    .await?;
    output::success(json!({
        "txHash": resp.tx_hash,
        "orderId": resp.order_id,
        "gasStationUsed": true,
        "serviceCharge": unsigned.service_charge,
        "serviceChargeSymbol": unsigned.service_charge_symbol,
    }));
    Ok(())
}

// ── Gas Station broadcast helpers ────────────────────────────────────
//
// Two distinct broadcast flows:
//
// Flow 1: gs_broadcast_with_7702_upgrade (needUpdate7702=true)
//   First-time Gas Station — upgrades wallet to 7702 + executes transaction in one broadcast.
//   Signs both 712 hash and 7702 authHash. Passes nonce(eoaNonce), user7702Data.
//   After this succeeds, wallet is upgraded; subsequent txs use Flow 2.
//
// Flow 2: gs_broadcast_transaction (needUpdate7702=false)
//   Normal Gas Station — wallet already upgraded to 7702, just executes transaction.
//   Signs only 712 hash. No nonce/user7702Data/authSignatureFor7702.

/// Gas Station msgForSign: TEE flow (sessionSignature), plus
/// authSignatureFor7702 when this is a 7702 upgrade. Does NOT write the
/// `signature` field — that is the EIP-191 signature from the Pay flow;
/// Gas Station goes through TEE, not Pay.
pub(super) fn gs_build_msg_for_sign(
    unsigned: &crate::wallet_api::UnsignedInfoResponse,
    session: &crate::wallet_store::SessionJson,
    signing_seed: &[u8],
) -> Result<Value> {
    let mut m = serde_json::Map::new();

    let signing_seed_b64 = base64::engine::general_purpose::STANDARD.encode(signing_seed);

    // Non-empty eip712_message_hash → ed25519_sign_encoded (standard TEE-flow
    // algorithm, same as unsigned_tx_hash → sessionSignature); the signature
    // is written into sessionSignature.
    if !unsigned.eip712_message_hash.is_empty() {
        let session_sig = crate::crypto::ed25519_sign_encoded(
            &unsigned.eip712_message_hash,
            &signing_seed_b64,
            &unsigned.encoding,
        )?;
        m.insert("sessionSignature".into(), json!(session_sig));
    }
    // Backward compatibility for the legacy `hash` field (newer backend
    // no longer returns it).
    if !unsigned.hash.is_empty() && unsigned.eip712_message_hash.is_empty() {
        let session_sig = crate::crypto::ed25519_sign_encoded(
            &unsigned.hash,
            &signing_seed_b64,
            &unsigned.encoding,
        )?;
        m.insert("sessionSignature".into(), json!(session_sig));
    }
    // Solana GS Phase 2: backend puts the message bytes to sign in
    // `unsignedTxHash` and the same base64 in `unsignedTx` for the broadcast
    // to relay on-chain. Sign `unsignedTxHash` via ed25519_sign_encoded →
    // write `sessionSignature`; mirror the standard `sign_and_broadcast`
    // Solana path.
    if !unsigned.unsigned_tx_hash.is_empty() {
        let sig = crate::crypto::ed25519_sign_encoded(
            &unsigned.unsigned_tx_hash,
            &signing_seed_b64,
            &unsigned.encoding,
        )?;
        m.insert("unsignedTxHash".into(), json!(&unsigned.unsigned_tx_hash));
        m.insert("sessionSignature".into(), json!(sig));
    }
    // Solana: pass the base64 message bytes through to broadcast so the
    // Relayer can submit it together with the user signature above.
    if !unsigned.unsigned_tx.is_empty() {
        m.insert("unsignedTx".into(), json!(&unsigned.unsigned_tx));
    }
    // Sign authHashFor7702 → authSignatureFor7702 whenever the backend
    // returned a non-empty 7702 auth hash (signal that the upgrade is needed).
    if !unsigned.auth_hash_for7702.is_empty() {
        let sig = crate::crypto::ed25519_sign_hex(&unsigned.auth_hash_for7702, &signing_seed_b64)?;
        m.insert("authSignatureFor7702".into(), json!(sig));
    }
    // sessionCert
    if !session.session_cert.is_empty() {
        m.insert("sessionCert".into(), json!(session.session_cert));
    }
    Ok(Value::Object(m))
}

/// Layer Gas Station core fields (no transfer semantics) onto an existing
/// extraData object. Sets paymentType, service charge, contract nonce,
/// relayer context, user712Data, and optionally nonce + user7702Data for
/// the 7702 upgrade case.
///
/// Does NOT touch `toAdr` / `coinAmount` / `tokenAddress` — those belong to
/// transfer semantics (wallet send) and do not apply to contract-call.
/// Wallet-send callers must additionally invoke `gs_apply_transfer_info`.
///
/// Does NOT touch `txType` — aligned with master: only wallet-send (non
/// contract-call) writes txType=2 in `sign_and_broadcast`; contract-call
/// paths (including GS contract-call) leave it unset for backend to derive.
pub(super) fn gs_apply_extra_data_fields(
    ed: &mut Value,
    unsigned: &crate::wallet_api::UnsignedInfoResponse,
) {
    ed["paymentType"] = json!("token");

    // Gas service charge.
    ed["serviceCharge"] = json!(unsigned.service_charge);
    ed["feeTokenAddress"] = json!(unsigned.service_charge_fee_token_address);
    // Contract nonce.
    if !unsigned.contract_nonce.is_empty() {
        ed["contractNonce"] = json!(unsigned.contract_nonce);
    }
    // relayerId + context: match against the selected token in tokenList.
    if let Some(selected) = unsigned.gas_station_token_list.iter().find(|t| {
        t.fee_token_address == unsigned.service_charge_fee_token_address
    }) {
        ed["relayerId"] = json!(selected.relayer_id);
        ed["context"] = json!(selected.context);
    }
    // user712Data: pass through verbatim on every Gas Station transaction.
    if !unsigned.user712_data.is_null() {
        ed["user712Data"] = unsigned.user712_data.clone();
    }

    // ── 7702 upgrade only fields — gated on the same signal that gates
    //    signing (authHashFor7702 presence) to stay consistent. ──
    if !unsigned.auth_hash_for7702.is_empty() {
        if !unsigned.eoa_nonce.is_empty() {
            ed["nonce"] = json!(unsigned.eoa_nonce);
        }
        if !unsigned.user7702_data.is_null() {
            ed["user7702Data"] = unsigned.user7702_data.clone();
        }
    }
}

/// Layer transaction amount + optional transfer semantics onto an existing
/// extraData object. Called by both wallet-send GS and contract-call/swap GS
/// paths to ensure consistent handling of the business amount (`coinAmount`).
///
/// - `coin_amount`: always written. Wallet-send passes the transferred amount
///   (e.g. ERC-20 raw units); contract-call / swap passes `tx.value` (the
///   native value attached to the call, typically "0" for ERC-20 swaps).
/// - `to_addr`: written only when `Some`. Wallet-send passes `Some(recipient)`.
///   Contract-call / swap passes `None` so that the field stays consistent
///   with master behavior (CLI does not derive it from `tx.contract_addr`,
///   which equals the call target / router for swap).
/// - `token_address`: written only when `Some`. Wallet-send passes the ERC-20
///   contract address; contract-call / swap passes `None` for the same
///   master-consistency reason.
#[allow(dead_code)]
pub(super) fn gs_apply_transfer_info(
    ed: &mut Value,
    to_addr: Option<&str>,
    coin_amount: &str,
    token_address: Option<&str>,
) {
    if let Some(addr) = to_addr {
        ed["toAdr"] = json!(addr);
    }
    ed["coinAmount"] = json!(coin_amount);
    if let Some(ta) = token_address {
        ed["tokenAddress"] = json!(ta);
    }
}

/// Build the base extraData: master fields + Gas Station fields.
/// Gas Station fields are layered on top of the normal broadcast structure.
#[allow(clippy::too_many_arguments)]
pub(super) fn gs_build_extra_data(
    unsigned: &crate::wallet_api::UnsignedInfoResponse,
    msg_for_sign: &Value,
    to_addr: &str,
    coin_amount: &str,
    token_address: Option<&str>,
    force: bool,
) -> Value {
    // Start from unsignedInfo.extraData (backend passthrough)
    let mut ed = if unsigned.extra_data.is_object() {
        unsigned.extra_data.clone()
    } else {
        json!({})
    };

    // ── Master base fields (same as sign_and_broadcast) ──
    ed["checkBalance"] = json!(true);
    ed["uopHash"] = json!(unsigned.uop_hash);
    ed["encoding"] = json!(unsigned.encoding);
    ed["signType"] = json!(unsigned.sign_type);
    ed["msgForSign"] = msg_for_sign.clone();
    if force {
        ed["skipWarning"] = json!(true);
    }

    gs_apply_extra_data_fields(&mut ed, unsigned);
    // toAdr / tokenAddress / coinAmount intentionally NOT written — aligned
    // with master: unsignedInfo.extraData is passthrough, backend owns those
    // transfer-semantic fields.
    let _ = (to_addr, coin_amount, token_address);

    ed
}

/// Flow 1: first-time Gas Station — upgrades to 7702 + executes the transaction
/// (`needUpdate7702=true`).
#[allow(clippy::too_many_arguments)]
pub(super) async fn gs_broadcast_with_7702_upgrade(
    client: &mut crate::wallet_api::WalletApiClient,
    access_token: &str,
    account_id: &str,
    addr_info: &crate::wallet_store::AddressInfo,
    session: &crate::wallet_store::SessionJson,
    unsigned: &crate::wallet_api::UnsignedInfoResponse,
    force: bool,
    to_addr: &str,
    coin_amount: &str,
    token_address: Option<&str>,
) -> Result<crate::wallet_api::BroadcastResponse> {
    let signing_seed =
        crate::crypto::hpke_decrypt_session_sk(&session.encrypted_session_sk, &crate::keyring_store::get("session_key")
            .map_err(|_| anyhow::anyhow!(super::super::common::ERR_NOT_LOGGED_IN))?)?;

    let msg_for_sign = gs_build_msg_for_sign(unsigned, session, &signing_seed)?;
    let extra_data_obj = gs_build_extra_data(unsigned, &msg_for_sign, to_addr, coin_amount, token_address, force);

    gs_do_broadcast(client, access_token, account_id, addr_info, &extra_data_obj, force).await
}

/// Flow 2: subsequent Gas Station transactions (`needUpdate7702=false`,
/// wallet already upgraded to 7702).
#[allow(clippy::too_many_arguments)]
pub(super) async fn gs_broadcast_transaction(
    client: &mut crate::wallet_api::WalletApiClient,
    access_token: &str,
    account_id: &str,
    addr_info: &crate::wallet_store::AddressInfo,
    session: &crate::wallet_store::SessionJson,
    unsigned: &crate::wallet_api::UnsignedInfoResponse,
    force: bool,
    to_addr: &str,
    coin_amount: &str,
    token_address: Option<&str>,
) -> Result<crate::wallet_api::BroadcastResponse> {
    let signing_seed =
        crate::crypto::hpke_decrypt_session_sk(&session.encrypted_session_sk, &crate::keyring_store::get("session_key")
            .map_err(|_| anyhow::anyhow!(super::super::common::ERR_NOT_LOGGED_IN))?)?;

    let msg_for_sign = gs_build_msg_for_sign(unsigned, session, &signing_seed)?;
    let extra_data_obj = gs_build_extra_data(unsigned, &msg_for_sign, to_addr, coin_amount, token_address, force);

    gs_do_broadcast(client, access_token, account_id, addr_info, &extra_data_obj, force).await
}

/// Gas Station broadcast: shared send logic + debug dump.
pub(super) async fn gs_do_broadcast(
    client: &mut crate::wallet_api::WalletApiClient,
    access_token: &str,
    account_id: &str,
    addr_info: &crate::wallet_store::AddressInfo,
    extra_data_obj: &Value,
    force: bool,
) -> Result<crate::wallet_api::BroadcastResponse> {
    let extra_data_str =
        serde_json::to_string(extra_data_obj).context("failed to serialize extraData")?;

    let broadcast_resp = client
        .broadcast_transaction(
            access_token,
            account_id,
            &addr_info.address,
            &addr_info.chain_index,
            &extra_data_str,
            None,
        )
        .await
        .map_err(|e| handle_confirming_error(e, force))?;

    Ok(broadcast_resp)
}

/// Gas Station: route to the matching broadcast flow based on `needUpdate7702`.
#[allow(clippy::too_many_arguments)]
pub(super) async fn gas_station_sign_and_broadcast(
    client: &mut crate::wallet_api::WalletApiClient,
    access_token: &str,
    account_id: &str,
    addr_info: &crate::wallet_store::AddressInfo,
    session: &crate::wallet_store::SessionJson,
    unsigned: &crate::wallet_api::UnsignedInfoResponse,
    force: bool,
    to_addr: &str,
    coin_amount: &str,
    token_address: Option<&str>,
) -> Result<crate::wallet_api::BroadcastResponse> {
    if unsigned.need_update7702 {
        gs_broadcast_with_7702_upgrade(
            client, access_token, account_id, addr_info, session, unsigned,
            force, to_addr, coin_amount, token_address,
        ).await
    } else {
        gs_broadcast_transaction(
            client, access_token, account_id, addr_info, session, unsigned,
            force, to_addr, coin_amount, token_address,
        ).await
    }
}

// ── Gas Station terminal-state emitters ───────────────────────────────────
// These are *diagnostic success* from the CLI's perspective — the CLI's Phase 1 call
// completed and correctly identified a state where the transfer cannot proceed. The Agent
// reads the JSON flags (`hasPendingTx` / `insufficientAll`) to surface the right passive
// template to the user; see `skills/okx-agentic-wallet/references/gas-station.md`
// "Passive Response Templates".

/// HAS_PENDING_TX: a prior Gas Station tx is still processing; caller cannot proceed.
pub(super) fn emit_gs_pending_tx_state() -> Result<()> {
    output::success(json!({
        "scene": "gs_pending_tx",
        "gasStationUsed": true,
        "hasPendingTx": true,
    }));
    Ok(())
}

/// INSUFFICIENT_ALL: every supported stablecoin is below the service-charge requirement;
/// caller must top up. Emits structured state including `fromAddr` so the Agent can render
/// a top-up hint.
pub(super) fn emit_gs_insufficient_all_state(
    unsigned: &crate::wallet_api::UnsignedInfoResponse,
    from_addr: &str,
) -> Result<()> {
    output::success(json!({
        "scene": "gs_insufficient_all",
        "gasStationUsed": true,
        "insufficientAll": true,
        "gasStationTokenList": unsigned.gas_station_token_list,
        "fromAddr": from_addr,
    }));
    Ok(())
}

/// Tx type (deposit / stake / etc.) not eligible for Gas Station — only transfer and swap are.
pub(super) fn gs_not_supported_err(from_addr: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "Gas Station does not support this transaction type — only transfers and swaps can pay \
         gas with a stablecoin. Pay with native SOL instead, then retry. Top up SOL at: {from_addr}"
    )
}

/// Serialize the full `gasStationTokenList` as JSON for inclusion in a `CliConfirming.next`
/// field. Downstream Agents parse this to reconstruct addresses / relayerIds when the user
/// picks a token.
pub(super) fn token_list_json(unsigned: &crate::wallet_api::UnsignedInfoResponse) -> String {
    serde_json::to_string(&unsigned.gas_station_token_list).unwrap_or_default()
}

/// Build sufficient-token list string for CliConfirming messages
pub(super) fn format_sufficient_tokens(unsigned: &crate::wallet_api::UnsignedInfoResponse) -> String {
    unsigned
        .gas_station_token_list
        .iter()
        .filter(|t| t.sufficient)
        .enumerate()
        .map(|(i, t)| format!("{}. {} (balance: {}, fee: {})", i + 1, t.symbol, t.balance, t.service_charge))
        .collect::<Vec<_>>()
        .join("\n")
}

/// FIRST_TIME_PROMPT: first-time enable. Emits a minimal Confirming with enough structured
/// data for the Agent to render the user-facing prompt via the Scene A template in
/// `skills/okx-agentic-wallet/references/gas-station.md`. Product copy (education paragraph,
/// academy link, "after enabling" bullets) lives in the skill — not duplicated here.
pub(super) fn build_gs_first_time_prompt(
    addr_info: &crate::wallet_store::AddressInfo,
    unsigned: &crate::wallet_api::UnsignedInfoResponse,
) -> anyhow::Error {
    let chain_display = crate::chains::chain_display_name(&addr_info.chain_index);
    let sufficient_summary = format_sufficient_tokens(unsigned);
    let message = format!(
        "Gas Station first-time setup required on {chain_display}. Render the user-facing prompt via the Scene A template in `skills/okx-agentic-wallet/references/gas-station.md` (do NOT paraphrase). Sufficient stablecoins now:\n{sufficient_summary}"
    );
    let next = format!(
        "On user pick `1` (decline): do not re-run; the user must top up native token.\n\
         On user pick `N` (N >= 2, one per sufficient token above): re-run `wallet send --enable-gas-station --gas-token-address <addr> --relayer-id <id>` with the chosen token.\n\
         Token list: {}",
        token_list_json(unsigned)
    );
    crate::output::CliConfirming { message, next, scene: Some("gs_first_time".into()) }.into()
}

/// REENABLE_ONLY: Gas Station was explicitly disabled by the user earlier. Backend overwrites
/// the previous default with the picked token on re-enable. Emits minimal Confirming; user-facing
/// wording lives in the Scene B' template in gas-station.md.
pub(super) fn build_gs_reenable_prompt(
    addr_info: &crate::wallet_store::AddressInfo,
    unsigned: &crate::wallet_api::UnsignedInfoResponse,
) -> anyhow::Error {
    let chain_display = crate::chains::chain_display_name(&addr_info.chain_index);
    let sufficient_summary = format_sufficient_tokens(unsigned);
    let message = format!(
        "Gas Station re-enable required on {chain_display} — the user previously disabled it. Render the user-facing prompt via the Scene B' template in `skills/okx-agentic-wallet/references/gas-station.md` (do NOT paraphrase). Previous default gas token address: {prev}. Sufficient stablecoins now:\n{sufficient_summary}",
        prev = if unsigned.default_gas_token_address.is_empty() {
            "(none)"
        } else {
            &unsigned.default_gas_token_address
        }
    );
    let next = format!(
        "On user pick `1` (decline): do not re-run; the user must top up native token.\n\
         On user pick `N` (N >= 2, one per sufficient token above): re-run `wallet send --enable-gas-station --gas-token-address <addr> --relayer-id <id>` with the chosen token. Backend will overwrite the previous default with the picked token.\n\
         Token list: {}",
        token_list_json(unsigned)
    );
    crate::output::CliConfirming { message, next, scene: Some("gs_reenable".into()) }.into()
}

/// Call-site adapter for the `sign_and_broadcast` (contract-call / send via TxParams)
/// path: build the `original_args` payload and pick the right command name, then
/// delegate to `build_gs_setup_required`.
pub(super) fn force_setup_required_for_tx_params(
    is_reenable: bool,
    is_contract_call: bool,
    chain: &str,
    from: Option<&str>,
    tx: &TxParams<'_>,
    addr_info: &crate::wallet_store::AddressInfo,
    unsigned: &crate::wallet_api::UnsignedInfoResponse,
) -> anyhow::Error {
    let original_args = serde_json::json!({
        "chain": chain,
        "from": from,
        "toAddr": tx.to_addr,
        "value": tx.value,
        "contractAddr": tx.contract_addr,
        "inputData": tx.input_data,
        "force": true,
    });
    let cmd_name = if is_contract_call {
        "wallet contract-call"
    } else {
        "wallet send"
    };
    build_gs_setup_required(addr_info, unsigned, is_reenable, cmd_name, original_args)
}

/// Call-site adapter for the `cmd_send` path: build the `original_args` payload
/// from send-style args, then delegate to `build_gs_setup_required`.
#[allow(clippy::too_many_arguments)]
pub(super) fn force_setup_required_for_send(
    is_reenable: bool,
    chain: &str,
    from: Option<&str>,
    recipient: &str,
    amount: &str,
    contract_token: Option<&str>,
    addr_info: &crate::wallet_store::AddressInfo,
    unsigned: &crate::wallet_api::UnsignedInfoResponse,
) -> anyhow::Error {
    let original_args = serde_json::json!({
        "chain": chain,
        "from": from,
        "recipient": recipient,
        "amount": amount,
        "contractToken": contract_token,
        "force": true,
    });
    build_gs_setup_required(addr_info, unsigned, is_reenable, "wallet send", original_args)
}

/// `--force` + GS first-time / re-enable required: build a `CliSetupRequired` error
/// (exit 3, errorCode = `GAS_STATION_SETUP_REQUIRED`). Used when a third-party plugin
/// invokes `wallet send` / `wallet contract-call` with `--force` and hits a state where
/// silent auto-enable would violate the user-consent contract.
///
/// The error data carries enough info for the agent to:
///   1. Render Scene A / B' from the bundled tokenList
///   2. Run `wallet gas-station setup` after user picks
///   3. Re-invoke the same plugin command (which will now succeed because GS is active)
pub(super) fn build_gs_setup_required(
    addr_info: &crate::wallet_store::AddressInfo,
    unsigned: &crate::wallet_api::UnsignedInfoResponse,
    is_reenable: bool,
    original_command: &str,
    original_args: serde_json::Value,
) -> anyhow::Error {
    let chain_display = crate::chains::chain_display_name(&addr_info.chain_index);
    let token_list: Vec<serde_json::Value> = unsigned
        .gas_station_token_list
        .iter()
        .map(|t| {
            serde_json::json!({
                "symbol": t.symbol,
                "feeTokenAddress": t.fee_token_address,
                "relayerId": t.relayer_id,
                "balance": t.balance,
                "serviceCharge": t.service_charge,
                "sufficient": t.sufficient,
            })
        })
        .collect();

    let scene = if is_reenable { "B'" } else { "A" };
    // message is self-describing — embeds a concrete executable command so even minimal
    // plugin error wrapping (e.g. `bail!("...: {}", stdout)`) lets the agent see the
    // setup command string directly without parsing structured `data`.
    let setup_hint = format!(
        "onchainos wallet gas-station setup --chain {} --gas-token-address <picked> --relayer-id <picked>",
        addr_info.chain_index
    );
    let message = format!(
        "Gas Station first-time setup required on {chain_display}. \
         Cannot proceed under `--force` because first-time activation needs explicit user consent. \
         Run `{setup_hint}` first (after rendering Scene {scene} to the user), then re-invoke the same command."
    );

    // data carries originalRequest + retryGuidance so an agent can recover via fast path.
    let data = serde_json::json!({
        "chainId": addr_info.chain_index,
        "chainName": chain_display,
        "fromAddress": addr_info.address,
        "scene": scene,
        "gasStationStatus": unsigned.gas_station_status,
        "defaultGasTokenAddress": unsigned.default_gas_token_address,
        "tokenList": token_list,
        "originalRequest": {
            "command": original_command,
            "args": original_args,
        },
        "retryGuidance": [
            format!("1) Render Scene {} via `skills/okx-agentic-wallet/references/gas-station.md` using `data.tokenList`.", scene),
            "2) On user pick, run `wallet gas-station setup --chain <chainId> --gas-token-address <picked.feeTokenAddress> --relayer-id <picked.relayerId>`.".to_string(),
            "3) Re-invoke the original command verbatim (it will succeed because Gas Station is now active).".to_string(),
        ],
    });

    crate::output::CliSetupRequired {
        error_code: "GAS_STATION_SETUP_REQUIRED".to_string(),
        message,
        data,
    }
    .into()
}

/// Scene C: READY_TO_USE but user input is needed to pick a token. Covers both "default
/// present but insufficient" and "no default + multiple sufficient tokens". Emits minimal
/// Confirming; user-facing wording lives in the Scene C template in gas-station.md.
pub(super) fn build_gs_token_selection_prompt(
    unsigned: &crate::wallet_api::UnsignedInfoResponse,
) -> anyhow::Error {
    let token_list_str = format_sufficient_tokens(unsigned);
    let message = format!(
        "Gas Station needs a token pick on this chain (default is missing or insufficient). Render the user-facing prompt via the Scene C template in `skills/okx-agentic-wallet/references/gas-station.md` (do NOT paraphrase). Sufficient stablecoins now:\n{token_list_str}"
    );
    let next = format!(
        "On user pick (this-time-only option): re-run with `--gas-token-address <addr> --relayer-id <id>`.\n\
         On user pick (set-as-new-default option): same re-run, then call `wallet gas-station update-default-token --chain <chain> --gas-token-address <addr>` after the tx completes.\n\
         Token list: {}",
        token_list_json(unsigned)
    );
    crate::output::CliConfirming { message, next, scene: Some("gs_token_switch".into()) }.into()
}

// ── Gas Station Phase 1 dispatch ───────────────────────────────────────────

/// Outcome of classifying a Phase 1 diagnostic response. Each variant maps to a distinct
/// Agent/CLI action; see callers for the per-site action (sign_and_broadcast reuses
/// `unsigned` in-place, cmd_send re-invokes via `gas_station_send`).
#[derive(Debug)]
pub(super) enum GsPhase1Decision {
    /// `FIRST_TIME_PROMPT`: first-time enable needs explicit user consent.
    FirstTime,
    /// `REENABLE_ONLY`: user previously disabled; re-enable needs explicit consent.
    Reenable,
    /// Scene B auto-pick: resume silently with this token. `needs_enable` is true when
    /// the chain still requires 7702 activation (PENDING_UPGRADE).
    AutoPick {
        fee_token_address: String,
        relayer_id: String,
        needs_enable: bool,
    },
    /// Scene C: user must pick a token (default insufficient, or ambiguous fallback).
    NeedsUserPick,
}

/// Classify a Phase 1 diagnostic response into the matching Scene. Callers own the action.
pub(super) fn classify_gs_phase1(
    unsigned: &crate::wallet_api::UnsignedInfoResponse,
) -> GsPhase1Decision {
    use crate::wallet_api::GasStationStatus as GS;
    let status = unsigned.gs_status();

    if unsigned.gas_station_first_time_prompt || status == GS::FirstTimePrompt {
        return GsPhase1Decision::FirstTime;
    }
    if status == GS::ReenableOnly {
        return GsPhase1Decision::Reenable;
    }
    match unsigned.auto_pick_gas_token() {
        Some(token) => GsPhase1Decision::AutoPick {
            fee_token_address: token.fee_token_address.clone(),
            relayer_id: token.relayer_id.clone(),
            needs_enable: status == GS::PendingUpgrade,
        },
        None => GsPhase1Decision::NeedsUserPick,
    }
}

/// PENDING_UPGRADE / REENABLE_ONLY / READY_TO_USE (default token has
/// sufficient balance): backend already returned the hash material — sign
/// and broadcast directly.
#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_gs_auto_sign_broadcast(
    client: &mut crate::wallet_api::WalletApiClient,
    access_token: &str,
    account_id: &str,
    addr_info: &crate::wallet_store::AddressInfo,
    session: &crate::wallet_store::SessionJson,
    unsigned: &crate::wallet_api::UnsignedInfoResponse,
    force: bool,
    recipient: &str,
    amt: &str,
    contract_token: Option<&str>,
) -> Result<()> {
    let resp = gas_station_sign_and_broadcast(
        client, access_token, account_id, addr_info, session, unsigned,
        force, recipient, amt, contract_token,
    )
    .await?;
    output::success(json!({
        "txHash": resp.tx_hash,
        "orderId": resp.order_id,
        "gasStationUsed": true,
        "autoSelectedToken": unsigned.auto_selected_token,
        "serviceCharge": unsigned.service_charge,
        "serviceChargeSymbol": unsigned.service_charge_symbol,
        "gasStationTokenList": unsigned.gas_station_token_list,
    }));
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Gas Station user-facing Confirming helpers ──

    use crate::test_helpers::gas_station::{
        make_token_full as mk_token,
        make_unsigned_with_tokens as mk_unsigned,
    };

    #[test]
    fn format_sufficient_tokens_filters_and_indexes_from_one() {
        let unsigned = mk_unsigned(
            "",
            vec![
                mk_token("USDT", "0xaaa", "100", "0.13", false), // filtered out
                mk_token("USDC", "0xbbb", "120", "0.14", true),
                mk_token("USDG", "0xccc", "50", "0.15", true),
            ],
        );
        let out = format_sufficient_tokens(&unsigned);
        assert!(out.contains("1. USDC"));
        assert!(out.contains("2. USDG"));
        assert!(!out.contains("USDT")); // insufficient token excluded
    }

    #[test]
    fn format_sufficient_tokens_empty_when_all_insufficient() {
        let unsigned = mk_unsigned(
            "",
            vec![mk_token("USDT", "0xaaa", "0", "0.13", false)],
        );
        assert_eq!(format_sufficient_tokens(&unsigned), "");
    }

    // ── Gas Station setup-required (exit 3) builders ──

    fn mk_addr_info(chain_index: &str, address: &str) -> crate::wallet_store::AddressInfo {
        crate::wallet_store::AddressInfo {
            account_id: "acct-1".to_string(),
            address: address.to_string(),
            chain_index: chain_index.to_string(),
            chain_name: "eth".to_string(),
            address_type: "eoa".to_string(),
            chain_path: "m/44'/60'/0'/0/0".to_string(),
        }
    }

    #[test]
    fn build_gs_setup_required_first_time_carries_scene_a_and_token_list() {
        let addr = mk_addr_info("42161", "0xaef7");
        let unsigned = mk_unsigned(
            "",
            vec![
                mk_token("USDC", "0xaaa", "1.50", "0.026", true),
                mk_token("USDT", "0xbbb", "0", "0.026", false),
            ],
        );
        let original_args = serde_json::json!({"chain": "42161", "force": true});
        let err = build_gs_setup_required(
            &addr, &unsigned, /*is_reenable*/ false, "wallet contract-call", original_args,
        );
        let setup = err
            .downcast_ref::<crate::output::CliSetupRequired>()
            .expect("CliSetupRequired");
        assert_eq!(setup.error_code, "GAS_STATION_SETUP_REQUIRED");
        assert_eq!(setup.data["scene"], "A");
        assert_eq!(setup.data["chainId"], "42161");
        assert_eq!(setup.data["fromAddress"], "0xaef7");
        assert_eq!(setup.data["originalRequest"]["command"], "wallet contract-call");
        assert_eq!(setup.data["originalRequest"]["args"]["chain"], "42161");
        assert_eq!(setup.data["tokenList"].as_array().unwrap().len(), 2);
        assert_eq!(setup.data["retryGuidance"].as_array().unwrap().len(), 3);
        assert!(setup.message.contains("wallet gas-station setup --chain 42161"));
        assert!(setup.message.contains("Scene A"));
    }

    #[test]
    fn build_gs_setup_required_reenable_carries_scene_b_prime() {
        let addr = mk_addr_info("1", "0xabc");
        let unsigned = mk_unsigned("0xaaa", vec![mk_token("USDC", "0xaaa", "1", "0.04", true)]);
        let err = build_gs_setup_required(
            &addr, &unsigned, /*is_reenable*/ true, "wallet send",
            serde_json::json!({"force": true}),
        );
        let setup = err
            .downcast_ref::<crate::output::CliSetupRequired>()
            .expect("CliSetupRequired");
        assert_eq!(setup.data["scene"], "B'");
        assert!(setup.message.contains("Scene B'"));
        assert_eq!(setup.data["originalRequest"]["command"], "wallet send");
    }

    #[test]
    fn force_setup_required_for_tx_params_picks_contract_call_command_name() {
        let addr = mk_addr_info("42161", "0xaef7");
        let unsigned = mk_unsigned("", vec![mk_token("USDC", "0xaaa", "1", "0.04", true)]);
        let tx = TxParams {
            to_addr: "0xpool",
            value: "0",
            contract_addr: Some("0xtoken"),
            input_data: Some("0xdeadbeef"),
            unsigned_tx: None,
            gas_limit: None,
            aa_dex_token_addr: None,
            aa_dex_token_amount: None,
            jito_unsigned_tx: None,
            gas_token_address: None,
            relayer_id: None,
            enable_gas_station: false,
        };
        let err = force_setup_required_for_tx_params(
            /*is_reenable*/ false, /*is_contract_call*/ true,
            "42161", Some("0xaef7"), &tx, &addr, &unsigned,
        );
        let setup = err
            .downcast_ref::<crate::output::CliSetupRequired>()
            .expect("CliSetupRequired");
        assert_eq!(setup.data["originalRequest"]["command"], "wallet contract-call");
        assert_eq!(setup.data["originalRequest"]["args"]["toAddr"], "0xpool");
        assert_eq!(setup.data["originalRequest"]["args"]["inputData"], "0xdeadbeef");
        assert_eq!(setup.data["originalRequest"]["args"]["force"], true);
    }

    #[test]
    fn force_setup_required_for_tx_params_picks_send_command_name() {
        let addr = mk_addr_info("42161", "0xaef7");
        let unsigned = mk_unsigned("", vec![mk_token("USDC", "0xaaa", "1", "0.04", true)]);
        let tx = TxParams {
            to_addr: "0xrecipient",
            value: "0",
            contract_addr: None,
            input_data: None,
            unsigned_tx: None,
            gas_limit: None,
            aa_dex_token_addr: None,
            aa_dex_token_amount: None,
            jito_unsigned_tx: None,
            gas_token_address: None,
            relayer_id: None,
            enable_gas_station: false,
        };
        let err = force_setup_required_for_tx_params(
            false, /*is_contract_call*/ false,
            "42161", None, &tx, &addr, &unsigned,
        );
        let setup = err
            .downcast_ref::<crate::output::CliSetupRequired>()
            .expect("CliSetupRequired");
        assert_eq!(setup.data["originalRequest"]["command"], "wallet send");
    }

    #[test]
    fn force_setup_required_for_send_carries_send_args() {
        let addr = mk_addr_info("10", "0xaef7");
        let unsigned = mk_unsigned("", vec![mk_token("USDC", "0xaaa", "1", "0.026", true)]);
        let err = force_setup_required_for_send(
            /*is_reenable*/ false,
            "10", Some("0xaef7"), "0xrecipient", "1000000", Some("0xtoken"),
            &addr, &unsigned,
        );
        let setup = err
            .downcast_ref::<crate::output::CliSetupRequired>()
            .expect("CliSetupRequired");
        assert_eq!(setup.data["originalRequest"]["command"], "wallet send");
        let args = &setup.data["originalRequest"]["args"];
        assert_eq!(args["chain"], "10");
        assert_eq!(args["recipient"], "0xrecipient");
        assert_eq!(args["amount"], "1000000");
        assert_eq!(args["contractToken"], "0xtoken");
        assert_eq!(args["force"], true);
    }
}
