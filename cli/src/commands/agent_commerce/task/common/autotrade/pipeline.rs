//! FR-4 fixed-order runtime validation chain + idempotency latch.
//!
//! Composes the Phase-2 building blocks into either an [`card::ExecutionCard`]
//! (all checks pass) or a [`card::NotifyOnly`] (any degrade). The order is fixed:
//! structure → freshness → subscription → wallet → type-enabled → grant cap →
//! holding (sell+pct) → idempotency latch. Every branch returns a stable
//! machine-readable reason matching the audit action names.

use std::io::ErrorKind;
use std::path::PathBuf;

use super::amount::Decimal;
use super::card::{self, DecisionRequest, ExecutionCard, NotifyOnly};
use super::schema::{self, AmountUnit, AutoTradeSignal, DefiAction, Side, TypedParams};
use super::{consent, holding, subscription};
use super::{AutoTradeError, DegradeReason};

/// Runtime inputs for the buyer inbound pipeline.
pub struct PipelineInput<'a> {
    /// The `autotrade:` line JSON (omitting nothing; `signalTime` present).
    pub signal_json: &'a str,
    pub job_id: &'a str,
    pub agent_id: &'a str,
    /// When the buyer received the delivery (ms since epoch).
    pub received_at_ms: u64,
    /// Locally-saved deliverable path — surfaced only on the notify-only path.
    pub saved_path: &'a str,
    /// When true, the caller (`autotrade-consent-set` replaying a signal the user just
    /// approved via option B — "execute this one, ask me each time after") holds the
    /// user's explicit consent for THIS delivery: skip the consent gate + cap and
    /// execute it once. Structure / freshness / subscription / wallet still apply.
    /// Normal inbound signals set this `false`.
    pub consent_override: bool,
}

/// The pipeline result: exactly one of card / notify / decision.
pub enum PipelineOutcome {
    Card(Box<ExecutionCard>),
    Notify(NotifyOnly),
    /// First-time / manual (ask-every-time) / over-cap — push a decision to the user; execute
    /// nothing yet. The caller stashes the signal so a follow-up consent-set can replay it.
    Decision(Box<DecisionRequest>),
}

/// Current wall-clock in milliseconds since the Unix epoch.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// AC-4 freshness: age is measured from `min(signalTime, receivedAt)`; expired
/// when `age > ttlSec`. Returns `FreshnessExpired` when stale.
fn check_freshness(
    signal_time_ms: u64,
    received_at_ms: u64,
    now_ms: u64,
    ttl_sec: u64,
) -> Result<(), AutoTradeError> {
    let effective = signal_time_ms.min(received_at_ms);
    // Clock skew / future timestamp ⇒ treat as fresh (age 0).
    let age_ms = now_ms.saturating_sub(effective);
    if age_ms > ttl_sec.saturating_mul(1000) {
        return Err(AutoTradeError::Degrade(DegradeReason::FreshnessExpired));
    }
    Ok(())
}

/// Resolve the buyer's active wallet address for `chain_index` (exact chain, then
/// same family). `None` ⇒ `no_active_wallet`.
fn resolve_active_wallet_address(chain_index: &str) -> Option<String> {
    let wallets = crate::wallet_store::load_wallets().ok().flatten()?;
    if wallets.selected_account_id.is_empty() {
        return None;
    }
    let entry = wallets.accounts_map.get(&wallets.selected_account_id)?;
    let family = crate::chains::chain_family(chain_index);
    entry
        .address_list
        .iter()
        .find(|a| a.chain_index == chain_index)
        .or_else(|| {
            entry
                .address_list
                .iter()
                .find(|a| crate::chains::chain_family(&a.chain_index) == family)
        })
        .map(|a| a.address.clone())
        .filter(|s| !s.is_empty())
}

/// `<onchainos_home>/autotrade/latch/<jobId>/<deliveryId>`.
fn latch_path(job_id: &str, delivery_id: &str) -> Result<PathBuf, AutoTradeError> {
    let home = crate::home::onchainos_home()
        .map_err(|_| AutoTradeError::Degrade(DegradeReason::LatchWriteFail))?;
    Ok(home
        .join("autotrade")
        .join("latch")
        .join(job_id)
        .join(delivery_id))
}

/// Idempotency latch: atomically create the marker (`create_new`) BEFORE returning
/// the card. An existing marker ⇒ `replay_skip`; any other IO error ⇒
/// `latch_write_fail`. Worst case = miss an execution, never a double.
fn write_latch(job_id: &str, delivery_id: &str) -> Result<(), AutoTradeError> {
    let path = latch_path(job_id, delivery_id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|_| AutoTradeError::Degrade(DegradeReason::LatchWriteFail))?;
    }
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
            Err(AutoTradeError::Degrade(DegradeReason::ReplaySkip))
        }
        Err(_) => Err(AutoTradeError::Degrade(DegradeReason::LatchWriteFail)),
    }
}

/// The venue + action + spend amount used for the grant-check.
struct GrantTarget {
    venue: &'static str,
    action: &'static str,
    amount: String,
}

/// Result of grant-target resolution: the optional grant to enforce plus the
/// pipeline-resolved absolute dex amount to embed in the command.
///
/// `grant` is `None` when the action is intentionally NOT grant-gated (dex sell —
/// L1; defi withdraw/claim — spend nothing). `resolved_dex_amount` is `Some` only
/// for dex sell+pct, where the pct is converted against on-chain holdings; it is
/// deliberately independent of `grant` so the command still carries an absolute
/// amount even though dex sell is not cap-checked.
struct GrantResolution {
    grant: Option<GrantTarget>,
    resolved_dex_amount: Option<String>,
}

/// Determine the grant-check target + resolved command amount. For `dex_trade`
/// sell+pct this resolves the on-chain holding (FR-7) so the command carries the
/// absolute amount. Withdraw / claim spend nothing, so they are not grant-gated.
///
/// L1 (WBW-13715): dex SELL is NOT grant-gated. A sell is naturally capped by the
/// position held, so v1 configures no `maxSell`; gating it would let a maxBuy-only
/// provisioning silently kill copy-sells. The sell+pct holding resolution is kept
/// (it feeds the command), only the cap check is dropped for sells.
async fn resolve_grant_target(
    typed: &TypedParams,
    wallet: &str,
) -> Result<GrantResolution, AutoTradeError> {
    match typed {
        TypedParams::Dex(p) => match (p.side, p.amount_unit) {
            (Side::Buy, _) => Ok(GrantResolution {
                grant: Some(GrantTarget {
                    venue: "dex",
                    action: "buy",
                    amount: p.amount.clone(),
                }),
                resolved_dex_amount: None,
            }),
            // dex sell — not grant-gated (L1); no pct resolution needed for +base.
            (Side::Sell, AmountUnit::Base) => Ok(GrantResolution {
                grant: None,
                resolved_dex_amount: None,
            }),
            // dex sell+pct — not grant-gated (L1), but still resolve the holding so
            // the executed command embeds an absolute amount.
            (Side::Sell, AmountUnit::Pct) => {
                let pct = Decimal::parse(&p.amount)
                    .map_err(|_| AutoTradeError::Degrade(DegradeReason::PctHoldingFail))?;
                let abs =
                    holding::resolve_pct_holding(wallet, &p.chain_index, &p.token_address, &pct)
                        .await?;
                Ok(GrantResolution {
                    grant: None,
                    resolved_dex_amount: Some(abs.to_plain_string()),
                })
            }
            // schema rejects sell+quote; unreachable, treat defensively.
            (Side::Sell, AmountUnit::Quote) => {
                Err(AutoTradeError::Degrade(DegradeReason::StructureReject))
            }
        },
        TypedParams::Defi(p) => match p.action {
            DefiAction::Deposit => Ok(GrantResolution {
                grant: Some(GrantTarget {
                    venue: "defi",
                    action: "buy",
                    amount: p.amount.clone().unwrap_or_default(),
                }),
                resolved_dex_amount: None,
            }),
            // withdraw / claim spend nothing → not grant-gated.
            DefiAction::Withdraw | DefiAction::Claim => Ok(GrantResolution {
                grant: None,
                resolved_dex_amount: None,
            }),
        },
        TypedParams::Polymarket(p) => {
            let action = match p.side {
                Side::Buy => "buy",
                Side::Sell => "sell",
            };
            Ok(GrantResolution {
                grant: Some(GrantTarget {
                    venue: "polymarket",
                    action,
                    amount: p.amount.clone(),
                }),
                resolved_dex_amount: None,
            })
        }
    }
}

/// The success shapes `run_inner` can produce (before degrade/reject → Notify).
enum RunOk {
    Card(ExecutionCard),
    Decision(DecisionRequest),
}

/// Run the full fixed-order chain. Returns `Ok(outcome)`; the caller maps the
/// [`PipelineOutcome`] onto `output::success`.
pub async fn run(input: PipelineInput<'_>) -> PipelineOutcome {
    match run_inner(&input).await {
        Ok(RunOk::Card(card)) => PipelineOutcome::Card(Box::new(card)),
        Ok(RunOk::Decision(d)) => PipelineOutcome::Decision(Box::new(d)),
        Err(AutoTradeError::Degrade(reason)) => {
            PipelineOutcome::Notify(card::make_notify_only(input.saved_path, reason.as_str()))
        }
        // A structural reject on the inbound path is surfaced as a notify-only
        // with the stable `structure_reject` reason (reject, not silent ignore).
        Err(AutoTradeError::Reject(_)) => PipelineOutcome::Notify(card::make_notify_only(
            input.saved_path,
            DegradeReason::StructureReject.as_str(),
        )),
    }
}

/// Entry charset guard for `job_id` (defense-in-depth). Pure so it is unit-testable
/// without the async network chain. `job_id` is system-issued (not ASP-controlled)
/// but is interpolated into the latch path
/// `<home>/autotrade/latch/<jobId>/<deliveryId>` for ALL types — including defi
/// withdraw/claim, which skip the grant check where `job_id` is otherwise validated.
/// Locking it here, identical to `deliveryId`'s charset, closes the path-traversal
/// surface for every path.
fn check_job_id(job_id: &str) -> Result<(), AutoTradeError> {
    if super::grants::job_id_is_safe(job_id) {
        Ok(())
    } else {
        Err(AutoTradeError::Degrade(DegradeReason::InvalidJobId))
    }
}

async fn run_inner(input: &PipelineInput<'_>) -> Result<RunOk, AutoTradeError> {
    // 0. jobId charset (defense-in-depth; before any filesystem/path use).
    check_job_id(input.job_id)?;

    // 1. structure.
    let signal: AutoTradeSignal = serde_json::from_str(input.signal_json)
        .map_err(|e| AutoTradeError::Reject(format!("signal parse: {e}")))?;
    schema::validate_structure(&signal)?;

    // 2. freshness (AC-4: min(signalTime, receivedAt) vs ttl).
    check_freshness(
        signal.signal_time,
        input.received_at_ms,
        now_ms(),
        signal.ttl_sec,
    )?;

    // 3. subscription Active / copyTrade.
    let mut client = super::super::network::task_api_client::TaskApiClient::new();
    let sub = subscription::determine_copy_trade(&mut client, input.job_id, input.agent_id).await?;

    // 5. type enabled (before touching params for the command).
    if !signal.signal_type.is_enabled() {
        return Err(AutoTradeError::Degrade(DegradeReason::TypeDegrade));
    }
    let typed = schema::parse_and_validate(&signal)?;

    // 4. wallet present (needs the signal's chain to pick the right address).
    let chain_index = signal_chain_index(&typed);
    let wallet = match chain_index {
        Some(idx) => resolve_active_wallet_address(idx)
            .ok_or(AutoTradeError::Degrade(DegradeReason::NoActiveWallet))?,
        // polymarket has no on-chain chain argument; still require an active wallet.
        None => resolve_active_wallet_address("1")
            .ok_or(AutoTradeError::Degrade(DegradeReason::NoActiveWallet))?,
    };

    // 6. resolve the command target (buy-side venue/action/amount + dex sell+pct
    // holding). Dex sell / defi withdraw+claim spend nothing (grant == None).
    let resolution = resolve_grant_target(&typed, &wallet).await?;
    let sig_type = signal.signal_type.as_str();
    // A buy's quote spend (U), for the success-notification amount. `None` for sells / no-spend
    // actions (they carry no cap-relevant amount).
    let buy_amt_str = resolution
        .grant
        .as_ref()
        .filter(|g| g.action == "buy")
        .map(|g| g.amount.clone());

    // Consent override: `autotrade-consent-set` is replaying a signal the user JUST
    // explicitly approved via the three-way decision — option B ("execute this one, ask
    // me each time after"). The user consented to THIS delivery, so skip the consent gate
    // + cap and execute it once (the stored `Manual` mode governs FUTURE signals).
    // `assemble_command_manual` keeps polymarket off the auto grant-check so this one-off
    // runs without a standing grant. Structure / freshness / subscription / wallet were
    // enforced above, so a stale signal / ended subscription still degrades safely.
    if input.consent_override {
        let resolved = resolution.resolved_dex_amount.as_deref();
        let command = card::assemble_command_manual(&typed, &wallet, input.job_id, resolved)?;
        // Plugin-install gate: a command needing an un-approved plugin defers to a
        // user-visible install decision instead of a silent (invisible) sub-side install.
        if let Some(plugin) = card::plugin_dependency(&command) {
            if !consent::plugin_approved(input.job_id, &plugin) {
                return Ok(RunOk::Decision(card::make_plugin_install_decision(
                    &signal.delivery_id,
                    sig_type,
                    input.job_id,
                    input.agent_id,
                    &plugin,
                )));
            }
        }
        write_latch(input.job_id, &signal.delivery_id)?;
        return Ok(RunOk::Card(card::make_execution_card(
            &signal.delivery_id,
            sig_type,
            &sub.provider_agent_id,
            command,
            buy_amt_str.clone(),
            None, // manual one-shot (B): no standing cap, so no "within limit" / pause line
        )));
    }

    // 7. CLIENT CONSENT gate (product 2026-07-17). The per-trade cap now lives in the
    // consent record (not a per-venue grant file). Cap-relevant amount = a buy's
    // quote/U spend; sells and no-spend actions are never cap-gated (they pass `None`).
    let buy_amount = match resolution.grant.as_ref().filter(|g| g.action == "buy") {
        Some(g) => Some(
            Decimal::parse(&g.amount)
                .map_err(|_| AutoTradeError::Degrade(DegradeReason::StructureReject))?,
        ),
        None => None,
    };
    match consent::evaluate_consent(input.job_id, buy_amount.as_ref())
        .map_err(|e| AutoTradeError::Degrade(DegradeReason::ConsentInvalid(e.0)))?
    {
        // First-time / manual (B) / declined (C) ⇒ push the three-way decision; the caller
        // stashes the signal so a follow-up consent-set can replay it (B executes this one,
        // C does not). PRD ③: while auto-execute is NOT enabled, EVERY signal re-prompts the
        // three-way card. B ("execute once, then ask every time") and C ("don't execute THIS
        // one") both leave auto off, so each subsequent signal re-asks — neither is a persistent
        // manual/decline mode. Only A (auto+cap) persists and auto-executes.
        consent::ConsentDecision::FirstTime
        | consent::ConsentDecision::Manual
        | consent::ConsentDecision::Declined => Ok(RunOk::Decision(card::make_first_time_decision(
            &signal.delivery_id,
            sig_type,
            input.job_id,
            input.agent_id,
        ))),
        // Auto but this buy exceeds the cap ⇒ re-ask (raise cap / skip this one).
        consent::ConsentDecision::AutoOverCap => {
            let cap = consent::load_consent(input.job_id)
                .ok()
                .flatten()
                .and_then(|c| c.cap_u)
                .unwrap_or_default();
            let amount = buy_amount
                .as_ref()
                .map(|d| d.to_plain_string())
                .unwrap_or_default();
            Ok(RunOk::Decision(card::make_over_cap_decision(
                &signal.delivery_id,
                sig_type,
                input.job_id,
                input.agent_id,
                &amount,
                &cap,
            )))
        }
        // Auto within cap ⇒ auto-execute.
        consent::ConsentDecision::AutoAllow => {
            // Defense-in-depth: consent already enforced the cap, but re-check the grant
            // file (written by `autotrade-consent-set`) so the in-process cap and the
            // out-of-process plugin `autotrade-grant-check` agree. Fail-closed; the
            // specific grant-deny reason is preserved for the notify + audit.
            if let Some(gt) = resolution.grant.as_ref().filter(|g| g.action == "buy") {
                super::grants::check_grant(input.job_id, gt.venue, gt.action, &gt.amount)
                    .map_err(|d| AutoTradeError::Degrade(DegradeReason::GrantDenied(d.code())))?;
            }
            // Assemble before latching (a build failure must not burn the latch).
            let resolved = resolution.resolved_dex_amount.as_deref();
            let command = card::assemble_command(&typed, &wallet, input.job_id, resolved)?;
            // Plugin-install gate: an un-approved plugin defers to a user-visible install
            // decision (the sub must not silently install; compliance + invisibility).
            if let Some(plugin) = card::plugin_dependency(&command) {
                if !consent::plugin_approved(input.job_id, &plugin) {
                    return Ok(RunOk::Decision(card::make_plugin_install_decision(
                        &signal.delivery_id,
                        sig_type,
                        input.job_id,
                        input.agent_id,
                        &plugin,
                    )));
                }
            }
            // The auto per-trade cap in effect (U) — feeds the success-notification "within
            // limit" clause + the pause line. Same load pattern as the AutoOverCap branch.
            let cap = consent::load_consent(input.job_id)
                .ok()
                .flatten()
                .and_then(|c| c.cap_u);
            write_latch(input.job_id, &signal.delivery_id)?;
            Ok(RunOk::Card(card::make_execution_card(
                &signal.delivery_id,
                sig_type,
                &sub.provider_agent_id,
                command,
                buy_amt_str.clone(),
                cap,
            )))
        }
    }
}

/// The chain index that governs the executed command's chain flag, if any.
fn signal_chain_index(typed: &TypedParams) -> Option<&str> {
    match typed {
        TypedParams::Dex(p) => Some(&p.chain_index),
        TypedParams::Defi(p) => p.chain_index.as_deref(),
        TypedParams::Polymarket(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ac4_freshness_uses_min_of_signal_and_received() {
        let ttl = 60u64; // 60s
        let now = 1_000_000_000_000u64;
        // signalTime is fresh but receivedAt is old → min(old) ⇒ expired.
        let signal_time = now - 10_000; // 10s ago
        let received_at = now - 120_000; // 120s ago
        assert!(check_freshness(signal_time, received_at, now, ttl).is_err());
        // both fresh ⇒ ok
        assert!(check_freshness(now - 5_000, now - 5_000, now, ttl).is_ok());
        // exactly at the boundary (age == ttl) ⇒ ok
        assert!(check_freshness(now - 60_000, now - 60_000, now, ttl).is_ok());
        // one ms past ⇒ expired
        assert!(check_freshness(now - 60_001, now - 60_001, now, ttl).is_err());
    }

    #[test]
    fn freshness_future_timestamp_is_fresh() {
        let now = 1_000u64;
        assert!(check_freshness(now + 5_000, now + 5_000, now, 1).is_ok());
    }

    #[test]
    fn ac6_latch_marker_before_card_and_replay_skip() {
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("ONCHAINOS_HOME", tmp.path());

        // First write succeeds and the marker exists on disk.
        assert!(write_latch("job1", "d-1").is_ok());
        let marker = latch_path("job1", "d-1").unwrap();
        assert!(
            marker.exists(),
            "marker must be written before the card is returned"
        );

        // Second write for the same (job, delivery) ⇒ replay_skip.
        assert!(matches!(
            write_latch("job1", "d-1"),
            Err(AutoTradeError::Degrade(DegradeReason::ReplaySkip))
        ));

        // A different deliveryId is independent.
        assert!(write_latch("job1", "d-2").is_ok());

        std::env::remove_var("ONCHAINOS_HOME");
    }

    // ── FR-4: jobId entry charset guard (path-traversal defense) ──
    #[test]
    fn job_id_charset_guard_at_entry() {
        // Safe ids pass.
        assert!(check_job_id("job1").is_ok());
        assert!(check_job_id("JOB_9-abc").is_ok());
        // Traversal / illegal chars degrade to invalid_job_id (not a filesystem touch).
        for bad in ["../../etc/passwd", "job/1", "", "job.1", "job 1", "a/b"] {
            assert!(
                matches!(
                    check_job_id(bad),
                    Err(AutoTradeError::Degrade(DegradeReason::InvalidJobId))
                ),
                "should reject job_id={bad}"
            );
        }
        assert_eq!(DegradeReason::InvalidJobId.as_str(), "invalid_job_id");
    }

    // ── FR-8: grant-deny reason is preserved, not collapsed onto over_cap ──
    #[test]
    fn grant_denied_reason_flows_to_notify() {
        use super::super::grants::{GrantDeny, DENY_EXPIRED, DENY_NO_GRANT_FILE};
        // The pipeline maps a GrantDeny to DegradeReason::GrantDenied(code); that code
        // is what make_notify_only surfaces as the notify-only reason.
        let expired = DegradeReason::GrantDenied(GrantDeny(DENY_EXPIRED).code());
        assert_eq!(expired.as_str(), "grant_expired");
        let notify = card::make_notify_only("/tmp/x", expired.as_str());
        assert_eq!(notify.reason, "grant_expired");

        let no_file = DegradeReason::GrantDenied(GrantDeny(DENY_NO_GRANT_FILE).code());
        assert_eq!(no_file.as_str(), "no_grant_file");
        // And it is distinct from the old blanket over_cap.
        assert_ne!(no_file.as_str(), DegradeReason::OverCap.as_str());
    }

    // ── L1 (WBW-13715): dex sell is NOT grant-gated; buy/defi-deposit still are ──
    fn dex_typed(params: serde_json::Value) -> schema::TypedParams {
        use super::schema::{parse_and_validate, AutoTradeSignal, SignalType};
        let sig = AutoTradeSignal {
            schema_version: 1,
            delivery_id: "d1".into(),
            signal_type: SignalType::DexTrade,
            signal_time: 1,
            ttl_sec: 60,
            params,
        };
        parse_and_validate(&sig).unwrap()
    }

    #[tokio::test]
    async fn dex_sell_base_is_not_grant_gated() {
        let typed = dex_typed(serde_json::json!({
            "chainIndex": "8453", "tokenAddress": "0xToken",
            "side": "sell", "amount": "25", "amountUnit": "base"
        }));
        let res = resolve_grant_target(&typed, "0xBuyer").await.unwrap();
        assert!(res.grant.is_none(), "dex sell must not be grant-gated (L1)");
        assert!(
            res.resolved_dex_amount.is_none(),
            "sell+base needs no pct holding resolution"
        );
    }

    #[tokio::test]
    async fn dex_buy_is_grant_gated() {
        let typed = dex_typed(serde_json::json!({
            "chainIndex": "8453", "tokenAddress": "0xToken",
            "side": "buy", "amount": "25", "amountUnit": "quote", "slippageBps": 500
        }));
        let res = resolve_grant_target(&typed, "0xBuyer").await.unwrap();
        let gt = res.grant.expect("dex buy is grant-gated");
        assert_eq!(gt.venue, "dex");
        assert_eq!(gt.action, "buy");
        assert_eq!(gt.amount, "25");
    }
}
