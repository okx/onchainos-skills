use anyhow::{bail, Result};

use crate::commands::agentic_wallet::{
    account, auth, balance,
    chain_profile::{self, ResolvedChainProfile, TransferDriver},
    common::ERR_NOT_LOGGED_IN,
};
use crate::wallet_api::WalletApiClient;
use crate::wallet_store::{self, AddressInfo, WalletsJson};

pub struct LoadedChainContext {
    pub access_token: String,
    pub account_id: String,
    pub login_type: String,
    pub profile: ResolvedChainProfile,
    pub address: AddressInfo,
}

/// Loads authentication, chain profile, account, and address data for a chain command.
pub async fn load_chain_context(
    resolver_input: &str,
    expected_driver: TransferDriver,
    chain_label: &str,
    from: Option<&str>,
    validate_address: fn(&str) -> Result<()>,
    same_address: fn(&str, &str) -> Result<bool>,
) -> Result<LoadedChainContext> {
    let access_token = auth::ensure_tokens_refreshed().await?;
    let profile = chain_profile::resolve(resolver_input).await?;
    if profile.capabilities.transfer != expected_driver {
        bail!("{resolver_input} profile resolved to a non-{chain_label} chain");
    }

    let mut wallets = wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!(ERR_NOT_LOGGED_IN))?;
    let account_id = account::resolve_active_account_id(&wallets)?;
    let mut address = select_current_address(
        &wallets,
        &account_id,
        &profile,
        chain_label,
        from,
        same_address,
    );
    if address.is_err() {
        let mut client = WalletApiClient::new()?;
        balance::ensure_wallet_accounts_fresh(
            &mut client,
            &access_token,
            &mut wallets,
            true,
        )
        .await?;
        address = select_current_address(
            &wallets,
            &account_id,
            &profile,
            chain_label,
            from,
            same_address,
        );
    }
    let address = address?;
    validate_address(&address.address)?;

    Ok(LoadedChainContext {
        access_token,
        account_id,
        login_type: wallets.login_type,
        profile,
        address,
    })
}

/// Selects the current account's sole address for `profile` and validates an optional `from`.
pub fn select_current_address(
    wallets: &WalletsJson,
    account_id: &str,
    profile: &ResolvedChainProfile,
    chain_label: &str,
    from: Option<&str>,
    same_address: fn(&str, &str) -> Result<bool>,
) -> Result<AddressInfo> {
    let account = wallets
        .accounts_map
        .get(account_id)
        .ok_or_else(|| anyhow::anyhow!("current account '{account_id}' was not found"))?;
    let candidates: Vec<&AddressInfo> = account
        .address_list
        .iter()
        .filter(|address| address.chain_index == profile.chain_index)
        .collect();
    if candidates.is_empty() {
        bail!("current account '{account_id}' has no {chain_label} address");
    }
    if candidates.len() != 1 {
        bail!("current account '{account_id}' has multiple {chain_label} addresses");
    }
    let selected = candidates[0];
    if let Some(from) = from {
        if !same_address(from, &selected.address)? {
            bail!("--from must be the {chain_label} address of the current account");
        }
    }
    Ok(selected.clone())
}
