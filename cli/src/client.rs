use anyhow::{bail, Context, Result};
use base64::Engine;
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde_json::Value;
use sha2::Sha256;

const DEFAULT_BASE_URL: &str = "https://web3.okx.com";
const DEFAULT_API_KEY: &str = "03f0b376-251c-4618-862e-ae92929e0416";
const DEFAULT_SECRET_KEY: &str = "652ECE8FF13210065B0851FFDA9191F7";
const DEFAULT_PASSPHRASE: &str = "onchainOS#666";

pub struct ApiClient {
    http: Client,
    base_url: String,
    api_key: String,
    secret_key: String,
    passphrase: String,
}

impl ApiClient {
    pub fn new(base_url_override: Option<&str>) -> Result<Self> {
        let api_key = std::env::var("OKX_API_KEY")
            .or_else(|_| std::env::var("OKX_ACCESS_KEY"))
            .unwrap_or_else(|_| DEFAULT_API_KEY.to_string());
        let secret_key =
            std::env::var("OKX_SECRET_KEY").unwrap_or_else(|_| DEFAULT_SECRET_KEY.to_string());
        let passphrase =
            std::env::var("OKX_PASSPHRASE").unwrap_or_else(|_| DEFAULT_PASSPHRASE.to_string());

        let base_url = base_url_override
            .map(|s| s.to_string())
            .or_else(|| std::env::var("OKX_BASE_URL").ok())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

        Ok(Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()?,
            base_url,
            api_key,
            secret_key,
            passphrase,
        })
    }

    fn sign(&self, timestamp: &str, method: &str, request_path: &str, body: &str) -> String {
        let prehash = format!("{}{}{}{}", timestamp, method, request_path, body);
        let mut mac = Hmac::<Sha256>::new_from_slice(self.secret_key.as_bytes())
            .expect("HMAC accepts any key length");
        mac.update(prehash.as_bytes());
        base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
    }

    fn apply_auth(
        &self,
        builder: reqwest::RequestBuilder,
        timestamp: &str,
        sign: &str,
    ) -> reqwest::RequestBuilder {
        builder
            .header("OK-ACCESS-KEY", &self.api_key)
            .header("OK-ACCESS-SIGN", sign)
            .header("OK-ACCESS-PASSPHRASE", &self.passphrase)
            .header("OK-ACCESS-TIMESTAMP", timestamp)
            .header("Content-Type", "application/json")
            .header("ok-client-type", "cli")
    }

    fn build_get_url_and_request_path(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<(reqwest::Url, String)> {
        let filtered: Vec<(&str, &str)> = query
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .copied()
            .collect();

        let mut url =
            reqwest::Url::parse(&format!("{}{}", self.base_url.trim_end_matches('/'), path))?;

        if !filtered.is_empty() {
            url.query_pairs_mut().extend_pairs(filtered.iter().copied());
        }

        let query_string = url
            .query()
            .map(|query| format!("?{}", query))
            .unwrap_or_default();
        let request_path = format!("{}{}", path, query_string);

        Ok((url, request_path))
    }

    /// GET request. `path` should be the API path without query string (e.g. "/api/v6/dex/market/candles").
    /// Query params are appended and included in the signature.
    pub async fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Value> {
        let (url, request_path) = self.build_get_url_and_request_path(path, query)?;
        let timestamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let sign = self.sign(&timestamp, "GET", &request_path, "");
        let req = self.http.get(url);
        let req = self.apply_auth(req, &timestamp, &sign);

        let resp = req.send().await.context("request failed")?;
        self.handle_response(resp).await
    }

    /// POST request. `path` is the API path (no query string). `body` is the JSON body.
    /// For POST, signature uses path only (no query string) + JSON body string.
    pub async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        let body_str = serde_json::to_string(body)?;
        let timestamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let sign = self.sign(&timestamp, "POST", path, &body_str);

        let url = format!("{}{}", self.base_url.trim_end_matches('/'), path);
        let req = self.http.post(&url).body(body_str);
        let req = self.apply_auth(req, &timestamp, &sign);

        let resp = req.send().await.context("request failed")?;
        self.handle_response(resp).await
    }

    async fn handle_response(&self, resp: reqwest::Response) -> Result<Value> {
        let status = resp.status();
        if status.as_u16() == 429 {
            bail!("Rate limited — retry with backoff");
        }
        if status.as_u16() >= 500 {
            bail!("Server error (HTTP {})", status.as_u16());
        }

        let body: Value = resp.json().await.context("failed to parse response")?;

        let code = body["code"].as_str().unwrap_or("-1");
        if code != "0" {
            let msg = body["msg"].as_str().unwrap_or("unknown error");
            bail!("API error (code={}): {}", code, msg);
        }

        Ok(body["data"].clone())
    }
}

#[cfg(test)]
mod tests {
    use super::ApiClient;

    #[test]
    fn build_get_request_path_percent_encodes_query_values() {
        let client = ApiClient::new(None).expect("client");
        let (_, request_path) = client
            .build_get_url_and_request_path(
                "/api/v6/dex/market/memepump/tokenList",
                &[
                    ("chainIndex", "501"),
                    ("keywordsInclude", "dog wif"),
                    ("keywordsExclude", "狗"),
                    ("empty", ""),
                ],
            )
            .expect("request path");

        assert_eq!(
            request_path,
            "/api/v6/dex/market/memepump/tokenList?chainIndex=501&keywordsInclude=dog+wif&keywordsExclude=%E7%8B%97"
        );
    }
}
