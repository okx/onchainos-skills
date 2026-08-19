//! Calls Bitcoin and BRC-20 Agentic Wallet APIs.

use anyhow::{Context, Result};
use serde_json::{json, Value};

use crate::commands::agentic_wallet::support::amount::{decimal_field, value_as_decimal_string};
use crate::commands::agentic_wallet::support::json::first_data_item;
use crate::wallet_api::{BroadcastResponse, WalletApiClient};

use super::context::BtcContext;
use super::models::BtcOutPoint;

pub struct BtcApi {
    client: WalletApiClient,
}

pub const UTXO_MANAGE_BATCH_SIZE: usize = 50;

impl BtcApi {
    /// Creates the authenticated wallet API adapter used by Bitcoin commands.
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: WalletApiClient::new()?,
        })
    }

    /// Fetches metadata for `token_address` on the current Bitcoin account and returns one item.
    pub async fn token_metadata(
        &mut self,
        context: &BtcContext,
        token_address: &str,
    ) -> Result<Value> {
        let data = self
            .client
            .get_token_info(
                &context.access_token,
                context.chain_index_u64()?,
                token_address,
            )
            .await
            .map_err(super::error::map_api_error)?;
        Ok(first_data_item(data))
    }

    /// Fetches the current account's balance for one BRC-20 token.
    ///
    /// This deliberately returns the raw balance payload: BRC-20 summary
    /// rendering derives its display values with exact decimal arithmetic.
    pub async fn brc20_balance(
        &mut self,
        context: &BtcContext,
        token_address: &str,
    ) -> Result<Value> {
        let chain_index = context.profile.chain_index.as_str();
        let query = [
            ("accountId", context.account_id.as_str()),
            ("chains", chain_index),
            ("tokenAddresses[0].chainIndex", chain_index),
            ("tokenAddresses[0].tokenAddress", token_address),
        ];
        self.client
            .balance_single(&context.access_token, &query)
            .await
            .map_err(super::error::map_api_error)
    }

    /// Queries one Bitcoin UTXO availability view and returns its normalized data object.
    pub async fn availability_details(
        &mut self,
        context: &BtcContext,
        query_type: &str,
    ) -> Result<Value> {
        self.availability_details_request(context, query_type, None)
            .await
    }

    /// Queries transferable UTXOs for a BRC-20 token and returns the normalized snapshot.
    pub async fn brc20_transferable_utxos(
        &mut self,
        context: &BtcContext,
        token_address: &str,
    ) -> Result<Value> {
        self.availability_details_request(
            context,
            "BRC20_TRANSFERABLE_UTXO_LIST",
            Some(token_address),
        )
        .await
    }

    /// Sends the shared availability-details request with an optional BRC-20 token address.
    async fn availability_details_request(
        &mut self,
        context: &BtcContext,
        query_type: &str,
        token_address: Option<&str>,
    ) -> Result<Value> {
        let mut body = json!({
            "chainIndex": context.profile.chain_index,
            "address": context.address.address,
            "queryType": query_type,
        });
        if let Some(token_address) = token_address {
            body["tokenAddress"] = json!(token_address);
        }
        let data = self
            .client
            .post_authed(
                "/priapi/v5/wallet/agentic/utxo/availability-details",
                &context.access_token,
                &body,
            )
            .await
            .map_err(super::error::map_api_error)?;
        Ok(first_data_item(data))
    }

    /// Applies `action` to the supplied outpoints and returns the service mutation result.
    pub async fn manage_utxos(
        &mut self,
        context: &BtcContext,
        action: &str,
        message: &str,
        outpoints: &[BtcOutPoint],
    ) -> Result<Value> {
        let body =
            build_manage_utxos_body(&context.profile.chain_index, action, message, outpoints)?;
        self.client
            .post_authed_mutation_no_retry(
                "/priapi/v5/wallet/agentic/utxo/user-asset-manage",
                &context.access_token,
                &body,
            )
            .await
            .map_err(super::error::map_api_error)
    }

    #[allow(clippy::too_many_arguments)]
    /// Requests unsigned data for a BTC or BRC-20 transfer and returns the prepared transaction.
    pub async fn prepare_transaction(
        &mut self,
        context: &BtcContext,
        to: &str,
        amount: &str,
        token_address: Option<&str>,
        sign_type: Option<&str>,
        operation_token: Option<&str>,
        preview_version: Option<&str>,
    ) -> Result<Value> {
        let mut body = json!({
            "chainIndex": context.chain_index_u64()?,
            "fromAddr": context.address.address,
            "toAddr": to,
            "amount": amount,
            "sessionCert": context.session_cert()?,
        });
        if let Some(value) = token_address {
            body["contractAddr"] = Value::String(value.to_string());
        }
        if let Some(value) = sign_type {
            body["signType"] = Value::String(value.to_string());
        }
        if let Some(value) = operation_token {
            body["operationToken"] = Value::String(value.to_string());
        }
        if let Some(value) = preview_version {
            body["previewVersion"] = Value::String(value.to_string());
        }
        self.request_unsigned_info(context, body).await
    }

    /// Requests unsigned BRC-20 transfer data using the user-selected carrier UTXO.
    pub async fn prepare_selected_brc20_transfer(
        &mut self,
        context: &BtcContext,
        to: &str,
        amount: &str,
        token_address: &str,
        tx_param: Value,
    ) -> Result<Value> {
        let mut body = json!({
            "chainIndex": context.chain_index_u64()?,
            "fromAddr": context.address.address,
            "toAddr": to,
            "contractAddr": token_address,
            "amount": amount,
            "sessionCert": context.session_cert()?,
            "signType": "transfer",
            "txParam": tx_param,
        });
        if let Some(wallet_type) = context.social_wallet_type() {
            body["walletType"] = json!(wallet_type);
        }
        self.request_unsigned_info(context, body).await
    }

    /// Posts an unsignedInfo body with an idempotency key and returns its first data item.
    async fn request_unsigned_info(&mut self, context: &BtcContext, body: Value) -> Result<Value> {
        let idempotency_key = uuid::Uuid::new_v4().to_string();
        let headers = [("idempotency-key", idempotency_key.as_str())];
        let data = self
            .client
            .post_authed_with_headers(
                "/priapi/v5/wallet/agentic/pre-transaction/unsignedInfo",
                &context.access_token,
                &body,
                Some(&headers),
            )
            .await?;
        Ok(first_data_item(data))
    }

    /// Sends locally signed inscription hashes to `sign-tx` and returns its normalized result.
    pub async fn sign_transaction(
        &mut self,
        context: &BtcContext,
        unsigned: &Value,
        signed_hashes: &[Value],
    ) -> Result<Value> {
        let sign_type = unsigned
            .get("signType")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow::anyhow!("unsignedInfo response is missing signType"))?;
        let tx_param = unsigned
            .get("txParam")
            .filter(|value| !value.is_null())
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("unsignedInfo response is missing txParam"))?;
        if signed_hashes.is_empty() {
            anyhow::bail!("signed hash list must not be empty");
        }
        let body = json!({
            "from": context.address.address,
            "chainIndex": context.chain_index_u64()?,
            "sessionCert": context.session_cert()?,
            "payload": [{
                "signType": sign_type,
                "txParam": tx_param,
                "unsignedHashList": signed_hashes,
            }],
        });
        let data = self
            .client
            .post_authed_mutation_no_retry(
                "/priapi/v5/wallet/agentic/pre-transaction/sign-tx",
                &context.access_token,
                &body,
            )
            .await?;
        Ok(first_data_item(data))
    }

    /// Broadcasts one signed Bitcoin transaction and returns the parsed broadcast response.
    pub async fn broadcast_transaction(
        &mut self,
        context: &BtcContext,
        extra_data: &str,
    ) -> Result<BroadcastResponse> {
        let body = json!({
            "accountId": context.account_id,
            "address": context.address.address,
            "chainIndex": context.profile.chain_index,
            "extraData": extra_data,
        });
        let data = self
            .client
            .post_authed_mutation_no_retry(
                "/priapi/v5/wallet/agentic/pre-transaction/broadcast-transaction",
                &context.access_token,
                &body,
            )
            .await?;
        let item = data
            .as_array()
            .and_then(|items| items.first())
            .context("broadcast: expected a non-empty data array")?;
        serde_json::from_value(item.clone()).context("broadcast: failed to parse response")
    }

    /// Broadcasts an ordered inscription transaction batch and returns every response item.
    pub async fn batch_broadcast_transactions(
        &mut self,
        context: &BtcContext,
        body: &Value,
    ) -> Result<Vec<BroadcastResponse>> {
        let data = self
            .client
            .post_authed_mutation_no_retry(
                "/priapi/v5/wallet/agentic/pre-transaction/batch-broadcast-transaction",
                &context.access_token,
                body,
            )
            .await?;
        let items = data
            .as_array()
            .context("batch broadcast: expected data to be an array")?;
        items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                serde_json::from_value(item.clone()).with_context(|| {
                    format!("batch broadcast: failed to parse response item {index}")
                })
            })
            .collect()
    }

    /// Queries a Bitcoin order by transaction hash or order ID and validates its account context.
    pub async fn order_detail(
        &mut self,
        context: &BtcContext,
        tx_hash: Option<&str>,
        order_id: Option<&str>,
    ) -> Result<Value> {
        let mut query = vec![
            ("accountId", context.account_id.as_str()),
            ("chainIndex", context.profile.chain_index.as_str()),
            ("address", context.address.address.as_str()),
        ];
        if let Some(value) = tx_hash {
            query.push(("txHash", value));
        }
        if let Some(value) = order_id {
            query.push(("orderId", value));
        }
        let data = self
            .client
            .get_authed(
                "/priapi/v5/wallet/agentic/order/detail",
                &context.access_token,
                &query,
            )
            .await?;
        let detail = first_data_item(data);
        validate_order_detail_context(context, &detail, tx_hash, order_id)?;
        Ok(detail)
    }

    /// Requests closure of the supplied pending transaction hashes and returns the service result.
    pub async fn close_transactions(
        &mut self,
        context: &BtcContext,
        tx_hashes: &[String],
    ) -> Result<Value> {
        self.client
            .post_authed_mutation_no_retry(
                "/api/v5/wallet/pre-transaction/close-transaction",
                &context.access_token,
                &json!({
                    "chainIndex": context.profile.chain_index,
                    "txHashList": tx_hashes,
                }),
            )
            .await
    }
}

/// Reads BRC-20 token precision from metadata and returns it as `u32`.
pub(in crate::commands::agentic_wallet) fn extract_token_decimals(metadata: &Value) -> Result<u32> {
    decimal_field(metadata)
        .ok_or_else(|| anyhow::anyhow!("BRC-20 token metadata is missing decimal/decimals"))
}

/// Ensures an order detail response belongs to the requested chain, account, and identifier.
fn validate_order_detail_context(
    context: &BtcContext,
    detail: &Value,
    tx_hash: Option<&str>,
    order_id: Option<&str>,
) -> Result<()> {
    if let Some(chain_index) = detail.get("chainIndex").and_then(value_as_decimal_string) {
        if chain_index != context.profile.chain_index {
            anyhow::bail!(
                "ORDER_CONTEXT_MISMATCH: response chainIndex {} does not match requested {}",
                chain_index,
                context.profile.chain_index
            );
        }
    }
    if let Some(account_id) = detail.get("accountId").and_then(Value::as_str) {
        if !account_id.is_empty() && account_id != context.account_id {
            anyhow::bail!(
                "ORDER_CONTEXT_MISMATCH: response accountId does not match current account"
            );
        }
    }
    if let Some(expected) = tx_hash {
        if let Some(actual) = detail.get("txHash").and_then(Value::as_str) {
            if !actual.is_empty() && actual != expected {
                anyhow::bail!("ORDER_CONTEXT_MISMATCH: response txHash does not match request");
            }
        }
    }
    if let Some(expected) = order_id {
        if let Some(actual) = detail.get("orderId").and_then(Value::as_str) {
            if !actual.is_empty() && actual != expected {
                anyhow::bail!("ORDER_CONTEXT_MISMATCH: response orderId does not match request");
            }
        }
    }
    Ok(())
}

/// Builds and validates one bounded UTXO-management request body.
fn build_manage_utxos_body(
    chain_index: &str,
    action: &str,
    message: &str,
    outpoints: &[BtcOutPoint],
) -> Result<Value> {
    if chain_index.trim().is_empty() {
        anyhow::bail!("UTXO management chainIndex must not be empty");
    }
    if !matches!(action, "ignoreAsset" | "cancelIgnore") {
        anyhow::bail!("unsupported UTXO management action: {action}");
    }
    if message.trim().is_empty() {
        anyhow::bail!("UTXO management message must not be empty");
    }
    if outpoints.is_empty() || outpoints.len() > UTXO_MANAGE_BATCH_SIZE {
        anyhow::bail!("UTXO management requires 1..={UTXO_MANAGE_BATCH_SIZE} outpoints per batch");
    }
    Ok(json!({
        "chainIndex": chain_index,
        "action": action,
        "message": message,
        "utxos": outpoints.iter().map(BtcOutPoint::to_api_value).collect::<Vec<_>>(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_decimals_accept_string_or_number() {
        assert_eq!(
            extract_token_decimals(&json!({"decimal": "18"})).unwrap(),
            18
        );
        assert_eq!(extract_token_decimals(&json!({"decimals": 8})).unwrap(), 8);
        assert!(extract_token_decimals(&json!({})).is_err());
    }

    fn test_context() -> BtcContext {
        BtcContext {
            access_token: String::new(),
            account_id: "account-1".to_string(),
            login_type: "email".to_string(),
            profile: crate::commands::agentic_wallet::chain_profile::from_entry(&json!({
                "chainIndex": 0,
                "realChainIndex": 0,
                "chainName": "btc",
                "isEvmChain": false
            }))
            .unwrap(),
            address: crate::wallet_store::AddressInfo {
                account_id: "account-1".to_string(),
                address: "bc1p7smn2yf58a3w5586c723fj6v5pgds39tuktv2xflz5f75kjhy4xsdyxgvr"
                    .to_string(),
                chain_index: "0".to_string(),
                chain_name: "btc".to_string(),
                address_type: "taproot".to_string(),
                chain_path: String::new(),
            },
        }
    }

    #[test]
    fn order_detail_rejects_cross_chain_response() {
        let context = test_context();
        let error = validate_order_detail_context(
            &context,
            &json!({
                "accountId": "account-1",
                "chainIndex": "607",
                "txHash": "requested"
            }),
            Some("requested"),
            None,
        )
        .unwrap_err();
        assert!(error.to_string().contains("ORDER_CONTEXT_MISMATCH"));
    }

    #[test]
    fn manage_request_matches_latest_utxo_contract() {
        let point = BtcOutPoint::parse(
            "4d3f6a7a45dbb9d3398a8f83c0219b6bedfdcd77d1de63cc09f9cfe360c553c0:3",
        )
        .unwrap();
        let body = build_manage_utxos_body(
            "0",
            "ignoreAsset",
            "User confirmed removal of UTXO asset protection",
            &[point],
        )
        .unwrap();

        assert_eq!(body["chainIndex"], "0");
        assert!(body.get("channels").is_none());
        assert_eq!(body["action"], "ignoreAsset");
        assert_eq!(
            body["message"],
            "User confirmed removal of UTXO asset protection"
        );
        assert_eq!(body["utxos"][0]["voutIndex"], "3");
    }

    #[test]
    fn manage_request_rejects_missing_reason_and_unknown_action() {
        let point = BtcOutPoint::parse(
            "4d3f6a7a45dbb9d3398a8f83c0219b6bedfdcd77d1de63cc09f9cfe360c553c0:0",
        )
        .unwrap();
        assert!(
            build_manage_utxos_body("0", "ignoreAsset", "", std::slice::from_ref(&point)).is_err()
        );
        assert!(
            build_manage_utxos_body("0", "remove", "reason", std::slice::from_ref(&point)).is_err()
        );
        assert!(build_manage_utxos_body(
            "0",
            "ignoreAsset",
            "reason",
            &vec![point; UTXO_MANAGE_BATCH_SIZE + 1]
        )
        .is_err());
    }
}
