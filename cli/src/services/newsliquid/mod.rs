//! Wallet-scoped service credentials. Business requests run outside the CLI.
use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::commands::agentic_wallet::account::resolve_account_address_for_chain;
use crate::commands::payment::addr::parse_recipient_addr;
use crate::{keyring_store, wallet_store};

mod login;
mod payment_check;
pub(crate) use login::login;
pub(crate) use payment_check::validate_payment;

pub(crate) const ID: &str = "newsliquid";
const BASE: &str = "https://x402-okx.newsliquid.com";

#[derive(Deserialize, Serialize)]
struct Login {
    wallet: String,
    token: String,
    #[serde(rename = "expiresAt")]
    expires_at: Value,
}

fn current_wallet() -> Result<String> {
    let wallets = wallet_store::load_wallets()?.ok_or_else(|| anyhow!("wallet_login_required"))?;
    let address = resolve_account_address_for_chain(&wallets, "196")?;
    Ok(parse_recipient_addr(&address, 196)?.0)
}

fn check_wallet(actual: &str, expected: &str) -> Result<()> {
    if !actual.eq_ignore_ascii_case(expected) {
        bail!("newsliquid_wallet_changed");
    }
    Ok(())
}

fn credential_key(wallet: &str) -> Result<String> {
    let address = parse_recipient_addr(wallet, 196)?.0;
    Ok(format!("newsliquid:{}", address.to_ascii_lowercase()))
}

fn expiry(value: &Value) -> Result<i64> {
    if let Some(seconds) = value
        .as_i64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
    {
        return Ok(if seconds > 100_000_000_000 {
            seconds / 1000
        } else {
            seconds
        });
    }
    chrono::DateTime::parse_from_rfc3339(
        value
            .as_str()
            .ok_or_else(|| anyhow!("invalid_newsliquid_expiry"))?,
    )
    .map(|date| date.timestamp())
    .map_err(|_| anyhow!("invalid_newsliquid_expiry"))
}

fn load_login(wallet: &str) -> Result<Option<Login>> {
    let map = keyring_store::read_blob()?;
    let Some(value) = map.get(&credential_key(wallet)?) else {
        return Ok(None);
    };
    let login: Login =
        serde_json::from_str(value).map_err(|_| anyhow!("newsliquid_credentials_corrupted"))?;
    check_wallet(&login.wallet, wallet)?;
    Ok(Some(login))
}

fn load_valid_login(wallet: &str) -> Result<Option<Login>> {
    Ok(load_login(wallet)?.filter(|login| {
        !login.token.is_empty()
            && expiry(&login.expires_at).unwrap_or(0) > chrono::Utc::now().timestamp()
    }))
}

fn store_login(wallet: &str, login: Login) -> Result<Value> {
    check_wallet(&login.wallet, wallet)?;
    if login.token.is_empty() || expiry(&login.expires_at)? <= chrono::Utc::now().timestamp() {
        bail!("newsliquid_login_invalid_or_expired");
    }
    let metadata = json!({"wallet":wallet,"expiresAt":login.expires_at});
    let value = serde_json::to_string(&login).map_err(|_| anyhow!("newsliquid_store_failed"))?;
    keyring_store::set(&credential_key(wallet)?, &value)?;
    Ok(metadata)
}

pub(crate) fn token(expected_wallet: Option<&str>) -> Result<Value> {
    let wallet = current_wallet()?;
    if let Some(expected) = expected_wallet {
        check_wallet(&wallet, expected)?;
    }
    let login = load_valid_login(&wallet)?.ok_or_else(|| anyhow!("newsliquid_login_required"))?;
    Ok(json!({"wallet":wallet,"token":login.token,"expiresAt":login.expires_at}))
}

pub(crate) fn require_service(service: &str) -> Result<()> {
    if [ID, "News Liquid", "6551", "6551Team"]
        .iter()
        .any(|name| name.eq_ignore_ascii_case(service.trim()))
    {
        return Ok(());
    }
    bail!("service_not_registered");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_keys_are_wallet_scoped_and_expiry_formats_are_supported() {
        let a = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        assert_eq!(
            credential_key(a).unwrap(),
            credential_key(&a.to_uppercase().replacen("0X", "0x", 1)).unwrap()
        );
        assert_ne!(
            credential_key(a).unwrap(),
            credential_key("0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap()
        );
        assert!(credential_key("../other").is_err());
        assert_eq!(expiry(&json!(1_800_000_000)).unwrap(), 1_800_000_000);
        assert_eq!(expiry(&json!("1800000000000")).unwrap(), 1_800_000_000);
        assert_eq!(
            expiry(&json!("2027-01-15T08:00:00Z")).unwrap(),
            1_800_000_000
        );
        assert!(expiry(&json!("invalid")).is_err());
    }

    #[test]
    fn token_reads_only_the_selected_wallet_and_rejects_invalid_credentials() {
        let _guard = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let root = tempfile::tempdir_in(concat!(env!("CARGO_MANIFEST_DIR"), "/target")).unwrap();
        let old_home = std::env::var_os("ONCHAINOS_HOME");
        let old_store = std::env::var_os("ONCHAINOS_CREDENTIAL_STORE");
        std::env::set_var("ONCHAINOS_HOME", root.path());
        std::env::set_var("ONCHAINOS_CREDENTIAL_STORE", "file");
        let a = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let b = "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let mut wallets: wallet_store::WalletsJson = serde_json::from_value(json!({
            "selectedAccountId":"a",
            "accountsMap":{
                "a":{"addressList":[{"address":a,"chainIndex":"196"}]},
                "b":{"addressList":[{"address":b,"chainIndex":"196"}]}
            }
        }))
        .unwrap();
        wallet_store::save_wallets(&wallets).unwrap();
        assert_eq!(
            token(None).unwrap_err().to_string(),
            "newsliquid_login_required"
        );
        for (wallet, value) in [(a, "service-a-fixture"), (b, "service-b-fixture")] {
            let metadata = store_login(
                wallet,
                Login {
                    wallet: wallet.into(),
                    token: value.into(),
                    expires_at: json!(chrono::Utc::now().timestamp() + 300),
                },
            )
            .unwrap();
            assert!(metadata.get("token").is_none());
        }
        assert_eq!(token(Some(a)).unwrap()["token"], "service-a-fixture");
        assert_eq!(
            token(Some(b)).unwrap_err().to_string(),
            "newsliquid_wallet_changed"
        );
        wallets.selected_account_id = "b".into();
        wallet_store::save_wallets(&wallets).unwrap();
        assert_eq!(token(None).unwrap()["token"], "service-b-fixture");
        for expires_at in [json!(0), json!("invalid")] {
            let stored = serde_json::to_string(&Login {
                wallet: b.into(),
                token: "expired-fixture".into(),
                expires_at,
            })
            .unwrap();
            keyring_store::set(&credential_key(b).unwrap(), &stored).unwrap();
            assert_eq!(
                token(None).unwrap_err().to_string(),
                "newsliquid_login_required"
            );
        }
        let encrypted = std::fs::read(root.path().join("keyring.enc")).unwrap();
        assert!(!String::from_utf8_lossy(&encrypted).contains("service-a-fixture"));
        assert_eq!(load_login(a).unwrap().unwrap().token, "service-a-fixture");
        for (key, previous) in [
            ("ONCHAINOS_HOME", old_home),
            ("ONCHAINOS_CREDENTIAL_STORE", old_store),
        ] {
            match previous {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}
