//! A2A Pay backend API client (defi-sa-gateway).
//!
//! Wraps `ApiClient` so we get DoH resolution + retries + uniform response
//! parsing. Decoupled from the main wallet backend: only serves
//! `/api/v6/pay/a2a/payment/*`.

use anyhow::Result;
use serde_json::Value;

use crate::client::ApiClient;

/// Default base URL; override at runtime via `A2A_PAY_BASE_URL` or `with_base_url(...)`.
const PAYMENT_BASE_URL: &str = "https://web3.okx.com";

pub struct PaymentApiClient {
    api: ApiClient,
    base_url: String,
}

impl PaymentApiClient {
    pub fn new() -> Self {
        Self::build(None)
    }

    /// Construct with an explicit base URL (overrides `A2A_PAY_BASE_URL` env).
    pub fn with_base_url(base_url: String) -> Self {
        Self::build(Some(base_url))
    }

    fn build(base_url_override: Option<String>) -> Self {
        let effective_url = base_url_override.unwrap_or_else(|| {
            std::env::var("A2A_PAY_BASE_URL")
                .ok()
                .unwrap_or_else(|| PAYMENT_BASE_URL.to_string())
        });
        let api = ApiClient::new(Some(effective_url.as_str()))
            .expect("failed to create ApiClient");
        Self {
            api,
            base_url: effective_url,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Seller: POST `/payment/create`
    pub async fn create(&mut self, body: &Value) -> Result<Value> {
        self.api.post("/api/v6/pay/a2a/payment/create", body).await
    }

    /// Buyer: GET `/payment/{id}` — returns challenge + business params.
    pub async fn get_payment(&mut self, payment_id: &str) -> Result<Value> {
        let path = format!("/api/v6/pay/a2a/payment/{payment_id}");
        self.api.get(&path, &[]).await
    }

    /// Buyer: POST `/payment/{id}/credential` — submit signed EIP-3009 credential.
    pub async fn submit_credential(
        &mut self,
        payment_id: &str,
        body: &Value,
    ) -> Result<Value> {
        let path = format!("/api/v6/pay/a2a/payment/{payment_id}/credential");
        self.api.post(&path, body).await
    }

    /// GET `/payment/{id}/status` — query current execution status.
    pub async fn get_status(&mut self, payment_id: &str) -> Result<Value> {
        let path = format!("/api/v6/pay/a2a/payment/{payment_id}/status");
        self.api.get(&path, &[]).await
    }
}

impl Default for PaymentApiClient {
    fn default() -> Self {
        Self::new()
    }
}
