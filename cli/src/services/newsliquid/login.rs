//! Wallet challenge login; the returned API token is stored before output.
use super::{check_wallet, current_wallet, expiry, load_valid_login, store_login, Login, BASE};
use crate::{commands::sink::CodedError, output};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::process::Command as Process;

struct AuthInput {
    path: &'static str,
    body: Value,
}

trait Io {
    async fn http(&mut self, input: AuthInput) -> Result<Value>;
    async fn child(&mut self, args: Vec<String>) -> Result<Value>;
    fn wallet(&self) -> Result<String>;
}
struct Live;
impl Io for Live {
    async fn http(&mut self, input: AuthInput) -> Result<Value> {
        auth_http(input).await
    }
    async fn child(&mut self, args: Vec<String>) -> Result<Value> {
        let output = tokio::time::timeout(
            Duration::from_secs(90),
            Process::new(std::env::current_exe()?)
                .args(&args)
                .kill_on_drop(true)
                .output(),
        )
        .await
        .map_err(|_| anyhow!("newsliquid_signer_outcome_unknown"))?
        .map_err(|_| anyhow!("newsliquid_signer_unavailable"))?;
        let body: Value = serde_json::from_slice(&output.stdout)
            .map_err(|_| anyhow!("newsliquid_signer_invalid_response"))?;
        child_result(output.status.code(), body)
    }
    fn wallet(&self) -> Result<String> {
        current_wallet()
    }
}
fn service_success(result: &Value) -> bool {
    result["ok"] == true
        && result["data"]["success"] != false
        && result["data"].get("error").is_none_or(Value::is_null)
}
pub(crate) async fn login(
    refresh: bool,
    warning_confirmed: bool,
    wallet: Option<&str>,
) -> Result<Value> {
    login_with(&mut Live, refresh, warning_confirmed, wallet).await
}

async fn auth_http(input: AuthInput) -> Result<Value> {
    if !matches!(input.path, "/auth/challenge" | "/auth/login") {
        bail!("newsliquid_auth_endpoint_not_allowed");
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| anyhow!("newsliquid_login_transport_failed"))?;
    let mut response = client
        .post(format!("{BASE}{}", input.path))
        .json(&input.body)
        .send()
        .await
        .map_err(|_| anyhow!("newsliquid_login_transport_failed"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(
            CodedError::new("newsliquid_login_rejected", None, "Service login rejected.")
                .with_data(json!({"httpStatus":status.as_u16()}))
                .into(),
        );
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow!("newsliquid_login_response_incomplete"))?
    {
        if bytes.len() + chunk.len() > 8 * 1024 * 1024 {
            bail!("newsliquid_login_response_too_large");
        }
        bytes.extend_from_slice(&chunk);
    }
    let body: Value =
        serde_json::from_slice(&bytes).map_err(|_| anyhow!("newsliquid_login_response_invalid"))?;
    Ok(json!({"ok":true,"data":body}))
}

fn child_result(code: Option<i32>, mut body: Value) -> Result<Value> {
    if code == Some(2) && body["confirming"] == true {
        let scene = body["scene"].as_str();
        if scene.is_some() {
            return Err(CodedError::new("approval_verification_unsupported",None,"This service login cannot continue this wallet verification scene. Do not use --force to bypass it.").with_data(json!({"scene":scene,"next":body["next"]})).into());
        }
        return Err(output::CliConfirming {
            message: body["message"]
                .as_str()
                .unwrap_or("Wallet confirmation required")
                .into(),
            next: body["next"].as_str().unwrap_or("").into(),
            scene: None,
        }
        .into());
    }
    if !matches!(code, Some(0)) || body["ok"] != true {
        if body["errorCode"] == "approval_verification_unsupported" {
            return Err(CodedError::new("approval_verification_unsupported",None,"The wallet login signer requires a verification step this login flow does not support.").into());
        }
        bail!("newsliquid_signer_failed; inspect the original operation and wallet state");
    }
    Ok(body["data"].take())
}
fn data(result: &Value) -> Result<Value> {
    if !service_success(result) {
        return Err(CodedError::new(
            "newsliquid_service_rejected",
            None,
            "Service operation rejected; inspect the original response.",
        )
        .into());
    }
    Ok(result["data"]
        .get("data")
        .unwrap_or(&result["data"])
        .clone())
}
fn warning(error: &anyhow::Error) -> bool {
    error.downcast_ref::<output::CliConfirming>().is_some()
}
fn confirming(mut error: anyhow::Error, next: String) -> anyhow::Error {
    if let Some(w) = error.downcast_mut::<output::CliConfirming>() {
        w.next = next;
    }
    error
}
fn checked_wallet<I: Io>(io: &I, wallet: &str) -> Result<()> {
    check_wallet(&io.wallet()?, wallet)
}

fn signature(signed: &Value) -> Result<&str> {
    signed["signature"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("wallet_signature_missing"))
}

async fn login_with<I: Io>(
    io: &mut I,
    refresh: bool,
    warning_confirmed: bool,
    expected_wallet: Option<&str>,
) -> Result<Value> {
    let wallet = io.wallet()?;
    if expected_wallet.is_some() || warning_confirmed {
        checked_wallet(
            io,
            expected_wallet.ok_or_else(|| anyhow!("wallet_warning_requires_original_wallet"))?,
        )?;
    }
    if !refresh {
        if let Some(login) = load_valid_login(&wallet)? {
            return Ok(json!({"wallet":wallet,"expiresAt":login.expires_at,"reused":true}));
        }
    }
    let challenge = data(
        &io.http(AuthInput {
            path: "/auth/challenge",
            body: json!({"wallet":wallet,"chainId":196}),
        })
        .await?,
    )?;
    let challenge_id = challenge["challengeId"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("invalid_login_challenge"))?;
    let message = challenge["message"]
        .as_str()
        .ok_or_else(|| anyhow!("invalid_login_challenge"))?;
    if !message.contains("x402-okx.newsliquid.com")
        || !message.to_lowercase().contains(&wallet.to_lowercase())
        || expiry(&challenge["expiresAt"])? <= chrono::Utc::now().timestamp()
    {
        bail!("challenge_identity_or_expiry_mismatch");
    }
    checked_wallet(io, &wallet)?;
    let mut args = vec![
        "wallet",
        "sign-message",
        "--type",
        "personal",
        "--chain",
        "xlayer",
        "--from",
        &wallet,
        "--message",
        message,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    if warning_confirmed {
        args.push("--force".into());
    }
    let signed = match io.child(args).await {
        Ok(v) => v,
        Err(e) if warning(&e) => return Err(confirming(e, format!("After user confirmation, run onchainos service login --service newsliquid --wallet {wallet} --confirm-wallet-warning{}", if refresh { " --refresh" } else { "" }))),
        Err(e) => return Err(e),
    };
    checked_wallet(io, &wallet)?;
    let body = data(
        &io.http(AuthInput {
            path: "/auth/login",
            body: json!({"challengeId":challenge_id,"signature":signature(&signed)?}),
        })
        .await?,
    )?;
    let login: Login =
        serde_json::from_value(body).map_err(|_| anyhow!("newsliquid_login_response_invalid"))?;
    checked_wallet(io, &wallet)?;
    let metadata = store_login(&wallet, login)?;
    Ok(json!({"wallet":wallet,"expiresAt":metadata["expiresAt"],"reused":false}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    const WALLET: &str = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const MERCHANT: &str = "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    struct Fake {
        responses: VecDeque<Result<Value>>,
        signed: VecDeque<Result<Value>>,
        requests: Vec<AuthInput>,
        children: Vec<Vec<String>>,
        wallet: String,
    }
    impl Fake {
        fn new() -> Self {
            Self {
                responses: VecDeque::new(),
                signed: VecDeque::new(),
                requests: vec![],
                children: vec![],
                wallet: WALLET.into(),
            }
        }
    }
    impl Io for Fake {
        async fn http(&mut self, input: AuthInput) -> Result<Value> {
            self.requests.push(input);
            self.responses.pop_front().expect("unexpected HTTP")
        }
        async fn child(&mut self, args: Vec<String>) -> Result<Value> {
            self.children.push(args);
            self.signed.pop_front().expect("unexpected signer")
        }
        fn wallet(&self) -> Result<String> {
            Ok(self.wallet.clone())
        }
    }
    fn response(data: Value) -> Value {
        json!({"ok":true,"data":{"success":true,"data":data}})
    }
    struct Environment {
        root: std::path::PathBuf,
        old: Option<std::ffi::OsString>,
        old_store: Option<std::ffi::OsString>,
        _guard: std::sync::MutexGuard<'static, ()>,
    }
    impl Environment {
        fn new(name: &str) -> Self {
            let guard = crate::home::TEST_ENV_MUTEX
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/test_tmp")
                .join(format!("native-flow-{name}"));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            let old = std::env::var_os("ONCHAINOS_HOME");
            let old_store = std::env::var_os("ONCHAINOS_CREDENTIAL_STORE");
            std::env::set_var("ONCHAINOS_HOME", &root);
            std::env::set_var("ONCHAINOS_CREDENTIAL_STORE", "file");
            Self {
                root,
                old,
                old_store,
                _guard: guard,
            }
        }
    }
    impl Drop for Environment {
        fn drop(&mut self) {
            if let Some(v) = self.old.take() {
                std::env::set_var("ONCHAINOS_HOME", v)
            } else {
                std::env::remove_var("ONCHAINOS_HOME")
            };
            if let Some(v) = self.old_store.take() {
                std::env::set_var("ONCHAINOS_CREDENTIAL_STORE", v)
            } else {
                std::env::remove_var("ONCHAINOS_CREDENTIAL_STORE")
            };
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[tokio::test]
    async fn refresh_replaces_a_rejected_cached_session_only_after_success() {
        let _env = Environment::new("login-refresh");
        store_login(
            WALLET,
            Login {
                wallet: WALLET.into(),
                token: "old-login-fixture".into(),
                expires_at: json!(chrono::Utc::now().timestamp() + 300),
            },
        )
        .unwrap();
        let mut io = Fake::new();
        assert_eq!(
            login_with(&mut io, false, false, None).await.unwrap()["reused"],
            true
        );
        assert!(io.requests.is_empty());
        let challenge = json!({"challengeId":"refresh-challenge","message":format!("Sign in x402-okx.newsliquid.com for {WALLET}"),"expiresAt":chrono::Utc::now().timestamp()+300});
        io.responses.push_back(Ok(response(challenge.clone())));
        io.signed.push_back(Err(output::CliConfirming {
            message: "Wallet warning".into(),
            next: "confirm".into(),
            scene: None,
        }
        .into()));
        let error = login_with(&mut io, true, false, None).await.unwrap_err();
        assert!(error
            .downcast_ref::<output::CliConfirming>()
            .unwrap()
            .next
            .contains("--refresh"));
        assert_eq!(
            load_valid_login(WALLET).unwrap().unwrap().token,
            "old-login-fixture"
        );
        io.responses.push_back(Ok(response(challenge.clone())));
        io.signed
            .push_back(Ok(json!({"signature":"login-proof-fixture"})));
        io.responses.push_back(Ok(response(json!({"wallet":MERCHANT,"token":"wrong-wallet-fixture","expiresAt":chrono::Utc::now().timestamp()+300}))));
        assert!(login_with(&mut io, true, true, Some(WALLET)).await.is_err());
        assert_eq!(
            load_valid_login(WALLET).unwrap().unwrap().token,
            "old-login-fixture"
        );
        io.responses.push_back(Ok(response(challenge)));
        io.signed
            .push_back(Ok(json!({"signature":"login-proof-fixture"})));
        io.responses.push_back(Ok(response(json!({"wallet":WALLET,"token":"new-login-fixture","expiresAt":chrono::Utc::now().timestamp()+300}))));
        let result = login_with(&mut io, true, true, Some(WALLET)).await.unwrap();
        assert_eq!(result["reused"], false);
        assert!(result.get("token").is_none());
        assert_eq!(
            load_valid_login(WALLET).unwrap().unwrap().token,
            "new-login-fixture"
        );
    }

    #[tokio::test]
    async fn login_transport_cannot_proxy_business_requests() {
        for path in [
            "/news_search",
            "/billing/orders",
            "https://other.invalid/auth/login",
        ] {
            assert_eq!(
                auth_http(AuthInput {
                    path,
                    body: json!({})
                })
                .await
                .unwrap_err()
                .to_string(),
                "newsliquid_auth_endpoint_not_allowed"
            );
        }
        let error = data(&json!({"ok":false,"data":{"token":"response-private-fixture","error":"response-private-fixture"}})).unwrap_err();
        assert!(!format!("{error:#}").contains("response-private-fixture"));
    }

    #[tokio::test]
    async fn login_warning_rechecks_wallet_and_fresh_challenge_without_file() {
        let env = Environment::new("login-stateless");
        let mut io = Fake::new();
        let c = json!({"challengeId":"challenge-1","message":format!("Sign in x402-okx.newsliquid.com for {WALLET}; no payment authority"),"expiresAt":chrono::Utc::now().timestamp()+300});
        io.responses.push_back(Ok(response(c.clone())));
        io.signed.push_back(Err(output::CliConfirming {
            message: "Wallet warning".into(),
            next: "confirm".into(),
            scene: None,
        }
        .into()));
        let error = login_with(&mut io, false, false, None).await.unwrap_err();
        assert!(warning(&error));
        assert!(error
            .downcast_ref::<output::CliConfirming>()
            .unwrap()
            .next
            .contains(WALLET));
        io.wallet = MERCHANT.into();
        assert!(login_with(&mut io, false, true, Some(WALLET))
            .await
            .is_err());
        io.wallet = WALLET.into();
        let mut fresh = c;
        fresh["challengeId"] = json!("challenge-2");
        io.responses.push_back(Ok(response(fresh)));
        io.signed
            .push_back(Ok(json!({"signature":"login-proof-fixture"})));
        io.responses.push_back(Ok(response(
            json!({"wallet":WALLET,"token":"login-token-fixture","expiresAt":chrono::Utc::now().timestamp()+300}),
        )));
        login_with(&mut io, false, true, Some(WALLET))
            .await
            .unwrap();
        assert_eq!(
            io.requests.last().unwrap().body["challengeId"],
            "challenge-2"
        );
        assert!(io.children.last().unwrap().contains(&"--force".into()));
        assert!(!env.root.join("newsliquid").exists());
    }

    #[tokio::test]
    async fn invalid_challenge_and_wallet_mismatch_do_not_sign() {
        let _env = Environment::new("login-invalid");
        for message in [
            "Sign for another host",
            "x402-okx.newsliquid.com but another wallet",
        ] {
            let mut io = Fake::new();
            io.responses.push_back(Ok(response(
                json!({"message":message,"expiresAt":chrono::Utc::now().timestamp()+300}),
            )));
            assert!(login_with(&mut io, false, false, None).await.is_err());
            assert!(io.children.is_empty());
            assert_eq!(io.requests.len(), 1);
        }
        let mut io = Fake::new();
        assert!(login_with(&mut io, false, false, Some(MERCHANT))
            .await
            .is_err());
        assert!(io.requests.is_empty());
    }
}
