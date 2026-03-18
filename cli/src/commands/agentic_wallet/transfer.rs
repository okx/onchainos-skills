use anyhow::{bail, Context, Result};
use base64::Engine;
use serde_json::{json, Value};

use crate::keyring_store;
use crate::output;
use crate::wallet_api::WalletApiClient;
use crate::wallet_store::{self, AddressInfo, WalletsJson};

use super::auth::{ensure_tokens_refreshed, format_api_error};

// ── resolve_address ───────────────────────────────────────────────────

/// Resolve a (from, chain) pair to (account_id, AddressInfo).
///
/// If `from_addr` is Some, scan ALL entries in accounts_map for a matching
/// (address, chain_name) pair. Otherwise use selected_account_id.
pub(super) fn resolve_address(
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
}

/// Shared flow: resolve wallet → unsignedInfo → sign → broadcast → output txHash.
/// `is_contract_call`: when true, omits `txType` from extraData.
/// `mev_protection`: when true, passes `isMEV: true` to the broadcast API (supported on ETH, BSC, Base).
/// `chain`: the realChainIndex (standard chain ID, e.g. "1" for Ethereum, "501" for Solana).
async fn sign_and_broadcast(
    chain: &str,
    from: Option<&str>,
    tx: TxParams<'_>,
    is_contract_call: bool,
    mev_protection: bool,
) -> Result<()> {
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

    let client = WalletApiClient::new()?;
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

    let signing_seed = crate::crypto::hpke_decrypt_session_sk(&encrypted_session_sk, &session_key)?;
    let signing_seed_b64 = base64::engine::general_purpose::STANDARD.encode(signing_seed);

    let mut msg_for_sign_map = serde_json::Map::new();

    if !unsigned.hash.is_empty() {
        let sig = crate::crypto::ed25519_sign_eip191(&unsigned.hash, &signing_seed)?;
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
    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][sign_and_broadcast] Step 10: extraData={}",
            serde_json::to_string_pretty(&extra_data_obj).unwrap_or_default()
        );
    }
    let extra_data_str =
        serde_json::to_string(&extra_data_obj).context("failed to serialize extraData")?;

    let broadcast_resp = client
        .broadcast_transaction(
            &access_token,
            &account_id,
            &addr_info.address,
            &addr_info.chain_index,
            &extra_data_str,
        )
        .await
        .map_err(format_api_error)?;

    if cfg!(feature = "debug-log") {
        eprintln!(
            "[DEBUG][sign_and_broadcast] === END SUCCESS: txHash={}",
            broadcast_resp.tx_hash
        );
    }
    output::success(json!({ "txHash": broadcast_resp.tx_hash }));
    Ok(())
}

// ── send ─────────────────────────────────────────────────────────────

/// onchainos wallet send
pub(super) async fn cmd_send(
    amount: &str,
    receipt: &str,
    chain: &str,
    from: Option<&str>,
    contract_token: Option<&str>,
) -> Result<()> {
    if amount.is_empty() || receipt.is_empty() || chain.is_empty() {
        bail!("amount, receipt and chain are required");
    }

    sign_and_broadcast(
        chain,
        from,
        TxParams {
            to_addr: receipt,
            value: amount,
            contract_addr: contract_token,
            input_data: None,
            unsigned_tx: None,
            gas_limit: None,
            aa_dex_token_addr: None,
            aa_dex_token_amount: None,
            jito_unsigned_tx: None,
        },
        false,
        false,
    )
    .await
}

// ── contract-call ─────────────────────────────────────────────────────

/// onchainos wallet contract-call
pub(super) async fn cmd_contract_call(
    to: &str,
    chain: &str,
    value: &str,
    input_data: Option<&str>,
    unsigned_tx: Option<&str>,
    gas_limit: Option<&str>,
    from: Option<&str>,
    aa_dex_token_addr: Option<&str>,
    aa_dex_token_amount: Option<&str>,
    mev_protection: bool,
    jito_unsigned_tx: Option<&str>,
) -> Result<()> {
    if to.is_empty() || chain.is_empty() {
        bail!("to and chain are required");
    }
    if input_data.is_none() && unsigned_tx.is_none() {
        bail!("either --input-data (EVM) or --unsigned-tx (SOL) is required");
    }

    sign_and_broadcast(
        chain,
        from,
        TxParams {
            to_addr: to,
            value,
            contract_addr: Some(to),
            input_data,
            unsigned_tx,
            gas_limit,
            aa_dex_token_addr,
            aa_dex_token_amount,
            jito_unsigned_tx,
        },
        true,
        mev_protection,
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
}
