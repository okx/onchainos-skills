//! OKX.AI trading hackathon — Trading ASP registration.
//!
//! Public API surface: `POST /priapi/v5/wallet/agentic/activity/registration`
//! (authenticated; requires wallet login).

use crate::commands::Context;
use crate::client::ApiClient;
use crate::output;
use crate::token_alias;
use crate::wallet_store;
use anyhow::{bail, Result};
use clap::Subcommand;
use serde_json::Value;

const PROJECT_HEADER: &str = "4d156bf0c61130f2692d097ecb68dbe4";

/// Fixed activity ID for the current OKX.AI trading hackathon — not user-configurable.
const ACTIVITY_ID: &str = "okx-marathon-0730";
/// Fixed chain index (X Layer) this hackathon registers on — not user-configurable.
const CHAIN_INDEX: &str = "196";

#[derive(Subcommand)]
pub enum HackathonCommand {
    /// Register the user's Trading ASP for the OKX.AI trading hackathon (requires wallet login).
    /// Always registers on X Layer — the top-level --chain flag is ignored.
    Register {
        /// Agent ID of the user's Trading ASP to register.
        #[arg(long)]
        agent_id: String,
        /// Account type: "web3" or "cefi".
        #[arg(long, value_parser = clap::builder::PossibleValuesParser::new(["web3", "cefi"]))]
        account_type: String,
        /// Wallet address; auto-resolved from the wallet's X Layer address when omitted (both web3 & cefi).
        #[arg(long)]
        address: Option<String>,
        /// CeFi user ID (required when --account-type=cefi).
        #[arg(long)]
        uid: Option<String>,
    },
}

pub async fn execute(_ctx: &Context, command: HackathonCommand) -> Result<()> {
    let result: Value = match command {
        HackathonCommand::Register {
            agent_id,
            account_type,
            address,
            uid,
        } => {
            // Pre-request validation (spec §1), mirroring how `join` resolves
            // addresses at the caller layer so CLI + MCP can share `register()`.
            // ② CeFi registration requires a --uid.
            require_uid_for_cefi(&account_type, uid.as_deref())?;
            // ① Auto-resolve the wallet's X Layer (EVM) address when omitted
            //    (both web3 & cefi register on X Layer).
            let address = match address {
                Some(a) => a,
                None => resolve_registration_evm_address()?,
            };
            // ③ Validate the (resolved) address for the target chain.
            token_alias::validate_address_for_chain(CHAIN_INDEX, &address, "address")?;
            register(&agent_id, &account_type, &address, uid.as_deref()).await?
        }
    };
    output::success(result);
    Ok(())
}

pub async fn register(
    agent_id: &str,
    account_type: &str,
    address: &str,
    uid: Option<&str>,
) -> Result<Value> {
    let body = build_registration_body(agent_id, account_type, address, uid);
    let path = "/priapi/v5/wallet/agentic/activity/registration";
    let headers = [("OK-ACCESS-PROJECT", PROJECT_HEADER)];
    let (_, mut auth_client) = ensure_logged_in_client().await?;
    auth_client
        .post_with_headers(path, &body, Some(&headers))
        .await?;
    Ok(build_registration_confirmation(
        agent_id,
        account_type,
        address,
        uid,
    ))
}

fn build_registration_body(
    agent_id: &str,
    account_type: &str,
    address: &str,
    uid: Option<&str>,
) -> Value {
    let mut body = serde_json::json!({
        "activityId": ACTIVITY_ID,
        "agentId": agent_id,
        "chainIndex": CHAIN_INDEX,
        "address": address,
    });
    if account_type == "cefi" {
        if let Some(uid) = uid {
            body["uid"] = Value::String(uid.to_string());
        }
    }
    body
}

/// Builds the CLI/MCP success payload.
///
/// `activityId` is deliberately NOT echoed: it is an internal identifier the
/// skill is forbidden to show the user, so keeping it out of the output removes
/// the leak vector entirely (and trims the caller's context).
fn build_registration_confirmation(
    agent_id: &str,
    account_type: &str,
    address: &str,
    uid: Option<&str>,
) -> Value {
    let mut confirmation = serde_json::json!({
        "registered": true,
        "agentId": agent_id,
        "accountType": account_type,
        "chainIndex": CHAIN_INDEX,
        "address": address,
    });
    if account_type == "cefi" {
        if let Some(uid) = uid {
            confirmation["uid"] = Value::String(uid.to_string());
        }
    }
    confirmation
}

pub(crate) fn require_uid_for_cefi(account_type: &str, uid: Option<&str>) -> Result<()> {
    if account_type == "cefi" && uid.is_none() {
        anyhow::bail!("--uid is required for CeFi account registration");
    }
    Ok(())
}

pub fn resolve_registration_evm_address() -> Result<String> {
    let account = selected_account_entry()?;
    account
        .address_list
        .iter()
        .find(|a| a.chain_index != "501" && a.address.starts_with("0x"))
        .map(|a| a.address.clone())
        .ok_or_else(|| anyhow::anyhow!("could not find an EVM address in the selected account"))
}

/// Shared login-check + selected-account lookup for the address resolver.
fn selected_account_entry() -> Result<wallet_store::AccountMapEntry> {
    let wallets = wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!("not logged in — please run: onchainos wallet login"))?;
    if wallets.selected_account_id.is_empty() {
        bail!("not logged in — please run: onchainos wallet login");
    }
    wallets
        .accounts_map
        .get(&wallets.selected_account_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("selected account has no address list — please re-login"))
}

/// Pre-flight login check for the authenticated registration endpoint.
///
/// Long-lived MCP server clients are constructed once via `ApiClient::new()`
/// (sync) and cache the JWT they had at startup — that token may have expired
/// by the time `register` runs. To avoid sharing a stale token, we always
/// build a fresh `ApiClient::new_async()` here: it has the full JWT lifecycle
/// (expiry check + refresh + AK fallback) baked in.
async fn ensure_logged_in_client() -> Result<(String, ApiClient)> {
    let account_id = match wallet_store::load_wallets() {
        Ok(Some(w)) if !w.selected_account_id.is_empty() => w.selected_account_id.clone(),
        _ => bail!("not logged in — please run: onchainos wallet login"),
    };
    let auth_client = ApiClient::new_async(None).await?;
    Ok((account_id, auth_client))
}

#[cfg(test)]
mod tests {
    use super::*;

    const EVM_ADDR: &str = "0x1111111111111111111111111111111111111111";

    #[test]
    fn register_body_web3_omits_uid() {
        let body = build_registration_body("agent-1", "web3", EVM_ADDR, None);
        assert_eq!(body["activityId"], "okx-marathon-0730");
        assert_eq!(body["agentId"], "agent-1");
        assert_eq!(body["chainIndex"], "196");
        assert_eq!(body["address"], EVM_ADDR);
        assert!(body.get("uid").is_none());
    }

    #[test]
    fn register_body_cefi_includes_uid() {
        let body = build_registration_body("agent-1", "cefi", EVM_ADDR, Some("uid-1"));
        assert_eq!(body["uid"], "uid-1");
    }

    #[test]
    fn register_confirmation_web3_shape() {
        let confirmation = build_registration_confirmation("agent-1", "web3", EVM_ADDR, None);
        assert_eq!(confirmation["registered"], true);
        assert_eq!(confirmation["agentId"], "agent-1");
        assert_eq!(confirmation["accountType"], "web3");
        assert_eq!(confirmation["chainIndex"], "196");
        assert_eq!(confirmation["address"], EVM_ADDR);
        assert!(confirmation.get("uid").is_none());
    }

    #[test]
    fn register_confirmation_never_echoes_activity_id() {
        // The internal activity id must stay out of the output on both account
        // types — the skill is forbidden to show it to the user.
        for (account_type, uid) in [("web3", None), ("cefi", Some("uid-1"))] {
            let confirmation =
                build_registration_confirmation("agent-1", account_type, EVM_ADDR, uid);
            assert!(
                confirmation.get("activityId").is_none(),
                "{account_type} confirmation leaked activityId: {confirmation}"
            );
        }
        // ...while the request body still carries it.
        let body = build_registration_body("agent-1", "web3", EVM_ADDR, None);
        assert_eq!(body["activityId"], ACTIVITY_ID);
    }

    #[test]
    fn register_confirmation_cefi_carries_uid() {
        let confirmation =
            build_registration_confirmation("agent-1", "cefi", EVM_ADDR, Some("uid-1"));
        assert_eq!(confirmation["uid"], "uid-1");
    }

    #[test]
    fn require_uid_for_cefi_missing_uid_errors() {
        let err = require_uid_for_cefi("cefi", None).expect_err("cefi without uid must error");
        assert!(err.to_string().contains("--uid is required"));
    }

    #[test]
    fn require_uid_for_cefi_web3_ok_without_uid() {
        assert!(require_uid_for_cefi("web3", None).is_ok());
    }

    #[test]
    fn require_uid_for_cefi_cefi_ok_with_uid() {
        assert!(require_uid_for_cefi("cefi", Some("uid-1")).is_ok());
    }
}
