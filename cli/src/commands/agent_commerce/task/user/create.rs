//! Publish a task (custom signing flow).
//!
//! User action: publish a task — `onchainos agent create-task`.
//!
//! Identity check: invokes the identity-module CLI (`onchainos agent get-my-agents`) to verify
//! that the current user has a user identity (role=1) before running the publish flow.

use anyhow::{bail, Result};
use std::time::Duration;

use crate::audit;

use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;
use crate::commands::agent_commerce::task::common::{
    self, fetch_my_agents_by_role, network::task_api_client::TaskApiClient,
    payment_mode::PaymentMode,
    AGENT_ROLE_USER, XLAYER_CHAIN_ID, DEBUG_LOG,
};
use crate::commands::agent_commerce::task::signing;

// ─── Constants ───────────────────────────────────────────────────────────

pub const MAX_BUDGET: f64 = 10_000_000.0;
pub const MIN_DESCRIPTION_CHARS: usize = 20;
pub const MAX_DESCRIPTION_CHARS: usize = 2000;
pub const MAX_BUDGET_DECIMALS: usize = 5;
pub const MAX_SUMMARY_CHARS: usize = 200;
pub const MAX_TITLE_CHARS: usize = 30;

// ─── Parameter struct ────────────────────────────────────────────────────

pub struct CreateTaskParams {
    pub description: String,
    pub description_summary: Option<String>,
    pub budget: f64,
    pub max_budget: f64,
    pub currency: String,
    pub title: Option<String>,
    pub provider: Option<String>,
    pub attachments: Option<Vec<String>>,
    pub endpoint: Option<String>,
    pub payment_mode: Option<String>,
    pub service_id: Option<String>,
    pub service_params: Option<String>,
    pub service_token_address: Option<String>,
    pub service_token_amount: Option<String>,
    pub visibility: i32,
}

struct ValidatedParams {
    currency: String,
    title: String,
    summary: String,
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
            bail!("--max-budget ({}) may not be less than --budget ({})", self.max_budget, self.budget);
        }
        validate_budget(self.max_budget)?;
        validate_budget_decimals(self.max_budget)?;

        let title = match &self.title {
            Some(t) if t.chars().count() > MAX_TITLE_CHARS => t.chars().take(MAX_TITLE_CHARS).collect(),
            Some(t) => t.clone(),
            None => self.description.chars().take(MAX_TITLE_CHARS).collect(),
        };
        let summary = match &self.description_summary {
            Some(s) if s.chars().count() > MAX_SUMMARY_CHARS => s.chars().take(MAX_SUMMARY_CHARS).collect(),
            Some(s) => s.clone(),
            None => self.description.chars().take(MAX_SUMMARY_CHARS).collect(),
        };

        if self.visibility != 0 && self.visibility != 1 {
            bail!("--visibility must be 0 (public) or 1 (private), got {}", self.visibility);
        }
        if self.visibility == 1 && self.provider.is_none() {
            bail!("visibility=1 (private) requires --provider; either set a provider or use --visibility 0 (public)");
        }

        if let Some(ref files) = self.attachments {
            for f in files {
                if !std::path::Path::new(f).exists() {
                    bail!("attachment file not found: {f}");
                }
            }
        }

        Ok(ValidatedParams { currency, title, summary })
    }
}

// ─── Validation helpers ─────────────────────────────────────────────────

pub fn normalize_currency(currency: &str) -> Result<String> {
    let normalized: String = currency.chars()
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
    if budget < 0.0 {
        bail!("budget must not be negative");
    }
    if budget > MAX_BUDGET {
        bail!("per-task budget may not exceed {} USDT/USDG", MAX_BUDGET as u64);
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

    let agent_id = user["agentId"].as_str()
        .ok_or_else(|| anyhow::anyhow!("agent is missing the agentId field"))?
        .to_string();
    let owner_address = user["ownerAddress"].as_str().unwrap_or("").to_string();
    Ok((agent_id, owner_address))
}

// ─── Create task ────────────────────────────────────────────────────────

pub async fn handle_create(
    client: &mut TaskApiClient,
    params: CreateTaskParams,
) -> Result<()> {
    let validated = params.validate()?;

    ensure_tokens_refreshed().await
        .map_err(|e| anyhow::anyhow!("session has expired; run `onchainos wallet login` first: {e}"))?;

    let (user_agent_id, _) = resolve_user_agent().await?;
    if DEBUG_LOG {
        eprintln!("[task-create] user identity check passed (agentId: {user_agent_id})");
    }

    let balance_warning = match common::ensure_sufficient_balance(params.budget, &validated.currency).await {
        Err(e) => {
            if DEBUG_LOG {
                eprintln!("[task-create] ⚠ balance warning: {e}");
            }
            Some(e.to_string())
        }
        Ok(()) => None,
    };

    let (account_id, address) = signing::resolve_wallet_by_agent_id(&user_agent_id).await?;

    let mut body = serde_json::json!({
        "title":              validated.title,
        "description":        params.description,
        "descriptionSummary": validated.summary,
        "paymentTokenSymbol": validated.currency.to_uppercase(),
        "paymentTokenAmount": params.budget.to_string(),
        "paymentMostTokenAmount": params.max_budget.to_string(),
        "chainId":            XLAYER_CHAIN_ID,
        "paymentMode":        PaymentMode::parse_flag(params.payment_mode.as_deref())?,
        "visibility":         params.visibility
    });
    if let Some(ref provider_id) = params.provider {
        body["providerAgentId"] = serde_json::json!(provider_id);
    }
    if let Some(ref sid) = params.service_id {
        body["serviceId"] = serde_json::json!(sid);
    }
    if let Some(ref sp) = params.service_params {
        body["serviceParams"] = serde_json::json!(sp);
    }
    if let Some(ref sta) = params.service_token_address {
        body["serviceTokenAddress"] = serde_json::json!(sta);
    }
    if let Some(ref stm) = params.service_token_amount {
        body["serviceTokenAmount"] = serde_json::json!(stm);
    }

    let resp = client.post_with_identity("/priapi/v1/aieco/task/create", &body, &user_agent_id).await?;
    let job_id = resp["jobId"].as_str().unwrap_or("?").to_string();

    if let Some(ref files) = params.attachments {
        if !files.is_empty() {
            super::attachments::copy_attachments_to_job(&job_id, files)?;
        }
    }

    println!("✓ Calldata generated (jobId: {job_id})");

    // Save designated-provider BEFORE broadcast: job_created event fires
    // on-chain during broadcast and may be processed by the agent before
    // sign_uop_and_broadcast returns — the file must already exist.
    if let Some(ref provider_id) = params.provider {
        super::negotiate::save_designated_provider_with_endpoint(&job_id, provider_id, params.endpoint.as_deref())?;
    }
    let provider_prebind = common::a2a_binding::bind_job_provider_to_current_runtime(&job_id).await;

    let tx_hash = match signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        &job_id, 1, &user_agent_id,
        None,
    ).await {
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
            format!("designatedProvider={}", params.provider.as_deref().unwrap_or("")),
            format!("paymentMode={}", params.payment_mode.as_deref().unwrap_or("unset")),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );

    println!("✓ Task publish in progress (transaction broadcast, awaiting on-chain confirmation)");
    println!("  jobId:  {job_id}");
    println!("  txHash: {tx_hash}");
    if let Some(ref provider_id) = params.provider {
        println!("  Designated provider: {provider_id}");
    }
    println!();
    if let Some(ref warning) = balance_warning {
        println!();
        println!("Advisory (NOT an error; task is on-chain; do NOT re-run create-task; the ASP may or may not apply — do NOT promise the user it will). Top up so the payment step doesn't fail if the ASP applies:");
        println!("{warning}");
    }
    // In CLI mode (Claude Code / Codex), skip the "Next: wait for ..." hint —
    // its passive "wait" + "automatically" phrasing reads as a conversation-ending
    // cue to LLM-driven watch loops and was observed to suppress the immediately
    // following [Watch] block. Native push clients (Hermes / OpenClaw) still get
    // the hint since a human reads it directly.
    if !super::content::is_cli_mode() {
        if params.provider.is_some() {
            println!("Next: wait for the on-chain confirmation; the designated provider will be contacted automatically.");
        } else {
            println!("Next: wait for the on-chain confirmation; the task is public — ASPs will discover it and apply.");
        }
    }
    if super::content::is_cli_mode() {
        println!("[Watch] 🛑 Mandatory next steps. End the turn after Step 2. Do NOT ask the user whether to watch — it is required to receive the next event.");
        println!();
        println!("**Step 1 (MANDATORY GATE) — Read `skills/okx-ai/references/watch-core.md` now.** If you have NOT read it in this session, you cannot proceed to Step 2 — Step 2's invocation, dispatch rules, and re-arm semantics live ONLY in that file. Skipping this step is a protocol violation.");
        println!();
        println!("**Step 2 — Execute the watch per `skills/okx-ai/references/watch-core.md` §Run watch, scoped to job-id `{job_id}`.** Then dispatch each returned item per §Dispatch by `kind` and re-enter per §Re-enter after processing. (Do NOT guess the bash invocation — read watch-core.md first.)");
        println!();
        println!("⏭ Skip `detect_watch_support` — this `[Watch]` block is only emitted on supported platforms.");
    }
    println!("🛑 Do NOT call set-payment-mode.");
    Ok(())
}
