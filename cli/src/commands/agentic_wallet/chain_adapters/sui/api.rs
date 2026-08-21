//! Calls SUI Agentic Wallet transaction APIs.

use anyhow::{Error, Result};
use serde_json::{json, Value};

use crate::commands::agentic_wallet::support::json::first_data_item;
use crate::commands::sink::CodedError;
use crate::wallet_api::{ApiCodeError, BroadcastResponse, WalletApiClient};

use super::context::SuiContext;

const UNSIGNED_INFO_PATH: &str = "/priapi/v5/wallet/agentic/pre-transaction/unsignedInfo";

pub struct SuiApi {
    client: WalletApiClient,
}

impl SuiApi {
    /// Creates the authenticated wallet API adapter used by SUI commands.
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: WalletApiClient::new()?,
        })
    }

    /// Fetches metadata for `coin_type` on the current SUI account and returns one item.
    pub async fn token_metadata(&mut self, context: &SuiContext, coin_type: &str) -> Result<Value> {
        let data = self
            .client
            .get_token_info(&context.access_token, context.chain_index_u64()?, coin_type)
            .await
            .map_err(map_api_error)?;
        Ok(first_data_item(data))
    }

    /// Requests unsigned data for a native or `Coin<T>` transfer and returns one prepared item.
    pub async fn prepare_transaction(
        &mut self,
        context: &SuiContext,
        to: &str,
        amount: &str,
        coin_type: Option<&str>,
    ) -> Result<Value> {
        let mut body = json!({
            "chainIndex": context.chain_index_u64()?,
            "fromAddr": context.address.address,
            "toAddr": to,
            "amount": amount,
            "sessionCert": context.session_cert()?,
        });
        if let Some(value) = coin_type {
            body["contractAddr"] = Value::String(value.to_string());
        }
        let idempotency_key = uuid::Uuid::new_v4().to_string();
        let headers = [("idempotency-key", idempotency_key.as_str())];
        let data = self
            .client
            .post_authed_with_headers(
                UNSIGNED_INFO_PATH,
                &context.access_token,
                &body,
                Some(&headers),
            )
            .await
            .map_err(map_api_error)?;
        Ok(first_data_item(data))
    }

    /// Requests unsigned data for a pre-built SUI TransactionData / PTB.
    pub async fn prepare_contract_call(
        &mut self,
        context: &SuiContext,
        to: Option<&str>,
        amount: &str,
        tx_bytes: &str,
    ) -> Result<Value> {
        let body = build_contract_call_body(
            context.chain_index_u64()?,
            &context.address.address,
            to,
            amount,
            &context.session_cert()?,
            tx_bytes,
        );
        let idempotency_key = uuid::Uuid::new_v4().to_string();
        let headers = [("idempotency-key", idempotency_key.as_str())];
        let data = self
            .client
            .post_authed_with_headers(
                UNSIGNED_INFO_PATH,
                &context.access_token,
                &body,
                Some(&headers),
            )
            .await
            .map_err(map_api_error)?;
        Ok(first_data_item(data))
    }

    /// Broadcasts one signed SUI transaction and returns the parsed broadcast response.
    pub async fn broadcast_transaction(
        &mut self,
        context: &SuiContext,
        extra_data: &str,
    ) -> Result<BroadcastResponse> {
        self.client
            .broadcast_transaction(
                &context.access_token,
                &context.account_id,
                &context.address.address,
                &context.profile.chain_index,
                extra_data,
                None,
            )
            .await
    }
}

/// Builds the SUI PTB unsignedInfo request. `toAddr` and `contractAddr` are
/// service envelope fields; the PTB in `txParam.txBytes` remains authoritative.
fn build_contract_call_body(
    chain_index: u64,
    from_addr: &str,
    to_addr: Option<&str>,
    amount: &str,
    session_cert: &str,
    tx_bytes: &str,
) -> Value {
    json!({
        "chainIndex": chain_index,
        "fromAddr": from_addr,
        "toAddr": to_addr.unwrap_or("0x"),
        "amount": amount,
        "contractAddr": "0x0",
        "sessionCert": session_cert,
        "txParam": {
            "txBytes": tx_bytes,
        },
    })
}

/// Maps SUI service failures into the CLI's stable coded-error representation.
pub(in crate::commands::agentic_wallet) fn map_api_error(error: Error) -> Error {
    match error.downcast::<ApiCodeError>() {
        Ok(api_error) => CodedError::new(&api_error.code, None, api_error.msg).into(),
        Err(error) => error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_call_body_nests_tx_bytes_in_tx_param() {
        let body = build_contract_call_body(784, "0xsender", None, "0", "session-cert", "AAECAwQ=");

        assert_eq!(body["chainIndex"], 784);
        assert_eq!(body["fromAddr"], "0xsender");
        assert_eq!(body["toAddr"], "0x");
        assert_eq!(body["amount"], "0");
        assert_eq!(body["contractAddr"], "0x0");
        assert_eq!(body["sessionCert"], "session-cert");
        assert_eq!(body["txParam"]["txBytes"], "AAECAwQ=");
        assert!(body.get("unsignedTx").is_none());
        assert!(body.get("inputData").is_none());
    }
}
