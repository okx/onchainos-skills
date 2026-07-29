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

#[derive(Subcommand)]
pub enum HackathonCommand {
    /// Register the user's Trading ASP for the OKX.AI trading hackathon (requires wallet login).
    Register {
        /// [UNIT: integer] Activity ID; default "5" (this hackathon; B1), overridable.
        #[arg(long, default_value = "5")]
        activity_id: String,
        /// Agent ID of the user's Trading ASP to register. [UNIT: id]
        #[arg(long)]
        agent_id: String,
        /// Account type: "web3" or "cefi"  [UNIT: enum]
        #[arg(long, value_parser = clap::builder::PossibleValuesParser::new(["web3", "cefi"]))]
        account_type: String,
        /// [UNIT: address-evm] Wallet address; auto-resolved from wallet X Layer addr when omitted (both web3 & cefi; B2).
        #[arg(long)]
        address: Option<String>,
        /// [UNIT: chain-index] Chain index; always "196" (X Layer) for this hackathon.
        #[arg(long, default_value = "196")]
        chain_index: String,
        /// CeFi user ID (required when --account-type=cefi). [UNIT: id]
        #[arg(long)]
        uid: Option<String>,
    },
}

pub async fn execute(_ctx: &Context, command: HackathonCommand) -> Result<()> {
    let result: Value = match command {
        HackathonCommand::Register {
            activity_id,
            agent_id,
            account_type,
            address,
            chain_index,
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
            token_alias::validate_address_for_chain(&chain_index, &address, "address")?;
            register(
                &activity_id,
                &agent_id,
                &account_type,
                &address,
                &chain_index,
                uid.as_deref(),
            )
            .await?
        }
    };
    output::success(result);
    Ok(())
}

pub async fn register(
    activity_id: &str,
    agent_id: &str,
    account_type: &str,
    address: &str,
    chain_index: &str,
    uid: Option<&str>,
) -> Result<Value> {
    let body = build_registration_body(
        activity_id,
        agent_id,
        account_type,
        address,
        chain_index,
        uid,
    );
    let path = "/priapi/v5/wallet/agentic/activity/registration";
    let headers = [("OK-ACCESS-PROJECT", PROJECT_HEADER)];
    let (_, mut auth_client) = ensure_logged_in_client().await?;
    auth_client
        .post_with_headers(path, &body, Some(&headers))
        .await?;
    Ok(build_registration_confirmation(
        activity_id,
        agent_id,
        account_type,
        address,
        chain_index,
        uid,
    ))
}

fn build_registration_body(
    activity_id: &str,
    agent_id: &str,
    account_type: &str,
    address: &str,
    chain_index: &str,
    uid: Option<&str>,
) -> Value {
    let mut body = serde_json::json!({
        "activityId": activity_id,
        "agentId": agent_id,
        "chainIndex": chain_index,
        "address": address,
    });
    if account_type == "cefi" {
        if let Some(uid) = uid {
            body["uid"] = Value::String(uid.to_string());
        }
    }
    body
}

fn build_registration_confirmation(
    activity_id: &str,
    agent_id: &str,
    account_type: &str,
    address: &str,
    chain_index: &str,
    uid: Option<&str>,
) -> Value {
    let mut confirmation = serde_json::json!({
        "registered": true,
        "activityId": activity_id,
        "agentId": agent_id,
        "accountType": account_type,
        "chainIndex": chain_index,
        "address": address,
    });
    if account_type == "cefi" {
        if let Some(uid) = uid {
            confirmation["uid"] = Value::String(uid.to_string());
        }
    }
    confirmation
}

fn require_uid_for_cefi(account_type: &str, uid: Option<&str>) -> Result<()> {
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
        let body = build_registration_body("5", "agent-1", "web3", EVM_ADDR, "196", None);
        assert_eq!(body["activityId"], "5");
        assert_eq!(body["agentId"], "agent-1");
        assert_eq!(body["chainIndex"], "196");
        assert_eq!(body["address"], EVM_ADDR);
        assert!(body.get("uid").is_none());
    }

    #[test]
    fn register_body_cefi_includes_uid() {
        let body = build_registration_body("5", "agent-1", "cefi", EVM_ADDR, "196", Some("uid-1"));
        assert_eq!(body["uid"], "uid-1");
    }

    #[test]
    fn register_confirmation_web3_shape() {
        let confirmation =
            build_registration_confirmation("5", "agent-1", "web3", EVM_ADDR, "196", None);
        assert_eq!(confirmation["registered"], true);
        assert_eq!(confirmation["activityId"], "5");
        assert_eq!(confirmation["agentId"], "agent-1");
        assert_eq!(confirmation["accountType"], "web3");
        assert_eq!(confirmation["chainIndex"], "196");
        assert_eq!(confirmation["address"], EVM_ADDR);
        assert!(confirmation.get("uid").is_none());
    }

    #[test]
    fn register_confirmation_cefi_carries_uid() {
        let confirmation =
            build_registration_confirmation("5", "agent-1", "cefi", EVM_ADDR, "196", Some("uid-1"));
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
