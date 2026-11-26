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
        /// Sign without changing the host's active subscription cache.
        #[arg(long)]
        no_cache: bool,
        /// Reject a changed Permit2 cap before producing either signature.
        #[arg(long)]
        expected_cap: Option<String>,
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
        /// Lifetime of the signed cancellation authorization; default one hour.
        #[arg(long, default_value_t = 3600, value_parser = clap::value_parser!(u64).range(1..=3600))]
        valid_for_seconds: u64,
    },

    /// Sign a PendingChangeCancelAuth to cancel a not-yet-effective downgrade.
    #[command(name = "cancel-pending")]
    CancelPending {
        #[arg(long)]
        sub_id: String,
        /// The PENDING downgrade's newSubId (from `my-subscriptions`
        /// pendingChange.newSubId). Signed into the auth (the authorization typehash).
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
        SubscriptionCommand::Subscribe {
            accepts,
            from,
            url,
            no_cache,
            expected_cap,
        } => {
            cmd_subscribe(
                &accepts,
                from.as_deref(),
                url.as_deref(),
                no_cache,
                expected_cap.as_deref(),
            )
            .await
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
            valid_for_seconds,
        } => {
            let chain = chains::resolve_chain(&chain);
            cmd_cancel(
                &sub_id,
                None,
                contract.as_deref(),
                token.as_deref(),
                &chain,
                from.as_deref(),
                valid_for_seconds,
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
                3600,
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
pub(crate) fn cache_subscription(
    protocol: &str,
    url: Option<&str>,
    payload: &SubscriptionPayload,
    sub_id: &str,
    plan_id: &str,
    no_cache: bool,
) {
    let Some(url) = url else {
        eprintln!(
            "Warning: subscription NOT cached (no --url). Subsequent \
             `payment subscription access` can't find this subId — pass --url to subscribe."
        );
        return;
    };
    let mut cache = SubscriptionCache::load();
    let mut entry = SubscriptionCacheEntry {
        sub_id: sub_id.to_string(),
        resource_host: host_of(url),
        merchant: payload.terms.merchant.clone(),
        plan_id: plan_id.to_string(),
        plan_tier: payload.terms.plan_tier,
        max_periods: payload.terms.max_periods,
        state: "active".to_string(),
        changed_to_sub_id: None,
        protocol: protocol.to_string(),
    };

    // APP credentials are handed to the seller after signing. Keep enough local
    // metadata for an authoritative `subscription list` to activate a new host,
    // but never let `access --url` resolve the entry before that confirmation.
    // For a change, preserve an existing active mapping so the current
    // subscription remains usable until the seller accepts the replacement.
    if no_cache {
        if protocol == "app/1" {
            let host = entry.resource_host.clone();
            let has_active = cache
                .get(protocol, &host)
                .is_some_and(|current| current.state == "active");
            if !has_active {
                entry.state = "pending".to_string();
                cache.put(entry);
                if let Err(e) = cache.save() {
                    let _ = writeln!(
                        std::io::stderr(),
                        "Warning: failed to stage subscription cache: {e:#}"
                    );
                }
            }
        }
        return;
    }

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

async fn cmd_subscribe(
    accepts: &str,
    from: Option<&str>,
    url: Option<&str>,
    no_cache: bool,
    expected_cap: Option<&str>,
) -> Result<()> {
    let accepts_val: Value = serde_json::from_str(accepts).context("parse --accepts JSON")?;
    let accepted = select_subscription_entry(&accepts_val)?;
    let (chain_index, chain_id, payer) =
        payment_flow::resolve_chain_and_payer(&accepted, from).await?;
    let signed =
        sign::sign_subscribe_with_cap(&chain_index, chain_id, &payer, &accepted, expected_cap)
            .await
            .map_err(payment_flow::map_pre_signing_allowance_error)?;
    let (hname, hvalue) = build_subscription_payment_header(&accepted, url, &signed.payload)?;
    cache_subscription(
        "x402",
        url,
        &signed.payload,
        &signed.sub_id,
        &signed.plan_id,
        no_cache,
    );
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
    cache_subscription(
        "x402",
        url,
        &signed.payload,
        &signed.sub_id,
        &signed.plan_id,
        false,
    );
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
            let entry = cache.resolve("x402", url).ok_or_else(|| {
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
    valid_for_seconds: u64,
) -> Result<()> {
    let (chain_index, chain_id, payer) =
        payment_flow::resolve_chain_and_payer_by_chain(chain, from).await?;
    let verifying_contract = resolve_contract(contract, token, &payer, &chain_index).await?;

    if let Some(new_sub_id) = new_sub_id {
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
        let auth = sign::sign_cancel(
            &chain_index,
            chain_id,
            &payer,
            sub_id,
            &verifying_contract,
            valid_for_seconds,
        )
        .await?;
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
    use crate::payment::subscription::types::BuyerSubscriptionItem;

    #[test]
    fn pre_signing_allowance_failure_preserves_structured_facts() {
        let failure = crate::payment::subscription::types::InsufficientPermit2Allowance {
            token: "asset-fixture".into(),
            chain_index: "196".into(),
            available: "0".into(),
            required: "5000000".into(),
            permit2_contract: "contract-fixture".into(),
            message: "A human explanation independent of recovery".into(),
        };
        let error = payment_flow::map_pre_signing_allowance_error(
            anyhow::Error::new(failure).context("a different human explanation"),
        );
        let coded = error
            .downcast_ref::<crate::commands::sink::CodedError>()
            .unwrap();
        assert_eq!(coded.code, "permit2_allowance_insufficient");
        let data = coded.data.as_ref().unwrap();
        assert_eq!(data["stage"], "before_signing");
        assert_eq!(data["requiredAmount"], "5000000");
        assert_eq!(data["availableAmount"], "0");
        assert_eq!(data["asset"], "asset-fixture");
        let unknown = payment_flow::map_pre_signing_allowance_error(anyhow!(
            "allowance insufficient after unknown signing"
        ));
        assert!(unknown
            .downcast_ref::<crate::commands::sink::CodedError>()
            .is_none());
    }

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

    fn subscription_payload_fixture() -> SubscriptionPayload {
        serde_json::from_value(json!({
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
        .unwrap()
    }

    #[test]
    fn header_uses_permit_single_payload_keys() {
        let payload = subscription_payload_fixture();
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

    #[test]
    fn no_cache_preserves_the_previously_confirmed_host_mapping() {
        let payload = subscription_payload_fixture();
        // A checked service handoff must not replace a previously active host mapping,
        // even if later proof validation/submission fails. No signer or network is used.
        let _guard = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let root = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("ONCHAINOS_HOME");
        std::env::set_var("ONCHAINOS_HOME", root.path());
        let mut cache = SubscriptionCache::default();
        cache.put(SubscriptionCacheEntry {
            sub_id: "old-confirmed-sub".into(),
            resource_host: "api.x.com".into(),
            merchant: "merchant-fixture".into(),
            plan_id: "old-plan".into(),
            plan_tier: 1,
            max_periods: 1,
            state: "active".into(),
            changed_to_sub_id: None,
            protocol: "x402".into(),
        });
        cache.save().unwrap();
        let before = std::fs::read(SubscriptionCache::cache_path().unwrap()).unwrap();
        cache_subscription(
            "x402",
            Some("https://api.x.com/d"),
            &payload,
            "unconfirmed-sub",
            "new-plan",
            true,
        );
        assert_eq!(
            std::fs::read(SubscriptionCache::cache_path().unwrap()).unwrap(),
            before
        );
        assert_eq!(
            SubscriptionCache::load()
                .resolve("x402", "https://api.x.com/d")
                .unwrap()
                .sub_id,
            "old-confirmed-sub"
        );
        match previous {
            Some(value) => std::env::set_var("ONCHAINOS_HOME", value),
            None => std::env::remove_var("ONCHAINOS_HOME"),
        }
    }

    #[test]
    fn app_no_cache_stages_inactive_mapping_until_authoritative_reconcile() {
        let payload = subscription_payload_fixture();
        let _guard = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let root = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("ONCHAINOS_HOME");
        std::env::set_var("ONCHAINOS_HOME", root.path());

        cache_subscription(
            "app/1",
            Some("https://api.x.com/d"),
            &payload,
            "new-sub",
            "new-plan",
            true,
        );

        let mut cache = SubscriptionCache::load();
        assert!(cache.resolve("app/1", "https://api.x.com/d").is_none());
        assert_eq!(
            cache.get("app/1", "api.x.com").unwrap().state,
            "pending"
        );

        cache.reconcile_from(&[BuyerSubscriptionItem {
            chain_index: 196,
            offer_id: "offer-new".into(),
            sub_id: "new-sub".into(),
            state: 1,
            payer: "0xbuyer".into(),
            token: "0xtoken".into(),
            amount_per_period: "5000000".into(),
            period_sec: 2_592_000,
            period_mode: 0,
            billing_anchor_at: 0,
            max_periods: 12,
            start_at: 0,
            initial_charge_periods: 1,
            initial_charge_amount: "5000000".into(),
            last_charged_period: 0,
            total_pulled: "0".into(),
            plan_id: "new-plan".into(),
            plan_tier: 2,
            changed_to_sub_id: None,
            pending_change: None,
            is_active: true,
            service_ended: false,
            current_period: 1,
            next_chargeable_at: None,
        }]);
        assert_eq!(
            cache
                .resolve("app/1", "https://api.x.com/d")
                .unwrap()
                .sub_id,
            "new-sub"
        );

        match previous {
            Some(value) => std::env::set_var("ONCHAINOS_HOME", value),
            None => std::env::remove_var("ONCHAINOS_HOME"),
        }
    }

    #[test]
    fn app_no_cache_preserves_existing_active_mapping() {
        let payload = subscription_payload_fixture();
        let _guard = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let root = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("ONCHAINOS_HOME");
        std::env::set_var("ONCHAINOS_HOME", root.path());
        let mut cache = SubscriptionCache::default();
        cache.put(SubscriptionCacheEntry {
            sub_id: "old-confirmed-sub".into(),
            resource_host: "api.x.com".into(),
            merchant: "merchant-fixture".into(),
            plan_id: "old-plan".into(),
            plan_tier: 1,
            max_periods: 12,
            state: "active".into(),
            changed_to_sub_id: None,
            protocol: "app/1".into(),
        });
        cache.save().unwrap();

        cache_subscription(
            "app/1",
            Some("https://api.x.com/d"),
            &payload,
            "unconfirmed-new-sub",
            "new-plan",
            true,
        );

        assert_eq!(
            SubscriptionCache::load()
                .resolve("app/1", "https://api.x.com/d")
                .unwrap()
                .sub_id,
            "old-confirmed-sub"
        );
        match previous {
            Some(value) => std::env::set_var("ONCHAINOS_HOME", value),
            None => std::env::remove_var("ONCHAINOS_HOME"),
        }
    }

    #[test]
    fn subscribe_no_cache_is_opt_in() {
        use clap::Parser;
        #[derive(Parser)]
        struct Cli {
            #[command(subcommand)]
            command: SubscriptionCommand,
        }
        for enabled in [false, true] {
            let mut args = vec![
                "test",
                "subscribe",
                "--accepts",
                "{}",
                "--url",
                "https://example.com/pay",
            ];
            if enabled {
                args.push("--no-cache");
            }
            assert!(matches!(Cli::parse_from(args).command,
                SubscriptionCommand::Subscribe { no_cache, .. } if no_cache == enabled));
        }
    }
}
