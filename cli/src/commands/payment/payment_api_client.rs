//! A2A Pay backend API client (defi-sa-gateway).
//!
//! Delegates HTTP to `WalletApiClient` to reuse its DoH resolution and failover
//! retry. Decoupled from the main wallet backend: only serves
//! `/api/v6/pay/a2a/payment/*`. Returns `body["data"]`.

use anyhow::Result;
use serde_json::Value;

use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;
use crate::wallet_api::WalletApiClient;

/// Default base URL; override at runtime via `A2A_PAY_BASE_URL` or `with_base_url(...)`.
const PAYMENT_BASE_URL: &str =
    "http://defi-sa-gateway.forked-okx-test2-dataasset.swim.env";

pub struct PaymentApiClient {
    wallet: WalletApiClient,
    base_url: String,
}

impl PaymentApiClient {
    pub fn new() -> Self {
        Self::build(None)
    }

    pub fn with_base_url(base_url: String) -> Self {
        Self::build(Some(base_url))
    }

    fn build(base_url_override: Option<String>) -> Self {
        let effective_url = base_url_override.unwrap_or_else(|| {
            std::env::var("A2A_PAY_BASE_URL")
                .ok()
                .unwrap_or_else(|| PAYMENT_BASE_URL.to_string())
        });
        let wallet = WalletApiClient::with_base_url(Some(effective_url.as_str()))
            .expect("failed to create WalletApiClient");
        Self {
            wallet,
            base_url: effective_url,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Seller: POST `/payment/create`
    pub async fn create(&mut self, body: &Value) -> Result<Value> {
        let token = ensure_tokens_refreshed().await?;
        self.wallet
            .post_authed("/api/v6/pay/a2a/payment/create", &token, body)
            .await
    }

    /// Buyer: GET `/payment/{id}` — returns challenge + business params.
    pub async fn get_payment(&mut self, payment_id: &str) -> Result<Value> {
        let token = ensure_tokens_refreshed().await?;
        let path = format!("/api/v6/pay/a2a/payment/{payment_id}");
        self.wallet.get_authed(&path, &token, &[]).await
    }

    /// Buyer: POST `/payment/{id}/credential` — submit signed EIP-3009 credential.
    pub async fn submit_credential(
        &mut self,
        payment_id: &str,
        body: &Value,
    ) -> Result<Value> {
        let token = ensure_tokens_refreshed().await?;
        let path = format!("/api/v6/pay/a2a/payment/{payment_id}/credential");
        self.wallet.post_authed(&path, &token, body).await
    }

    /// GET `/payment/{id}/status` — query current execution status.
    pub async fn get_status(&mut self, payment_id: &str) -> Result<Value> {
        let token = ensure_tokens_refreshed().await?;
        let path = format!("/api/v6/pay/a2a/payment/{payment_id}/status");
        self.wallet.get_authed(&path, &token, &[]).await
    }
}

impl Default for PaymentApiClient {
    fn default() -> Self {
        Self::new()
    }
}
