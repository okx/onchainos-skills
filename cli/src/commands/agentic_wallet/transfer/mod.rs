use anyhow::{bail, Context, Result};
use base64::Engine;
use serde_json::{json, Value};

use crate::token_alias::validate_address_for_chain;
use crate::validators::{validate_amount, validate_non_negative_integer};
use crate::keyring_store;
use crate::output;
use crate::wallet_api::{UnsignedInfoResponse, WalletApiClient};
use crate::wallet_store::{self, AddressInfo, WalletsJson};

use super::auth::{ensure_tokens_refreshed, format_api_error};
use super::common::handle_confirming_error;

mod gas_station;
use gas_station::*;

// ── resolve_address ───────────────────────────────────────────────────

/// Resolve a (from, chain) pair to (account_id, AddressInfo).
///
/// If `from_addr` is Some, scan ALL entries in accounts_map for a matching
/// (address, chain_name) pair. Otherwise use selected_account_id.
pub(crate) fn resolve_address(
    wallets: &WalletsJson,
    from_addr: Option<&str>,
    chain: &str,
) -> Result<(String, AddressInfo)> {
    match from_addr {
        Some(from) => {
            for (account_id, entry) in &wallets.accounts_map {
                for addr in &entry.address_list {
                    if addr.address.eq_ignore_ascii_case(from) && addr.chain_name == chain {
                        return Ok((account_id.clone(), addr.clone()));
                    }
                }
            }
            bail!("no address matches from={} chain={}", from, chain);
        }
        None => {
            let acct_id = &wallets.selected_account_id;
            if acct_id.is_empty() {
                bail!("no currentAccountId");
            }
            let entry = wallets
                .accounts_map
                .get(acct_id)
                .ok_or_else(|| anyhow::anyhow!("not found currentAccountId"))?;
            for addr in &entry.address_list {
                if addr.chain_name == chain {
                    return Ok((acct_id.clone(), addr.clone()));
                }
            }
            bail!("no address for chain={} in account={}", chain, acct_id);
        }
    }
}

// ── sign_and_build_extra_data ─────────────────────────────────────────

/// Sign the unsigned info and build the serialized `extraData` JSON string
/// used by `broadcast_transaction`.
#[allow(clippy::too_many_arguments)]
fn sign_and_build_extra_data(
    unsigned: &UnsignedInfoResponse,
    session_cert: &str,
    encrypted_session_sk: &str,
    session_key: &str,
    is_contract_call: bool,
    mev_protection: bool,
    force: bool,
) -> Result<String> {
    let signing_seed =
        crate::crypto::hpke_decrypt_session_sk(encrypted_session_sk, session_key)?;
    let signing_seed_b64 = base64::engine::general_purpose::STANDARD.encode(signing_seed);

    let mut msg_for_sign_map = serde_json::Map::new();

    if !unsigned.hash.is_empty() {
        let sig = crate::crypto::ed25519_sign_eip191(&unsigned.hash, &signing_seed, "hex")?;
        msg_for_sign_map.insert("signature".into(), json!(sig));
    }
    if !unsigned.auth_hash_for7702.is_empty() {
        let sig =
            crate::crypto::ed25519_sign_hex(&unsigned.auth_hash_for7702, &signing_seed_b64)?;
        msg_for_sign_map.insert("authSignatureFor7702".into(), json!(sig));
    }
    if !unsigned.unsigned_tx_hash.is_empty() {
        let sig = crate::crypto::ed25519_sign_encoded(
            &unsigned.unsigned_tx_hash,
            &signing_seed_b64,
            &unsigned.encoding,
        )?;
        msg_for_sign_map.insert("unsignedTxHash".into(), json!(&unsigned.unsigned_tx_hash));
        msg_for_sign_map.insert("sessionSignature".into(), json!(sig));
    }
    if !unsigned.unsigned_tx.is_empty() {
        msg_for_sign_map.insert("unsignedTx".into(), json!(&unsigned.unsigned_tx));
    }
    if !unsigned.jito_unsigned_tx.is_empty() {
        let jito_sig = crate::crypto::ed25519_sign_encoded(
            &unsigned.jito_unsigned_tx,
            &signing_seed_b64,
            &unsigned.encoding,
        )?;
        msg_for_sign_map.insert("jitoUnsignedTx".into(), json!(&unsigned.jito_unsigned_tx));
        msg_for_sign_map.insert("jitoSessionSignature".into(), json!(jito_sig));
    }
    if !session_cert.is_empty() {
        msg_for_sign_map.insert("sessionCert".into(), json!(session_cert));
    }

    let msg_for_sign = Value::Object(msg_for_sign_map);

    let mut extra_data_obj = if unsigned.extra_data.is_object() {
        unsigned.extra_data.clone()
    } else {
        json!({})
    };
    extra_data_obj["checkBalance"] = json!(true);
    extra_data_obj["uopHash"] = json!(unsigned.uop_hash);
    extra_data_obj["encoding"] = json!(unsigned.encoding);
    extra_data_obj["signType"] = json!(unsigned.sign_type);
    extra_data_obj["msgForSign"] = json!(msg_for_sign);
    if !is_contract_call {
        extra_data_obj["txType"] = json!(2);
    }
    if mev_protection {
        extra_data_obj["isMEV"] = json!(true);
    }
    if force {
        extra_data_obj["skipWarning"] = json!(true);
    }

    serde_json::to_string(&extra_data_obj).context("failed to serialize extraData")
}

/// Resolve address with a one-shot refresh fallback.
///
/// If the initial lookup fails (e.g. wallets.json is missing the chain's address
/// because a prior `chainUpdated` push failed to persist), call `refresh` once
/// to fetch the updated wallet state then retry the lookup.
pub(crate) async fn resolve_address_with_refresh<F, Fut>(
    wallets: &mut WalletsJson,
    from: Option<&str>,
    chain_name: &str,
    refresh: F,
) -> Result<(String, AddressInfo)>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<WalletsJson>>,
{
    if let Ok(r) = resolve_address(wallets, from, chain_name) {
        return Ok(r);
    }
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][resolve_address_with_refresh] first attempt failed, refreshing and retrying"
        );
    }
    *wallets = refresh().await?;
    resolve_address(wallets, from, chain_name)
}

// ── sign_and_broadcast ────────────────────────────────────────────────

/// Parameters for the unsignedInfo API call.
pub(super) struct TxParams<'a> {
    to_addr: &'a str,
    value: &'a str,
    contract_addr: Option<&'a str>,
    input_data: Option<&'a str>,
    unsigned_tx: Option<&'a str>,
    gas_limit: Option<&'a str>,
    aa_dex_token_addr: Option<&'a str>,
    aa_dex_token_amount: Option<&'a str>,
    jito_unsigned_tx: Option<&'a str>,
    // Gas Station params (Phase 2 execution)
    gas_token_address: Option<&'a str>,
    relayer_id: Option<&'a str>,
    enable_gas_station: bool,
}

/// Shared flow: resolve wallet → unsignedInfo → sign → broadcast → output txHash.
/// `is_contract_call`: when true, omits `txType` from extraData.
/// `mev_protection`: when true, passes `isMEV: true` to the broadcast API (supported on ETH, BSC, Base).
/// `chain`: the realChainIndex (standard chain ID, e.g. "1" for Ethereum, "501" for Solana).
/// `force`: when true, passes `skipWarning: true` in extraData and bypasses confirmation prompts.
/// `agent_biz_type`: transaction category for broadcast (e.g. "transfer", "dex", "defi", "dapp").
/// `agent_skill_name`: strategy / skill name the caller is using.
#[allow(clippy::too_many_arguments)]
pub(super) async fn sign_and_broadcast(
    chain: &str,
    from: Option<&str>,
    tx: TxParams<'_>,
    is_contract_call: bool,
    mev_protection: bool,
    force: bool,
    tx_source: Option<&str>,
    agent_biz_type: Option<&str>,
    agent_skill_name: Option<&str>,
) -> Result<crate::wallet_api::BroadcastResponse> {
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][sign_and_broadcast] enter: chain={}, from={:?}, to={}, value={}, contractAddr={:?}, inputData={}, unsignedTx={}, gasLimit={:?}, mev={}, agentBizType={:?}, agentSkillName={:?}",
            chain, from, tx.to_addr, tx.value, tx.contract_addr,
            tx.input_data.map(|s| format!("{}...({})", &s[..s.len().min(20)], s.len())).unwrap_or_else(|| "None".into()),
            tx.unsigned_tx.map(|s| format!("{}...({})", &s[..s.len().min(20)], s.len())).unwrap_or_else(|| "None".into()),
            tx.gas_limit,
            mev_protection,
            agent_biz_type,
            agent_skill_name,
        );
    }

    let access_token = ensure_tokens_refreshed().await?;
    if cfg!(feature = "debug-log") {
        eprintln!("[DEBUG][sign_and_broadcast] Step 1: access_token refreshed OK");
    }

    // Resolve realChainIndex to chain entry, then extract chainName for address lookup
    let chain_entry = super::chain::get_chain_by_real_chain_index(chain)
        .await?
        .ok_or_else(|| anyhow::anyhow!("unsupported chain: {chain}"))?;
    let chain_name = chain_entry["chainName"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("chain entry missing chainName for chain {chain}"))?;
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][sign_and_broadcast] Step 1.5: resolved realChainIndex={} -> chainName={}",
            chain, chain_name
        );
    }

    let mut wallets = wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;

    // Fallback: if local wallets.json is missing this chain's address (e.g. a prior
    // chainUpdated push failed to persist), force-refresh via account/address/list once and retry.
    let (account_id, addr_info) =
        resolve_address_with_refresh(&mut wallets, from, chain_name, || async {
            let mut refresh_client = WalletApiClient::new()?;
            let mut fresh = wallet_store::load_wallets()?
                .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
            super::balance::ensure_wallet_accounts_fresh(
                &mut refresh_client,
                &access_token,
                &mut fresh,
                true,
            )
            .await?;
            Ok(fresh)
        })
        .await?;
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][sign_and_broadcast] Step 3: resolve_address => account_id={}, addr={}",
            account_id, addr_info.address
        );
    }

    let session = wallet_store::load_session()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let session_cert = session.session_cert;
    let encrypted_session_sk = session.encrypted_session_sk;
    let session_key = keyring_store::get("session_key")
        .map_err(|_| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][sign_and_broadcast] Step 4: TEE session loaded, session_cert length={}, session_key length={}",
            session_cert.len(), session_key.len()
        );
    }

    let chain_index_num: u64 = addr_info.chain_index.parse().map_err(|_| {
        anyhow::anyhow!("chain id '{}' is not a valid number", addr_info.chain_index)
    })?;

    // ── Address validation ──
    let ci = &addr_info.chain_index;
    validate_address_for_chain(ci, tx.to_addr, "to")?;
    if let Some(ca) = tx.contract_addr {
        validate_address_for_chain(ci, ca, "contract-token")?;
    }
    if let Some(aa_addr) = tx.aa_dex_token_addr {
        validate_address_for_chain(ci, aa_addr, "aa-dex-token-addr")?;
    }
    // ── Optional field validation ──
    if let Some(gl) = tx.gas_limit {
        validate_non_negative_integer(gl, "gas-limit")?;
    }
    if let Some(aa_amount) = tx.aa_dex_token_amount {
        validate_non_negative_integer(aa_amount, "aa-dex-token-amount")?;
    }

    let mut client = WalletApiClient::new()?;
    // Only read swap trace ID from cache for contract calls (swap flow)
    let cached_tid = if is_contract_call {
        crate::wallet_store::get_swap_trace_id().ok().flatten()
    } else {
        None
    };
    let ts_unsigned = chrono::Utc::now().timestamp_millis().to_string();
    let trace_headers_unsigned: Vec<(&str, &str)> = if let Some(ref tid) = cached_tid {
        vec![
            ("ok-client-tid", tid.as_str()),
            ("ok-client-timestamp", ts_unsigned.as_str()),
        ]
    } else {
        vec![]
    };
    let trace_ref = if trace_headers_unsigned.is_empty() {
        None
    } else {
        if cfg!(feature = "debug-log") {
            eprintln!(
                "[DEBUG][sign_and_broadcast] unsignedInfo trace headers: ok-client-tid={}, ok-client-timestamp={}",
                cached_tid.as_deref().unwrap_or(""), ts_unsigned
            );
        }
        Some(trace_headers_unsigned.as_slice())
    };
    let mut unsigned = client
        .pre_transaction_unsigned_info(
            &access_token,
            &addr_info.chain_path,
            chain_index_num,
            &addr_info.address,
            tx.to_addr,
            tx.value,
            tx.contract_addr,
            &session_cert,
            tx.input_data,
            tx.unsigned_tx,
            tx.gas_limit,
            tx.aa_dex_token_addr,
            tx.aa_dex_token_amount,
            tx.jito_unsigned_tx,
            trace_ref,
            if tx.enable_gas_station { Some(true) } else { None },
            tx.gas_token_address,
            tx.relayer_id,
        )
        .await
        .map_err(format_api_error)?;
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][sign_and_broadcast] Step 6: unsignedInfo: hash={}, uopHash={}, executeResult={}",
            unsigned.hash, unsigned.uop_hash, unsigned.execute_result
        );
    }

    let exec_ok = match &unsigned.execute_result {
        Value::Bool(b) => *b,
        Value::Null => true,
        _ => true,
    };
    if !exec_ok {
        let err_msg = if unsigned.execute_error_msg.is_empty() {
            "transaction simulation failed".to_string()
        } else {
            unsigned.execute_error_msg.clone()
        };
        bail!("transaction simulation failed: {}", err_msg);
    }

    // Tx type (deposit / stake / etc.) not eligible for Gas Station. Bail only when the backend
    // returned no signable payload; if it did, fall through to sign with native SOL.
    if unsigned.gs_status() == crate::wallet_api::GasStationStatus::NotSupportIntention
        && !unsigned.has_sign_material()
    {
        return Err(gs_not_supported_err(&addr_info.address));
    }

    // Gas Station guard (also reached by contract-call and other non-GS
    // dispatch paths). Backend uses a two-phase protocol: Phase 1 (diagnosis)
    // returns only gasStationStatus + tokenList with empty hash fields;
    // Phase 2 (execution, called with enableGasStation=true + gasTokenAddress
    // + relayerId) is the call that returns signing material. We intercept
    // Phase 1 responses here so the CLI does not broadcast with an empty
    // msgForSign and get 81358 back.
    if unsigned.gas_station_used {
        if unsigned.has_pending_tx {
            bail!(
                "Gas Station has a pending transaction. Wait for it to complete, or run \
                 `wallet gas-station disable --chain <chain>` to use native token path."
            );
        }
        if unsigned.insufficient_all {
            bail!(
                "Gas Station cannot proceed — all supported tokens (USDT/USDC/USDG) are \
                 below the service charge. Top up at: {}",
                addr_info.address
            );
        }
        if unsigned.hash.is_empty()
            && unsigned.eip712_message_hash.is_empty()
            && unsigned.unsigned_tx_hash.is_empty()
        {
            match classify_gs_phase1(&unsigned) {
                GsPhase1Decision::FirstTime => {
                    if force {
                        // `--force` + first-time GS: third-party plugin path. Return exit 3
                        // with structured error so plugin's outer caller (agent) can run
                        // `wallet gas-station setup` then re-invoke the plugin command.
                        return Err(force_setup_required_for_tx_params(
                            false, is_contract_call, chain, from, &tx, &addr_info, &unsigned,
                        ));
                    }
                    return Err(build_gs_first_time_prompt(&addr_info, &unsigned));
                }
                GsPhase1Decision::Reenable => {
                    if force {
                        return Err(force_setup_required_for_tx_params(
                            true, is_contract_call, chain, from, &tx, &addr_info, &unsigned,
                        ));
                    }
                    return Err(build_gs_reenable_prompt(&addr_info, &unsigned));
                }
                GsPhase1Decision::AutoPick {
                    fee_token_address,
                    relayer_id,
                    needs_enable,
                } => {
                    // Scene B: re-issue Phase 2 with the auto-picked token and rebind `unsigned`.
                    let phase2 = client
                        .pre_transaction_unsigned_info(
                            &access_token,
                            &addr_info.chain_path,
                            chain_index_num,
                            &addr_info.address,
                            tx.to_addr,
                            tx.value,
                            tx.contract_addr,
                            &session_cert,
                            tx.input_data,
                            tx.unsigned_tx,
                            tx.gas_limit,
                            tx.aa_dex_token_addr,
                            tx.aa_dex_token_amount,
                            tx.jito_unsigned_tx,
                            trace_ref,
                            if needs_enable { Some(true) } else { None },
                            Some(&fee_token_address),
                            Some(&relayer_id),
                        )
                        .await
                        .map_err(format_api_error)?;
                    unsigned = phase2;
                }
                GsPhase1Decision::NeedsUserPick => {
                    return Err(build_gs_token_selection_prompt(&unsigned));
                }
            }
        }
        // Phase 2 response (one of hash / eip712MessageHash / unsignedTxHash non-empty) falls
        // through to the normal signing flow below.
    }

    // Defensive guard: backend may return a "diagnostic-only" response where every signing-material
    // field is empty and only gasStationStatus is set. In that case the CLI must not send an empty
    // msgForSign to broadcast -- the backend TEE would reject it with code=81358 "empty signedTx",
    // which is unfriendly to the user. Emit an actionable error classified by GasStationStatus.
    let has_sign_data = !unsigned.hash.is_empty()
        || !unsigned.eip712_message_hash.is_empty()
        || !unsigned.unsigned_tx_hash.is_empty()
        || !unsigned.unsigned_tx.is_empty()
        || !unsigned.auth_hash_for7702.is_empty()
        || !unsigned.jito_unsigned_tx.is_empty();
    if !has_sign_data {
        use crate::wallet_api::GasStationStatus as GS;
        match unsigned.gs_status() {
            GS::FirstTimePrompt | GS::ReenableOnly => bail!(
                "Gas Station activation required (status: {}), but backend did not return \
                 a token list. Re-run with `--enable-gas-station --gas-token-address <addr> \
                 --relayer-id <id>` after picking a token, or first activate Gas Station via \
                 a small `wallet send` ERC-20 transfer.",
                unsigned.gas_station_status
            ),
            GS::PendingUpgrade => bail!(
                "Gas Station activation is pending on-chain. Wait ~30s and retry. If this \
                 persists, the account may be stuck — contact support to reset."
            ),
            GS::InsufficientAll => bail!(
                "Insufficient balance across native token and all Gas Station stablecoins \
                 (USDT / USDC / USDG). Top up at: {}",
                addr_info.address
            ),
            GS::HasPendingTx => bail!(
                "A pending Gas Station transaction is blocking this request. Wait for it to \
                 complete, or run `wallet gas-station disable --chain <chain>` to bypass."
            ),
            // Backup for match exhaustiveness; normally intercepted by the guard above.
            GS::NotSupportIntention => return Err(gs_not_supported_err(&addr_info.address)),
            GS::NotApplicable | GS::ReadyToUse | GS::Unknown => bail!(
                "Backend returned empty signing materials with gasStationStatus=\"{}\". \
                 This is unexpected — likely a backend/environment issue.",
                unsigned.gas_station_status
            ),
        }
    }

    let signing_seed = crate::crypto::hpke_decrypt_session_sk(&encrypted_session_sk, &session_key)?;
    let signing_seed_b64 = base64::engine::general_purpose::STANDARD.encode(signing_seed);

    let mut msg_for_sign_map = serde_json::Map::new();

    if !unsigned.hash.is_empty() {
        let sig = crate::crypto::ed25519_sign_eip191(&unsigned.hash, &signing_seed, "hex")?;
        msg_for_sign_map.insert("signature".into(), json!(sig));
    }
    if !unsigned.auth_hash_for7702.is_empty() {
        let sig = crate::crypto::ed25519_sign_hex(&unsigned.auth_hash_for7702, &signing_seed_b64)?;
        msg_for_sign_map.insert("authSignatureFor7702".into(), json!(sig));
    }
    if !unsigned.unsigned_tx_hash.is_empty() {
        let sig = crate::crypto::ed25519_sign_encoded(
            &unsigned.unsigned_tx_hash,
            &signing_seed_b64,
            &unsigned.encoding,
        )?;
        msg_for_sign_map.insert("unsignedTxHash".into(), json!(&unsigned.unsigned_tx_hash));
        msg_for_sign_map.insert("sessionSignature".into(), json!(sig));
    }
    // eip712MessageHash: 712 hash for the TEE session flow. Same signing
    // algorithm as unsigned_tx_hash → sessionSignature (ed25519_sign_encoded);
    // the signature is written into the sessionSignature field.
    if !unsigned.eip712_message_hash.is_empty() {
        let sig = crate::crypto::ed25519_sign_encoded(
            &unsigned.eip712_message_hash,
            &signing_seed_b64,
            &unsigned.encoding,
        )?;
        msg_for_sign_map.insert("sessionSignature".into(), json!(sig));
    }
    if !unsigned.unsigned_tx.is_empty() {
        msg_for_sign_map.insert("unsignedTx".into(), json!(&unsigned.unsigned_tx));
    }
    if !unsigned.jito_unsigned_tx.is_empty() {
        let jito_sig = crate::crypto::ed25519_sign_encoded(
            &unsigned.jito_unsigned_tx,
            &signing_seed_b64,
            &unsigned.encoding,
        )?;
        msg_for_sign_map.insert("jitoUnsignedTx".into(), json!(&unsigned.jito_unsigned_tx));
        msg_for_sign_map.insert("jitoSessionSignature".into(), json!(jito_sig));
    }
    if !session_cert.is_empty() {
        msg_for_sign_map.insert("sessionCert".into(), json!(session_cert));
    }

    let msg_for_sign = Value::Object(msg_for_sign_map);

    let mut extra_data_obj = if unsigned.extra_data.is_object() {
        unsigned.extra_data.clone()
    } else {
        json!({})
    };
    extra_data_obj["checkBalance"] = json!(true);
    extra_data_obj["uopHash"] = json!(unsigned.uop_hash);
    extra_data_obj["encoding"] = json!(unsigned.encoding);
    extra_data_obj["signType"] = json!(unsigned.sign_type);
    extra_data_obj["msgForSign"] = json!(msg_for_sign);
    if !is_contract_call {
        extra_data_obj["txType"] = json!(2);
    }
    if mev_protection {
        extra_data_obj["isMEV"] = json!(true);
    }
    if force {
        extra_data_obj["skipWarning"] = json!(true);
    }
    if let Some(src) = tx_source {
        extra_data_obj["txSource"] = json!(src);
    }
    if let Some(bt) = agent_biz_type {
        extra_data_obj["agentBizType"] = json!(bt);
    }
    if let Some(sk) = agent_skill_name {
        extra_data_obj["agentSkillName"] = json!(sk);
    }
    // Gas Station: layer on GS core fields only.
    // - gs_apply_extra_data_fields: paymentType / serviceCharge / relayerId /
    //   context / user712Data / user7702Data (for 7702 upgrade).
    // - toAdr / tokenAddress / coinAmount are NOT written here — aligned with
    //   master behavior which treats unsignedInfo.extraData as a passthrough
    //   (backend fills these semantic fields in its response).
    if unsigned.gas_station_used {
        gs_apply_extra_data_fields(&mut extra_data_obj, &unsigned);
    }
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][sign_and_broadcast] Step 10: extraData={}",
            serde_json::to_string_pretty(&extra_data_obj).unwrap_or_default()
        );
    }
    let extra_data_str =
        serde_json::to_string(&extra_data_obj).context("failed to serialize extraData")?;

    let ts_broadcast = chrono::Utc::now().timestamp_millis().to_string();
    let trace_headers_broadcast: Vec<(&str, &str)> = if let Some(ref tid) = cached_tid {
        vec![
            ("ok-client-tid", tid.as_str()),
            ("ok-client-timestamp", ts_broadcast.as_str()),
        ]
    } else {
        vec![]
    };
    let trace_ref_broadcast = if trace_headers_broadcast.is_empty() {
        None
    } else {
        if cfg!(feature = "debug-log") {
            eprintln!(
                "[DEBUG][sign_and_broadcast] broadcast trace headers: ok-client-tid={}, ok-client-timestamp={}",
                cached_tid.as_deref().unwrap_or(""), ts_broadcast
            );
        }
        Some(trace_headers_broadcast.as_slice())
    };
    let broadcast_resp = client
        .broadcast_transaction(
            &access_token,
            &account_id,
            &addr_info.address,
            &addr_info.chain_index,
            &extra_data_str,
            trace_ref_broadcast,
        )
        .await
        .map_err(|e| handle_confirming_error(e, force))?;

    // Clear cached swap trace ID after successful broadcast (contract calls only)
    if is_contract_call {
        let _ = crate::wallet_store::clear_swap_trace_id();
    }
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][sign_and_broadcast] === END SUCCESS: txHash={}, orderId={}",
            broadcast_resp.tx_hash, broadcast_resp.order_id
        );
    }
    Ok(broadcast_resp)
}

// ── build_broadcast_body ─────────────────────────────────────────────

/// Build the JSON body for `broadcast_transaction` from an `UnsignedInfoResponse`.
///
/// Loads the stored TEE session, signs the unsigned data, builds extraData,
/// and returns the complete broadcast request body as a JSON `Value`:
/// ```json
/// { "accountId": "…", "address": "…", "chainIndex": "…", "extraData": "…" }
/// ```
///
/// Internal CLI use only (e.g., agent-commerce task flows).
#[allow(clippy::too_many_arguments)]
pub async fn build_broadcast_body(
    unsigned: &UnsignedInfoResponse,
    account_id: &str,
    address: &str,
    chain_index: &str,
    is_contract_call: bool,
    mev_protection: bool,
    force: bool,
) -> Result<Value> {
    let session = wallet_store::load_session()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let session_cert = session.session_cert;
    let encrypted_session_sk = session.encrypted_session_sk;
    let session_key = keyring_store::get("session_key")
        .map_err(|_| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;

    let extra_data_str = sign_and_build_extra_data(
        unsigned,
        &session_cert,
        &encrypted_session_sk,
        &session_key,
        is_contract_call,
        mev_protection,
        force,
    )?;

    Ok(json!({
        "accountId": account_id,
        "address": address,
        "chainIndex": chain_index,
        "extraData": extra_data_str,
    }))
}

// ── batch_sign_and_broadcast ─────────────────────────────────────────
// EVM only, no Gas Station / 7702 / Jito. Any executeResult=false fails the
// whole batch (no partial rollback). Broadcast dispatch: response.len==1 →
// single-tx broadcast (X Layer merge); else → batch broadcast.

/// Pre-validate batch unsignedInfo response.
///
/// Backend contract: when any element fails simulation, only that element
/// carries `executeErrorMsg`; the rest come back with empty signing
/// materials. So scan `executeResult` across ALL elements first — otherwise
/// an earlier empty-but-success-flagged element would mask the real failure.
fn validate_batch_unsigned_responses(
    unsigned_responses: &[crate::wallet_api::UnsignedInfoResponse],
) -> Result<()> {
    // Pass 1: any executeResult == false → bail with that element's errorMsg.
    for (i, unsigned) in unsigned_responses.iter().enumerate() {
        let exec_ok = match &unsigned.execute_result {
            Value::Bool(b) => *b,
            Value::Null => true,
            _ => true,
        };
        if !exec_ok {
            let msg = if unsigned.execute_error_msg.is_empty() {
                "transaction simulation failed".to_string()
            } else {
                unsigned.execute_error_msg.clone()
            };
            bail!("batch element {i}: {msg}");
        }
    }
    // Pass 2: all elements exec_ok → any missing signing materials is an anomaly.
    for (i, unsigned) in unsigned_responses.iter().enumerate() {
        let has_sign_data = !unsigned.hash.is_empty()
            || !unsigned.eip712_message_hash.is_empty()
            || !unsigned.unsigned_tx_hash.is_empty()
            || !unsigned.unsigned_tx.is_empty()
            || !unsigned.auth_hash_for7702.is_empty();
        if !has_sign_data {
            bail!(
                "batch element {i}: backend returned empty signing materials                  (gasStationStatus={:?})",
                unsigned.gas_station_status
            );
        }
    }
    Ok(())
}

/// Per-element transaction parameters for batch sign + broadcast.
#[derive(Debug, Clone, Default)]
pub struct BatchTxParams {
    pub to_addr: String,
    pub value: String,
    pub contract_addr: Option<String>,
    pub input_data: Option<String>,
    pub gas_limit: Option<String>,
    pub aa_dex_token_addr: Option<String>,
    pub aa_dex_token_amount: Option<String>,
}

/// Build the per-element `msgForSign` JSON for one batch element. Mirrors the
/// branch set used by single-tx `sign_and_broadcast`; `authSignatureFor7702`
/// is signed whenever the backend returned a non-empty `authHashFor7702`
/// (the gate is hash-presence, not flow type — same rule as single-tx and GS).
fn build_batch_element_msg_for_sign(
    unsigned: &crate::wallet_api::UnsignedInfoResponse,
    signing_seed: &[u8],
    session_cert: &str,
) -> Result<Value> {
    let signing_seed_b64 = base64::engine::general_purpose::STANDARD.encode(signing_seed);
    let mut msg_for_sign_map = serde_json::Map::new();

    if !unsigned.hash.is_empty() {
        let sig = crate::crypto::ed25519_sign_eip191(&unsigned.hash, signing_seed, "hex")?;
        msg_for_sign_map.insert("signature".into(), json!(sig));
    }
    if !unsigned.auth_hash_for7702.is_empty() {
        let sig = crate::crypto::ed25519_sign_hex(&unsigned.auth_hash_for7702, &signing_seed_b64)?;
        msg_for_sign_map.insert("authSignatureFor7702".into(), json!(sig));
    }
    if !unsigned.unsigned_tx_hash.is_empty() {
        let sig = crate::crypto::ed25519_sign_encoded(
            &unsigned.unsigned_tx_hash,
            &signing_seed_b64,
            &unsigned.encoding,
        )?;
        msg_for_sign_map.insert("unsignedTxHash".into(), json!(&unsigned.unsigned_tx_hash));
        msg_for_sign_map.insert("sessionSignature".into(), json!(sig));
    }
    if !unsigned.eip712_message_hash.is_empty() {
        let sig = crate::crypto::ed25519_sign_encoded(
            &unsigned.eip712_message_hash,
            &signing_seed_b64,
            &unsigned.encoding,
        )?;
        msg_for_sign_map.insert("sessionSignature".into(), json!(sig));
    }
    if !unsigned.unsigned_tx.is_empty() {
        msg_for_sign_map.insert("unsignedTx".into(), json!(&unsigned.unsigned_tx));
    }
    if !session_cert.is_empty() {
        msg_for_sign_map.insert("sessionCert".into(), json!(session_cert));
    }

    Ok(Value::Object(msg_for_sign_map))
}

#[allow(clippy::too_many_arguments)]
pub async fn batch_sign_and_broadcast(
    chain: &str,
    from: Option<&str>,
    txs: &[BatchTxParams],
    is_contract_call: bool,
    mev_protection: bool,
    force: bool,
    tx_source: Option<&str>,
    agent_biz_type: Option<&str>,
    agent_skill_name: Option<&str>,
) -> Result<Vec<crate::wallet_api::BroadcastResponse>> {
    if txs.is_empty() {
        bail!("batch_sign_and_broadcast: empty txs");
    }
    if txs.len() > 5 {
        bail!(
            "batch_sign_and_broadcast: backend allows up to 5 elements, got {}",
            txs.len()
        );
    }

    let access_token = ensure_tokens_refreshed().await?;

    let chain_entry = super::chain::get_chain_by_real_chain_index(chain)
        .await?
        .ok_or_else(|| anyhow::anyhow!("unsupported chain: {chain}"))?;
    let chain_name = chain_entry["chainName"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("chain entry missing chainName for chain {chain}"))?;

    let mut wallets = wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let (account_id, addr_info) =
        resolve_address_with_refresh(&mut wallets, from, chain_name, || async {
            let mut refresh_client = WalletApiClient::new()?;
            let mut fresh = wallet_store::load_wallets()?
                .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
            super::balance::ensure_wallet_accounts_fresh(
                &mut refresh_client,
                &access_token,
                &mut fresh,
                true,
            )
            .await?;
            Ok(fresh)
        })
        .await?;

    let session = wallet_store::load_session()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let session_cert = session.session_cert;
    let encrypted_session_sk = session.encrypted_session_sk;
    let session_key = keyring_store::get("session_key")
        .map_err(|_| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;

    let chain_index_num: u64 = addr_info.chain_index.parse().map_err(|_| {
        anyhow::anyhow!("chain id '{}' is not a valid number", addr_info.chain_index)
    })?;

    // Per-element validation (mirrors single-tx sign_and_broadcast).
    let ci = &addr_info.chain_index;
    for tx in txs {
        validate_address_for_chain(ci, &tx.to_addr, "to")?;
        if let Some(ca) = tx.contract_addr.as_deref() {
            validate_address_for_chain(ci, ca, "contract-token")?;
        }
        if let Some(aa_addr) = tx.aa_dex_token_addr.as_deref() {
            validate_address_for_chain(ci, aa_addr, "aa-dex-token-addr")?;
        }
        if let Some(gl) = tx.gas_limit.as_deref() {
            validate_non_negative_integer(gl, "gas-limit")?;
        }
        if let Some(aa_amount) = tx.aa_dex_token_amount.as_deref() {
            validate_non_negative_integer(aa_amount, "aa-dex-token-amount")?;
        }
    }

    let elements: Vec<crate::wallet_api::BatchUnsignedInfoElement> = txs
        .iter()
        .map(|tx| crate::wallet_api::BatchUnsignedInfoElement {
            chain_path: addr_info.chain_path.clone(),
            chain_index: chain_index_num,
            from_addr: addr_info.address.clone(),
            to_addr: tx.to_addr.clone(),
            amount: tx.value.clone(),
            contract_addr: tx.contract_addr.clone(),
            session_cert: session_cert.clone(),
            input_data: tx.input_data.clone(),
            unsigned_tx: None,
            gas_limit: tx.gas_limit.clone(),
            aa_dex_token_addr: tx.aa_dex_token_addr.clone(),
            aa_dex_token_amount: tx.aa_dex_token_amount.clone(),
            transaction_type: None,
        })
        .collect();

    let mut client = WalletApiClient::new()?;
    let unsigned_responses = client
        .batch_pre_transaction_unsigned_info(&access_token, &elements, None)
        .await
        .map_err(format_api_error)?;

    // Response length may be smaller than request length when the backend
    // merges elements (XLayer EIP-5792 collapses [approve, swap] into a single
    // unsigned tx). Empty / overflow are still hard errors.
    if unsigned_responses.is_empty() {
        bail!("batch unsignedInfo: empty response");
    }
    if unsigned_responses.len() > txs.len() {
        bail!(
            "batch unsignedInfo: response length {} exceeds request length {}",
            unsigned_responses.len(),
            txs.len()
        );
    }

    validate_batch_unsigned_responses(&unsigned_responses)?;

    let signing_seed = crate::crypto::hpke_decrypt_session_sk(&encrypted_session_sk, &session_key)?;

    let mut broadcast_elements: Vec<crate::wallet_api::BatchBroadcastElement> =
        Vec::with_capacity(unsigned_responses.len());
    for unsigned in &unsigned_responses {
        let msg_for_sign = build_batch_element_msg_for_sign(unsigned, &signing_seed, &session_cert)?;

        let mut extra_data_obj = if unsigned.extra_data.is_object() {
            unsigned.extra_data.clone()
        } else {
            json!({})
        };
        extra_data_obj["checkBalance"] = json!(true);
        extra_data_obj["uopHash"] = json!(unsigned.uop_hash);
        extra_data_obj["encoding"] = json!(unsigned.encoding);
        extra_data_obj["signType"] = json!(unsigned.sign_type);
        extra_data_obj["msgForSign"] = json!(msg_for_sign);
        if !is_contract_call {
            extra_data_obj["txType"] = json!(2);
        }
        if mev_protection {
            extra_data_obj["isMEV"] = json!(true);
        }
        if force {
            extra_data_obj["skipWarning"] = json!(true);
        }
        if let Some(src) = tx_source {
            extra_data_obj["txSource"] = json!(src);
        }
        if let Some(bt) = agent_biz_type {
            extra_data_obj["agentBizType"] = json!(bt);
        }
        if let Some(sk) = agent_skill_name {
            extra_data_obj["agentSkillName"] = json!(sk);
        }
        // Batch broadcast control fields (Lark doc: WalletMain transaction
        // broadcast & query API, batch broadcast section, Web3 main schema).
        // DEX must set `extJson.batchBroadcastType=1`; `from7702Address`
        // defaults to false and same value across the whole batch (vault
        // uses it to decide split-broadcast vs merged). Same-batch
        // consistency is a vault contract: every element here gets the
        // same values.
        let mut ext_json_obj = if extra_data_obj["extJson"].is_object() {
            extra_data_obj["extJson"].clone()
        } else {
            json!({})
        };
        ext_json_obj["batchBroadcastType"] = json!(1);
        extra_data_obj["extJson"] = ext_json_obj;
        extra_data_obj["from7702Address"] = json!(false);
        // `walletMainSaveConfirming=true` is required in batch mode: confirmed
        // with backend RD. The walletMain side keeps the confirming hook
        // available even when the batch is dispatched (e.g. for risk-control
        // / GS-adjacent flows). Single-tx `sign_and_broadcast` does not set
        // this field — the walletMain default there is fine.
        extra_data_obj["walletMainSaveConfirming"] = json!(true);

        let extra_data_str =
            serde_json::to_string(&extra_data_obj).context("failed to serialize extraData")?;

        broadcast_elements.push(crate::wallet_api::BatchBroadcastElement {
            account_id: account_id.clone(),
            address: addr_info.address.clone(),
            chain_index: addr_info.chain_index.clone(),
            extra_data: extra_data_str,
        });
    }

    // Broadcast endpoint dispatch (driven by unsignedInfo response length):
    //   len == 1 → single broadcast endpoint (X Layer EIP-5792 merge case)
    //   len > 1  → batch broadcast endpoint
    if broadcast_elements.len() == 1 {
        let elem = &broadcast_elements[0];
        let resp = client
            .broadcast_transaction(
                &access_token,
                &elem.account_id,
                &elem.address,
                &elem.chain_index,
                &elem.extra_data,
                None,
            )
            .await
            .map_err(|e| handle_confirming_error(e, force))?;
        Ok(vec![resp])
    } else {
        client
            .batch_broadcast_transaction(&access_token, &broadcast_elements, None)
            .await
            .map_err(|e| handle_confirming_error(e, force))
    }
}


// ── send ─────────────────────────────────────────────────────────────

/// onchainos wallet send
#[allow(clippy::too_many_arguments)]
pub(super) async fn cmd_send(
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
    validate_amount(amt)?;
    if recipient.is_empty() || chain.is_empty() {
        bail!("recipient and chain are required");
    }

    // ── Gas Station second-phase call: user already selected token ──
    if gas_token_address.is_some() || enable_gas_station {
        return gas_station_send(
            amt,
            recipient,
            chain,
            from,
            contract_token,
            force,
            gas_token_address,
            relayer_id,
            enable_gas_station,
        )
        .await;
    }

    // ── First-phase call: let backend decide ──
    let access_token =
        crate::commands::agentic_wallet::auth::ensure_tokens_refreshed().await?;
    let wallets = crate::wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let chain_entry = super::chain::get_chain_by_real_chain_index(chain)
        .await?
        .ok_or_else(|| anyhow::anyhow!("unsupported chain: {}", chain))?;
    let chain_name = chain_entry["chainName"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing chainName"))?;
    let (account_id, addr_info) = resolve_address(&wallets, from, chain_name)?;
    let chain_index_num: u64 = addr_info.chain_index.parse().unwrap_or(1);

    let session = crate::wallet_store::load_session()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let session_cert = &session.session_cert;

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
            session_cert,
            None, None, None, None, None, None, None,
            None, // enable_gas_station
            None, // gas_token_address
            None, // relayer_id
        )
        .await
        .map_err(format_api_error)?;

    // Tx type not eligible for Gas Station — bail only when no signable payload was returned.
    if unsigned.gs_status() == crate::wallet_api::GasStationStatus::NotSupportIntention
        && !unsigned.has_sign_material()
    {
        return Err(gs_not_supported_err(&addr_info.address));
    }

    // ── Gas Station dispatch (two-phase protocol + client-side Scene B/C decision) ──
    // Phase 1 diagnostic: backend returns gasStationStatus + gasStationTokenList +
    // defaultGasTokenAddress with all hash fields null. CLI matches defaultGasTokenAddress
    // against the token list:
    //   - hit + sufficient → Scene B: CLI auto-runs Phase 2 with that token + sign + broadcast
    //   - otherwise → Scene C: return Confirming so the user picks a token
    if unsigned.gas_station_used {
        // Terminal states: report directly to the user, no further action.
        if unsigned.has_pending_tx {
            return emit_gs_pending_tx_state();
        }
        if unsigned.insufficient_all {
            return emit_gs_insufficient_all_state(&unsigned, &addr_info.address);
        }
        // Phase 2 response: backend returned signing material — sign and broadcast.
        if !unsigned.hash.is_empty()
            || !unsigned.eip712_message_hash.is_empty()
            || !unsigned.unsigned_tx_hash.is_empty()
        {
            return handle_gs_auto_sign_broadcast(
                &mut client, &access_token, &account_id, &addr_info, &session,
                &unsigned, force, recipient, amt, contract_token,
            )
            .await;
        }
        match classify_gs_phase1(&unsigned) {
            GsPhase1Decision::FirstTime => {
                if force {
                    return Err(force_setup_required_for_send(
                        false, chain, from, recipient, amt, contract_token,
                        &addr_info, &unsigned,
                    ));
                }
                return Err(build_gs_first_time_prompt(&addr_info, &unsigned));
            }
            GsPhase1Decision::Reenable => {
                if force {
                    return Err(force_setup_required_for_send(
                        true, chain, from, recipient, amt, contract_token,
                        &addr_info, &unsigned,
                    ));
                }
                return Err(build_gs_reenable_prompt(&addr_info, &unsigned));
            }
            GsPhase1Decision::AutoPick {
                fee_token_address,
                relayer_id,
                needs_enable,
            } => {
                return gas_station_send(
                    amt,
                    recipient,
                    chain,
                    from,
                    contract_token,
                    force,
                    Some(&fee_token_address),
                    Some(&relayer_id),
                    needs_enable,
                )
                .await;
            }
            GsPhase1Decision::NeedsUserPick => {
                return Err(build_gs_token_selection_prompt(&unsigned));
            }
        }
    }

    // ── Not Gas Station: original flow ──
    let resp = sign_and_broadcast(
        chain,
        from,
        TxParams {
            to_addr: recipient,
            value: amt,
            contract_addr: contract_token,
            input_data: None,
            unsigned_tx: None,
            gas_limit: None,
            aa_dex_token_addr: None,
            aa_dex_token_amount: None,
            jito_unsigned_tx: None,
            gas_token_address: None,
            relayer_id: None,
            enable_gas_station: false,
        },
        false,
        false,
        force,
        None, // tx_source: not cross-chain
        Some("transfer"),
        None, // agent_skill_name: not applicable for plain transfers
    )
    .await?;
    output::success(json!({ "txHash": resp.tx_hash, "orderId": resp.order_id }));
    Ok(())
}

// ── contract-call ─────────────────────────────────────────────────────

/// onchainos wallet contract-call
#[allow(clippy::too_many_arguments)]
pub async fn cmd_contract_call(
    to: &str,
    chain: &str,
    amt: &str,
    input_data: Option<&str>,
    unsigned_tx: Option<&str>,
    gas_limit: Option<&str>,
    from: Option<&str>,
    aa_dex_token_addr: Option<&str>,
    aa_dex_token_amount: Option<&str>,
    mev_protection: bool,
    jito_unsigned_tx: Option<&str>,
    force: bool,
    gas_token_address: Option<&str>,
    relayer_id: Option<&str>,
    enable_gas_station: bool,
    biz_type: Option<&str>,
    strategy: Option<&str>,
) -> Result<()> {
    let resp = execute_contract_call(
        to,
        chain,
        amt,
        input_data,
        unsigned_tx,
        gas_limit,
        from,
        aa_dex_token_addr,
        aa_dex_token_amount,
        mev_protection,
        jito_unsigned_tx,
        force,
        None, // tx_source: not cross-chain
        gas_token_address,
        relayer_id,
        enable_gas_station,
        biz_type,
        strategy,
    )
    .await?;
    output::success(json!({ "txHash": resp.tx_hash, "orderId": resp.order_id }));
    Ok(())
}

/// Core contract-call logic: validate → sign → broadcast → return BroadcastResponse.
/// Used by `cmd_contract_call` (CLI entry point) and directly by swap execute.
#[allow(clippy::too_many_arguments)]
pub async fn execute_contract_call(
    to: &str,
    chain: &str,
    amt: &str,
    input_data: Option<&str>,
    unsigned_tx: Option<&str>,
    gas_limit: Option<&str>,
    from: Option<&str>,
    aa_dex_token_addr: Option<&str>,
    aa_dex_token_amount: Option<&str>,
    mev_protection: bool,
    jito_unsigned_tx: Option<&str>,
    force: bool,
    tx_source: Option<&str>,
    gas_token_address: Option<&str>,
    relayer_id: Option<&str>,
    enable_gas_station: bool,
    agent_biz_type: Option<&str>,
    agent_skill_name: Option<&str>,
) -> Result<crate::wallet_api::BroadcastResponse> {
    if to.is_empty() || chain.is_empty() {
        bail!("to and chain are required");
    }
    validate_non_negative_integer(amt, "amt")?;
    if input_data.is_none() && unsigned_tx.is_none() {
        bail!("either --input-data (EVM) or --unsigned-tx (SOL) is required");
    }

    sign_and_broadcast(
        chain,
        from,
        TxParams {
            to_addr: to,
            value: amt,
            contract_addr: Some(to),
            input_data,
            unsigned_tx,
            gas_limit,
            aa_dex_token_addr,
            aa_dex_token_amount,
            jito_unsigned_tx,
            gas_token_address,
            relayer_id,
            enable_gas_station,
        },
        true,
        mev_protection,
        force,
        tx_source,
        agent_biz_type,
        agent_skill_name,
    )
    .await
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::commands::agentic_wallet::common::handle_confirming_error;
    use crate::wallet_store::{AccountMapEntry, AddressInfo, WalletsJson};

    fn make_test_wallets() -> WalletsJson {
        let mut accounts_map = HashMap::new();
        accounts_map.insert(
            "acc-1".to_string(),
            AccountMapEntry {
                address_list: vec![
                    AddressInfo {
                        account_id: "acc-1".to_string(),
                        address: "0xAAA".to_string(),
                        chain_index: "1".to_string(),
                        chain_name: "eth".to_string(),
                        address_type: "eoa".to_string(),
                        chain_path: "/evm/1".to_string(),
                    },
                    AddressInfo {
                        account_id: "acc-1".to_string(),
                        address: "SolAdr1".to_string(),
                        chain_index: "501".to_string(),
                        chain_name: "sol".to_string(),
                        address_type: "eoa".to_string(),
                        chain_path: "/sol/501".to_string(),
                    },
                ],
            },
        );
        accounts_map.insert(
            "acc-2".to_string(),
            AccountMapEntry {
                address_list: vec![AddressInfo {
                    account_id: "acc-2".to_string(),
                    address: "0xBBB".to_string(),
                    chain_index: "1".to_string(),
                    chain_name: "eth".to_string(),
                    address_type: "eoa".to_string(),
                    chain_path: "/evm/1".to_string(),
                }],
            },
        );
        WalletsJson {
            email: "test@example.com".to_string(),
            selected_account_id: "acc-1".to_string(),
            accounts_map,
            ..Default::default()
        }
    }

    #[test]
    fn resolve_address_by_selected_account() {
        let w = make_test_wallets();
        let (acct_id, info) = resolve_address(&w, None, "eth").unwrap();
        assert_eq!(acct_id, "acc-1");
        assert_eq!(info.address, "0xAAA");
        assert_eq!(info.chain_path, "/evm/1");
    }

    #[test]
    fn resolve_address_by_selected_account_solana() {
        let w = make_test_wallets();
        let (acct_id, info) = resolve_address(&w, None, "sol").unwrap();
        assert_eq!(acct_id, "acc-1");
        assert_eq!(info.address, "SolAdr1");
    }

    #[test]
    fn resolve_address_by_from_addr() {
        let w = make_test_wallets();
        let (acct_id, info) = resolve_address(&w, Some("0xBBB"), "eth").unwrap();
        assert_eq!(acct_id, "acc-2");
        assert_eq!(info.address, "0xBBB");
    }

    #[test]
    fn resolve_address_case_insensitive() {
        let w = make_test_wallets();
        let (acct_id, _) = resolve_address(&w, Some("0xaaa"), "eth").unwrap();
        assert_eq!(acct_id, "acc-1");
    }

    #[test]
    fn resolve_address_not_found() {
        let w = make_test_wallets();
        let result = resolve_address(&w, Some("0xNOPE"), "eth");
        assert!(result.is_err());
    }

    #[test]
    fn resolve_address_wrong_chain() {
        let w = make_test_wallets();
        let result = resolve_address(&w, None, "unknown");
        assert!(result.is_err());
    }

    // ── resolve_address_with_refresh tests ────────────────────────────

    #[tokio::test]
    async fn resolve_address_with_refresh_succeeds_on_first_try() {
        // Initial wallets already has "eth" address — refresh must NOT be invoked.
        let mut w = make_test_wallets();

        let (acct_id, info) = resolve_address_with_refresh(&mut w, None, "eth", || async {
            panic!("refresh should not fire on happy path");
        })
        .await
        .unwrap();

        assert_eq!(acct_id, "acc-1");
        assert_eq!(info.address, "0xAAA");
    }

    #[tokio::test]
    async fn resolve_address_with_refresh_retries_after_refresh_injects_address() {
        // Initial wallets has NO tempo address. Refresh returns a new WalletsJson
        // containing the tempo address; retry lookup succeeds.
        let mut w = make_test_wallets();
        assert!(resolve_address(&w, None, "tempo").is_err());

        let (acct_id, info) =
            resolve_address_with_refresh(&mut w, None, "tempo", || async {
                let mut fresh = make_test_wallets();
                fresh
                    .accounts_map
                    .get_mut("acc-1")
                    .unwrap()
                    .address_list
                    .push(AddressInfo {
                        account_id: "acc-1".to_string(),
                        address: "0xTempoAddr".to_string(),
                        chain_index: "4217".to_string(),
                        chain_name: "tempo".to_string(),
                        address_type: "eoa".to_string(),
                        chain_path: "m/44/60/0/0/0".to_string(),
                    });
                Ok(fresh)
            })
            .await
            .unwrap();

        assert_eq!(acct_id, "acc-1");
        assert_eq!(info.address, "0xTempoAddr");
        assert_eq!(info.chain_index, "4217");
    }

    #[tokio::test]
    async fn resolve_address_with_refresh_fails_when_refresh_errors() {
        // If the refresh closure itself fails, the error propagates — no further retry.
        let mut w = make_test_wallets();

        let result: Result<(String, AddressInfo)> =
            resolve_address_with_refresh(&mut w, None, "tempo", || async {
                Err(anyhow::anyhow!("network down"))
            })
            .await;

        assert!(result.is_err());
        assert!(format!("{:#}", result.unwrap_err()).contains("network down"));
    }

    #[tokio::test]
    async fn resolve_address_with_refresh_fails_when_retry_still_misses() {
        // Refresh returns unchanged wallets — final lookup still fails.
        let mut w = make_test_wallets();

        let result: Result<(String, AddressInfo)> =
            resolve_address_with_refresh(&mut w, None, "tempo", || async {
                Ok(make_test_wallets())
            })
            .await;

        assert!(result.is_err());
        assert!(format!("{:#}", result.unwrap_err()).contains("no address for chain=tempo"));
    }

    // ── handle_confirming_error tests ─────────────────────────────────

    #[test]
    fn broadcast_error_81362_no_force_returns_cli_confirming() {
        let api_err = crate::wallet_api::ApiCodeError {
            http_status: 200,
            code: "81362".to_string(),
            msg: "please confirm".to_string(),
        };
        let err: anyhow::Error = api_err.into();
        let result = handle_confirming_error(err, false);
        let confirming = result
            .downcast_ref::<crate::output::CliConfirming>()
            .expect("should be CliConfirming");
        assert_eq!(confirming.message, "please confirm");
        assert!(confirming.next.contains("--force"));
    }

    #[test]
    fn broadcast_error_81362_with_force_returns_plain_error() {
        let api_err = crate::wallet_api::ApiCodeError {
            http_status: 200,
            code: "81362".to_string(),
            msg: "please confirm".to_string(),
        };
        let err: anyhow::Error = api_err.into();
        let result = handle_confirming_error(err, true);
        // Should NOT be CliConfirming when force=true
        assert!(result
            .downcast_ref::<crate::output::CliConfirming>()
            .is_none());
        // Preserves the structured ApiCodeError so callers can downcast on `code`.
        let api_err_back = result
            .downcast_ref::<crate::wallet_api::ApiCodeError>()
            .expect("should be ApiCodeError");
        assert_eq!(api_err_back.code, "81362");
        assert_eq!(api_err_back.msg, "please confirm");
        // String form includes the `code=N` prefix so downstream string matching keeps the code.
        assert_eq!(
            format!("{}", result),
            "Wallet API error (code=81362): please confirm"
        );
    }

    #[test]
    fn broadcast_error_other_code_returns_plain_error() {
        let api_err = crate::wallet_api::ApiCodeError {
            http_status: 200,
            code: "50000".to_string(),
            msg: "server error".to_string(),
        };
        let err: anyhow::Error = api_err.into();
        let result = handle_confirming_error(err, false);
        assert!(result
            .downcast_ref::<crate::output::CliConfirming>()
            .is_none());
        let api_err_back = result
            .downcast_ref::<crate::wallet_api::ApiCodeError>()
            .expect("should be ApiCodeError");
        assert_eq!(api_err_back.code, "50000");
        assert_eq!(api_err_back.msg, "server error");
        assert_eq!(
            format!("{}", result),
            "Wallet API error (code=50000): server error"
        );
    }

    #[test]
    fn broadcast_error_81363_preserves_code_for_diagnosis() {
        // Regression for the cross-chain v6 commit: backend returns code=81363 on TEE
        // pre-execute / broadcast revert. Earlier the wrapper stripped the code and
        // surfaced only "execution reverted", which made 81362 / 81363 / on-chain
        // revert indistinguishable. This test pins the new contract.
        let api_err = crate::wallet_api::ApiCodeError {
            http_status: 200,
            code: "81363".to_string(),
            msg: "execution reverted".to_string(),
        };
        let err: anyhow::Error = api_err.into();
        let result = handle_confirming_error(err, false);
        assert!(result
            .downcast_ref::<crate::output::CliConfirming>()
            .is_none());
        let api_err_back = result
            .downcast_ref::<crate::wallet_api::ApiCodeError>()
            .expect("should be ApiCodeError");
        assert_eq!(api_err_back.code, "81363");
        assert_eq!(
            format!("{}", result),
            "Wallet API error (code=81363): execution reverted"
        );
    }

    #[test]
    fn broadcast_error_non_api_error_passes_through() {
        let err = anyhow::anyhow!("network timeout");
        let result = handle_confirming_error(err, false);
        assert!(result
            .downcast_ref::<crate::output::CliConfirming>()
            .is_none());
        assert_eq!(format!("{}", result), "network timeout");
    }

    // ── cmd_send input validation tests ──────────────────────────────

    #[tokio::test]
    async fn cmd_send_rejects_empty_amt() {
        let result = cmd_send("", "0xRecipient", "1", None, None, false, None, None, false).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("--amount"));
    }

    #[tokio::test]
    async fn cmd_send_rejects_decimal_amt() {
        let result = cmd_send("1.5", "0xRecipient", "1", None, None, false, None, None, false).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("--amount"));
    }

    #[tokio::test]
    async fn cmd_send_rejects_empty_recipient() {
        let result = cmd_send("100", "", "1", None, None, false, None, None, false).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("recipient and chain are required"));
    }

    #[tokio::test]
    async fn cmd_send_rejects_empty_chain() {
        let result = cmd_send("100", "0xRecipient", "", None, None, false, None, None, false).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("recipient and chain are required"));
    }

    // ── cmd_contract_call input validation tests ─────────────────────

    #[tokio::test]
    async fn cmd_contract_call_rejects_empty_to() {
        let result = cmd_contract_call(
            "",
            "1",
            "0",
            Some("0xdata"),
            None,
            None,
            None,
            None,
            None,
            false,
            None,
            false,
            None,
            None,
            false,
            None,
            None,
        )
        .await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("to and chain are required"));
    }

    #[tokio::test]
    async fn cmd_contract_call_rejects_empty_chain() {
        let result = cmd_contract_call(
            "0xTo",
            "",
            "0",
            Some("0xdata"),
            None,
            None,
            None,
            None,
            None,
            false,
            None,
            false,
            None,
            None,
            false,
            None,
            None,
        )
        .await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("to and chain are required"));
    }

    #[tokio::test]
    async fn cmd_contract_call_rejects_decimal_amt() {
        let result = cmd_contract_call(
            "0xTo",
            "1",
            "1.5",
            Some("0xdata"),
            None,
            None,
            None,
            None,
            None,
            false,
            None,
            false,
            None,
            None,
            false,
            None,
            None,
        )
        .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("--amt"));
    }

    #[tokio::test]
    async fn cmd_contract_call_rejects_missing_input_and_unsigned() {
        let result = cmd_contract_call(
            "0xTo", "1", "0", None, None, None, None, None, None, false, None, false, None, None,
            false,
            None,
            None,
        )
        .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("--input-data"));
    }

    // ── validate_address_for_chain integration tests (from swap.rs) ──

    #[test]
    fn transfer_uses_validate_address_for_chain() {
        // Ensure the imported function works correctly in this module context
        assert!(validate_address_for_chain(
            "1",
            "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
            "to"
        )
        .is_ok());
        assert!(validate_address_for_chain(
            "501",
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            "to"
        )
        .is_ok());
        // EVM short address rejected
        assert!(validate_address_for_chain("1", "0xabc", "to").is_err());
        // Solana short address rejected
        assert!(validate_address_for_chain("501", "short", "to").is_err());
    }

    // ── validate_non_negative_integer integration tests (from swap.rs) ──

    #[test]
    fn transfer_uses_validate_non_negative_integer() {
        assert!(validate_non_negative_integer("0", "gas-limit").is_ok());
        assert!(validate_non_negative_integer("21000", "gas-limit").is_ok());
        assert!(validate_non_negative_integer("-1", "gas-limit").is_err());
        assert!(validate_non_negative_integer("abc", "aa-dex-token-amount").is_err());
        assert!(validate_non_negative_integer("007", "gas-limit").is_err());
    }

    /// Build a minimal UnsignedInfoResponse via serde, since the struct has
    /// many private-ish fields and constructing it field-by-field is brittle.
    fn unsigned_info_from_json(json: serde_json::Value) -> crate::wallet_api::UnsignedInfoResponse {
        serde_json::from_value(json).expect("valid UnsignedInfoResponse JSON")
    }

    #[test]
    fn validate_batch_all_success_passes() {
        // Every element executeResult=true AND has signing materials → Ok.
        let responses = vec![
            unsigned_info_from_json(serde_json::json!({
                "executeResult": true,
                "unsignedTxHash": "0xaa",
                "encoding": "hex",
            })),
            unsigned_info_from_json(serde_json::json!({
                "executeResult": true,
                "unsignedTxHash": "0xbb",
                "encoding": "hex",
            })),
        ];
        assert!(validate_batch_unsigned_responses(&responses).is_ok());
    }

    #[test]
    fn validate_batch_surfaces_failing_element_even_when_earlier_one_has_empty_sign_data() {
        // Backend contract: when element[1] fails simulation, element[0] often
        // comes back with executeResult=true but every sign-data field empty.
        // The validator must scan executeResult first and report element[1]'s
        // executeErrorMsg, not bail on element[0]'s "empty signing materials".
        let responses = vec![
            unsigned_info_from_json(serde_json::json!({
                "executeResult": true,
                "executeErrorMsg": "",
                // All sign-data fields empty (the bug condition).
            })),
            unsigned_info_from_json(serde_json::json!({
                "executeResult": false,
                "executeErrorMsg": "execution reverted: Min return not reached",
            })),
        ];
        let err = validate_batch_unsigned_responses(&responses).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("batch element 1") && msg.contains("Min return not reached"),
            "expected error to point at element 1 with backend errorMsg, got: {msg}"
        );
    }

    #[test]
    fn validate_batch_reports_empty_signing_materials_when_all_exec_ok() {
        // All executeResult=true but element[0] has no sign-data fields.
        // This is a genuine anomaly (not a connected-batch-failure case),
        // so we should bail on the missing-materials path.
        let responses = vec![
            unsigned_info_from_json(serde_json::json!({
                "executeResult": true,
                // No sign-data fields.
            })),
            unsigned_info_from_json(serde_json::json!({
                "executeResult": true,
                "unsignedTxHash": "0xbb",
                "encoding": "hex",
            })),
        ];
        let err = validate_batch_unsigned_responses(&responses).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("batch element 0") && msg.contains("empty signing materials"),
            "expected error to point at element 0 missing-materials, got: {msg}"
        );
    }

    #[test]
    fn build_batch_element_signs_auth_hash_for7702_when_present() {
        // Backend returned a non-empty authHashFor7702 for one batch element:
        // the per-element builder MUST sign it into authSignatureFor7702,
        // regardless of `needUpdate7702` (the gate is hash-presence, not the
        // boolean flag — same rule as single-tx sign_and_broadcast).
        let unsigned = unsigned_info_from_json(serde_json::json!({
            "executeResult": true,
            "unsignedTxHash": "0xaa",
            "encoding": "hex",
            "authHashFor7702": "deadbeefdeadbeefdeadbeefdeadbeef",
        }));
        let seed = [7u8; 32];
        let msg = build_batch_element_msg_for_sign(&unsigned, &seed, "cert-xxx").unwrap();
        let obj = msg.as_object().expect("msgForSign is object");
        assert!(
            obj.get("authSignatureFor7702")
                .and_then(|v| v.as_str())
                .map(|s| !s.is_empty())
                .unwrap_or(false),
            "authSignatureFor7702 must be present + non-empty when authHashFor7702 returned, got: {msg}"
        );
        // sessionCert + unsignedTxHash branches still populated as before.
        assert_eq!(obj.get("sessionCert").and_then(|v| v.as_str()), Some("cert-xxx"));
        assert_eq!(obj.get("unsignedTxHash").and_then(|v| v.as_str()), Some("0xaa"));
    }

    #[test]
    fn build_batch_element_omits_auth_hash_for7702_when_empty() {
        // When the backend does NOT return authHashFor7702, the builder must
        // not emit a stray authSignatureFor7702 key.
        let unsigned = unsigned_info_from_json(serde_json::json!({
            "executeResult": true,
            "unsignedTxHash": "0xbb",
            "encoding": "hex",
        }));
        let seed = [7u8; 32];
        let msg = build_batch_element_msg_for_sign(&unsigned, &seed, "").unwrap();
        let obj = msg.as_object().expect("msgForSign is object");
        assert!(
            obj.get("authSignatureFor7702").is_none(),
            "authSignatureFor7702 must be absent when authHashFor7702 is empty, got: {msg}"
        );
        assert!(
            obj.get("sessionCert").is_none(),
            "sessionCert must be absent when session_cert is empty, got: {msg}"
        );
    }
}
