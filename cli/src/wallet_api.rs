use anyhow::{bail, Context, Result};
use reqwest::Client;
use serde::{Deserialize, Deserializer};
use serde_json::{json, Value};

/// Structured error for non-zero API response codes.
/// Preserves the original backend `code` and `msg` so callers can
/// output them directly via `output::error`.
#[derive(Debug)]
pub struct ApiCodeError {
    pub code: String,
    pub msg: String,
}

impl std::fmt::Display for ApiCodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Wallet API error (code={}): {}", self.code, self.msg)
    }
}

impl std::error::Error for ApiCodeError {}

/// Deserialize a value that may be null, a string, or a number into a String.
/// null → "".
fn string_or_number<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let v = Value::deserialize(deserializer)?;
    match v {
        Value::Null => Ok(String::new()),
        Value::String(s) => Ok(s),
        Value::Number(n) => Ok(n.to_string()),
        other => Err(serde::de::Error::custom(format!(
            "expected string or number, got {}",
            other
        ))),
    }
}

/// Deserialize a value that may be null, a bool, or an integer (0/1) into a bool.
/// null → false.
fn bool_or_int<'de, D>(deserializer: D) -> std::result::Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let v = Value::deserialize(deserializer)?;
    match v {
        Value::Null => Ok(false),
        Value::Bool(b) => Ok(b),
        Value::Number(n) => Ok(n.as_i64().unwrap_or(0) != 0),
        other => Err(serde::de::Error::custom(format!(
            "expected bool or integer, got {}",
            other
        ))),
    }
}

/// Deserialize a nullable string: null → "".
fn nullable_string<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let v = Value::deserialize(deserializer)?;
    match v {
        Value::Null => Ok(String::new()),
        Value::String(s) => Ok(s),
        other => Err(serde::de::Error::custom(format!(
            "expected string or null, got {}",
            other
        ))),
    }
}

/// Build a URL-encoded query string from key-value pairs, filtering out empty values.
fn build_query_string(query: &[(&str, &str)]) -> String {
    let filtered: Vec<(&str, &str)> = query
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .copied()
        .collect();
    if filtered.is_empty() {
        return String::new();
    }
    let pairs: Vec<String> = filtered
        .iter()
        .map(|(k, v)| {
            let encoded_value = url::form_urlencoded::Serializer::new(String::new())
                .append_pair("", v)
                .finish();
            // encoded_value starts with "=", strip the leading "="
            format!("{}={}", k, &encoded_value[1..])
        })
        .collect();
    format!("?{}", pairs.join("&"))
}

/// HTTP client for the agentic-wallet API endpoints.
pub struct WalletApiClient {
    http: Client,
    base_url: String,
}

// ── API response types ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitResponse {
    pub flow_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyResponse {
    pub refresh_token: String,
    pub access_token: String,
    pub tee_id: String,
    pub session_cert: String,
    pub encrypted_session_sk: String,
    #[serde(deserialize_with = "string_or_number")]
    pub session_key_expire_at: String,
    pub project_id: String,
    pub account_id: String,
    pub account_name: String,
    #[serde(deserialize_with = "bool_or_int")]
    pub is_new: bool,
    #[serde(default)]
    pub address_list: Vec<VerifyAddressInfo>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyAddressInfo {
    #[serde(default)]
    pub account_id: String,
    pub address: String,
    #[serde(deserialize_with = "string_or_number")]
    pub chain_index: String,
    pub chain_name: String,
    pub address_type: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub chain_path: String,
}

#[derive(Debug, Deserialize)]
pub struct AkInitResponse {
    pub nonce: String,
    pub iss: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshResponse {
    pub refresh_token: String,
    pub access_token: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAccountResponse {
    pub project_id: String,
    pub account_id: String,
    pub account_name: String,
    #[serde(default)]
    pub address_list: Vec<VerifyAddressInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountListItem {
    pub project_id: String,
    pub account_id: String,
    pub account_name: String,
    #[serde(default)]
    pub is_default: bool,
}

/// Per-account entry from the address/list API.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressListAccountItem {
    pub account_id: String,
    #[serde(default)]
    pub addresses: Vec<VerifyAddressInfo>,
}

/// Wrapper for the account/address/list response `data` object.
/// `{ "accountCnt": N, "validAccountCnt": N, "addressCnt": N, "accounts": [...] }`
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddressListData {
    #[serde(default)]
    accounts: Vec<AddressListAccountItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnsignedInfoResponse {
    #[serde(default, deserialize_with = "nullable_string")]
    pub unsigned_tx_hash: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub unsign_hash: String, // Solana uses this instead of unsignedTxHash
    #[serde(default, deserialize_with = "nullable_string")]
    pub unsigned_tx: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub uop_hash: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub hash: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub auth_hash_for7702: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub execute_error_msg: String,
    #[serde(default)]
    pub execute_result: Value,
    #[serde(default)]
    pub extra_data: Value,
    #[serde(default, deserialize_with = "nullable_string")]
    pub sign_type: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub encoding: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub jito_unsigned_tx: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BroadcastResponse {
    #[serde(default, deserialize_with = "nullable_string")]
    pub pkg_id: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub order_id: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub order_type: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub tx_hash: String,
}

impl WalletApiClient {
    pub fn new() -> Result<Self> {
        let base_url = option_env!("OKX_BASE_URL")
            .map(|s| s.to_string())
            .unwrap_or_else(|| crate::client::DEFAULT_BASE_URL.to_string());

        Ok(Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
            base_url,
        })
    }

    // ── Low-level POST helpers ──────────────────────────────────────

    /// POST without Authorization header (for init / verify / refresh).
    pub async fn post_public(&self, path: &str, body: &Value) -> Result<Value> {
        let url = format!("{}{}", self.base_url, path);

        if cfg!(feature = "debug-log") {
            eprintln!("[DEBUG][post_public] url_path={}", &url);
        }

        let resp = self
            .http
            .post(&url)
            .headers(crate::client::ApiClient::anonymous_headers())
            .json(body)
            .send()
            .await
            .context("wallet API request failed")?;
        self.handle_response(resp).await
    }

    /// POST with Bearer accessToken (for create / list / refresh / x402).
    pub async fn post_authed(&self, path: &str, access_token: &str, body: &Value) -> Result<Value> {
        let url = format!("{}{}", self.base_url, path);

        if cfg!(feature = "debug-log") {
            eprintln!("[DEBUG][post_authed] url_path={}", &url);
        }

        let resp = self
            .http
            .post(&url)
            .headers(crate::client::ApiClient::jwt_headers(access_token))
            .json(body)
            .send()
            .await
            .context("wallet API request failed")?;
        self.handle_response(resp).await
    }

    async fn handle_response(&self, resp: reqwest::Response) -> Result<Value> {
        let status = resp.status();
        if status.as_u16() >= 500 {
            bail!("Wallet API server error (HTTP {})", status.as_u16());
        }

        let body: Value = resp
            .json()
            .await
            .context("failed to parse wallet API response")?;

        // Handle code as either string "0" or number 0
        let code_ok = match &body["code"] {
            Value::String(s) => s == "0",
            Value::Number(n) => n.as_i64() == Some(0),
            _ => false,
        };
        if !code_ok {
            let code_str = match &body["code"] {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                other => other.to_string(),
            };
            let msg = body["msg"].as_str().unwrap_or("unknown error").to_string();
            return Err(ApiCodeError {
                code: code_str,
                msg,
            }
            .into());
        }

        Ok(body["data"].clone())
    }

    /// GET with Bearer accessToken + query params.
    pub async fn get_authed(
        &self,
        path: &str,
        access_token: &str,
        query: &[(&str, &str)],
    ) -> Result<Value> {
        let query_string = build_query_string(query);
        let url = format!("{}{}{}", self.base_url, path, query_string);
        let resp = self
            .http
            .get(&url)
            .headers(crate::client::ApiClient::jwt_headers(access_token))
            .send()
            .await
            .context("wallet API request failed")?;
        self.handle_response(resp).await
    }

    // ── Public API methods ──────────────────────────────────────────

    /// POST /priapi/v5/wallet/agentic/auth/init
    pub async fn auth_init(&self, email: &str, locale: Option<&str>) -> Result<InitResponse> {
        let mut body = json!({ "email": email });
        if let Some(loc) = locale {
            body["locale"] = serde_json::Value::String(loc.to_string());
        }
        let data = self
            .post_public("/priapi/v5/wallet/agentic/auth/init", &body)
            .await?;
        // data is an array, take first element
        let arr = data
            .as_array()
            .context("auth/init: expected data to be an array")?;
        let item = arr.first().context("auth/init: data array is empty")?;
        let resp: InitResponse =
            serde_json::from_value(item.clone()).context("auth/init: failed to parse response")?;
        Ok(resp)
    }

    /// POST /priapi/v5/wallet/agentic/auth/verify
    pub async fn auth_verify(
        &self,
        email: &str,
        flow_id: &str,
        otp: &str,
        temp_pub_key: &str,
    ) -> Result<VerifyResponse> {
        let body = json!({
            "email": email,
            "flowId": flow_id,
            "otp": otp,
            "tempPubKey": temp_pub_key,
        });
        let data = self
            .post_public("/priapi/v5/wallet/agentic/auth/verify", &body)
            .await?;
        let arr = data
            .as_array()
            .context("auth/verify: expected data to be an array")?;
        let item = arr.first().context("auth/verify: data array is empty")?;
        let resp: VerifyResponse = serde_json::from_value(item.clone())
            .context("auth/verify: failed to parse response")?;
        Ok(resp)
    }

    /// POST /priapi/v5/wallet/agentic/auth/ak/init
    pub async fn ak_auth_init(&self, api_key: &str) -> Result<AkInitResponse> {
        let body = json!({ "apiKey": api_key });
        let data = self
            .post_public("/priapi/v5/wallet/agentic/auth/ak/init", &body)
            .await?;
        let arr = data
            .as_array()
            .context("ak/init: expected data to be an array")?;
        let item = arr.first().context("ak/init: data array is empty")?;
        let resp: AkInitResponse =
            serde_json::from_value(item.clone()).context("ak/init: failed to parse response")?;
        Ok(resp)
    }

    /// POST /priapi/v5/wallet/agentic/auth/ak/verify
    pub async fn ak_auth_verify(
        &self,
        temp_pub_key: &str,
        api_key: &str,
        passphrase: &str,
        timestamp: &str,
        sign: &str,
        locale: &str,
    ) -> Result<VerifyResponse> {
        let body = json!({
            "tempPubKey": temp_pub_key,
            "apiKey": api_key,
            "passphrase": passphrase,
            "timestamp": timestamp,
            "sign": sign,
            "locale": locale,
        });
        let data = self
            .post_public("/priapi/v5/wallet/agentic/auth/ak/verify", &body)
            .await?;
        let arr = data
            .as_array()
            .context("ak/verify: expected data to be an array")?;
        let item = arr.first().context("ak/verify: data array is empty")?;
        let resp: VerifyResponse =
            serde_json::from_value(item.clone()).context("ak/verify: failed to parse response")?;
        Ok(resp)
    }

    /// POST /priapi/v5/wallet/agentic/auth/refresh
    pub async fn auth_refresh(&self, refresh_token: &str) -> Result<RefreshResponse> {
        let body = json!({ "refreshToken": refresh_token });
        let data = self
            .post_public("/priapi/v5/wallet/agentic/auth/refresh", &body)
            .await?;
        let arr = data
            .as_array()
            .context("auth/refresh: expected data to be an array")?;
        let item = arr.first().context("auth/refresh: data array is empty")?;
        let resp: RefreshResponse = serde_json::from_value(item.clone())
            .context("auth/refresh: failed to parse response")?;
        Ok(resp)
    }

    /// POST /priapi/v5/wallet/agentic/account/create
    pub async fn account_create(
        &self,
        access_token: &str,
        project_id: &str,
    ) -> Result<CreateAccountResponse> {
        let body = json!({
            "projectId": project_id,
        });
        let data = self
            .post_authed(
                "/priapi/v5/wallet/agentic/account/create",
                access_token,
                &body,
            )
            .await?;
        let arr = data
            .as_array()
            .context("account/create: expected data to be an array")?;
        let item = arr.first().context("account/create: data array is empty")?;
        let resp: CreateAccountResponse = serde_json::from_value(item.clone())
            .context("account/create: failed to parse response")?;
        Ok(resp)
    }

    /// POST /priapi/v5/wallet/agentic/account/list
    pub async fn account_list(
        &self,
        access_token: &str,
        project_id: &str,
    ) -> Result<Vec<AccountListItem>> {
        let body = json!({ "projectId": project_id });
        let data = self
            .post_authed(
                "/priapi/v5/wallet/agentic/account/list",
                access_token,
                &body,
            )
            .await?;
        let arr = data
            .as_array()
            .context("account/list: expected data to be an array")?;
        let items: Vec<AccountListItem> = serde_json::from_value(Value::Array(arr.clone()))
            .context("account/list: failed to parse response")?;
        Ok(items)
    }

    /// POST /priapi/v5/wallet/agentic/account/address/list
    ///
    /// Batch-fetch address lists for multiple accounts.
    pub async fn account_address_list(
        &self,
        access_token: &str,
        account_ids: &[String],
    ) -> Result<Vec<AddressListAccountItem>> {
        let body = json!({ "accountIds": account_ids });
        let data = self
            .post_authed(
                "/priapi/v5/wallet/agentic/account/address/list",
                access_token,
                &body,
            )
            .await?;
        let arr = data
            .as_array()
            .context("account/address/list: expected data to be an array")?;
        let item = arr
            .first()
            .context("account/address/list: data array is empty")?;
        let resp: AddressListData = serde_json::from_value(item.clone())
            .context("account/address/list: failed to parse response")?;
        Ok(resp.accounts)
    }

    // ── Balance API methods ─────────────────────────────────────────

    /// GET /priapi/v5/wallet/agentic/asset/wallet-all-token-balances-batch
    ///
    /// Fetch balances for multiple accounts at once.
    pub async fn balance_batch(&self, access_token: &str, account_ids: &str) -> Result<Value> {
        self.get_authed(
            "/priapi/v5/wallet/agentic/asset/wallet-all-token-balances-batch",
            access_token,
            &[("accountIds", account_ids)],
        )
        .await
    }

    /// GET /priapi/v5/wallet/agentic/asset/wallet-all-token-balances
    ///
    /// Fetch balances for a single account with optional chain / token filters.
    pub async fn balance_single(
        &self,
        access_token: &str,
        query: &[(&str, &str)],
    ) -> Result<Value> {
        self.get_authed(
            "/priapi/v5/wallet/agentic/asset/wallet-all-token-balances",
            access_token,
            query,
        )
        .await
    }

    /// POST /priapi/v5/wallet/agentic/pre-transaction/unsignedInfo
    #[allow(clippy::too_many_arguments)]
    pub async fn pre_transaction_unsigned_info(
        &self,
        access_token: &str,
        chain_path: &str,
        chain_index: u64,
        from_addr: &str,
        to_addr: &str,
        value: &str,
        contract_addr: Option<&str>,
        session_cert: &str,
        input_data: Option<&str>,
        unsigned_tx: Option<&str>,
        gas_limit: Option<&str>,
        aa_dex_token_addr: Option<&str>,
        aa_dex_token_amount: Option<&str>,
        jito_unsigned_tx: Option<&str>,
    ) -> Result<UnsignedInfoResponse> {
        let mut body = json!({
            "chainPath": chain_path,
            "chainIndex": chain_index,
            "fromAddr": from_addr,
            "toAddr": to_addr,
            "value": value,
            "sessionCert": session_cert,
        });
        if let Some(ca) = contract_addr {
            body["contractAddr"] = Value::String(ca.to_string());
        }
        if let Some(data) = input_data {
            body["inputData"] = Value::String(data.to_string());
        }
        if let Some(tx) = unsigned_tx {
            body["unsignedTx"] = Value::String(tx.to_string());
        }
        if let Some(gl) = gas_limit {
            body["gasLimit"] = Value::String(gl.to_string());
        }
        if let Some(addr) = aa_dex_token_addr {
            body["aaDexTokenAddr"] = Value::String(addr.to_string());
        }
        if let Some(amount) = aa_dex_token_amount {
            body["aaDexTokenAmount"] = Value::String(amount.to_string());
        }
        if let Some(jito_tx) = jito_unsigned_tx {
            body["jitoUnsignedTx"] = Value::String(jito_tx.to_string());
        }
        let data = self
            .post_authed(
                "/priapi/v5/wallet/agentic/pre-transaction/unsignedInfo",
                access_token,
                &body,
            )
            .await?;
        let arr = data
            .as_array()
            .context("unsignedInfo: expected data to be an array")?;
        let item = arr.first().context("unsignedInfo: data array is empty")?;
        let resp: UnsignedInfoResponse = serde_json::from_value(item.clone())
            .context("unsignedInfo: failed to parse response")?;
        Ok(resp)
    }

    /// POST /priapi/v5/wallet/agentic/pre-transaction/broadcast-transaction
    pub async fn broadcast_transaction(
        &self,
        access_token: &str,
        account_id: &str,
        address: &str,
        chain_index: &str,
        extra_data: &str,
    ) -> Result<BroadcastResponse> {
        let body = json!({
            "accountId": account_id,
            "address": address,
            "chainIndex": chain_index,
            "extraData": extra_data,
        });
        let data = self
            .post_authed(
                "/priapi/v5/wallet/agentic/pre-transaction/broadcast-transaction",
                access_token,
                &body,
            )
            .await?;
        let arr = data
            .as_array()
            .context("broadcast: expected data to be an array")?;
        let item = arr.first().context("broadcast: data array is empty")?;
        let resp: BroadcastResponse =
            serde_json::from_value(item.clone()).context("broadcast: failed to parse response")?;
        Ok(resp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_init_response() {
        let json = r#"{"flowId": "abc-123"}"#;
        let resp: InitResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.flow_id, "abc-123");
    }

    #[test]
    fn parse_verify_response() {
        // New format: addressList items no longer contain accountId
        let json = r#"{
            "refreshToken": "rt",
            "accessToken": "at",
            "teeId": "tee1",
            "sessionCert": "cert",
            "encryptedSessionSk": "esk",
            "sessionKeyExpireAt": "2025-12-31",
            "projectId": "proj",
            "accountId": "acc",
            "accountName": "My Wallet",
            "isNew": true,
            "addressList": [
                {"chainIndex": "1", "address": "0xabc", "chainName": "ETH", "addressType": "eoa", "chainPath": "m/44/60"},
                {"chainIndex": "501", "address": "SoLaddr", "chainName": "SOL", "addressType": "eoa", "chainPath": "m/44/501"}
            ]
        }"#;
        let resp: VerifyResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.refresh_token, "rt");
        assert_eq!(resp.access_token, "at");
        assert_eq!(resp.tee_id, "tee1");
        assert_eq!(resp.session_cert, "cert");
        assert_eq!(resp.encrypted_session_sk, "esk");
        assert_eq!(resp.session_key_expire_at, "2025-12-31");
        assert_eq!(resp.project_id, "proj");
        assert_eq!(resp.account_id, "acc");
        assert_eq!(resp.account_name, "My Wallet");
        assert!(resp.is_new);
        assert_eq!(resp.address_list.len(), 2);
        assert_eq!(resp.address_list[0].chain_index, "1");
        assert_eq!(resp.address_list[0].account_id, ""); // no accountId in new format → default ""
        assert_eq!(resp.address_list[1].address, "SoLaddr");
    }

    #[test]
    fn parse_refresh_response() {
        let json = r#"{"refreshToken": "new_rt", "accessToken": "new_at"}"#;
        let resp: RefreshResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.refresh_token, "new_rt");
        assert_eq!(resp.access_token, "new_at");
    }

    #[test]
    fn parse_create_account_response() {
        let json = r#"{
            "projectId": "proj2",
            "accountId": "acc2",
            "accountName": "Wallet 2",
            "addressList": [{"chainIndex": "1", "address": "0xdef", "chainName": "ETH", "addressType": "eoa", "chainPath": "m/44/60"}]
        }"#;
        let resp: CreateAccountResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.project_id, "proj2");
        assert_eq!(resp.account_id, "acc2");
        assert_eq!(resp.account_name, "Wallet 2");
        assert_eq!(resp.address_list.len(), 1);
    }

    #[test]
    fn parse_account_list_item() {
        let json = r#"[
            {"projectId": "p1", "accountId": "a1", "accountName": "Default", "isDefault": true},
            {"projectId": "p1", "accountId": "a2", "accountName": "Second"}
        ]"#;
        let items: Vec<AccountListItem> = serde_json::from_str(json).unwrap();
        assert_eq!(items.len(), 2);
        assert!(items[0].is_default);
        assert!(!items[1].is_default); // #[serde(default)]
        assert_eq!(items[1].account_name, "Second");
    }

    #[test]
    fn parse_unsigned_info_response_evm() {
        let json = r#"{
            "unsignedTxHash": "0xabc123",
            "unsignedTx": "0xrawtx",
            "uopHash": "0xuop",
            "hash": "0xhash",
            "authHashFor7702": "0xauth",
            "executeErrorMsg": "",
            "executeResult": true,
            "extraData": {
                "to": "0xrecipient",
                "value": "0x0",
                "data": "0x",
                "chainId": "0x1",
                "nonce": "0x1",
                "gasLimit": "0x5208",
                "maxFeePerGas": "0x3b9aca00",
                "maxPriorityFeePerGas": "0x59682f00"
            },
            "signType": "eip1559Tx",
            "encoding": "hex"
        }"#;
        let resp: UnsignedInfoResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.unsigned_tx_hash, "0xabc123");
        assert_eq!(resp.unsigned_tx, "0xrawtx");
        assert_eq!(resp.uop_hash, "0xuop");
        assert_eq!(resp.hash, "0xhash");
        assert_eq!(resp.auth_hash_for7702, "0xauth");
        assert_eq!(resp.sign_type, "eip1559Tx");
        assert_eq!(resp.encoding, "hex");
        assert_eq!(resp.execute_result, Value::Bool(true));
        assert!(resp.unsign_hash.is_empty()); // EVM doesn't use this
                                              // Verify extraData is parsed as an object with expected fields
        assert!(resp.extra_data.is_object());
        assert_eq!(resp.extra_data["to"], "0xrecipient");
        assert_eq!(resp.extra_data["chainId"], "0x1");
        assert_eq!(resp.extra_data["gasLimit"], "0x5208");
    }

    #[test]
    fn parse_unsigned_info_response_solana() {
        let json = r#"{
            "unsignHash": "sol_hash",
            "unsignedTx": "sol_raw",
            "executeResult": true,
            "signType": "solTx",
            "encoding": "base58"
        }"#;
        let resp: UnsignedInfoResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.unsign_hash, "sol_hash");
        assert_eq!(resp.unsigned_tx, "sol_raw");
        assert_eq!(resp.sign_type, "solTx");
        assert!(resp.unsigned_tx_hash.is_empty()); // Solana uses unsignHash
    }

    #[test]
    fn parse_broadcast_response() {
        let json = r#"{
            "pkgId": "pkg-1",
            "orderId": "order-1",
            "orderType": "normal",
            "txHash": "0xtxhash123"
        }"#;
        let resp: BroadcastResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.pkg_id, "pkg-1");
        assert_eq!(resp.order_id, "order-1");
        assert_eq!(resp.order_type, "normal");
        assert_eq!(resp.tx_hash, "0xtxhash123");
    }

    #[test]
    fn parse_unsigned_info_from_numeric_code_envelope() {
        // unsignedInfo API returns numeric code 0, not string "0"
        let api_json = r#"{
            "code": 0,
            "msg": "",
            "data": [{
                "unsignedTxHash": "0xabc",
                "unsignedTx": "0xraw",
                "hash": "0xhash",
                "executeResult": true,
                "signType": "eip1559Tx",
                "encoding": "hex"
            }]
        }"#;
        let body: Value = serde_json::from_str(api_json).unwrap();
        // Verify numeric code handling
        let code_ok = match &body["code"] {
            Value::String(s) => s == "0",
            Value::Number(n) => n.as_i64() == Some(0),
            _ => false,
        };
        assert!(code_ok);
        let data = &body["data"];
        let arr = data.as_array().unwrap();
        let item = arr.first().unwrap();
        let resp: UnsignedInfoResponse = serde_json::from_value(item.clone()).unwrap();
        assert_eq!(resp.unsigned_tx_hash, "0xabc");
        assert_eq!(resp.sign_type, "eip1559Tx");
    }

    #[test]
    fn parse_verify_response_from_api_envelope() {
        // Simulate the full API response shape: { "code": "0", "data": [...] }
        let api_json = r#"{
            "code": "0",
            "msg": "success",
            "data": [{
                "refreshToken": "rt",
                "accessToken": "at",
                "apiKey": "ak",
                "passphrase": "pp",
                "secretKey": "sk",
                "teeId": "t",
                "sessionCert": "c",
                "encryptedSessionSk": "e",
                "sessionKeyExpireAt": "2025-12-31",
                "projectId": "p",
                "accountId": "a",
                "accountName": "W",
                "isNew": false,
                "addressList": []
            }]
        }"#;
        let body: Value = serde_json::from_str(api_json).unwrap();
        let data = &body["data"];
        let arr = data.as_array().unwrap();
        let item = arr.first().unwrap();
        let resp: VerifyResponse = serde_json::from_value(item.clone()).unwrap();
        assert_eq!(resp.project_id, "p");
        assert!(!resp.is_new);
        assert!(resp.address_list.is_empty());
    }

    #[test]
    fn parse_verify_address_info_with_null_fields() {
        // New format: no accountId in addressList items, chainPath can be null
        let json = r#"{
            "address": "0xabc",
            "chainIndex": 196,
            "chainName": "okb",
            "addressType": "aa",
            "chainPath": null
        }"#;
        let info: VerifyAddressInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.account_id, ""); // not present → default ""
        assert_eq!(info.address, "0xabc");
        assert_eq!(info.chain_index, "196");
        assert_eq!(info.chain_name, "okb");
        assert!(info.chain_path.is_empty()); // null → ""
    }

    #[test]
    fn parse_verify_response_with_number_and_bool_fields() {
        // sessionKeyExpireAt comes as Number, isNew may come as 0/1 or bool
        let json = r#"{
            "refreshToken": "rt",
            "accessToken": "at",
            "apiKey": "ak",
            "passphrase": "pp",
            "secretKey": "sk",
            "teeId": "t",
            "sessionCert": "c",
            "encryptedSessionSk": "e",
            "sessionKeyExpireAt": 1781959290,
            "projectId": "p",
            "accountId": "a",
            "accountName": "W",
            "isNew": 1,
            "addressList": []
        }"#;
        let resp: VerifyResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.session_key_expire_at, "1781959290"); // Number → String
        assert!(resp.is_new); // 1 → true
    }

    #[test]
    fn parse_address_list_data() {
        // API returns accounts as an array; chainIndex is a number.
        let json = r#"{
            "accountCnt": 2,
            "validAccountCnt": 2,
            "addressCnt": 3,
            "accounts": [
                {
                    "accountId": "acc-1",
                    "addresses": [
                        {"address": "0xabc", "chainIndex": 1, "chainName": "ETH", "addressType": "EOA", "chainPath": "m/44/60"},
                        {"address": "SoLaddr", "chainIndex": 501, "chainName": "SOL", "addressType": "EOA", "chainPath": "m/44/501"}
                    ]
                },
                {
                    "accountId": "acc-2",
                    "addresses": [
                        {"address": "0xdef", "chainIndex": 1, "chainName": "ETH", "addressType": "EOA", "chainPath": "m/44/60"}
                    ]
                }
            ]
        }"#;
        let data: AddressListData = serde_json::from_str(json).unwrap();
        assert_eq!(data.accounts.len(), 2);
        assert_eq!(data.accounts[0].account_id, "acc-1");
        assert_eq!(data.accounts[0].addresses.len(), 2);
        assert_eq!(data.accounts[0].addresses[0].chain_name, "ETH");
        assert_eq!(data.accounts[0].addresses[0].chain_index, "1"); // number → string
        assert_eq!(data.accounts[0].addresses[1].chain_name, "SOL");
        assert_eq!(data.accounts[1].account_id, "acc-2");
        assert_eq!(data.accounts[1].addresses.len(), 1);
    }

    #[test]
    fn parse_address_list_data_empty_accounts() {
        let json = r#"{"accounts": []}"#;
        let data: AddressListData = serde_json::from_str(json).unwrap();
        assert!(data.accounts.is_empty());
    }

    #[test]
    fn parse_address_list_data_missing_accounts() {
        // accounts field missing entirely → default to empty vec
        let json = r#"{"accountCnt": 0}"#;
        let data: AddressListData = serde_json::from_str(json).unwrap();
        assert!(data.accounts.is_empty());
    }

    #[test]
    fn parse_address_list_data_from_api_envelope() {
        // Full API response: data is an array with a single element (like all other endpoints).
        let json = r#"{
            "code": 0,
            "msg": "success",
            "data": [{
                "accountCnt": 2,
                "validAccountCnt": 2,
                "addressCnt": 3,
                "accounts": [
                    {
                        "accountId": "acc-001",
                        "addresses": [
                            {"address": "0xabc", "chainIndex": 1, "chainName": "ETH", "addressType": "EOA", "chainPath": "m/44/60"}
                        ]
                    },
                    {
                        "accountId": "acc-002",
                        "addresses": [
                            {"address": "0xdef", "chainIndex": 56, "chainName": "BSC", "addressType": "EOA", "chainPath": "m/44/56"},
                            {"address": "SoLaddr", "chainIndex": 501, "chainName": "SOL", "addressType": "EOA", "chainPath": "m/44/501"}
                        ]
                    }
                ]
            }]
        }"#;
        let body: Value = serde_json::from_str(json).unwrap();
        let data_val = &body["data"];
        let arr = data_val.as_array().unwrap();
        let item = arr.first().unwrap();
        let resp: AddressListData = serde_json::from_value(item.clone()).unwrap();
        assert_eq!(resp.accounts.len(), 2);
        assert_eq!(resp.accounts[0].account_id, "acc-001");
        assert_eq!(resp.accounts[0].addresses.len(), 1);
        assert_eq!(resp.accounts[0].addresses[0].chain_index, "1");
        assert_eq!(resp.accounts[1].account_id, "acc-002");
        assert_eq!(resp.accounts[1].addresses.len(), 2);
        assert_eq!(resp.accounts[1].addresses[0].chain_index, "56");
        assert_eq!(resp.accounts[1].addresses[1].chain_index, "501");
    }
}
