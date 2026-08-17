//! Synchronous foreground handling for free-form replies to the auto-trade
//! consent cards.
//!
//! The foreground model extracts a small candidate JSON object; this module is
//! the trust boundary that validates it, persists incomplete configuration as a
//! draft, and writes consent + grants before the resolver returns. It never
//! parses the user's natural-language reply and never executes a delivery.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use super::amount::Decimal;
use super::card;
use super::consent::{self, ConsentMode, DeliveryContext};
use super::grants;
use crate::commands::agent_commerce::task::common::pending_v2;

const DRAFT_VERSION: u32 = 1;
// Match the pending-decision lifetime. Signal freshness is re-validated by the
// subscription session; expiring this configuration earlier would lose the
// selected A/B mode before a delayed but still valid user reply arrives.
const DRAFT_TTL_SEC: u64 = 7 * 24 * 60 * 60;
const CONSENT_TTL_SEC: u64 = 31_536_000;

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum CandidateMode {
    Auto,
    Manual,
    Decline,
}

impl CandidateMode {
    fn consent_mode(self) -> ConsentMode {
        match self {
            Self::Auto => ConsentMode::Auto,
            Self::Manual => ConsentMode::Manual,
            Self::Decline => ConsentMode::Decline,
        }
    }
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum CandidateField {
    Mode,
    #[serde(alias = "tradeAmount")]
    TradeAmount,
    Cap,
    Quote,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CandidateInput {
    #[serde(default)]
    mode: Option<CandidateMode>,
    #[serde(default)]
    trade_amount: Option<String>,
    #[serde(default)]
    cap: Option<String>,
    #[serde(default)]
    quote: Option<String>,
    #[serde(default)]
    ambiguous_fields: Vec<CandidateField>,
    #[serde(default)]
    confirm: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PendingConfig {
    version: u32,
    job_id: String,
    agent_id: String,
    delivery_id: String,
    mode: ConsentMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    trade_amount_u: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cap_u: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    quote_token: Option<String>,
    #[serde(default)]
    confirmation_required: bool,
    created_at: u64,
    expires_at: u64,
}

pub(crate) enum ApplyResult {
    /// Compatibility escape hatch for a stale card created before foreground
    /// drafts existed. The resolver relays the original wording unchanged.
    FallbackRelay,
    /// A missing-field or canonical-confirmation card has already been pushed.
    Awaiting(serde_json::Value),
    /// Policy is on disk. Relay only this normalized reply to resume the saved
    /// delivery in the subscription session.
    Relay {
        normalized_reply: String,
        outcome: serde_json::Value,
    },
}

pub(crate) fn is_candidate_source(source_event: &str) -> bool {
    matches!(
        source_event,
        card::CONSENT_SOURCE_EVENT | card::CONFIG_REQUIRED_SOURCE_EVENT
    )
}

pub(crate) fn apply_candidate_json(
    job_id: &str,
    agent_id: &str,
    source_event: &str,
    candidate_json: &str,
) -> Result<ApplyResult> {
    let target =
        consent::load_pending_delivery_context(job_id)?.map(|context| context.provider_agent_id);
    apply_candidate_json_with(job_id, agent_id, source_event, candidate_json, |decision| {
        pending_v2::push_decision_direct(
            job_id,
            "user",
            agent_id,
            target.as_deref(),
            &decision.user_content,
            &card::decision_list_label(decision),
            &decision.source_event,
        )
    })
}

fn apply_candidate_json_with<F>(
    job_id: &str,
    agent_id: &str,
    source_event: &str,
    candidate_json: &str,
    push: F,
) -> Result<ApplyResult>
where
    F: Fn(&card::DecisionRequest) -> Result<()>,
{
    if !is_candidate_source(source_event) {
        bail!("auto-trade candidate JSON is not valid for this decision type");
    }
    let candidate: CandidateInput = serde_json::from_str(candidate_json)
        .map_err(|e| anyhow::anyhow!("invalid auto-trade candidate JSON: {e}"))?;
    let context = match consent::load_pending_delivery_context(job_id)? {
        Some(context) => context,
        None => return Ok(ApplyResult::FallbackRelay),
    };
    if context.agent_id != agent_id {
        bail!("auto-trade candidate agent does not match pending delivery");
    }

    let mut draft = load_draft(job_id, agent_id, &context)?;
    if source_event == card::CONFIG_REQUIRED_SOURCE_EVENT && draft.is_none() {
        // Cards issued by an older binary have no foreground draft. Preserve
        // their old background-session path instead of guessing prior state.
        return Ok(ApplyResult::FallbackRelay);
    }

    if let Some(mode) = candidate.mode {
        let mode = mode.consent_mode();
        let changed_mode = draft.as_ref().is_some_and(|saved| saved.mode != mode);
        if changed_mode {
            draft = None;
        }
        if draft.is_none() {
            let now = now_secs();
            draft = Some(PendingConfig {
                version: DRAFT_VERSION,
                job_id: job_id.to_string(),
                agent_id: agent_id.to_string(),
                delivery_id: context.delivery_id.clone(),
                mode,
                trade_amount_u: None,
                cap_u: None,
                quote_token: None,
                confirmation_required: false,
                created_at: now,
                expires_at: now.saturating_add(DRAFT_TTL_SEC),
            });
        }
    }

    let Some(mut draft) = draft else {
        let decision =
            card::make_first_time_decision(&context.delivery_id, "trade", job_id, agent_id);
        return Ok(awaiting_outcome("mode_required", None, &decision, &push));
    };

    merge_candidate(&mut draft, &candidate)?;
    if !candidate.ambiguous_fields.is_empty() {
        draft.confirmation_required = true;
    } else if candidate.confirm {
        draft.confirmation_required = false;
    }

    let missing_fields = missing_fields(&draft);
    if !missing_fields.is_empty() {
        write_draft(&draft)?;
        let decision =
            card::make_consent_input_required_decision(job_id, agent_id, mode_token(draft.mode));
        return Ok(awaiting_outcome(
            "need_fields",
            Some(serde_json::json!({
                "draftPersisted": true,
                "missingFields": missing_fields,
                "authorizationPersisted": false,
            })),
            &decision,
            &push,
        ));
    }

    validate_policy(&draft)?;
    if draft.confirmation_required {
        write_draft(&draft)?;
        let decision = card::make_consent_confirmation_decision(
            job_id,
            agent_id,
            mode_token(draft.mode),
            draft.trade_amount_u.as_deref(),
            draft.cap_u.as_deref(),
            draft.quote_token.as_deref().unwrap_or("usdt"),
        );
        return Ok(awaiting_outcome(
            "confirmation_required",
            Some(serde_json::json!({
                "draftPersisted": true,
                "authorizationPersisted": false,
                "normalizedCandidate": normalized_candidate(&draft),
            })),
            &decision,
            &push,
        ));
    }

    if draft.mode == ConsentMode::Decline {
        return Ok(ApplyResult::Relay {
            normalized_reply: "C".to_string(),
            outcome: serde_json::json!({
                "status": "skipped",
                "jobId": job_id,
                "consentMode": "decline",
                "authorizationPersisted": false,
                "pendingDeliveryPreserved": true,
            }),
        });
    }

    persist_policy(job_id, &draft)?;
    let normalized_reply = normalized_reply(&draft);
    Ok(ApplyResult::Relay {
        normalized_reply,
        outcome: serde_json::json!({
            "status": "persisted",
            "jobId": job_id,
            "consentMode": mode_token(draft.mode),
            "authorizationPersisted": true,
            "pendingDeliveryPreserved": true,
            "normalizedPolicy": normalized_candidate(&draft),
        }),
    })
}

fn merge_candidate(draft: &mut PendingConfig, candidate: &CandidateInput) -> Result<()> {
    match draft.mode {
        ConsentMode::Auto => {
            if let Some(amount) = candidate.trade_amount.as_deref() {
                draft.trade_amount_u = Some(normalize_positive_decimal(amount, "tradeAmount")?);
            }
            if let Some(cap) = candidate.cap.as_deref() {
                draft.cap_u = Some(normalize_positive_decimal(cap, "cap")?);
            }
        }
        ConsentMode::Manual => {
            if candidate.cap.is_some() {
                bail!("cap is only valid for auto mode");
            }
            if let Some(amount) = candidate.trade_amount.as_deref() {
                draft.trade_amount_u = Some(normalize_positive_decimal(amount, "tradeAmount")?);
            }
            draft.cap_u = None;
        }
        ConsentMode::Decline => {
            if candidate.trade_amount.is_some()
                || candidate.cap.is_some()
                || candidate.quote.is_some()
            {
                bail!("decline mode does not accept amount, cap, or quote");
            }
            draft.trade_amount_u = None;
            draft.cap_u = None;
            draft.quote_token = None;
        }
    }
    if draft.mode != ConsentMode::Decline {
        if let Some(quote) = candidate.quote.as_deref() {
            let quote = quote.to_ascii_lowercase();
            if !consent::QUOTE_WHITELIST.contains(&quote.as_str()) {
                bail!("quote must be one of: usdt | usdc");
            }
            draft.quote_token = Some(quote);
        } else if draft.quote_token.is_none() {
            draft.quote_token = Some("usdt".to_string());
        }
    }
    Ok(())
}

fn missing_fields(draft: &PendingConfig) -> Vec<&'static str> {
    match draft.mode {
        ConsentMode::Auto => {
            let mut missing = Vec::new();
            if draft.trade_amount_u.is_none() {
                missing.push("trade_amount");
            }
            if draft.cap_u.is_none() {
                missing.push("cap");
            }
            missing
        }
        ConsentMode::Manual => {
            if draft.trade_amount_u.is_none() {
                vec!["trade_amount"]
            } else {
                Vec::new()
            }
        }
        ConsentMode::Decline => Vec::new(),
    }
}

fn validate_policy(draft: &PendingConfig) -> Result<()> {
    if draft.mode == ConsentMode::Auto {
        let amount = Decimal::parse(draft.trade_amount_u.as_deref().unwrap_or_default())?;
        let cap = Decimal::parse(draft.cap_u.as_deref().unwrap_or_default())?;
        if !amount.le(&cap) {
            bail!("tradeAmount must not exceed cap");
        }
    }
    Ok(())
}

fn persist_policy(job_id: &str, draft: &PendingConfig) -> Result<()> {
    if let Some(existing) = consent::load_consent(job_id).map_err(|e| anyhow::anyhow!(e.0))? {
        let same = existing.mode == draft.mode
            && existing.trade_amount_u == draft.trade_amount_u
            && existing.cap_u == draft.cap_u
            && existing.quote_token == draft.quote_token;
        if same {
            let remaining_ttl = existing.expires_at.saturating_sub(now_secs()).max(1);
            match draft.mode {
                ConsentMode::Auto => grants::write_cap_grant(
                    job_id,
                    draft.cap_u.as_deref().unwrap_or_default(),
                    remaining_ttl,
                )?,
                ConsentMode::Manual | ConsentMode::Decline => grants::clear_grant(job_id),
            }
            return Ok(());
        }
        bail!("auto-trade consent changed while this decision was pending");
    }

    consent::write_consent_with_trade_amount(
        job_id,
        draft.mode,
        draft.cap_u.as_deref(),
        draft.trade_amount_u.as_deref(),
        draft.quote_token.as_deref(),
        CONSENT_TTL_SEC,
    )?;
    match draft.mode {
        ConsentMode::Auto => {
            if let Err(err) = grants::write_cap_grant(
                job_id,
                draft.cap_u.as_deref().unwrap_or_default(),
                CONSENT_TTL_SEC,
            ) {
                consent::clear_consent(job_id);
                grants::clear_grant(job_id);
                return Err(err);
            }
        }
        ConsentMode::Manual | ConsentMode::Decline => grants::clear_grant(job_id),
    }
    Ok(())
}

fn awaiting_outcome<F>(
    status: &str,
    details: Option<serde_json::Value>,
    decision: &card::DecisionRequest,
    push: &F,
) -> ApplyResult
where
    F: Fn(&card::DecisionRequest) -> Result<()>,
{
    let pushed = push(decision).is_ok();
    let mut outcome = serde_json::json!({
        "status": status,
        "decisionPushed": pushed,
    });
    if let Some(details) = details {
        if let (Some(target), Some(source)) = (outcome.as_object_mut(), details.as_object()) {
            target.extend(source.clone());
        }
    }
    if !pushed {
        outcome["decisionRequest"] = serde_json::to_value(decision).unwrap_or_default();
    }
    ApplyResult::Awaiting(outcome)
}

fn normalized_candidate(draft: &PendingConfig) -> serde_json::Value {
    serde_json::json!({
        "mode": mode_token(draft.mode),
        "tradeAmount": draft.trade_amount_u,
        "cap": draft.cap_u,
        "quote": draft.quote_token,
    })
}

fn normalized_reply(draft: &PendingConfig) -> String {
    let quote = draft
        .quote_token
        .as_deref()
        .unwrap_or("usdt")
        .to_ascii_uppercase();
    match draft.mode {
        ConsentMode::Auto => format!(
            "A; fixed amount {} {}; per-trade cap {} {}",
            draft.trade_amount_u.as_deref().unwrap_or_default(),
            quote,
            draft.cap_u.as_deref().unwrap_or_default(),
            quote,
        ),
        ConsentMode::Manual => format!(
            "B; one-time amount {} {}",
            draft.trade_amount_u.as_deref().unwrap_or_default(),
            quote,
        ),
        ConsentMode::Decline => "C".to_string(),
    }
}

fn normalize_positive_decimal(value: &str, field: &str) -> Result<String> {
    let parsed = Decimal::parse(value)
        .map_err(|_| anyhow::anyhow!("{field} must be a positive decimal string"))?;
    if parsed.is_zero() {
        bail!("{field} must be greater than 0");
    }
    Ok(parsed.to_plain_string())
}

fn mode_token(mode: ConsentMode) -> &'static str {
    match mode {
        ConsentMode::Auto => "auto",
        ConsentMode::Manual => "manual",
        ConsentMode::Decline => "decline",
    }
}

fn draft_path(job_id: &str) -> Result<std::path::PathBuf> {
    if !grants::job_id_is_safe(job_id) {
        bail!("invalid job id");
    }
    Ok(crate::home::onchainos_home()?
        .join("autotrade")
        .join("pending-config")
        .join(format!("{job_id}.json")))
}

fn load_draft(
    job_id: &str,
    agent_id: &str,
    context: &DeliveryContext,
) -> Result<Option<PendingConfig>> {
    let path = draft_path(job_id)?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read(&path)?;
    let draft: PendingConfig = serde_json::from_slice(&raw)?;
    if draft.version != DRAFT_VERSION
        || draft.job_id != job_id
        || draft.agent_id != agent_id
        || draft.delivery_id != context.delivery_id
        || draft.expires_at <= now_secs()
    {
        let _ = std::fs::remove_file(path);
        return Ok(None);
    }
    Ok(Some(draft))
}

fn write_draft(draft: &PendingConfig) -> Result<()> {
    let path = draft_path(&draft.job_id)?;
    crate::home::write_secure(&path, &serde_json::to_vec_pretty(draft)?)?;
    Ok(())
}

/// Remove the foreground draft only after the normalized relay has been queued.
/// Keeping it across a relay failure makes a retry idempotent.
pub(crate) fn clear_candidate_draft(job_id: &str) {
    if let Ok(path) = draft_path(job_id) {
        let _ = std::fs::remove_file(path);
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_home<F: FnOnce()>(f: F) {
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let tmp = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
        std::env::set_var("ONCHAINOS_HOME", tmp.path());
        register_context();
        f();
        std::env::remove_var("ONCHAINOS_HOME");
    }

    fn register_context() {
        consent::register_delivery_context(
            "job1",
            "8315",
            "8779",
            None,
            "delivery-1",
            "/tmp/delivery.txt",
            "text",
            1,
        )
        .unwrap();
        consent::activate_delivery_context("job1", "delivery-1").unwrap();
    }

    fn apply(source: &str, json: &str) -> Result<ApplyResult> {
        apply_candidate_json_with("job1", "8315", source, json, |_| Ok(()))
    }

    #[test]
    fn incomplete_auto_reply_persists_draft_then_completes_synchronously() {
        with_home(|| {
            let first = apply(
                card::CONSENT_SOURCE_EVENT,
                r#"{"mode":"auto","tradeAmount":"1","quote":"USDT"}"#,
            )
            .unwrap();
            assert!(matches!(first, ApplyResult::Awaiting(_)));
            assert!(draft_path("job1").unwrap().exists());
            assert!(consent::load_consent("job1").unwrap().is_none());

            let second = apply(card::CONFIG_REQUIRED_SOURCE_EVENT, r#"{"cap":"10"}"#).unwrap();
            assert!(matches!(second, ApplyResult::Relay { .. }));
            let saved = consent::load_consent("job1").unwrap().unwrap();
            assert_eq!(saved.mode, ConsentMode::Auto);
            assert_eq!(saved.trade_amount_u.as_deref(), Some("1"));
            assert_eq!(saved.cap_u.as_deref(), Some("10"));
            assert_eq!(saved.quote_token.as_deref(), Some("usdt"));
            assert!(grants::check_grant("job1", "dex", "buy", "10").is_ok());
            assert!(consent::load_pending_delivery_context("job1")
                .unwrap()
                .is_some());
            clear_candidate_draft("job1");
            assert!(!draft_path("job1").unwrap().exists());
        });
    }

    #[test]
    fn ambiguous_complete_candidate_requires_confirmation_before_write() {
        with_home(|| {
            let first = apply(
                card::CONSENT_SOURCE_EVENT,
                r#"{"mode":"auto","tradeAmount":"1","cap":"10","ambiguousFields":["cap"]}"#,
            )
            .unwrap();
            assert!(matches!(first, ApplyResult::Awaiting(_)));
            assert!(consent::load_consent("job1").unwrap().is_none());

            let second = apply(card::CONFIG_REQUIRED_SOURCE_EVENT, r#"{"confirm":true}"#).unwrap();
            assert!(matches!(second, ApplyResult::Relay { .. }));
            assert!(consent::load_consent("job1").unwrap().is_some());
        });
    }

    #[test]
    fn ambiguity_survives_missing_fields_until_explicit_confirmation() {
        with_home(|| {
            assert!(matches!(
                apply(
                    card::CONSENT_SOURCE_EVENT,
                    r#"{"mode":"auto","tradeAmount":"1","ambiguousFields":["mode"]}"#,
                )
                .unwrap(),
                ApplyResult::Awaiting(_)
            ));
            assert!(matches!(
                apply(card::CONFIG_REQUIRED_SOURCE_EVENT, r#"{"cap":"10"}"#,).unwrap(),
                ApplyResult::Awaiting(_)
            ));
            assert!(consent::load_consent("job1").unwrap().is_none());
            assert!(matches!(
                apply(card::CONFIG_REQUIRED_SOURCE_EVENT, r#"{"confirm":true}"#,).unwrap(),
                ApplyResult::Relay { .. }
            ));
            assert!(consent::load_consent("job1").unwrap().is_some());
        });
    }

    #[test]
    fn decline_relays_skip_without_writing_authorization() {
        with_home(|| {
            let result = apply(card::CONSENT_SOURCE_EVENT, r#"{"mode":"decline"}"#).unwrap();
            match result {
                ApplyResult::Relay {
                    normalized_reply,
                    outcome,
                } => {
                    assert_eq!(normalized_reply, "C");
                    assert_eq!(outcome["authorizationPersisted"], false);
                }
                _ => panic!("decline should be ready to relay"),
            }
            assert!(consent::load_consent("job1").unwrap().is_none());
            assert!(grants::check_grant("job1", "dex", "buy", "1").is_err());
        });
    }

    #[test]
    fn retry_repairs_a_missing_grant_for_an_identical_persisted_policy() {
        with_home(|| {
            let candidate = r#"{"mode":"auto","tradeAmount":"1","cap":"10"}"#;
            assert!(matches!(
                apply(card::CONSENT_SOURCE_EVENT, candidate).unwrap(),
                ApplyResult::Relay { .. }
            ));
            grants::clear_grant("job1");
            assert!(grants::check_grant("job1", "dex", "buy", "1").is_err());

            assert!(matches!(
                apply(card::CONSENT_SOURCE_EVENT, candidate).unwrap(),
                ApplyResult::Relay { .. }
            ));
            assert!(grants::check_grant("job1", "dex", "buy", "10").is_ok());
        });
    }

    #[test]
    fn amount_above_cap_is_rejected_without_authorization() {
        with_home(|| {
            let err = apply(
                card::CONSENT_SOURCE_EVENT,
                r#"{"mode":"auto","tradeAmount":"11","cap":"10"}"#,
            )
            .err()
            .unwrap();
            assert!(err.to_string().contains("must not exceed cap"));
            assert!(consent::load_consent("job1").unwrap().is_none());
            assert!(grants::check_grant("job1", "dex", "buy", "1").is_err());
        });
    }

    #[test]
    fn candidate_json_rejects_unknown_or_numeric_fields() {
        with_home(|| {
            assert!(apply(
                card::CONSENT_SOURCE_EVENT,
                r#"{"mode":"auto","tradeAmount":1,"cap":"10"}"#,
            )
            .is_err());
            assert!(apply(
                card::CONSENT_SOURCE_EVENT,
                r#"{"mode":"auto","tradeAmount":"1","cap":"10","extra":true}"#,
            )
            .is_err());
        });
    }
}
