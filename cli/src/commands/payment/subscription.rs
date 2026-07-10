//! CLI handlers for the x402 `period` scheme.
//!
//! The signing commands (`subscribe` / `change` / `cancel` / `cancel-pending`)
//! sign and emit a `PAYMENT-SIGNATURE` header (or a signed `CancelAuth`) for
//! the agent/skill to relay to the Seller. The buyer-direct reads
//! (`allowance-status` / `my-subscriptions`) perform HTTP themselves. `access`
//! resolves a subId (cache or `--sub-id`) and personal-signs an `APP-Access`
//! AccessProof header.
//!
//! The local subId cache is a convenience index only (see [`crate::payment::subscription::cache`]).

use std::io::Write;

use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use clap::Subcommand;
use serde_json::{json, Value};

use crate::chains;
use crate::commands::payment::payment_flow;
use crate::output;
use crate::payment::subscription::cache::{host_of, SubscriptionCache};
use crate::payment::subscription::facilitator;
use crate::payment::subscription::sign;
use crate::payment::subscription::types::{SubscriptionCacheEntry, SubscriptionPayload};

#[derive(Subcommand)]
pub enum SubscriptionCommand {
    /// Sign a subscribe double-signature (PermitSingle + SubscriptionTerms) for
    /// a `period` 402 and emit the PAYMENT-SIGNATURE header.
    Subscribe {
        /// JSON accepts array (or single object) from the 402 response.
        #[arg(long)]
        accepts: String,
        /// Payer address (optional; defaults to the selected account).
        #[arg(long)]
        from: Option<String>,
        /// Resource URL (used as the header `resource.url` and the cache key).
        #[arg(long)]
        url: Option<String>,
    },

    /// Build an `APP-Access` AccessProof header for a resource URL: resolve the
    /// active subId (cache or `--sub-id`), then personal-sign
    /// `(subId, payer, timestamp)`.
    Access {
        /// Resource URL whose host maps to a cached subscription.
        #[arg(long)]
        url: String,
        /// Explicit subId override (skips the cache lookup).
        #[arg(long)]
        sub_id: Option<String>,
        /// Payer address (optional; defaults to the selected account).
        #[arg(long)]
        from: Option<String>,
        /// Chain name or index (default: xlayer).
        #[arg(long, default_value = "xlayer")]
        chain: String,
    },

    /// Sign a change (up/downgrade) double-signature from a change-offer 402.
    Change {
        /// JSON accepts array (or single object) from the change-offer 402.
        #[arg(long)]
        accepts: String,
        /// The subId being changed (overrides `extra.changeFrom.fromSubId`).
        #[arg(long)]
        sub_id: Option<String>,
        #[arg(long)]
        from: Option<String>,
        /// Resource URL (cache key for the resulting subscription).
        #[arg(long)]
        url: Option<String>,
    },

    /// Sign a CancelAuth to cancel a subscription (delivered to the Seller).
    Cancel {
        #[arg(long)]
        sub_id: String,
        /// Subscription contract address (EIP-712 domain). If omitted, it is
        /// fetched via allowance-status using `--token`.
        #[arg(long)]
        contract: Option<String>,
        /// ERC-20 token — only needed to look up the contract when `--contract`
        /// is omitted.
        #[arg(long)]
        token: Option<String>,
        /// Chain name or index (default: xlayer).
        #[arg(long, default_value = "xlayer")]
        chain: String,
        #[arg(long)]
        from: Option<String>,
    },

    /// Sign a PendingChangeCancelAuth to cancel a not-yet-effective downgrade.
    #[command(name = "cancel-pending")]
    CancelPending {
        #[arg(long)]
        sub_id: String,
        /// The PENDING downgrade's newSubId (from `my-subscriptions`
        /// pendingPlanChange.newSubId). Signed into the auth (MR6 typehash).
        #[arg(long)]
        new_sub_id: String,
        #[arg(long)]
        contract: Option<String>,
        #[arg(long)]
        token: Option<String>,
        #[arg(long, default_value = "xlayer")]
        chain: String,
        #[arg(long)]
        from: Option<String>,
    },

    /// List the buyer's own subscriptions (buyer-direct) and reconcile the
    /// local subId cache.
    #[command(name = "my-subscriptions")]
    MySubscriptions {
        #[arg(long, default_value = "xlayer")]
        chain: String,
        #[arg(long)]
        from: Option<String>,
        #[arg(long, default_value_t = 50)]
        limit: u32,
        #[arg(long, default_value_t = 0)]
        offset: u32,
    },

    /// Query Permit2 allowance status (buyer-direct): nonce / reserved /
    /// contracts / layer-1 allowance.
    #[command(name = "allowance-status")]
    AllowanceStatus {
        #[arg(long)]
        token: String,
        #[arg(long, default_value = "xlayer")]
        chain: String,
        #[arg(long)]
        from: Option<String>,
    },
}

pub async fn execute(cmd: SubscriptionCommand) -> Result<()> {
    match cmd {
        SubscriptionCommand::Subscribe { accepts, from, url } => {
            cmd_subscribe(&accepts, from.as_deref(), url.as_deref()).await
        }
        SubscriptionCommand::Access {
            url,
            sub_id,
            from,
            chain,
        } => {
            let chain = chains::resolve_chain(&chain);
            cmd_access(&url, sub_id.as_deref(), from.as_deref(), &chain).await
        }
        SubscriptionCommand::Change {
            accepts,
            sub_id,
            from,
            url,
        } => cmd_change(&accepts, sub_id.as_deref(), from.as_deref(), url.as_deref()).await,
        SubscriptionCommand::Cancel {
            sub_id,
            contract,
            token,
            chain,
            from,
        } => {
            let chain = chains::resolve_chain(&chain);
            cmd_cancel(
                &sub_id,
                None,
                contract.as_deref(),
                token.as_deref(),
                &chain,
                from.as_deref(),
                false,
            )
            .await
        }
        SubscriptionCommand::CancelPending {
            sub_id,
            new_sub_id,
            contract,
            token,
            chain,
            from,
        } => {
            let chain = chains::resolve_chain(&chain);
            cmd_cancel(
                &sub_id,
                Some(&new_sub_id),
                contract.as_deref(),
                token.as_deref(),
                &chain,
                from.as_deref(),
                true,
            )
            .await
        }
        SubscriptionCommand::MySubscriptions {
            chain,
            from,
            limit,
            offset,
        } => {
            let chain = chains::resolve_chain(&chain);
            cmd_my_subscriptions(&chain, from.as_deref(), limit, offset).await
        }
        SubscriptionCommand::AllowanceStatus { token, chain, from } => {
            let chain = chains::resolve_chain(&chain);
            cmd_allowance_status(&token, &chain, from.as_deref()).await
        }
    }
}

/// Select the `period` entry from an accepts array (or accept a single object).
fn select_subscription_entry(accepts: &Value) -> Result<Value> {
    match accepts.as_array() {
        Some(arr) => arr
            .iter()
            .find(|e| e.get("scheme").and_then(Value::as_str) == Some("period"))
            .cloned()
            .ok_or_else(|| anyhow!("no period entry in accepts[]")),
        None => Ok(accepts.clone()),
    }
}

/// Build the base64 `PAYMENT-SIGNATURE` header for a subscribe / change
/// double-sign (x402 v2 envelope + subscription payload).
fn build_subscription_payment_header(
    accepted: &Value,
    resource_url: Option<&str>,
    payload: &SubscriptionPayload,
) -> Result<(&'static str, String)> {
    let permit_v = serde_json::to_value(&payload.permit).context("serialize permitSingle")?;
    let terms_v = serde_json::to_value(&payload.terms).context("serialize terms")?;
    let body = json!({
        "x402Version": 2,
        "resource": {
            "url": resource_url.unwrap_or(""),
            "mimeType": "application/json",
        },
        "accepted": accepted,
        "payload": {
            "permitSingle": permit_v,
            "permitSingleSignature": payload.permit_signature,
            "terms": terms_v,
            "termsSignature": payload.terms_signature,
        },
    });
    let encoded = B64.encode(serde_json::to_vec(&body).context("encode subscription header")?);
    Ok(("PAYMENT-SIGNATURE", encoded))
}

/// Write-through cache update after a successful subscribe / change sign.
/// `plan_id` is the `extra.plan.id` business identifier (cache-only).
fn cache_subscription(
    url: Option<&str>,
    payload: &SubscriptionPayload,
    sub_id: &str,
    plan_id: &str,
) {
    let Some(url) = url else {
        eprintln!(
            "Warning: subscription NOT cached (no --url). Subsequent \
             `payment subscription access` can't find this subId — pass --url to subscribe."
        );
        return;
    };
    let mut cache = SubscriptionCache::load();
    let entry = SubscriptionCacheEntry {
        sub_id: sub_id.to_string(),
        resource_host: host_of(url),
        merchant: payload.terms.merchant.clone(),
        plan_id: plan_id.to_string(),
        plan_tier: payload.terms.plan_tier,
        max_periods: payload.terms.max_periods,
        state: "active".to_string(),
        changed_to_sub_id: None,
    };
    let old_sub_id = payload.terms.change_from_sub_id.clone();
    let is_change = !old_sub_id
        .trim_start_matches("0x")
        .trim_matches('0')
        .is_empty();
    if is_change {
        // changeEffectiveAt: 1 = upgrade (immediate) / 2 = downgrade (period_end).
        if payload.terms.change_effective_at == 2 {
            // Downgrade activates only on the next charge after the period
            // boundary; until then the old sub stays active, so don't switch the
            // cache now. `my-subscriptions` reconcile switches it once on-chain.
            eprintln!(
                "Note: downgrade scheduled — current plan stays active until period end. \
                 Run `payment subscription my-subscriptions` after it activates to switch."
            );
        } else {
            // Upgrade is immediate — switch the host to the new subId now.
            cache.mark_changed(&old_sub_id, entry);
        }
    } else {
        cache.put(entry);
    }
    if let Err(e) = cache.save() {
        let _ = writeln!(
            std::io::stderr(),
            "Warning: failed to update subscription cache: {e:#}"
        );
    }
}

async fn cmd_subscribe(accepts: &str, from: Option<&str>, url: Option<&str>) -> Result<()> {
    let accepts_val: Value = serde_json::from_str(accepts).context("parse --accepts JSON")?;
    let accepted = select_subscription_entry(&accepts_val)?;
    let (chain_index, chain_id, payer) =
        payment_flow::resolve_chain_and_payer(&accepted, from).await?;
    let signed = sign::sign_subscribe(&chain_index, chain_id, &payer, &accepted).await?;
    let (hname, hvalue) = build_subscription_payment_header(&accepted, url, &signed.payload)?;
    cache_subscription(url, &signed.payload, &signed.sub_id, &signed.plan_id);
    let payload_v =
        serde_json::to_value(&signed.payload).context("serialize subscription payload")?;
    output::success(json!({
        "paymentHeaderName": hname,
        "paymentHeaderValue": hvalue,
        "subId": signed.sub_id,
        "chainIndex": signed.chain_index,
        "payload": payload_v,
    }));
    Ok(())
}

async fn cmd_change(
    accepts: &str,
    sub_id: Option<&str>,
    from: Option<&str>,
    url: Option<&str>,
) -> Result<()> {
    let accepts_val: Value = serde_json::from_str(accepts).context("parse --accepts JSON")?;
    let accepted = select_subscription_entry(&accepts_val)?;
    let (chain_index, chain_id, payer) =
        payment_flow::resolve_chain_and_payer(&accepted, from).await?;
    let signed = sign::sign_change(
        &chain_index,
        chain_id,
        &payer,
        sub_id.unwrap_or(""),
        &accepted,
    )
    .await?;
    let (hname, hvalue) = build_subscription_payment_header(&accepted, url, &signed.payload)?;
    cache_subscription(url, &signed.payload, &signed.sub_id, &signed.plan_id);
    let payload_v =
        serde_json::to_value(&signed.payload).context("serialize subscription payload")?;
    output::success(json!({
        "paymentHeaderName": hname,
        "paymentHeaderValue": hvalue,
        "subId": signed.sub_id,
        "chainIndex": signed.chain_index,
        "payload": payload_v,
    }));
    Ok(())
}

async fn cmd_access(
    url: &str,
    sub_id: Option<&str>,
    from: Option<&str>,
    chain: &str,
) -> Result<()> {
    // Resolve the subId: explicit override, else the active cached entry.
    let (resolved_sub, source) = match sub_id {
        Some(sid) => (sid.to_string(), "override"),
        None => {
            let cache = SubscriptionCache::load();
            let entry = cache.resolve(url).ok_or_else(|| {
                anyhow!(
                    "no active subscription cached for host {}. Run \
                     `payment subscription my-subscriptions` to reconcile, or pass --sub-id.",
                    host_of(url)
                )
            })?;
            (entry.sub_id.clone(), "cache")
        }
    };

    let (chain_index, _chain_id, payer) =
        payment_flow::resolve_chain_and_payer_by_chain(chain, from).await?;
    let (hname, hvalue) = sign::build_access_proof(&chain_index, &payer, &resolved_sub).await?;

    output::success(json!({
        "subId": resolved_sub,
        "host": host_of(url),
        "source": source,
        "accessHeaderName": hname,
        "accessHeaderValue": hvalue,
    }));
    Ok(())
}

/// Resolve the subscription contract address: explicit `--contract`, else via
/// allowance-status using `--token`.
async fn resolve_contract(
    contract: Option<&str>,
    token: Option<&str>,
    payer: &str,
    chain_index: &str,
) -> Result<String> {
    if let Some(c) = contract {
        return Ok(c.to_string());
    }
    let token = token.ok_or_else(|| {
        anyhow!("cancel requires --contract (subscription contract) or --token (to look it up)")
    })?;
    let a = facilitator::allowance_status(payer, token, chain_index).await?;
    if a.subscription_contract.is_empty() {
        bail!("allowance-status returned no subscriptionContract for token {token}");
    }
    Ok(a.subscription_contract)
}

async fn cmd_cancel(
    sub_id: &str,
    new_sub_id: Option<&str>,
    contract: Option<&str>,
    token: Option<&str>,
    chain: &str,
    from: Option<&str>,
    pending: bool,
) -> Result<()> {
    let (chain_index, chain_id, payer) =
        payment_flow::resolve_chain_and_payer_by_chain(chain, from).await?;
    let verifying_contract = resolve_contract(contract, token, &payer, &chain_index).await?;

    if pending {
        let new_sub_id = new_sub_id.ok_or_else(|| {
            anyhow!("cancel-pending requires --new-sub-id (the PENDING downgrade's newSubId)")
        })?;
        let auth = sign::sign_cancel_pending_change(
            &chain_index,
            chain_id,
            &payer,
            sub_id,
            new_sub_id,
            &verifying_contract,
        )
        .await?;
        let auth_v = serde_json::to_value(&auth).context("serialize pendingChangeCancelAuth")?;
        output::success(json!({ "pendingChangeCancelAuth": auth_v, "chainIndex": chain_index }));
    } else {
        let auth =
            sign::sign_cancel(&chain_index, chain_id, &payer, sub_id, &verifying_contract).await?;
        // Don't mark the cache canceled here — this only signs the CancelAuth.
        // The sub stays active and billable until the contract executes; the
        // `my-subscriptions` reconcile corrects it once on-chain confirms.
        let auth_v = serde_json::to_value(&auth).context("serialize cancelAuth")?;
        output::success(json!({ "cancelAuth": auth_v, "chainIndex": chain_index }));
    }
    Ok(())
}

async fn cmd_my_subscriptions(
    chain: &str,
    from: Option<&str>,
    limit: u32,
    offset: u32,
) -> Result<()> {
    let (_chain_index, _chain_id, payer) =
        payment_flow::resolve_chain_and_payer_by_chain(chain, from).await?;
    let resp = facilitator::my_subscriptions(&payer, limit, offset).await?;

    // Reconcile the local cache from authoritative state.
    let mut cache = SubscriptionCache::load();
    cache.reconcile_from(&resp.subscriptions);
    if let Err(e) = cache.save() {
        let _ = writeln!(
            std::io::stderr(),
            "Warning: failed to reconcile subscription cache: {e:#}"
        );
    }

    let subs_v = serde_json::to_value(&resp.subscriptions).context("serialize subscriptions")?;
    output::success(json!({ "subscriptions": subs_v }));
    Ok(())
}

async fn cmd_allowance_status(token: &str, chain: &str, from: Option<&str>) -> Result<()> {
    let (chain_index, _chain_id, payer) =
        payment_flow::resolve_chain_and_payer_by_chain(chain, from).await?;
    let a = facilitator::allowance_status(&payer, token, &chain_index).await?;
    output::success(json!({
        "approvedAmount": a.approved_amount,
        "expiration": a.expiration,
        "nonce": a.nonce,
        "reservedAmount": a.reserved_amount,
        "reservedExpiration": a.reserved_expiration,
        "tokenBalance": a.token_balance,
        "availableAmount": a.available_amount,
        "permit2Allowance": a.permit2_allowance,
        "subscriptionContract": a.subscription_contract,
        "permit2Contract": a.permit2_contract,
    }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_subscription_entry_from_array() {
        let accepts = json!([
            { "scheme": "exact", "network": "eip155:196" },
            { "scheme": "period", "network": "eip155:196", "asset": "0xtok" }
        ]);
        let e = select_subscription_entry(&accepts).unwrap();
        assert_eq!(e["scheme"], "period");
        assert_eq!(e["asset"], "0xtok");
    }

    #[test]
    fn select_subscription_entry_single_object() {
        let accepts = json!({ "scheme": "period", "asset": "0xtok" });
        assert_eq!(
            select_subscription_entry(&accepts).unwrap()["asset"],
            "0xtok"
        );
    }

    #[test]
    fn select_subscription_entry_missing_errors() {
        let accepts = json!([{ "scheme": "exact" }]);
        assert!(select_subscription_entry(&accepts).is_err());
    }

    #[test]
    fn header_uses_permit_single_payload_keys() {
        let payload: SubscriptionPayload = serde_json::from_value(json!({
            "terms": {
                "payer": "0xp", "merchant": "0xm", "facilitator": "0xf", "token": "0xt",
                "amountPerPeriod": "5000000", "periodSec": 2592000, "maxPeriods": 12, "startAt": 0,
                "initialChargePeriods": 1, "initialChargeAmount": "5000000", "termsDeadline": 1750000000,
                "permitHash": "0xph", "salt": "0xsalt", "planTier": 2,
                "changeFromSubId": "0x0000000000000000000000000000000000000000000000000000000000000000",
                "changeEffectiveAt": 0, "periodMode": 0, "planId": "pro_monthly"
            },
            "termsSignature": "0xtsig",
            "permit": { "details": { "token": "0xt", "amount": "60000000", "expiration": 1782000000, "nonce": 7 },
                        "spender": "0xsub", "sigDeadline": "1750000000" },
            "permitSignature": "0xpsig"
        }))
        .unwrap();
        let accepted = json!({ "scheme": "period" });
        let (name, value) =
            build_subscription_payment_header(&accepted, Some("https://api.x.com/d"), &payload)
                .unwrap();
        assert_eq!(name, "PAYMENT-SIGNATURE");
        let body: Value = serde_json::from_slice(&B64.decode(&value).unwrap()).unwrap();
        assert_eq!(body["x402Version"], 2);
        assert_eq!(body["resource"]["url"], "https://api.x.com/d");
        assert_eq!(body["payload"]["permitSingleSignature"], "0xpsig");
        assert_eq!(body["payload"]["termsSignature"], "0xtsig");
        assert_eq!(body["payload"]["permitSingle"]["details"]["nonce"], 7);
        assert_eq!(body["payload"]["terms"]["planTier"], 2);
    }
}
