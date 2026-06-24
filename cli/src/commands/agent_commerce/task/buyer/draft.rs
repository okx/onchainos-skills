//! Draft task commands — create / list / update / delete / publish.
//!
//! Drafts are off-chain (status = -1, `Status::Init`). No state-machine events
//! fire until `publish` signs and broadcasts the task on-chain.

use anyhow::{bail, Result};
use std::time::Duration;

use crate::audit;

use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;
use crate::commands::agent_commerce::task::common::{
    self, network::task_api_client::TaskApiClient,
    payment_mode::PaymentMode, DEBUG_LOG,
};
use crate::commands::agent_commerce::task::signing;

use super::create::{
    normalize_currency, resolve_buyer_agent, validate_budget,
    validate_budget_decimals,
    MAX_DESCRIPTION_CHARS, MAX_SUMMARY_CHARS, MAX_TITLE_CHARS, MIN_DESCRIPTION_CHARS,
};

// ─── Draft API paths ────────────────────────────────────────────────────

const DRAFT_CREATE: &str = "/priapi/v1/aieco/task/draft/create";
const DRAFT_LIST: &str = "/priapi/v1/aieco/task/draft/list";

fn draft_update_path(job_id: &str) -> String {
    format!("/priapi/v1/aieco/task/draft/{job_id}/update")
}
fn draft_delete_path(job_id: &str) -> String {
    format!("/priapi/v1/aieco/task/draft/{job_id}/delete")
}
fn draft_publish_path(job_id: &str) -> String {
    format!("/priapi/v1/aieco/task/draft/{job_id}/publish")
}

// ─── Validation ─────────────────────────────────────────────────────────

fn validate_title(title: &str) -> Result<()> {
    if title.is_empty() {
        bail!("title must not be empty");
    }
    if title.chars().count() > MAX_TITLE_CHARS {
        bail!(
            "title may not exceed {MAX_TITLE_CHARS} characters (currently {})",
            title.chars().count()
        );
    }
    Ok(())
}

fn validate_description(desc: &str) -> Result<()> {
    let len = desc.chars().count();
    if len < MIN_DESCRIPTION_CHARS {
        bail!("description is too short (minimum {MIN_DESCRIPTION_CHARS} chars, currently {len})");
    }
    if len > MAX_DESCRIPTION_CHARS {
        bail!("description may not exceed {MAX_DESCRIPTION_CHARS} chars (currently {len})");
    }
    Ok(())
}

fn validate_description_opt(desc: Option<&str>) -> Result<()> {
    if let Some(d) = desc {
        validate_description(d)?;
    }
    Ok(())
}

fn validate_summary(summary: &str) -> Result<()> {
    if summary.is_empty() {
        bail!("description-summary must not be empty");
    }
    let len = summary.chars().count();
    if len > MAX_SUMMARY_CHARS {
        bail!("description-summary may not exceed {MAX_SUMMARY_CHARS} chars (currently {len})");
    }
    Ok(())
}

fn validate_budget_opt(budget: Option<f64>) -> Result<()> {
    if let Some(b) = budget {
        validate_budget(b)?;
        validate_budget_decimals(b)?;
    }
    Ok(())
}

fn validate_currency_opt(currency: Option<&str>) -> Result<Option<String>> {
    match currency {
        Some(c) => Ok(Some(normalize_currency(c)?)),
        None => Ok(None),
    }
}

fn validate_draft_for_publish(detail: &serde_json::Value) -> Result<()> {
    let mut missing: Vec<&str> = Vec::new();

    let title = detail["title"].as_str().unwrap_or("");
    if title.is_empty() {
        missing.push("title");
    }

    let desc = detail["description"].as_str().unwrap_or("");
    if desc.chars().count() < MIN_DESCRIPTION_CHARS {
        missing.push("description (min 20 characters)");
    }

    let token_sym = detail["tokenSymbol"].as_str()
        .or_else(|| detail["paymentTokenSymbol"].as_str())
        .unwrap_or("");
    if token_sym.is_empty() {
        missing.push("paymentTokenSymbol (currency: USDT or USDG)");
    }

    let token_amount_str = detail["tokenAmount"].as_str()
        .or_else(|| detail["paymentTokenAmount"].as_str())
        .unwrap_or("0");
    let token_amount: f64 = token_amount_str.parse().unwrap_or(0.0);
    if token_amount <= 0.0 {
        missing.push("paymentTokenAmount (budget required, must be > 0)");
    }

    let most_amount_str = detail["paymentMostTokenAmount"].as_str().unwrap_or("0");
    let most_amount: f64 = most_amount_str.parse().unwrap_or(0.0);
    if most_amount <= 0.0 {
        missing.push("paymentMostTokenAmount (max budget required, must be > 0)");
    } else if most_amount < token_amount {
        missing.push("paymentMostTokenAmount (max budget must be >= budget)");
    }

    if !missing.is_empty() {
        let list = missing
            .iter()
            .map(|m| format!("  - {m}"))
            .collect::<Vec<_>>()
            .join("\n");
        bail!(
            "Draft is missing required fields for publishing:\n{list}\n\
             🛑 Do NOT auto-fill these fields. Ask the user to provide values for each missing field, \
             then call `draft update` with the user's values, then retry `draft publish`."
        );
    }
    Ok(())
}

// ─── 0. validate-draft (pure local, no network) ───────────────────────

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
            Ok(()) => checks.push(serde_json::json!({"field": "description", "ok": true, "chars": d.chars().count()})),
            Err(e) => {
                let msg = e.to_string();
                checks.push(serde_json::json!({"field": "description", "ok": false, "error": msg}));
                errors.push(msg);
            }
        }
    }

    if let Some(t) = title {
        match validate_title(t) {
            Ok(()) => checks.push(serde_json::json!({"field": "title", "ok": true, "chars": t.chars().count()})),
            Err(e) => {
                let msg = e.to_string();
                checks.push(serde_json::json!({"field": "title", "ok": false, "error": msg}));
                errors.push(msg);
            }
        }
    }

    if let Some(c) = currency {
        match normalize_currency(c) {
            Ok(norm) => checks.push(serde_json::json!({"field": "currency", "ok": true, "normalized": norm})),
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
            Ok(()) => checks.push(serde_json::json!({"field": "max_budget", "ok": true, "value": mb})),
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
            checks.push(serde_json::json!({"field": "max_budget_vs_budget", "ok": false, "error": msg}));
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

pub fn handle_validate_draft(
    description: Option<&str>,
    title: Option<&str>,
    budget: Option<f64>,
    max_budget: Option<f64>,
    currency: Option<&str>,
) -> Result<()> {
    let result = validate_draft_fields(description, title, budget, max_budget, currency);
    crate::output::success(result);
    Ok(())
}

// ─── 1. draft create ────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub async fn handle_draft_create(
    client: &mut TaskApiClient,
    title: &str,
    description: &str,
    description_summary: &str,
    budget: Option<f64>,
    max_budget: Option<f64>,
    currency: Option<&str>,
    provider: Option<&str>,
    attachments: Option<&[String]>,
    service_id: Option<&str>,
    service_params: Option<&str>,
    service_token_address: Option<&str>,
    service_token_amount: Option<&str>,
    payment_mode: Option<&str>,
    visibility: i32,
) -> Result<()> {
    validate_title(title)?;
    validate_description(description)?;
    validate_summary(description_summary)?;
    validate_budget_opt(budget)?;
    if let (Some(b), Some(mb)) = (budget, max_budget) {
        if mb < b {
            bail!("--max-budget ({mb}) may not be less than --budget ({b})");
        }
    }
    validate_budget_opt(max_budget)?;
    let currency_norm = validate_currency_opt(currency)?;

    if visibility != 0 && visibility != 1 {
        bail!("--visibility must be 0 (public) or 1 (private), got {visibility}");
    }
    if visibility == 1 && provider.is_none() {
        bail!("visibility=1 (private) requires --provider; either set a provider or use --visibility 0 (public)");
    }

    let payment_mode_int = PaymentMode::parse_flag(payment_mode)?;

    ensure_tokens_refreshed().await
        .map_err(|e| anyhow::anyhow!("session has expired; run `onchainos wallet login` first: {e}"))?;

    let (buyer_agent_id, _) = resolve_buyer_agent().await?;
    if DEBUG_LOG {
        eprintln!("[draft-create] buyer identity check passed (agentId: {buyer_agent_id})");
    }

    let balance_warning = if let (Some(b), Some(ref c)) = (budget, &currency_norm) {
        match common::ensure_sufficient_balance(b, c).await {
            Err(e) => {
                if DEBUG_LOG {
                    eprintln!("[draft-create] ⚠ balance warning: {e}");
                }
                Some(format!("⚠️ {e}"))
            }
            Ok(()) => None,
        }
    } else {
        None
    };

    let mut body = serde_json::json!({
        "title": title,
        "description": description,
        "descriptionSummary": description_summary,
        "paymentMode": payment_mode_int,
        "visibility": visibility,
    });
    let obj = body.as_object_mut().unwrap();

    if let Some(ref c) = currency_norm {
        obj.insert("paymentTokenSymbol".into(), serde_json::json!(c.to_uppercase()));
    }
    if let Some(b) = budget {
        obj.insert("paymentTokenAmount".into(), serde_json::json!(b.to_string()));
    }
    if let Some(mb) = max_budget {
        obj.insert("paymentMostTokenAmount".into(), serde_json::json!(mb.to_string()));
    }
    if let Some(pid) = provider {
        obj.insert("providerAgentId".into(), serde_json::json!(pid));
    }
    if let Some(sid) = service_id {
        obj.insert("serviceId".into(), serde_json::json!(sid));
    }
    if let Some(sp) = service_params {
        obj.insert("serviceParams".into(), serde_json::json!(sp));
    }
    if let Some(sta) = service_token_address {
        obj.insert("serviceTokenAddress".into(), serde_json::json!(sta));
    }
    if let Some(stm) = service_token_amount {
        obj.insert("serviceTokenAmount".into(), serde_json::json!(stm));
    }

    let resp = client.post_with_identity(DRAFT_CREATE, &body, &buyer_agent_id).await?;
    let job_id = resp["jobId"].as_str().unwrap_or("?").to_string();

    if let Some(files) = attachments {
        if !files.is_empty() {
            super::attachments::copy_attachments_to_job(&job_id, files)?;
        }
    }

    audit::log(
        "cli",
        "buyer/draft_created",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={buyer_agent_id}"),
        ]),
        None,
    );

    println!("✓ Draft saved (jobId: {job_id})");
    if let Some(ref warning) = balance_warning {
        println!();
        println!("{warning}");
    }
    Ok(())
}

// ─── 2. draft list ──────────────────────────────────────────────────────

pub async fn handle_draft_list(
    client: &mut TaskApiClient,
    page: u32,
    limit: u32,
) -> Result<()> {
    ensure_tokens_refreshed().await
        .map_err(|e| anyhow::anyhow!("session has expired; run `onchainos wallet login` first: {e}"))?;

    let buyer_agent_id = match resolve_buyer_agent().await {
        Ok((id, _)) => id,
        Err(e) => {
            bail!("buyer identity required for listing drafts: {e}");
        }
    };

    let body = serde_json::json!({
        "page": page,
        "pageSize": limit,
    });

    let resp = client.post_with_identity(DRAFT_LIST, &body, &buyer_agent_id).await?;

    let list = resp["list"].as_array();
    let total = resp["total"].as_u64().unwrap_or(0);

    match list {
        Some(items) if !items.is_empty() => {
            println!("Drafts (page {page}, {total} total):\n");
            println!("{:<46} {:<32} {:<16} status", "jobId", "title", "budget");
            println!("{}", "-".repeat(100));
            for item in items {
                let jid = item["jobId"].as_str().unwrap_or("?");
                let t = item["title"].as_str().unwrap_or("(untitled)");
                let amount = item["tokenAmount"].as_str()
                    .or_else(|| item["paymentTokenAmount"].as_str())
                    .unwrap_or("-");
                let sym = item["tokenSymbol"].as_str()
                    .or_else(|| item["paymentTokenSymbol"].as_str())
                    .unwrap_or("");
                let budget_str = if amount == "-" {
                    "-".to_string()
                } else {
                    format!("{amount} {sym}")
                };
                let title_display: String = t.chars().take(30).collect();
                println!("{:<46} {:<32} {:<16} 📝 Draft", jid, title_display, budget_str);
            }
        }
        _ => {
            println!("No drafts found.");
        }
    }

    audit::log(
        "cli",
        "buyer/draft_listed",
        true,
        Duration::default(),
        Some(vec![
            format!("agentId={buyer_agent_id}"),
            format!("page={page}"),
            format!("total={total}"),
        ]),
        None,
    );

    Ok(())
}

// ─── 3. draft update ────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub async fn handle_draft_update(
    client: &mut TaskApiClient,
    job_id: &str,
    title: Option<&str>,
    description: Option<&str>,
    description_summary: Option<&str>,
    budget: Option<f64>,
    max_budget: Option<f64>,
    currency: Option<&str>,
    provider: Option<&str>,
    service_id: Option<&str>,
    service_params: Option<&str>,
    service_token_address: Option<&str>,
    service_token_amount: Option<&str>,
) -> Result<()> {
    if let Some(t) = title {
        validate_title(t)?;
    }
    validate_description_opt(description)?;
    if let Some(ds) = description_summary {
        validate_summary(ds)?;
    }
    validate_budget_opt(budget)?;
    if let (Some(b), Some(mb)) = (budget, max_budget) {
        if mb < b {
            bail!("--max-budget ({mb}) may not be less than --budget ({b})");
        }
    }
    validate_budget_opt(max_budget)?;
    let currency_norm = validate_currency_opt(currency)?;

    ensure_tokens_refreshed().await
        .map_err(|e| anyhow::anyhow!("session has expired; run `onchainos wallet login` first: {e}"))?;

    let (buyer_agent_id, _) = resolve_buyer_agent().await?;
    if DEBUG_LOG {
        eprintln!("[draft-update] buyer identity check passed (agentId: {buyer_agent_id})");
    }

    let mut body = serde_json::Map::new();

    if let Some(t) = title {
        body.insert("title".into(), serde_json::json!(t));
    }
    if let Some(d) = description {
        body.insert("description".into(), serde_json::json!(d));
        if description_summary.is_none() {
            let summary: String = d.chars().take(MAX_SUMMARY_CHARS).collect();
            body.insert("descriptionSummary".into(), serde_json::json!(summary));
        }
    }
    if let Some(ds) = description_summary {
        body.insert("descriptionSummary".into(), serde_json::json!(ds));
    }
    if let Some(ref c) = currency_norm {
        body.insert("paymentTokenSymbol".into(), serde_json::json!(c.to_uppercase()));
    }
    if let Some(b) = budget {
        body.insert("paymentTokenAmount".into(), serde_json::json!(b.to_string()));
    }
    if let Some(mb) = max_budget {
        body.insert("paymentMostTokenAmount".into(), serde_json::json!(mb.to_string()));
    }
    if let Some(pid) = provider {
        body.insert("providerAgentId".into(), serde_json::json!(pid));
    }
    if let Some(sid) = service_id {
        body.insert("serviceId".into(), serde_json::json!(sid));
    }
    if let Some(sp) = service_params {
        body.insert("serviceParams".into(), serde_json::json!(sp));
    }
    if let Some(sta) = service_token_address {
        body.insert("serviceTokenAddress".into(), serde_json::json!(sta));
    }
    if let Some(stm) = service_token_amount {
        body.insert("serviceTokenAmount".into(), serde_json::json!(stm));
    }

    if body.is_empty() {
        bail!("no fields specified for update; pass at least one of --title, --description, --budget, etc.");
    }

    let body_val = serde_json::Value::Object(body);
    client.post_with_identity(&draft_update_path(job_id), &body_val, &buyer_agent_id).await?;

    audit::log(
        "cli",
        "buyer/draft_updated",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={buyer_agent_id}"),
        ]),
        None,
    );

    println!("✓ Draft updated (jobId: {job_id})");
    Ok(())
}

// ─── 4. draft delete ────────────────────────────────────────────────────

pub async fn handle_draft_delete(
    client: &mut TaskApiClient,
    job_id: &str,
) -> Result<()> {
    ensure_tokens_refreshed().await
        .map_err(|e| anyhow::anyhow!("session has expired; run `onchainos wallet login` first: {e}"))?;

    let buyer_agent_id = match resolve_buyer_agent().await {
        Ok((id, _)) => id,
        Err(e) => {
            bail!("buyer identity required for deleting draft: {e}");
        }
    };

    let body = serde_json::json!({});
    client.post_with_identity(&draft_delete_path(job_id), &body, &buyer_agent_id).await?;

    audit::log(
        "cli",
        "buyer/draft_deleted",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={buyer_agent_id}"),
        ]),
        None,
    );

    println!("✓ Draft deleted (jobId: {job_id})");
    Ok(())
}

// ─── 5. draft publish ───────────────────────────────────────────────────

pub async fn handle_draft_publish(
    client: &mut TaskApiClient,
    job_id: &str,
) -> Result<()> {
    ensure_tokens_refreshed().await
        .map_err(|e| anyhow::anyhow!("session has expired; run `onchainos wallet login` first: {e}"))?;

    let (buyer_agent_id, _) = resolve_buyer_agent().await?;
    if DEBUG_LOG {
        eprintln!("[draft-publish] buyer identity check passed (agentId: {buyer_agent_id})");
    }

    // ── Fetch draft detail for pre-publish validation ────────────
    let detail = client
        .get_with_identity(&client.task_path(job_id), &buyer_agent_id)
        .await
        .map_err(|e| anyhow::anyhow!("failed to fetch draft detail: {e}"))?;

    validate_draft_for_publish(&detail)?;

    // ── Balance check (blocking for publish) ─────────────────────
    let token_sym = detail["tokenSymbol"].as_str()
        .or_else(|| detail["paymentTokenSymbol"].as_str())
        .unwrap_or("");
    let token_amount: f64 = detail["tokenAmount"].as_str()
        .or_else(|| detail["paymentTokenAmount"].as_str())
        .unwrap_or("0")
        .parse().unwrap_or(0.0);
    if !token_sym.is_empty() && token_amount > 0.0 {
        common::ensure_sufficient_balance(token_amount, token_sym).await?;
    }

    // ── Publish API → uopData ────────────────────────────────────
    let resp = client
        .post_with_identity(&draft_publish_path(job_id), &serde_json::json!({}), &buyer_agent_id)
        .await?;

    let returned_job_id = resp["jobId"].as_str().unwrap_or(job_id);
    println!("✓ Calldata generated (jobId: {returned_job_id})");

    // ── Save designated provider (before broadcast, same as create) ─
    let provider_id = detail["providerAgentId"].as_str()
        .filter(|s| !s.is_empty());
    if let Some(pid) = provider_id {
        super::negotiate::save_designated_provider(returned_job_id, pid)?;
    }
    let provider_prebind = common::a2a_binding::bind_job_provider_to_current_runtime(returned_job_id).await;

    // ── Sign + Broadcast ─────────────────────────────────────────
    let (account_id, address) = signing::resolve_wallet_by_agent_id(&buyer_agent_id).await?;

    let tx_hash = match signing::sign_uop_and_broadcast(
        client,
        &resp["uopData"],
        &account_id,
        &address,
        returned_job_id,
        1,
        &buyer_agent_id,
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
        "buyer/draft_published",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={returned_job_id}"),
            format!("agentId={buyer_agent_id}"),
            format!("currency={token_sym}"),
            format!("budget={token_amount}"),
            format!("designatedProvider={}", provider_id.unwrap_or("")),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );

    println!("✓ Task publish in progress (transaction broadcast, awaiting on-chain confirmation)");
    println!("  jobId:  {returned_job_id}");
    println!("  txHash: {tx_hash}");
    if let Some(pid) = provider_id {
        println!("  Designated provider: {pid}");
    }
    println!();
    // See create.rs: skip the "Next: wait ..." hint in CLI mode to avoid the
    // passive phrasing suppressing the [Watch] block that follows.
    if !super::content::is_cli_mode() {
        println!("Next: wait for the on-chain confirmation; the designated provider will be contacted automatically.");
    }
    if super::content::is_cli_mode() {
        println!("[Watch] 🛑 Mandatory next steps. End the turn after Step 2. Do NOT ask the user whether to watch — it is required to receive the next event.");
        println!();
        println!("**Step 1 (MANDATORY GATE) — Read `skills/okx-task-watch/SKILL.md` now.** If you have NOT read it in this session, you cannot proceed to Step 2 — Step 2's invocation, dispatch rules, and re-arm semantics live ONLY in that file. Skipping this step is a protocol violation.");
        println!();
        println!("**Step 2 — Execute the watch per `skills/okx-task-watch/SKILL.md` §Run watch, scoped to job-id `{returned_job_id}`.** Then dispatch each returned item per §Dispatch by `kind` and re-enter per §Re-enter after processing. (Do NOT guess the bash invocation — read SKILL.md first.)");
        println!();
        println!("⏭ Skip `detect_watch_support` — this `[Watch]` block is only emitted on supported platforms.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    // ═══════════════════════════════════════════════════════════════════
    // Validation tests
    // ═══════════════════════════════════════════════════════════════════

    // ── validate_title ──────────────────────────────────────────────

    #[test]
    fn title_empty_rejected() {
        assert!(validate_title("").is_err());
    }

    #[test]
    fn title_one_char_ok() {
        assert!(validate_title("a").is_ok());
    }

    #[test]
    fn title_exact_limit_ok() {
        let t: String = "x".repeat(MAX_TITLE_CHARS);
        assert!(validate_title(&t).is_ok());
    }

    #[test]
    fn title_over_limit_rejected() {
        let t: String = "x".repeat(MAX_TITLE_CHARS + 1);
        assert!(validate_title(&t).is_err());
    }

    #[test]
    fn title_cjk_counts_chars_not_bytes() {
        let t: String = "字".repeat(MAX_TITLE_CHARS);
        assert!(validate_title(&t).is_ok());
        let t2: String = "字".repeat(MAX_TITLE_CHARS + 1);
        assert!(validate_title(&t2).is_err());
    }

    // ── validate_description_opt ────────────────────────────────────

    #[test]
    fn description_none_ok() {
        assert!(validate_description_opt(None).is_ok());
    }

    #[test]
    fn description_too_short_rejected() {
        let d: String = "a".repeat(MIN_DESCRIPTION_CHARS - 1);
        assert!(validate_description_opt(Some(&d)).is_err());
    }

    #[test]
    fn description_exact_min_ok() {
        let d: String = "a".repeat(MIN_DESCRIPTION_CHARS);
        assert!(validate_description_opt(Some(&d)).is_ok());
    }

    #[test]
    fn description_exact_max_ok() {
        let d: String = "a".repeat(MAX_DESCRIPTION_CHARS);
        assert!(validate_description_opt(Some(&d)).is_ok());
    }

    #[test]
    fn description_over_max_rejected() {
        let d: String = "a".repeat(MAX_DESCRIPTION_CHARS + 1);
        assert!(validate_description_opt(Some(&d)).is_err());
    }

    // ── validate_budget_opt ─────────────────────────────────────────

    #[test]
    fn budget_none_ok() {
        assert!(validate_budget_opt(None).is_ok());
    }

    #[test]
    fn budget_zero_rejected() {
        assert!(validate_budget_opt(Some(0.0)).is_err());
    }

    #[test]
    fn budget_negative_rejected() {
        assert!(validate_budget_opt(Some(-1.0)).is_err());
    }

    #[test]
    fn budget_normal_ok() {
        assert!(validate_budget_opt(Some(100.0)).is_ok());
    }

    #[test]
    fn budget_five_decimals_ok() {
        assert!(validate_budget_opt(Some(1.12345)).is_ok());
    }

    #[test]
    fn budget_six_decimals_rejected() {
        assert!(validate_budget_opt(Some(1.123456)).is_err());
    }

    #[test]
    fn budget_exceeds_max_rejected() {
        assert!(validate_budget_opt(Some(10_000_001.0)).is_err());
    }

    // ── validate_currency_opt ───────────────────────────────────────

    #[test]
    fn currency_none_ok() {
        assert_eq!(validate_currency_opt(None).unwrap(), None);
    }

    #[test]
    fn currency_usdt_normalized() {
        assert_eq!(validate_currency_opt(Some("usdt")).unwrap(), Some("USDT".to_string()));
    }

    #[test]
    fn currency_usdt0_normalized() {
        assert_eq!(validate_currency_opt(Some("USDT0")).unwrap(), Some("USDT".to_string()));
    }

    #[test]
    fn currency_usd_manat_normalized() {
        assert_eq!(validate_currency_opt(Some("USD₮0")).unwrap(), Some("USDT".to_string()));
    }

    #[test]
    fn currency_usdg_ok() {
        assert_eq!(validate_currency_opt(Some("USDG")).unwrap(), Some("USDG".to_string()));
    }

    #[test]
    fn currency_unsupported_rejected() {
        assert!(validate_currency_opt(Some("ETH")).is_err());
        assert!(validate_currency_opt(Some("BTC")).is_err());
    }

    // ── validate_description (required) ──────────────────────────

    #[test]
    fn description_required_too_short_rejected() {
        let d: String = "a".repeat(MIN_DESCRIPTION_CHARS - 1);
        assert!(validate_description(&d).is_err());
    }

    #[test]
    fn description_required_exact_min_ok() {
        let d: String = "a".repeat(MIN_DESCRIPTION_CHARS);
        assert!(validate_description(&d).is_ok());
    }

    #[test]
    fn description_required_over_max_rejected() {
        let d: String = "a".repeat(MAX_DESCRIPTION_CHARS + 1);
        assert!(validate_description(&d).is_err());
    }

    // ── validate_summary (required) ────────────────────────────────

    #[test]
    fn summary_within_limit_ok() {
        assert!(validate_summary("hello world").is_ok());
    }

    #[test]
    fn summary_exact_max_ok() {
        let s: String = "b".repeat(MAX_SUMMARY_CHARS);
        assert!(validate_summary(&s).is_ok());
    }

    #[test]
    fn summary_over_max_rejected() {
        let s: String = "a".repeat(MAX_SUMMARY_CHARS + 1);
        assert!(validate_summary(&s).is_err());
    }

    // ── cross-validation: both budget values individually valid ────

    #[test]
    fn budget_and_max_budget_both_valid_individually() {
        validate_budget_opt(Some(100.0)).unwrap();
        validate_budget_opt(Some(50.0)).unwrap();
    }

    // ═══════════════════════════════════════════════════════════════════
    // validate_draft_for_publish
    // ═══════════════════════════════════════════════════════════════════

    fn full_draft_json() -> serde_json::Value {
        serde_json::json!({
            "title": "Test task title",
            "description": "A sufficiently long description for testing purposes here.",
            "paymentTokenSymbol": "USDT",
            "paymentTokenAmount": "10",
            "paymentMostTokenAmount": "20"
        })
    }

    #[test]
    fn publish_full_draft_ok() {
        assert!(validate_draft_for_publish(&full_draft_json()).is_ok());
    }

    #[test]
    fn publish_missing_title() {
        let mut d = full_draft_json();
        d["title"] = serde_json::json!("");
        let err = validate_draft_for_publish(&d).unwrap_err().to_string();
        assert!(err.contains("title"), "{err}");
    }

    #[test]
    fn publish_missing_description() {
        let mut d = full_draft_json();
        d["description"] = serde_json::json!("short");
        let err = validate_draft_for_publish(&d).unwrap_err().to_string();
        assert!(err.contains("description"), "{err}");
    }

    #[test]
    fn publish_missing_currency() {
        let mut d = full_draft_json();
        d.as_object_mut().unwrap().remove("paymentTokenSymbol");
        let err = validate_draft_for_publish(&d).unwrap_err().to_string();
        assert!(err.contains("paymentTokenSymbol"), "{err}");
    }

    #[test]
    fn publish_missing_budget() {
        let mut d = full_draft_json();
        d["paymentTokenAmount"] = serde_json::json!("0");
        let err = validate_draft_for_publish(&d).unwrap_err().to_string();
        assert!(err.contains("paymentTokenAmount"), "{err}");
    }

    #[test]
    fn publish_missing_max_budget() {
        let mut d = full_draft_json();
        d["paymentMostTokenAmount"] = serde_json::json!("0");
        let err = validate_draft_for_publish(&d).unwrap_err().to_string();
        assert!(err.contains("paymentMostTokenAmount"), "{err}");
    }

    #[test]
    fn publish_max_budget_less_than_budget() {
        let mut d = full_draft_json();
        d["paymentTokenAmount"] = serde_json::json!("100");
        d["paymentMostTokenAmount"] = serde_json::json!("50");
        let err = validate_draft_for_publish(&d).unwrap_err().to_string();
        assert!(err.contains("max budget must be >= budget"), "{err}");
    }

    #[test]
    fn publish_multiple_missing_fields() {
        let d = serde_json::json!({});
        let err = validate_draft_for_publish(&d).unwrap_err().to_string();
        assert!(err.contains("title"));
        assert!(err.contains("description"));
        assert!(err.contains("paymentTokenSymbol"));
        assert!(err.contains("paymentTokenAmount"));
        assert!(err.contains("paymentMostTokenAmount"));
    }

    #[test]
    #[allow(non_snake_case)]
    fn publish_accepts_tokenSymbol_alias() {
        let d = serde_json::json!({
            "title": "Test task",
            "description": "A sufficiently long description for testing purposes here.",
            "tokenSymbol": "USDT",
            "tokenAmount": "10",
            "paymentMostTokenAmount": "20"
        });
        assert!(validate_draft_for_publish(&d).is_ok());
    }

    // ═══════════════════════════════════════════════════════════════════
    // CLI argument parsing (clap)
    // ═══════════════════════════════════════════════════════════════════

    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: super::super::DraftCommand,
    }

    // ── draft create ────────────────────────────────────────────────

    #[test]
    fn cli_create_required_fields_only() {
        let cli = TestCli::parse_from([
            "test", "create",
            "--title", "my task",
            "--description", "a valid description here",
            "--description-summary", "summary",
        ]);
        match cli.cmd {
            super::super::DraftCommand::Create { title, description, description_summary, budget, visibility, payment_mode, .. } => {
                assert_eq!(title, "my task");
                assert_eq!(description, "a valid description here");
                assert_eq!(description_summary, "summary");
                assert!(budget.is_none());
                assert_eq!(visibility, 1);
                assert!(payment_mode.is_none());
            }
            _ => panic!("expected Create"),
        }
    }

    #[test]
    fn cli_create_all_fields() {
        let cli = TestCli::parse_from([
            "test", "create",
            "--title", "full task",
            "--description", "a long description here",
            "--description-summary", "short summary",
            "--budget", "50.5",
            "--max-budget", "100",
            "--currency", "USDT",
            "--provider", "agent-123",
            "--file", "/tmp/a.pdf",
            "--file", "/tmp/b.png",
            "--payment-mode", "escrow",
            "--visibility", "0",
        ]);
        match cli.cmd {
            super::super::DraftCommand::Create {
                title, description, description_summary, budget, max_budget,
                currency, provider, attachments, payment_mode, visibility, ..
            } => {
                assert_eq!(title, "full task");
                assert_eq!(description, "a long description here");
                assert_eq!(description_summary, "short summary");
                assert_eq!(budget, Some(50.5));
                assert_eq!(max_budget, Some(100.0));
                assert_eq!(currency.as_deref(), Some("USDT"));
                assert_eq!(provider.as_deref(), Some("agent-123"));
                assert_eq!(attachments, Some(vec!["/tmp/a.pdf".to_string(), "/tmp/b.png".to_string()]));
                assert_eq!(payment_mode.as_deref(), Some("escrow"));
                assert_eq!(visibility, 0);
            }
            _ => panic!("expected Create"),
        }
    }

    #[test]
    fn cli_create_missing_required_fails() {
        // missing --title
        assert!(TestCli::try_parse_from(["test", "create"]).is_err());
        // missing --description
        assert!(TestCli::try_parse_from(["test", "create", "--title", "t", "--description-summary", "s"]).is_err());
        // missing --description-summary
        assert!(TestCli::try_parse_from(["test", "create", "--title", "t", "--description", "d"]).is_err());
    }

    // ── draft list ──────────────────────────────────────────────────

    #[test]
    fn cli_list_defaults() {
        let cli = TestCli::parse_from(["test", "list"]);
        match cli.cmd {
            super::super::DraftCommand::List { page, limit } => {
                assert_eq!(page, 1);
                assert_eq!(limit, 20);
            }
            _ => panic!("expected List"),
        }
    }

    #[test]
    fn cli_list_custom_page() {
        let cli = TestCli::parse_from(["test", "list", "--page", "3", "--limit", "5"]);
        match cli.cmd {
            super::super::DraftCommand::List { page, limit } => {
                assert_eq!(page, 3);
                assert_eq!(limit, 5);
            }
            _ => panic!("expected List"),
        }
    }

    // ── draft update ────────────────────────────────────────────────

    #[test]
    fn cli_update_title_only() {
        let cli = TestCli::parse_from(["test", "update", "job-abc", "--title", "new title"]);
        match cli.cmd {
            super::super::DraftCommand::Update { job_id, title, description, budget, .. } => {
                assert_eq!(job_id, "job-abc");
                assert_eq!(title.as_deref(), Some("new title"));
                assert!(description.is_none());
                assert!(budget.is_none());
            }
            _ => panic!("expected Update"),
        }
    }

    #[test]
    fn cli_update_missing_job_id_fails() {
        assert!(TestCli::try_parse_from(["test", "update", "--title", "x"]).is_err());
    }

    #[test]
    fn cli_update_max_budget_long_form() {
        let cli = TestCli::parse_from(["test", "update", "j1", "--max-budget", "200"]);
        match cli.cmd {
            super::super::DraftCommand::Update { max_budget, .. } => {
                assert_eq!(max_budget, Some(200.0));
            }
            _ => panic!("expected Update"),
        }
    }

    // ── draft delete ────────────────────────────────────────────────

    #[test]
    fn cli_delete_parses_job_id() {
        let cli = TestCli::parse_from(["test", "delete", "job-xyz"]);
        match cli.cmd {
            super::super::DraftCommand::Delete { job_id } => {
                assert_eq!(job_id, "job-xyz");
            }
            _ => panic!("expected Delete"),
        }
    }

    #[test]
    fn cli_delete_missing_job_id_fails() {
        assert!(TestCli::try_parse_from(["test", "delete"]).is_err());
    }

    // ── draft publish ───────────────────────────────────────────────

    #[test]
    fn cli_publish_parses_job_id() {
        let cli = TestCli::parse_from(["test", "publish", "job-pub"]);
        match cli.cmd {
            super::super::DraftCommand::Publish { job_id } => {
                assert_eq!(job_id, "job-pub");
            }
            _ => panic!("expected Publish"),
        }
    }

    #[test]
    fn cli_publish_missing_job_id_fails() {
        assert!(TestCli::try_parse_from(["test", "publish"]).is_err());
    }

    // ═══════════════════════════════════════════════════════════════════
    // API path helpers
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn path_update_contains_job_id() {
        assert_eq!(draft_update_path("j-42"), "/priapi/v1/aieco/task/draft/j-42/update");
    }

    #[test]
    fn path_delete_contains_job_id() {
        assert_eq!(draft_delete_path("j-42"), "/priapi/v1/aieco/task/draft/j-42/delete");
    }

    #[test]
    fn path_publish_contains_job_id() {
        assert_eq!(draft_publish_path("j-42"), "/priapi/v1/aieco/task/draft/j-42/publish");
    }

    // ═══════════════════════════════════════════════════════════════════
    // validate_draft_for_publish — boundary & edge cases
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn publish_description_exact_min_ok() {
        let mut d = full_draft_json();
        d["description"] = serde_json::json!("a".repeat(MIN_DESCRIPTION_CHARS));
        assert!(validate_draft_for_publish(&d).is_ok());
    }

    #[test]
    fn publish_description_one_below_min() {
        let mut d = full_draft_json();
        d["description"] = serde_json::json!("a".repeat(MIN_DESCRIPTION_CHARS - 1));
        let err = validate_draft_for_publish(&d).unwrap_err().to_string();
        assert!(err.contains("description"), "{err}");
    }

    #[test]
    fn publish_negative_budget_rejected() {
        let mut d = full_draft_json();
        d["paymentTokenAmount"] = serde_json::json!("-5");
        let err = validate_draft_for_publish(&d).unwrap_err().to_string();
        assert!(err.contains("paymentTokenAmount"), "{err}");
    }

    #[test]
    fn publish_non_numeric_budget_rejected() {
        let mut d = full_draft_json();
        d["paymentTokenAmount"] = serde_json::json!("abc");
        let err = validate_draft_for_publish(&d).unwrap_err().to_string();
        assert!(err.contains("paymentTokenAmount"), "{err}");
    }

    #[test]
    fn publish_non_numeric_max_budget_rejected() {
        let mut d = full_draft_json();
        d["paymentMostTokenAmount"] = serde_json::json!("xyz");
        let err = validate_draft_for_publish(&d).unwrap_err().to_string();
        assert!(err.contains("paymentMostTokenAmount"), "{err}");
    }

    #[test]
    #[allow(non_snake_case)]
    fn publish_accepts_tokenAmount_alias() {
        let d = serde_json::json!({
            "title": "Test",
            "description": "A sufficiently long description for testing purposes here.",
            "tokenSymbol": "USDT",
            "tokenAmount": "10",
            "paymentMostTokenAmount": "20"
        });
        assert!(validate_draft_for_publish(&d).is_ok());
    }

    #[test]
    fn publish_max_budget_equal_to_budget_ok() {
        let mut d = full_draft_json();
        d["paymentTokenAmount"] = serde_json::json!("50");
        d["paymentMostTokenAmount"] = serde_json::json!("50");
        assert!(validate_draft_for_publish(&d).is_ok());
    }

    #[test]
    fn publish_title_null_treated_as_missing() {
        let mut d = full_draft_json();
        d["title"] = serde_json::Value::Null;
        let err = validate_draft_for_publish(&d).unwrap_err().to_string();
        assert!(err.contains("title"), "{err}");
    }

    #[test]
    fn publish_budget_as_number_not_string_rejected() {
        let mut d = full_draft_json();
        d["paymentTokenAmount"] = serde_json::json!(10);
        let err = validate_draft_for_publish(&d).unwrap_err().to_string();
        assert!(err.contains("paymentTokenAmount"), "{err}");
    }

    // ═══════════════════════════════════════════════════════════════════
    // validate_summary — CJK and edge cases
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn summary_cjk_over_max_rejected() {
        let text: String = "中".repeat(MAX_SUMMARY_CHARS + 10);
        assert!(validate_summary(&text).is_err());
    }

    #[test]
    fn summary_empty_string_rejected() {
        assert!(validate_summary("").is_err());
    }

    // ═══════════════════════════════════════════════════════════════════
    // CLI — additional edge cases
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn cli_create_single_attachment() {
        let cli = TestCli::parse_from([
            "test", "create",
            "--title", "t",
            "--description", "desc for test",
            "--description-summary", "sum",
            "--file", "/tmp/a.pdf",
        ]);
        match cli.cmd {
            super::super::DraftCommand::Create { attachments, .. } => {
                assert_eq!(attachments, Some(vec!["/tmp/a.pdf".to_string()]));
            }
            _ => panic!("expected Create"),
        }
    }

    #[test]
    fn cli_create_no_attachments() {
        let cli = TestCli::parse_from([
            "test", "create",
            "--title", "t",
            "--description", "desc for test",
            "--description-summary", "sum",
        ]);
        match cli.cmd {
            super::super::DraftCommand::Create { attachments, .. } => {
                assert!(attachments.is_none());
            }
            _ => panic!("expected Create"),
        }
    }

    #[test]
    fn cli_update_all_optional_fields() {
        let cli = TestCli::parse_from([
            "test", "update", "j-99",
            "--title", "new",
            "--description", "updated description text",
            "--budget", "25",
            "--max-budget", "50",
            "--currency", "USDG",
            "--provider", "prov-456",
        ]);
        match cli.cmd {
            super::super::DraftCommand::Update {
                job_id, title, description, budget, max_budget,
                currency, provider, ..
            } => {
                assert_eq!(job_id, "j-99");
                assert_eq!(title.as_deref(), Some("new"));
                assert_eq!(description.as_deref(), Some("updated description text"));
                assert_eq!(budget, Some(25.0));
                assert_eq!(max_budget, Some(50.0));
                assert_eq!(currency.as_deref(), Some("USDG"));
                assert_eq!(provider.as_deref(), Some("prov-456"));
            }
            _ => panic!("expected Update"),
        }
    }

    #[test]
    fn cli_update_no_optional_fields() {
        let cli = TestCli::parse_from(["test", "update", "j-99"]);
        match cli.cmd {
            super::super::DraftCommand::Update {
                title, description, budget, max_budget,
                currency, provider, ..
            } => {
                assert!(title.is_none());
                assert!(description.is_none());
                assert!(budget.is_none());
                assert!(max_budget.is_none());
                assert!(currency.is_none());
                assert!(provider.is_none());
            }
            _ => panic!("expected Update"),
        }
    }

    #[test]
    fn cli_list_page_zero_ok() {
        let cli = TestCli::parse_from(["test", "list", "--page", "0"]);
        match cli.cmd {
            super::super::DraftCommand::List { page, .. } => {
                assert_eq!(page, 0);
            }
            _ => panic!("expected List"),
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // validate_draft_fields
    // ═══════════════════════════════════════════════════════════════════

    fn vf(
        desc: Option<&str>, title: Option<&str>,
        budget: Option<f64>, max_budget: Option<f64>,
        currency: Option<&str>,
    ) -> serde_json::Value {
        validate_draft_fields(desc, title, budget, max_budget, currency)
    }

    fn vf_ok(
        desc: Option<&str>, title: Option<&str>,
        budget: Option<f64>, max_budget: Option<f64>,
        currency: Option<&str>,
    ) -> bool {
        vf(desc, title, budget, max_budget, currency)
            ["ok"].as_bool().unwrap()
    }

    fn vf_errors(
        desc: Option<&str>, title: Option<&str>,
        budget: Option<f64>, max_budget: Option<f64>,
        currency: Option<&str>,
    ) -> Vec<String> {
        let v = vf(desc, title, budget, max_budget, currency);
        v["errors"].as_array()
            .map(|a| a.iter().map(|e| e.as_str().unwrap().to_string()).collect())
            .unwrap_or_default()
    }

    fn vf_checks(
        desc: Option<&str>, title: Option<&str>,
        budget: Option<f64>, max_budget: Option<f64>,
        currency: Option<&str>,
    ) -> Vec<serde_json::Value> {
        let v = vf(desc, title, budget, max_budget, currency);
        v["checks"].as_array().unwrap().clone()
    }

    // ── all fields valid ───────────────────────────────────────────

    #[test]
    fn validate_all_fields_ok() {
        assert!(vf_ok(
            Some("查询河南省明天天气，包括温度、湿度、降雨概率"),
            Some("查询河南天气"),
            Some(0.01), Some(0.011),
            Some("USDT"),
        ));
    }

    // ── no fields → ok (nothing to check) ──────────────────────────

    #[test]
    fn validate_no_fields_ok() {
        assert!(vf_ok(None, None, None, None, None));
    }

    #[test]
    fn validate_no_fields_empty_checks() {
        let checks = vf_checks(None, None, None, None, None);
        assert!(checks.is_empty());
    }

    // ── description ────────────────────────────────────────────────

    #[test]
    fn validate_description_too_short() {
        let errs = vf_errors(Some("短"), None, None, None, None);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("description"));
    }

    #[test]
    fn validate_description_exact_min() {
        let desc: String = "a".repeat(MIN_DESCRIPTION_CHARS);
        assert!(vf_ok(Some(&desc), None, None, None, None));
    }

    #[test]
    fn validate_description_over_max() {
        let desc: String = "a".repeat(MAX_DESCRIPTION_CHARS + 1);
        let errs = vf_errors(Some(&desc), None, None, None, None);
        assert!(errs[0].contains("description"));
    }

    // ── title ──────────────────────────────────────────────────────

    #[test]
    fn validate_title_empty() {
        let errs = vf_errors(None, Some(""), None, None, None);
        assert!(errs[0].contains("title"));
    }

    #[test]
    fn validate_title_over_limit() {
        let t: String = "x".repeat(MAX_TITLE_CHARS + 1);
        let errs = vf_errors(None, Some(&t), None, None, None);
        assert!(errs[0].contains("title"));
    }

    #[test]
    fn validate_title_exact_limit() {
        let t: String = "x".repeat(MAX_TITLE_CHARS);
        assert!(vf_ok(None, Some(&t), None, None, None));
    }

    // ── currency ───────────────────────────────────────────────────

    #[test]
    fn validate_currency_usdt_normalized() {
        let checks = vf_checks(None, None, None, None, Some("usdt"));
        let c = checks.iter().find(|c| c["field"] == "currency").unwrap();
        assert_eq!(c["ok"], true);
        assert_eq!(c["normalized"], "USDT");
    }

    #[test]
    fn validate_currency_unsupported() {
        let errs = vf_errors(None, None, None, None, Some("BTC"));
        assert!(errs[0].contains("unsupported"));
    }

    // ── budget ─────────────────────────────────────────────────────

    #[test]
    fn validate_budget_zero() {
        let errs = vf_errors(None, None, Some(0.0), None, None);
        assert!(errs[0].contains("budget"));
    }

    #[test]
    fn validate_budget_too_many_decimals() {
        let errs = vf_errors(None, None, Some(1.123456), None, None);
        assert!(errs[0].contains("decimal"));
    }

    #[test]
    fn validate_budget_exceeds_max() {
        let errs = vf_errors(None, None, Some(10_000_001.0), None, None);
        assert!(errs[0].contains("budget"));
    }

    // ── max_budget vs budget ───────────────────────────────────────

    #[test]
    fn validate_max_budget_less_than_budget() {
        let errs = vf_errors(None, None, Some(100.0), Some(50.0), None);
        assert!(errs.iter().any(|e| e.contains("max_budget")));
    }

    #[test]
    fn validate_max_budget_equal_ok() {
        assert!(vf_ok(None, None, Some(10.0), Some(10.0), None));
    }

    // ── multiple errors collected ──────────────────────────────────

    #[test]
    fn validate_multiple_errors_all_collected() {
        let errs = vf_errors(
            Some("短"), Some(""), Some(0.0), Some(0.0), Some("ETH"),
        );
        assert!(errs.len() >= 4);
        assert!(errs.iter().any(|e| e.contains("description")));
        assert!(errs.iter().any(|e| e.contains("title")));
        assert!(errs.iter().any(|e| e.contains("unsupported")));
        assert!(errs.iter().any(|e| e.contains("budget")));
    }

    // ── clap: validate subcommand ──────────────────────────────────

    #[test]
    fn cli_validate_all_fields() {
        let cli = TestCli::parse_from([
            "test", "validate",
            "--description", "a long description for testing",
            "--title", "test title",
            "--budget", "50",
            "--max-budget", "100",
            "--currency", "USDT",
        ]);
        match cli.cmd {
            super::super::DraftCommand::Validate {
                description, title, budget, max_budget, currency,
            } => {
                assert_eq!(description.as_deref(), Some("a long description for testing"));
                assert_eq!(title.as_deref(), Some("test title"));
                assert_eq!(budget, Some(50.0));
                assert_eq!(max_budget, Some(100.0));
                assert_eq!(currency.as_deref(), Some("USDT"));
            }
            _ => panic!("expected Validate"),
        }
    }

    #[test]
    fn cli_validate_no_fields() {
        let cli = TestCli::parse_from(["test", "validate"]);
        match cli.cmd {
            super::super::DraftCommand::Validate {
                description, title, budget, max_budget, currency,
            } => {
                assert!(description.is_none());
                assert!(title.is_none());
                assert!(budget.is_none());
                assert!(max_budget.is_none());
                assert!(currency.is_none());
            }
            _ => panic!("expected Validate"),
        }
    }

    #[test]
    fn cli_validate_partial_fields() {
        let cli = TestCli::parse_from([
            "test", "validate", "--description", "just checking description", "--currency", "usdg",
        ]);
        match cli.cmd {
            super::super::DraftCommand::Validate { description, currency, budget, .. } => {
                assert_eq!(description.as_deref(), Some("just checking description"));
                assert_eq!(currency.as_deref(), Some("usdg"));
                assert!(budget.is_none());
            }
            _ => panic!("expected Validate"),
        }
    }
}
