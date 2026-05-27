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
};
use crate::commands::agent_commerce::task::signing;

use super::create::{
    normalize_currency, parse_duration_secs, resolve_buyer_agent, validate_budget,
    validate_budget_decimals, ACCEPT_MAX, ACCEPT_MIN,
    MAX_DESCRIPTION_CHARS, MAX_SUMMARY_CHARS, MAX_TITLE_CHARS, MIN_DESCRIPTION_CHARS,
    SUBMIT_MAX, SUBMIT_MIN,
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

// ─── Validation (optional fields: only validate when present) ───────────

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

fn validate_description_opt(desc: Option<&str>) -> Result<()> {
    if let Some(d) = desc {
        let len = d.chars().count();
        if len < MIN_DESCRIPTION_CHARS {
            bail!("description is too short (minimum {MIN_DESCRIPTION_CHARS} chars, currently {len})");
        }
        if len > MAX_DESCRIPTION_CHARS {
            bail!("description may not exceed {MAX_DESCRIPTION_CHARS} chars (currently {len})");
        }
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

fn validate_deadline_open_opt(dl: Option<&str>) -> Result<Option<u64>> {
    match dl {
        Some(s) => {
            let secs = parse_duration_secs(s)
                .map_err(|e| anyhow::anyhow!("--deadline-open {e}"))?;
            if secs < ACCEPT_MIN {
                bail!("--deadline-open must be at least 10m; current value {s}, allowed range 10m ~ 180d");
            }
            if secs > ACCEPT_MAX {
                bail!("--deadline-open must not exceed 180d; current value {s}, allowed range 10m ~ 180d");
            }
            Ok(Some(secs))
        }
        None => Ok(None),
    }
}

fn validate_deadline_submit_opt(dl: Option<&str>) -> Result<Option<u64>> {
    match dl {
        Some(s) => {
            let secs = parse_duration_secs(s)
                .map_err(|e| anyhow::anyhow!("--deadline-submit {e}"))?;
            if secs < SUBMIT_MIN {
                bail!("--deadline-submit must be at least 1m; current value {s}, allowed range 1m ~ 180d");
            }
            if secs > SUBMIT_MAX {
                bail!("--deadline-submit must not exceed 180d; current value {s}, allowed range 1m ~ 180d");
            }
            Ok(Some(secs))
        }
        None => Ok(None),
    }
}

fn make_summary(description: Option<&str>) -> Option<String> {
    description.map(|d| d.chars().take(MAX_SUMMARY_CHARS).collect())
}

// ─── 1. draft create ────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub async fn handle_draft_create(
    client: &mut TaskApiClient,
    title: &str,
    description: Option<&str>,
    budget: Option<f64>,
    max_budget: Option<f64>,
    currency: Option<&str>,
    deadline_open: Option<&str>,
    deadline_submit: Option<&str>,
    provider: Option<&str>,
    attachments: Option<&[String]>,
) -> Result<()> {
    validate_title(title)?;
    validate_description_opt(description)?;
    validate_budget_opt(budget)?;
    if let (Some(b), Some(mb)) = (budget, max_budget) {
        if mb < b {
            bail!("--max-budget ({mb}) may not be less than --budget ({b})");
        }
    }
    validate_budget_opt(max_budget)?;
    let currency_norm = validate_currency_opt(currency)?;
    let open_secs = validate_deadline_open_opt(deadline_open)?;
    let submit_secs = validate_deadline_submit_opt(deadline_submit)?;

    ensure_tokens_refreshed().await
        .map_err(|e| anyhow::anyhow!("session has expired; run `onchainos wallet login` first: {e}"))?;

    let (buyer_agent_id, _) = resolve_buyer_agent().await?;
    eprintln!("[draft-create] buyer identity check passed (agentId: {buyer_agent_id})");

    let balance_warning = if let (Some(b), Some(ref c)) = (budget, &currency_norm) {
        match common::ensure_sufficient_balance(b, c).await {
            Err(e) => {
                eprintln!("[draft-create] ⚠ balance warning: {e}");
                Some(format!(
                    "⚠️ Insufficient {c} balance on XLayer (need {b} {c}). Draft saved, but payment may fail when publishing — please top up via swap."
                ))
            }
            Ok(()) => None,
        }
    } else {
        None
    };

    let summary = make_summary(description);

    let mut body = serde_json::json!({
        "title": title,
        "paymentMode": 0,
        "visibility": 1,
    });
    let obj = body.as_object_mut().unwrap();

    if let Some(d) = description {
        obj.insert("description".into(), serde_json::json!(d));
    }
    if let Some(s) = &summary {
        obj.insert("descriptionSummary".into(), serde_json::json!(s));
    }
    if let Some(ref c) = currency_norm {
        obj.insert("paymentTokenSymbol".into(), serde_json::json!(c.to_uppercase()));
    }
    if let Some(b) = budget {
        obj.insert("paymentTokenAmount".into(), serde_json::json!(b.to_string()));
    }
    if let Some(mb) = max_budget {
        obj.insert("paymentMostTokenAmount".into(), serde_json::json!(mb.to_string()));
    }
    if open_secs.is_some() || submit_secs.is_some() {
        let mut expire = serde_json::Map::new();
        if let Some(o) = open_secs {
            expire.insert("acceptDeadline".into(), serde_json::json!(o));
        }
        if let Some(s) = submit_secs {
            expire.insert("submittedDeadline".into(), serde_json::json!(s));
        }
        obj.insert("expireConfig".into(), serde_json::Value::Object(expire));
    }
    if let Some(pid) = provider {
        obj.insert("providerAgentId".into(), serde_json::json!(pid));
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
    budget: Option<f64>,
    max_budget: Option<f64>,
    currency: Option<&str>,
    deadline_open: Option<&str>,
    deadline_submit: Option<&str>,
    provider: Option<&str>,
) -> Result<()> {
    if let Some(t) = title {
        validate_title(t)?;
    }
    validate_description_opt(description)?;
    validate_budget_opt(budget)?;
    if let (Some(b), Some(mb)) = (budget, max_budget) {
        if mb < b {
            bail!("--max-budget ({mb}) may not be less than --budget ({b})");
        }
    }
    validate_budget_opt(max_budget)?;
    let currency_norm = validate_currency_opt(currency)?;
    let open_secs = validate_deadline_open_opt(deadline_open)?;
    let submit_secs = validate_deadline_submit_opt(deadline_submit)?;

    ensure_tokens_refreshed().await
        .map_err(|e| anyhow::anyhow!("session has expired; run `onchainos wallet login` first: {e}"))?;

    let (buyer_agent_id, _) = resolve_buyer_agent().await?;
    eprintln!("[draft-update] buyer identity check passed (agentId: {buyer_agent_id})");

    let mut body = serde_json::Map::new();

    if let Some(t) = title {
        body.insert("title".into(), serde_json::json!(t));
    }
    if let Some(d) = description {
        body.insert("description".into(), serde_json::json!(d));
        let summary: String = d.chars().take(MAX_SUMMARY_CHARS).collect();
        body.insert("descriptionSummary".into(), serde_json::json!(summary));
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
    if open_secs.is_some() || submit_secs.is_some() {
        let mut expire = serde_json::Map::new();
        if let Some(o) = open_secs {
            expire.insert("acceptDeadline".into(), serde_json::json!(o));
        }
        if let Some(s) = submit_secs {
            expire.insert("submittedDeadline".into(), serde_json::json!(s));
        }
        body.insert("expireConfig".into(), serde_json::Value::Object(expire));
    }
    if let Some(pid) = provider {
        body.insert("providerAgentId".into(), serde_json::json!(pid));
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
    eprintln!("[draft-publish] buyer identity check passed (agentId: {buyer_agent_id})");

    // ── Fetch draft detail for pre-publish validation ────────────
    let detail = client
        .get_with_identity(&client.task_path(job_id), &buyer_agent_id)
        .await
        .map_err(|e| anyhow::anyhow!("failed to fetch draft detail: {e}"))?;

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

    let expire = &detail["expireConfig"];
    let accept_dl = expire["acceptDeadline"].as_u64().unwrap_or(0);
    if !(ACCEPT_MIN..=ACCEPT_MAX).contains(&accept_dl) {
        missing.push("expireConfig.acceptDeadline (10m ~ 180d)");
    }
    let submit_dl = expire["submittedDeadline"].as_u64().unwrap_or(0);
    if !(SUBMIT_MIN..=SUBMIT_MAX).contains(&submit_dl) {
        missing.push("expireConfig.submittedDeadline (1m ~ 180d)");
    }

    if !missing.is_empty() {
        let list = missing
            .iter()
            .map(|m| format!("  - {m}"))
            .collect::<Vec<_>>()
            .join("\n");
        bail!(
            "Draft is missing required fields for publishing:\n{list}\n\
             Please update the draft with the missing fields, then retry publish."
        );
    }

    // ── Balance check (blocking for publish) ─────────────────────
    if !token_sym.is_empty() && token_amount > 0.0 {
        common::ensure_sufficient_balance(token_amount, token_sym).await
            .map_err(|e| anyhow::anyhow!(
                "Insufficient {token_sym} balance on XLayer (need {token_amount} {token_sym}): {e}. \
                 Please top up via swap before publishing."
            ))?;
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

    // ── Sign + Broadcast ─────────────────────────────────────────
    let (account_id, address) = signing::resolve_wallet(None, None)?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client,
        &resp["uopData"],
        &account_id,
        &address,
        returned_job_id,
        1,
        &buyer_agent_id,
        None,
    )
    .await?;

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
        println!("  Designated provider: {pid} (skip recommend, direct routing)");
    }
    println!();
    if provider_id.is_some() {
        println!("Next: wait for the job_created notification; the designated provider's service will be queried and routed automatically.");
    } else {
        println!("Next: onchainos agent recommend {returned_job_id}");
    }
    Ok(())
}
