use anyhow::{bail, Context, Result};
use base64::Engine;
use serde_json::{json, Value};

use crate::commands::swap::{
    validate_address_for_chain, validate_amount, validate_non_negative_integer,
};
use crate::keyring_store;
use crate::output;
use crate::wallet_api::WalletApiClient;
use crate::wallet_store::{self, AddressInfo, WalletsJson};

use super::auth::{ensure_tokens_refreshed, format_api_error};
use super::common::handle_confirming_error;

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

// ── sign_and_broadcast ────────────────────────────────────────────────

/// Parameters for the unsignedInfo API call.
struct TxParams<'a> {
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
async fn sign_and_broadcast(
    chain: &str,
    from: Option<&str>,
    tx: TxParams<'_>,
    is_contract_call: bool,
    mev_protection: bool,
    force: bool,
    tx_source: Option<&str>,
) -> Result<crate::wallet_api::BroadcastResponse> {
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][sign_and_broadcast] enter: chain={}, from={:?}, to={}, value={}, contractAddr={:?}, inputData={}, unsignedTx={}, gasLimit={:?}, mev={}",
            chain, from, tx.to_addr, tx.value, tx.contract_addr,
            tx.input_data.map(|s| format!("{}...({})", &s[..s.len().min(20)], s.len())).unwrap_or_else(|| "None".into()),
            tx.unsigned_tx.map(|s| format!("{}...({})", &s[..s.len().min(20)], s.len())).unwrap_or_else(|| "None".into()),
            tx.gas_limit,
            mev_protection,
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

    let wallets = wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;

    let (account_id, addr_info) = resolve_address(&wallets, from, chain_name)?;
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
    let unsigned = client
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

    // Gas Station guard（contract-call 等非 GS 分发路径走这里）：
    // backend 两阶段协议——Phase 1 诊断只返 gasStationStatus + tokenList，所有 hash 字段为空；
    // Phase 2 执行（带 enableGasStation=true + gasTokenAddress + relayerId）才返签名材料。
    // 这里拦住 Phase 1 诊断响应，防止 CLI 用空 msgForSign 发 broadcast 拿到 81358。
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
            // Phase 1 diagnostic response: backend is waiting for the CLI to re-invoke the
            // command with `--gas-token-address <addr> --relayer-id <id>` (and
            // `--enable-gas-station` for first-time activation or re-enable) so it can call
            // wallet.callData712() and return the 712 message hash for signing.
            let token_list_str = unsigned
                .gas_station_token_list
                .iter()
                .filter(|t| t.sufficient)
                .enumerate()
                .map(|(i, t)| {
                    format!(
                        "{}. {} (balance: {}, fee: {})",
                        i + 1,
                        t.symbol,
                        t.balance,
                        t.service_charge
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            let message = format!(
                "Gas Station is active on this chain. Choose a token to pay gas:\n{}\n\n\
                 Re-run the command with `--gas-token-address <addr> --relayer-id <id>` \
                 (add `--enable-gas-station` if the status is FIRST_TIME_PROMPT or REENABLE_ONLY).",
                token_list_str
            );
            let next = format!(
                "Token list: {}",
                serde_json::to_string(&unsigned.gas_station_token_list).unwrap_or_default()
            );
            return Err(crate::output::CliConfirming { message, next }.into());
        }
        // Phase 2 响应（hash / eip712MessageHash / unsignedTxHash 之一非空）继续走下面的正常签名流程。
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
    // eip712MessageHash: 712 hash，TEE session 场景。算法跟 unsigned_tx_hash→sessionSignature 一致
    // （ed25519_sign_encoded），结果写入 sessionSignature 字段。
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
    // TEMPORARY: debug_dump — 联调后删除 unsigned_req 定义和 debug_dump::dump 调用
    let unsigned_req = json!({
        "chainPath": addr_info.chain_path,
        "chainIndex": chain_index_num,
        "fromAddr": addr_info.address,
        "toAddr": recipient,
        "amount": amt,
        "contractAddr": contract_token,
    });
    let unsigned = match client
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
    {
        Ok(v) => v,
        Err(e) => {
            // TEMPORARY: debug_dump — 记录失败的第一次 unsignedInfo
            super::debug_dump::dump_error("01-unsignedInfo-first", &unsigned_req, &format!("{e:#}"));
            return Err(format_api_error(e));
        }
    };
    super::debug_dump::dump("01-unsignedInfo-first", &unsigned_req, &json!({
        "gasStationUsed": unsigned.gas_station_used,
        "gasStationFirstTimePrompt": unsigned.gas_station_first_time_prompt,
        "hash": &unsigned.hash,
        "authHashFor7702": &unsigned.auth_hash_for7702,
        "signType": &unsigned.sign_type,
        "hasPendingTx": unsigned.has_pending_tx,
        "insufficientAll": unsigned.insufficient_all,
        "autoSelectedToken": unsigned.auto_selected_token,
        "serviceCharge": &unsigned.service_charge,
        "serviceChargeSymbol": &unsigned.service_charge_symbol,
        "needUpdate7702": unsigned.need_update7702,
        "gasStationTokenList": &unsigned.gas_station_token_list,
        "executeResult": &unsigned.execute_result,
        "executeErrorMsg": &unsigned.execute_error_msg,
    }));

    // ── Gas Station dispatch（两阶段协议） ──
    // 第一次调用（本次）= Phase 1 诊断：backend 只返 gasStationStatus + tokenList，
    // hash / eip712MessageHash / authHashFor7702 等故意全 null；
    // 第二次调用需要带 enableGasStation=true + gasTokenAddress + relayerId，backend 才返完整签名数据。
    // 因此判断走哪个分支：看 backend 有没有返任何可签字段。
    if unsigned.gas_station_used {
        // 终结类状态：直接告知用户
        if unsigned.has_pending_tx {
            return handle_gs_pending_tx();
        }
        if unsigned.insufficient_all {
            return handle_gs_insufficient_all(&unsigned, &addr_info.address);
        }
        // Phase 2 响应：backend 返了签名材料，直接签广播
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
        // Phase 1 诊断响应：让用户从 tokenList 选 token
        // FIRST_TIME_PROMPT / REENABLE_ONLY 都需要带 enableGasStation=true 再调
        // READY_TO_USE + hash=null（Scene C 默认 token 不够）只需选备选 token，不带 enableGasStation
        if unsigned.gas_station_first_time_prompt
            || unsigned.gs_status() == crate::wallet_api::GasStationStatus::FirstTimePrompt
            || unsigned.gs_status() == crate::wallet_api::GasStationStatus::ReenableOnly
        {
            return handle_gs_first_time_prompt(&unsigned);
        }
        return handle_gs_default_token_insufficient(&unsigned);
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
    )
    .await?;
    output::success(json!({ "txHash": resp.tx_hash, "orderId": resp.order_id }));
    Ok(())
}

/// Gas Station second-phase: user selected token, call unsignedInfo with gasTokenAddress
#[allow(clippy::too_many_arguments)]
async fn gas_station_send(
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
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let chain_entry = super::chain::get_chain_by_real_chain_index(chain)
        .await?
        .ok_or_else(|| anyhow::anyhow!("unsupported chain: {}", chain))?;
    let chain_name = chain_entry["chainName"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing chainName"))?;
    let (_account_id, addr_info) = resolve_address(&wallets, from, chain_name)?;
    let chain_index_num: u64 = addr_info.chain_index.parse().unwrap_or(1);

    let session = crate::wallet_store::load_session()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;

    let mut client = crate::wallet_api::WalletApiClient::new()?;
    // TEMPORARY: debug_dump — 联调后删除 unsigned_req 定义和 debug_dump::dump 调用
    let unsigned_req = json!({
        "chainPath": addr_info.chain_path,
        "chainIndex": chain_index_num,
        "fromAddr": addr_info.address,
        "toAddr": recipient,
        "amount": amt,
        "contractAddr": contract_token,
        "enableGasStation": enable_gas_station,
        "gasTokenAddress": gas_token_address,
        "relayerId": relayer_id,
    });
    let unsigned = match client
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
    {
        Ok(v) => v,
        Err(e) => {
            // TEMPORARY: debug_dump — 记录失败的第二次 unsignedInfo
            super::debug_dump::dump_error("02-unsignedInfo-second", &unsigned_req, &format!("{e:#}"));
            return Err(format_api_error(e));
        }
    };
    super::debug_dump::dump("02-unsignedInfo-second", &unsigned_req, &json!({
        "gasStationUsed": unsigned.gas_station_used,
        "hash": &unsigned.hash,
        "authHashFor7702": &unsigned.auth_hash_for7702,
        "signType": &unsigned.sign_type,
        "serviceCharge": &unsigned.service_charge,
        "serviceChargeSymbol": &unsigned.service_charge_symbol,
        "serviceChargeFeeTokenAddress": &unsigned.service_charge_fee_token_address,
        "needUpdate7702": unsigned.need_update7702,
        "contractNonce": &unsigned.contract_nonce,
        "eoaNonce": &unsigned.eoa_nonce,
        "executeResult": &unsigned.execute_result,
        "executeErrorMsg": &unsigned.execute_error_msg,
        "extraData": &unsigned.extra_data,
    }));

    if !unsigned.gas_station_used {
        bail!("Gas Station not activated by backend for this transaction");
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

/// Gas Station msgForSign: TEE 场景（sessionSignature）+ 7702 升级时附带 authSignatureFor7702。
/// 不写入 signature（那是 Pay 场景的 EIP-191 签，GS 走 TEE 不走 Pay）。
fn gs_build_msg_for_sign(
    unsigned: &crate::wallet_api::UnsignedInfoResponse,
    session: &crate::wallet_store::SessionJson,
    signing_seed: &[u8],
    include_7702: bool,
) -> Result<Value> {
    let mut m = serde_json::Map::new();

    let signing_seed_b64 = base64::engine::general_purpose::STANDARD.encode(signing_seed);

    // eip712_message_hash 非空 → ed25519_sign_encoded（TEE 场景标准算法，跟 unsigned_tx_hash→sessionSignature 一致），
    // 结果写入 sessionSignature。
    if !unsigned.eip712_message_hash.is_empty() {
        let session_sig = crate::crypto::ed25519_sign_encoded(
            &unsigned.eip712_message_hash,
            &signing_seed_b64,
            &unsigned.encoding,
        )?;
        m.insert("sessionSignature".into(), json!(session_sig));
    }
    // 向后兼容旧字段 hash（新后端不再返）
    if !unsigned.hash.is_empty() && unsigned.eip712_message_hash.is_empty() {
        let session_sig = crate::crypto::ed25519_sign_encoded(
            &unsigned.hash,
            &signing_seed_b64,
            &unsigned.encoding,
        )?;
        m.insert("sessionSignature".into(), json!(session_sig));
    }
    // 签 authHashFor7702 → authSignatureFor7702（仅 7702 升级流程）
    if include_7702 && !unsigned.auth_hash_for7702.is_empty() {
        let sig = crate::crypto::ed25519_sign_hex(&unsigned.auth_hash_for7702, &signing_seed_b64)?;
        m.insert("authSignatureFor7702".into(), json!(sig));
    }
    // sessionCert
    if !session.session_cert.is_empty() {
        m.insert("sessionCert".into(), json!(session.session_cert));
    }
    Ok(Value::Object(m))
}

/// Build the base extraData: master fields + Gas Station fields.
/// Gas Station fields are layered on top of the normal broadcast structure.
#[allow(clippy::too_many_arguments)]
fn gs_build_extra_data(
    unsigned: &crate::wallet_api::UnsignedInfoResponse,
    msg_for_sign: &Value,
    to_addr: &str,
    coin_amount: &str,
    token_address: Option<&str>,
    force: bool,
    include_7702: bool,
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
    ed["txType"] = json!(2);
    ed["paymentType"] = json!("token");
    if force {
        ed["skipWarning"] = json!(true);
    }

    // ── Gas Station specific fields ──
    // 交易信息
    ed["toAdr"] = json!(to_addr);
    ed["coinAmount"] = json!(coin_amount);
    if let Some(ta) = token_address {
        ed["tokenAddress"] = json!(ta);
    }
    // Gas 手续费
    ed["serviceCharge"] = json!(unsigned.service_charge);
    ed["feeTokenAddress"] = json!(unsigned.service_charge_fee_token_address);
    // 合约 nonce
    if !unsigned.contract_nonce.is_empty() {
        ed["contractNonce"] = json!(unsigned.contract_nonce);
    }
    // relayerId + context: 从 tokenList 中匹配选中的 token
    if let Some(selected) = unsigned.gas_station_token_list.iter().find(|t| {
        t.fee_token_address == unsigned.service_charge_fee_token_address
    }) {
        ed["relayerId"] = json!(selected.relayer_id);
        ed["context"] = json!(selected.context);
    }
    // user712Data: 每次 Gas Station 交易都透传
    if !unsigned.user712_data.is_null() {
        ed["user712Data"] = unsigned.user712_data.clone();
    }

    // ── 7702 upgrade only fields ──
    if include_7702 {
        if !unsigned.eoa_nonce.is_empty() {
            ed["nonce"] = json!(unsigned.eoa_nonce);
        }
        if !unsigned.user7702_data.is_null() {
            ed["user7702Data"] = unsigned.user7702_data.clone();
        }
    }

    ed
}

/// Flow 1: 首次 Gas Station — 升级 7702 + 交易（needUpdate7702=true）
#[allow(clippy::too_many_arguments)]
async fn gs_broadcast_with_7702_upgrade(
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
            .map_err(|_| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?)?;

    let msg_for_sign = gs_build_msg_for_sign(unsigned, session, &signing_seed, true)?;
    let extra_data_obj = gs_build_extra_data(unsigned, &msg_for_sign, to_addr, coin_amount, token_address, force, true);

    gs_do_broadcast(client, access_token, account_id, addr_info, &extra_data_obj, force).await
}

/// Flow 2: 后续 Gas Station 交易（needUpdate7702=false，已升级 7702）
#[allow(clippy::too_many_arguments)]
async fn gs_broadcast_transaction(
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
            .map_err(|_| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?)?;

    let msg_for_sign = gs_build_msg_for_sign(unsigned, session, &signing_seed, false)?;
    let extra_data_obj = gs_build_extra_data(unsigned, &msg_for_sign, to_addr, coin_amount, token_address, force, false);

    gs_do_broadcast(client, access_token, account_id, addr_info, &extra_data_obj, force).await
}

/// Gas Station broadcast 公共发送逻辑 + debug dump
async fn gs_do_broadcast(
    client: &mut crate::wallet_api::WalletApiClient,
    access_token: &str,
    account_id: &str,
    addr_info: &crate::wallet_store::AddressInfo,
    extra_data_obj: &Value,
    force: bool,
) -> Result<crate::wallet_api::BroadcastResponse> {
    let extra_data_str =
        serde_json::to_string(extra_data_obj).context("failed to serialize extraData")?;

    // TEMPORARY: debug_dump
    let broadcast_req = json!({
        "accountId": account_id,
        "address": addr_info.address,
        "chainIndex": addr_info.chain_index,
        "extraData": extra_data_obj,
    });
    let broadcast_resp = match client
        .broadcast_transaction(
            access_token,
            account_id,
            &addr_info.address,
            &addr_info.chain_index,
            &extra_data_str,
            None,
        )
        .await
    {
        Ok(v) => v,
        Err(e) => {
            super::debug_dump::dump_error("03-broadcast", &broadcast_req, &format!("{e:#}"));
            return Err(handle_confirming_error(e, force));
        }
    };
    super::debug_dump::dump("03-broadcast", &broadcast_req, &json!({
        "txHash": &broadcast_resp.tx_hash,
        "orderId": &broadcast_resp.order_id,
        "pkgId": &broadcast_resp.pkg_id,
        "orderType": &broadcast_resp.order_type,
    }));

    Ok(broadcast_resp)
}

/// Gas Station: 根据 needUpdate7702 路由到对应的 broadcast 流程
#[allow(clippy::too_many_arguments)]
async fn gas_station_sign_and_broadcast(
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

// ── Gas Station status handlers（对齐 review.md Section 0） ─────────────────

/// HAS_PENDING_TX: 有 pending 交易，拦截不继续
fn handle_gs_pending_tx() -> Result<()> {
    output::success(json!({
        "gasStationUsed": true,
        "hasPendingTx": true,
    }));
    Ok(())
}

/// INSUFFICIENT_ALL: 所有 gas token 不足，引导充值
fn handle_gs_insufficient_all(
    unsigned: &crate::wallet_api::UnsignedInfoResponse,
    from_addr: &str,
) -> Result<()> {
    output::success(json!({
        "gasStationUsed": true,
        "insufficientAll": true,
        "gasStationTokenList": unsigned.gas_station_token_list,
        "fromAddr": from_addr,
    }));
    Ok(())
}

/// Build sufficient-token list string for CliConfirming messages
fn format_sufficient_tokens(unsigned: &crate::wallet_api::UnsignedInfoResponse) -> String {
    unsigned
        .gas_station_token_list
        .iter()
        .filter(|t| t.sufficient)
        .enumerate()
        .map(|(i, t)| format!("{}. {} (balance: {}, fee: {})", i + 1, t.symbol, t.balance, t.service_charge))
        .collect::<Vec<_>>()
        .join("\n")
}

/// FIRST_TIME_PROMPT: 首次启用，需要用户确认选 token（Scene A）
fn handle_gs_first_time_prompt(
    unsigned: &crate::wallet_api::UnsignedInfoResponse,
) -> Result<()> {
    let token_list_str = format_sufficient_tokens(unsigned);
    let message = format!(
        "Your native token balance is insufficient to pay gas. You can enable Gas Station to pay gas with stablecoins.\n\
         \nAvailable tokens:\n{}\n\
         \nThree options:\n\
         1. Enable and set a default token: choose a token above, future transactions will use it automatically.\n\
         2. Enable without setting default: system will auto-select the token with highest balance each time.\n\
         3. Do NOT enable Gas Station: terminate this flow. Top up native token and try again.",
        token_list_str
    );
    let next = format!(
        "Option 1 (enable + set default): --gas-token-address <addr> --relayer-id <id> --enable-gas-station\n\
         Option 2 (enable, no default): --enable-gas-station\n\
         Option 3 (do NOT enable): skip re-run, tell user to top up native token\n\
         Token list: {}",
        serde_json::to_string(&unsigned.gas_station_token_list).unwrap_or_default()
    );
    Err(crate::output::CliConfirming { message, next }.into())
}

/// READY_TO_USE 但默认 token 不足（Scene C）：展示备选 token 让用户改选
fn handle_gs_default_token_insufficient(
    unsigned: &crate::wallet_api::UnsignedInfoResponse,
) -> Result<()> {
    let token_list_str = format_sufficient_tokens(unsigned);
    let message = format!(
        "Your default gas token has insufficient balance. Choose an alternative token to pay gas:\n{}\n\
         \nOptions:\n\
         1. Use for this transaction only (default token unchanged).\n\
         2. Use and update as new default token.",
        token_list_str
    );
    let next = format!(
        "Option 1 (this time only): --gas-token-address <addr> --relayer-id <id>\n\
         Option 2 (use + update default): --gas-token-address <addr> --relayer-id <id>, then run: wallet gas-station update-default-token --chain <chain> --gas-token-address <addr>\n\
         Token list: {}",
        serde_json::to_string(&unsigned.gas_station_token_list).unwrap_or_default()
    );
    Err(crate::output::CliConfirming { message, next }.into())
}

/// PENDING_UPGRADE / REENABLE_ONLY / READY_TO_USE（默认 token 充足）：后端已给 hash，直接签+广播
#[allow(clippy::too_many_arguments)]
async fn handle_gs_auto_sign_broadcast(
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
    )
    .await
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
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

    // ── handle_confirming_error tests ─────────────────────────────────

    #[test]
    fn broadcast_error_81362_no_force_returns_cli_confirming() {
        let api_err = crate::wallet_api::ApiCodeError {
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
            code: "81362".to_string(),
            msg: "please confirm".to_string(),
        };
        let err: anyhow::Error = api_err.into();
        let result = handle_confirming_error(err, true);
        // Should NOT be CliConfirming when force=true
        assert!(result
            .downcast_ref::<crate::output::CliConfirming>()
            .is_none());
        assert_eq!(format!("{}", result), "please confirm");
    }

    #[test]
    fn broadcast_error_other_code_returns_plain_error() {
        let api_err = crate::wallet_api::ApiCodeError {
            code: "50000".to_string(),
            msg: "server error".to_string(),
        };
        let err: anyhow::Error = api_err.into();
        let result = handle_confirming_error(err, false);
        assert!(result
            .downcast_ref::<crate::output::CliConfirming>()
            .is_none());
        assert_eq!(format!("{}", result), "server error");
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
}
