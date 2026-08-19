//! Publish a task (custom signing flow).
//!
//! User action: publish a task — `onchainos agent create-task`.
//!
//! Identity check: invokes the identity-module CLI (`onchainos agent get-my-agents`) to verify
//! that the current user has a user identity (role=1) before running the publish flow.

use anyhow::{bail, Result};
use std::io::Write;
use std::time::Duration;

use crate::audit;

use crate::commands::agent_commerce::task::common::{
    self, fetch_my_agents_by_role, network::task_api_client::TaskApiClient,
    payment_mode::PaymentMode, AGENT_ROLE_USER, DEBUG_LOG, XLAYER_CHAIN_ID,
};
use crate::commands::agent_commerce::task::signing;
use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;

// ─── Constants ───────────────────────────────────────────────────────────

pub const MAX_BUDGET: f64 = 10_000_000.0;
pub const MIN_DESCRIPTION_CHARS: usize = 20;
pub const MAX_DESCRIPTION_CHARS: usize = 2000;
pub const MAX_BUDGET_DECIMALS: usize = 5;
pub const MAX_TITLE_CHARS: usize = 30;

// ─── Parameter struct ────────────────────────────────────────────────────

pub struct CreateTaskParams {
    pub description: String,
    pub budget: f64,
    pub max_budget: f64,
    pub currency: String,
    pub title: Option<String>,
    pub provider: String,
    pub attachments: Option<Vec<String>>,
    pub endpoint: Option<String>,
    pub payment_mode: String,
    pub service_id: String,
    pub service_params: Option<String>,
    pub service_token_address: Option<String>,
    pub service_token_amount: Option<String>,
}

struct ValidatedParams {
    currency: String,
    title: String,
}

impl CreateTaskParams {
    fn validate(&self) -> Result<ValidatedParams> {
        let desc_len = self.description.chars().count();
        if desc_len < MIN_DESCRIPTION_CHARS {
            bail!("description is too short; please add more detail (minimum {MIN_DESCRIPTION_CHARS} chars, currently {desc_len})");
        }
        if desc_len > MAX_DESCRIPTION_CHARS {
            bail!(
                "task description may not exceed {MAX_DESCRIPTION_CHARS} chars (currently {desc_len}); \
                ask the AI to summarize, or shorten it manually and retry."
            );
        }

        let currency = normalize_currency(&self.currency)?;
        validate_budget(self.budget)?;
        validate_budget_decimals(self.budget)?;

        if self.max_budget < self.budget {
            bail!(
                "--max-budget ({}) may not be less than --budget ({})",
                self.max_budget,
                self.budget
            );
        }
        validate_budget(self.max_budget)?;
        validate_budget_decimals(self.max_budget)?;

        let title = match &self.title {
            Some(t) if t.chars().count() > MAX_TITLE_CHARS => {
                t.chars().take(MAX_TITLE_CHARS).collect()
            }
            Some(t) => t.clone(),
            None => self.description.chars().take(MAX_TITLE_CHARS).collect(),
        };
        // Neutralize shell metacharacters after truncation and before the title is accepted —
        // covers both the explicit --title and derive-from-description branches (WBW-14039 FR-3).
        // Sanitization only removes/replaces chars, so the ≤ MAX_TITLE_CHARS invariant still holds.
        let title = common::util::sanitize_title_for_shell(&title);

        if self.provider.trim().is_empty() {
            bail!("A designated provider is required. Use asp-match to find a provider first.");
        }
        if self.service_id.trim().is_empty() {
            bail!("A service id is required. Use asp-match to find a service first.");
        }

        if let Some(ref files) = self.attachments {
            for f in files {
                if !std::path::Path::new(f).exists() {
                    bail!("attachment file not found: {f}");
                }
            }
        }

        Ok(ValidatedParams { currency, title })
    }
}

// ─── Validation helpers ─────────────────────────────────────────────────

pub fn normalize_currency(currency: &str) -> Result<String> {
    let normalized: String = currency
        .chars()
        .map(|c| if c == '₮' { 'T' } else { c })
        .collect::<String>()
        .to_uppercase();
    match normalized.as_str() {
        "USDT" | "USDT0" => Ok("USDT".to_string()),
        "USDG" => Ok("USDG".to_string()),
        _ => bail!("unsupported token: {currency}; only USDT (USD₮0) and USDG are supported"),
    }
}

pub fn validate_budget(budget: f64) -> Result<()> {
    if budget <= 0.0 {
        bail!("budget must be a positive amount (greater than 0)");
    }
    if budget > MAX_BUDGET {
        bail!(
            "per-task budget may not exceed {} USDT/USDG",
            MAX_BUDGET as u64
        );
    }
    Ok(())
}

pub fn validate_budget_decimals(budget: f64) -> Result<()> {
    let s = format!("{budget}");
    if let Some(dot_pos) = s.find('.') {
        let frac = s[dot_pos + 1..].trim_end_matches('0');
        if frac.len() > MAX_BUDGET_DECIMALS {
            bail!(
                "budget precision is limited to {MAX_BUDGET_DECIMALS} decimal places, currently {}",
                frac.len()
            );
        }
    }
    Ok(())
}

// ─── Identity check ─────────────────────────────────────────────────────

pub(crate) async fn resolve_user_agent() -> Result<(String, String)> {
    let agents = fetch_my_agents_by_role("user").await;

    let user = agents.iter()
        .find(|a| a["role"].as_i64() == Some(AGENT_ROLE_USER))
        .ok_or_else(|| anyhow::anyhow!("the current account has no user (requestor) identity; run `onchainos agent create --role user` first"))?;

    let agent_id = user["agentId"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("agent is missing the agentId field"))?
        .to_string();
    let owner_address = user["ownerAddress"].as_str().unwrap_or("").to_string();
    Ok((agent_id, owner_address))
}

// ─── Create task ────────────────────────────────────────────────────────

/// Build the `POST /priapi/v1/aieco/task/create` request body.
///
/// Extracted from `handle_create` so the body shape is reachable from unit
/// tests. The `descriptionSummary` key is deliberately NOT emitted (WBW-14172
/// FR-1.3 / AC-3) — the field was removed from the CLI surface.
fn build_create_body(
    params: &CreateTaskParams,
    validated: &ValidatedParams,
) -> Result<serde_json::Value> {
    let mut body = serde_json::json!({
        "title":              validated.title,
        "description":        params.description,
        "paymentTokenSymbol": validated.currency.to_uppercase(),
        "paymentTokenAmount": params.budget.to_string(),
        "paymentMostTokenAmount": params.max_budget.to_string(),
        "chainId":            XLAYER_CHAIN_ID,
        "paymentMode":        PaymentMode::parse_flag(Some(&params.payment_mode))?,
        // Public task type removed: always private. The backend still owns the
        // visibility field (Phase 2 migration is backend-side), so keep sending
        // the constant private value rather than dropping it from the body.
        "visibility":         1
    });
    body["providerAgentId"] = serde_json::json!(params.provider);
    body["serviceId"] = serde_json::json!(params.service_id);
    if let Some(ref sp) = params.service_params {
        body["serviceParams"] = serde_json::json!(sp);
    }
    if let Some(ref sta) = params.service_token_address {
        body["serviceTokenAddress"] = serde_json::json!(sta);
    }
    if let Some(ref stm) = params.service_token_amount {
        body["serviceTokenAmount"] = serde_json::json!(stm);
    }
    Ok(body)
}

pub async fn handle_create(
    client: &mut TaskApiClient,
    params: CreateTaskParams,
) -> Result<()> {
    let validated = params.validate()?;

    ensure_tokens_refreshed().await.map_err(|e| {
        anyhow::anyhow!("session has expired; run `onchainos wallet login` first: {e}")
    })?;

    let (user_agent_id, _) = resolve_user_agent().await?;
    if DEBUG_LOG {
        eprintln!("[task-create] user identity check passed (agentId: {user_agent_id})");
    }

    // Advisory balance check (non-blocking — the task still on-chains). On an
    // insufficiency we attach a structured `balanceWarning` (FR-1) to the success
    // `data`; `balance_warning_json` resolves the caller's XLayer deposit address
    // and renders the QR to stderr (TTY-only). A non-insufficiency error (e.g.
    // balance-query failure) is not surfaced — create-task is advisory.
    let (balance_warning, balance_msg) =
        match common::ensure_sufficient_balance(params.budget, &validated.currency).await {
            Err(e) => {
                if DEBUG_LOG {
                    let mut err = std::io::stderr();
                    let _ = writeln!(err, "[task-create] ⚠ balance warning: {e}");
                }
                match e.downcast_ref::<common::deposit_qr::InsufficientBalanceError>() {
                    Some(ib) => {
                        let ib_owned = ib.clone();
                        // `balance_warning_json` returns the marker-filled advisory
                        // message alongside the structured warning, so the guidance
                        // text carries the QR position (under option 1) regardless of
                        // the stderr side-effect.
                        let (warning, msg) =
                            common::deposit_qr::balance_warning_json(&ib_owned, &user_agent_id)
                                .await;
                        (Some(warning), Some(msg))
                    }
                    None => (None, None),
                }
            }
            Ok(()) => (None, None),
        };

    let (account_id, address) = signing::resolve_wallet_by_agent_id(&user_agent_id).await?;

    let body = build_create_body(&params, &validated)?;

    let resp = client
        .post_with_identity("/priapi/v1/aieco/task/create", &body, &user_agent_id)
        .await?;
    let job_id = resp["jobId"].as_str().unwrap_or("?").to_string();

    if let Some(ref files) = params.attachments {
        if !files.is_empty() {
            super::attachments::copy_attachments_to_job(&job_id, files)?;
        }
    }

    // Progress chatter goes to stderr so stdout stays a pure JSON envelope.
    if DEBUG_LOG {
        let mut err = std::io::stderr();
        let _ = writeln!(err, "✓ Calldata generated (jobId: {job_id})");
    }

    // Save designated-provider BEFORE broadcast: job_created event fires
    // on-chain during broadcast and may be processed by the agent before
    // sign_uop_and_broadcast returns — the file must already exist.
    //
    // FR-8.2/8.4: when --endpoint is omitted/empty but a serviceId is given,
    // auto-resolve the x402 endpoint from the provider's service catalog and
    // persist it. Explicit --endpoint is used verbatim (unchanged). A2A /
    // no-endpoint services resolve to None → endpoint-less save, unchanged
    // routing (FR-8.5/AC-11).
    let resolved_endpoint: Option<String> =
        if let Some(ep) = params.endpoint.as_deref().filter(|s| !s.is_empty()) {
            Some(ep.to_string())
        } else if !params.service_id.trim().is_empty() {
            common::find_service(&params.provider, &params.service_id)
                .await?
                .and_then(|svc| svc.get("endpoint").and_then(|v| v.as_str()).map(str::to_string))
                .filter(|s| !s.is_empty())
        } else {
            None
        };
    super::negotiate::save_designated_provider_with_endpoint(
        &job_id,
        &params.provider,
        resolved_endpoint.as_deref(),
    )?;
    let provider_prebind = common::a2a_binding::bind_job_provider_to_current_runtime(&job_id).await;

    let tx_hash = match signing::sign_uop_and_broadcast(
        client,
        &resp["uopData"],
        &account_id,
        &address,
        &job_id,
        1,
        &user_agent_id,
        None,
    )
    .await
    {
        Ok(tx_hash) => tx_hash,
        Err(err) => {
            if let Some(prebind) = &provider_prebind {
                prebind.rollback_if_created().await;
            }
            return Err(err);
        }
    };

    audit::log(
        "cli",
        "user/task_created",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={user_agent_id}"),
            format!("currency={}", validated.currency),
            format!("budget={}", params.budget),
            format!("maxBudget={}", params.max_budget),
            format!("designatedProvider={}", params.provider),
            format!("paymentMode={}", params.payment_mode),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );

    // Terminal success emit — routed through `output::success` so stdout is a
    // pure JSON envelope carrying the advisory `data.balanceWarning` (drift
    // resolved per architecture §5.2/§10 #3). The human/agent-facing guidance
    // (summary, advisory framing, [Watch] mandatory-gate block, set-payment-mode
    // note) is preserved verbatim under `data.guidance` so agent orchestration is
    // unchanged; only its carrier moved from raw stdout lines into the envelope.
    let mut guidance = String::new();
    guidance.push_str(
        "✓ Task publish in progress (transaction broadcast, awaiting on-chain confirmation)\n",
    );
    guidance.push_str(&format!("  jobId:  {job_id}\n"));
    guidance.push_str(&format!("  txHash: {tx_hash}\n"));
    if !params.provider.is_empty() {
        guidance.push_str(&format!("  Designated provider: {}\n", params.provider));
    }
    guidance.push('\n');
    if let Some(ref msg) = balance_msg {
        guidance.push('\n');
        guidance.push_str("Advisory (NOT an error; task is on-chain; do NOT re-run create-task; the ASP may or may not apply — do NOT promise the user it will). Top up so the payment step doesn't fail if the ASP applies:\n");
        guidance.push_str(msg);
        guidance.push('\n');
    }
    // In CLI mode (Claude Code / Codex), skip the "Next: wait for ..." hint —
    // its passive "wait" + "automatically" phrasing reads as a conversation-ending
    // cue to LLM-driven watch loops and was observed to suppress the immediately
    // following [Watch] block. Native push clients (Hermes / OpenClaw) still get
    // the hint since a human reads it directly.
    if !super::content::is_cli_mode() {
        guidance.push_str("Next: wait for the on-chain confirmation; the designated provider will be contacted automatically.\n");
    }
    if super::content::is_cli_mode() {
        guidance.push_str("[Watch] 🛑 Mandatory next steps. End the turn after Step 2. Do NOT ask the user whether to watch — it is required to receive the next event.\n");
        guidance.push('\n');
        guidance.push_str("**Step 1 (MANDATORY GATE) — Read `skills/okx-ai/references/watch-core.md` now.** If you have NOT read it in this session, you cannot proceed to Step 2 — Step 2's invocation, dispatch rules, and re-arm semantics live ONLY in that file. Skipping this step is a protocol violation.\n");
        guidance.push('\n');
        guidance.push_str(&format!("**Step 2 — Execute the watch per `skills/okx-ai/references/watch-core.md` §Run watch, scoped to job-id `{job_id}`.** Then dispatch each returned item per §Dispatch by `kind` and re-enter per §Re-enter after processing. (Do NOT guess the bash invocation — read watch-core.md first.)\n"));
        guidance.push('\n');
        guidance.push_str("⏭ Skip `detect_watch_support` — this `[Watch]` block is only emitted on supported platforms.\n");
    }
    guidance.push_str("🛑 Do NOT call set-payment-mode.");

    let mut data = serde_json::json!({
        "jobId": job_id,
        "txHash": tx_hash,
        "guidance": guidance,
    });
    if !params.provider.is_empty() {
        data["designatedProvider"] = serde_json::json!(params.provider);
    }
    // `balanceWarning` is present ONLY on insufficiency (absent when sufficient).
    if let Some(warning) = balance_warning {
        data["balanceWarning"] = warning;
    }
    crate::output::success(data);
    Ok(())
}

// ─── Field validation helpers (shared with prepare-create) ───────────────

fn validate_title(title: &str) -> Result<()> {
    if title.is_empty() {
        anyhow::bail!("title must not be empty");
    }
    if title.chars().count() > MAX_TITLE_CHARS {
        anyhow::bail!(
            "title may not exceed {MAX_TITLE_CHARS} characters (currently {})",
            title.chars().count()
        );
    }
    Ok(())
}

fn validate_description_body(desc: &str) -> Result<()> {
    let len = desc.chars().count();
    if len < MIN_DESCRIPTION_CHARS {
        anyhow::bail!(
            "description is too short (minimum {MIN_DESCRIPTION_CHARS} chars, currently {len})"
        );
    }
    if len > MAX_DESCRIPTION_CHARS {
        anyhow::bail!("description may not exceed {MAX_DESCRIPTION_CHARS} chars (currently {len})");
    }
    Ok(())
}

fn validate_description_opt(desc: Option<&str>) -> Result<()> {
    if let Some(d) = desc {
        validate_description_body(d)?;
    }
    Ok(())
}

/// Validate task fields without network calls.
/// Used by `prepare-create` (common/mod.rs) to give early field-level feedback.
pub(crate) fn validate_draft_fields(
    description: Option<&str>,
    title: Option<&str>,
    budget: Option<f64>,
    max_budget: Option<f64>,
    currency: Option<&str>,
) -> serde_json::Value {
    let mut checks = Vec::<serde_json::Value>::new();
    let mut errors = Vec::<String>::new();

    if let Some(d) = description {
        match validate_description_opt(Some(d)) {
            Ok(()) => checks.push(
                serde_json::json!({"field": "description", "ok": true, "chars": d.chars().count()}),
            ),
            Err(e) => {
                let msg = e.to_string();
                checks.push(serde_json::json!({"field": "description", "ok": false, "error": msg}));
                errors.push(msg);
            }
        }
    }

    if let Some(t) = title {
        match validate_title(t) {
            Ok(()) => checks.push(
                serde_json::json!({"field": "title", "ok": true, "chars": t.chars().count()}),
            ),
            Err(e) => {
                let msg = e.to_string();
                checks.push(serde_json::json!({"field": "title", "ok": false, "error": msg}));
                errors.push(msg);
            }
        }
    }

    if let Some(c) = currency {
        match normalize_currency(c) {
            Ok(norm) => checks
                .push(serde_json::json!({"field": "currency", "ok": true, "normalized": norm})),
            Err(e) => {
                let msg = e.to_string();
                checks.push(serde_json::json!({"field": "currency", "ok": false, "error": msg}));
                errors.push(msg);
            }
        }
    }

    if let Some(b) = budget {
        match validate_budget(b).and_then(|()| validate_budget_decimals(b)) {
            Ok(()) => checks.push(serde_json::json!({"field": "budget", "ok": true, "value": b})),
            Err(e) => {
                let msg = e.to_string();
                checks.push(serde_json::json!({"field": "budget", "ok": false, "error": msg}));
                errors.push(msg);
            }
        }
    }

    if let Some(mb) = max_budget {
        match validate_budget(mb).and_then(|()| validate_budget_decimals(mb)) {
            Ok(()) => {
                checks.push(serde_json::json!({"field": "max_budget", "ok": true, "value": mb}))
            }
            Err(e) => {
                let msg = e.to_string();
                checks.push(serde_json::json!({"field": "max_budget", "ok": false, "error": msg}));
                errors.push(msg);
            }
        }
    }

    if let (Some(b), Some(mb)) = (budget, max_budget) {
        if mb < b {
            let msg = format!("max_budget ({mb}) must be >= budget ({b})");
            checks.push(
                serde_json::json!({"field": "max_budget_vs_budget", "ok": false, "error": msg}),
            );
            errors.push(msg);
        } else {
            checks.push(serde_json::json!({"field": "max_budget_vs_budget", "ok": true}));
        }
    }

    if errors.is_empty() {
        serde_json::json!({"ok": true, "checks": checks})
    } else {
        serde_json::json!({"ok": false, "checks": checks, "errors": errors})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params_with_provider(provider: String) -> CreateTaskParams {
        CreateTaskParams {
            description: "a long enough description text for the task".to_string(),
            budget: 10.0,
            max_budget: 20.0,
            currency: "USDT".to_string(),
            title: Some("t".to_string()),
            provider,
            attachments: None,
            endpoint: None,
            payment_mode: "escrow".to_string(),
            service_id: "svc-1".to_string(),
            service_params: None,
            service_token_address: None,
            service_token_amount: None,
        }
    }

    #[test]
    fn validate_requires_designated_provider() {
        let err = params_with_provider(String::new())
            .validate()
            .err()
            .unwrap()
            .to_string();
        assert!(
            err.contains("A designated provider is required"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_accepts_with_provider() {
        assert!(params_with_provider("agent-1".to_string())
            .validate()
            .is_ok());
    }

    #[test]
    fn create_body_has_no_description_summary() {
        // AC-3: the built request body must not carry a descriptionSummary key,
        // and create-task must still build a valid body when no summary was ever
        // supplied (the flag has been removed from the CLI surface entirely).
        let params = params_with_provider("agent-1".to_string());
        let validated = params.validate().unwrap();
        let body = build_create_body(&params, &validated).unwrap();
        assert!(
            body.get("descriptionSummary").is_none(),
            "request body must not contain descriptionSummary: {body}"
        );
        assert_eq!(body["title"], "t");
        assert_eq!(body["description"], "a long enough description text for the task");
        assert_eq!(body["providerAgentId"], "agent-1");
    }

    // Reconciled with master's `validate_budget` after MR !187 review note-9880442
    // asked to revert the budget change out of this doc-removal MR as out-of-scope.
    // Master's guard is `budget <= 0.0`, so zero is rejected here. NOTE: MR !150
    // (discussion d2c53d84) had settled zero-budget as legal (`< 0.0`); re-landing
    // that zero-budget-allowed behavior belongs in its own dedicated MR, not here.
    #[test]
    fn validate_budget_rejects_nonpositive_accepts_positive() {
        assert!(validate_budget(0.0).is_err());
        assert!(validate_budget(-1.0).is_err());
        assert!(validate_budget(1.0).is_ok());
    }
}
