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
             🛑 Do NOT auto-fill these fields. Ask the user to provide values for each missing field, \
             then call `draft update` with the user's values, then retry `draft publish`."
        );
    }
    Ok(())
}

// ─── 1. draft create ────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub async fn handle_draft_create(
    client: &mut TaskApiClient,
    title: &str,
    description: Option<&str>,
    description_summary: Option<&str>,
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

    let summary = description_summary.map(|s| s.to_string()).or_else(|| make_summary(description));

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
    let (account_id, address) = signing::resolve_wallet_by_agent_id(&buyer_agent_id).await?;

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
        println!("  Designated provider: {pid}");
    }
    println!();
    if provider_id.is_some() {
        println!("Next: wait for the on-chain confirmation; the designated provider will be contacted automatically.");
    } else {
        println!("Next: wait for the on-chain confirmation; provider recommendations will be generated automatically.");
    }
    if super::content::is_cli_mode() {
        println!("Due to client limitations, you need to proactively ask me to \"monitor task progress\" for me to continue pushing updates to you.");
        println!();
        println!("🛑 [Watch] You MUST now execute the watch command below to monitor task progress — do NOT skip or end the turn without running it:");
        println!("okx-a2a user watch --once --json --timeout 300 --poll-ms 1000 --limit 50");
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

    // ── validate_deadline_open_opt ──────────────────────────────────

    #[test]
    fn deadline_open_none_ok() {
        assert_eq!(validate_deadline_open_opt(None).unwrap(), None);
    }

    #[test]
    fn deadline_open_exact_min_ok() {
        assert_eq!(validate_deadline_open_opt(Some("10m")).unwrap(), Some(600));
    }

    #[test]
    fn deadline_open_below_min_rejected() {
        assert!(validate_deadline_open_opt(Some("9m")).is_err());
    }

    #[test]
    fn deadline_open_exact_max_ok() {
        assert_eq!(validate_deadline_open_opt(Some("180d")).unwrap(), Some(180 * 86400));
    }

    #[test]
    fn deadline_open_over_max_rejected() {
        assert!(validate_deadline_open_opt(Some("181d")).is_err());
    }

    #[test]
    fn deadline_open_hours_ok() {
        assert_eq!(validate_deadline_open_opt(Some("2h")).unwrap(), Some(7200));
    }

    #[test]
    fn deadline_open_invalid_format_rejected() {
        assert!(validate_deadline_open_opt(Some("abc")).is_err());
    }

    // ── validate_deadline_submit_opt ────────────────────────────────

    #[test]
    fn deadline_submit_none_ok() {
        assert_eq!(validate_deadline_submit_opt(None).unwrap(), None);
    }

    #[test]
    fn deadline_submit_exact_min_ok() {
        assert_eq!(validate_deadline_submit_opt(Some("1m")).unwrap(), Some(60));
    }

    #[test]
    fn deadline_submit_below_min_rejected() {
        assert!(validate_deadline_submit_opt(Some("30s")).is_err());
    }

    #[test]
    fn deadline_submit_exact_max_ok() {
        assert_eq!(validate_deadline_submit_opt(Some("180d")).unwrap(), Some(180 * 86400));
    }

    #[test]
    fn deadline_submit_over_max_rejected() {
        assert!(validate_deadline_submit_opt(Some("181d")).is_err());
    }

    // ── make_summary ────────────────────────────────────────────────

    #[test]
    fn summary_none_returns_none() {
        assert_eq!(make_summary(None), None);
    }

    #[test]
    fn summary_short_text_unchanged() {
        assert_eq!(make_summary(Some("hello world")), Some("hello world".to_string()));
    }

    #[test]
    fn summary_truncates_at_max() {
        let text: String = "a".repeat(MAX_SUMMARY_CHARS + 50);
        let result = make_summary(Some(&text)).unwrap();
        assert_eq!(result.chars().count(), MAX_SUMMARY_CHARS);
    }

    #[test]
    fn summary_exact_max_unchanged() {
        let text: String = "b".repeat(MAX_SUMMARY_CHARS);
        assert_eq!(make_summary(Some(&text)), Some(text));
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
            "paymentMostTokenAmount": "20",
            "expireConfig": {
                "acceptDeadline": 3600,
                "submittedDeadline": 3600
            }
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
    fn publish_missing_accept_deadline() {
        let mut d = full_draft_json();
        d["expireConfig"]["acceptDeadline"] = serde_json::json!(0);
        let err = validate_draft_for_publish(&d).unwrap_err().to_string();
        assert!(err.contains("acceptDeadline"), "{err}");
    }

    #[test]
    fn publish_missing_submit_deadline() {
        let mut d = full_draft_json();
        d["expireConfig"]["submittedDeadline"] = serde_json::json!(0);
        let err = validate_draft_for_publish(&d).unwrap_err().to_string();
        assert!(err.contains("submittedDeadline"), "{err}");
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
        assert!(err.contains("acceptDeadline"));
        assert!(err.contains("submittedDeadline"));
    }

    #[test]
    #[allow(non_snake_case)]
    fn publish_accepts_tokenSymbol_alias() {
        let d = serde_json::json!({
            "title": "Test task",
            "description": "A sufficiently long description for testing purposes here.",
            "tokenSymbol": "USDT",
            "tokenAmount": "10",
            "paymentMostTokenAmount": "20",
            "expireConfig": { "acceptDeadline": 3600, "submittedDeadline": 3600 }
        });
        assert!(validate_draft_for_publish(&d).is_ok());
    }

    #[test]
    fn publish_accept_deadline_out_of_range() {
        let mut d = full_draft_json();
        d["expireConfig"]["acceptDeadline"] = serde_json::json!(1);
        assert!(validate_draft_for_publish(&d).unwrap_err().to_string().contains("acceptDeadline"));
    }

    #[test]
    fn publish_submit_deadline_out_of_range() {
        let mut d = full_draft_json();
        d["expireConfig"]["submittedDeadline"] = serde_json::json!(180 * 86400 + 1);
        assert!(validate_draft_for_publish(&d).unwrap_err().to_string().contains("submittedDeadline"));
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
    fn cli_create_title_only() {
        let cli = TestCli::parse_from(["test", "create", "--title", "my task"]);
        match cli.cmd {
            super::super::DraftCommand::Create { title, description, budget, .. } => {
                assert_eq!(title, "my task");
                assert!(description.is_none());
                assert!(budget.is_none());
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
            "--budget", "50.5",
            "--max-budget", "100",
            "--currency", "USDT",
            "--deadline-open", "3d",
            "--deadline-submit", "7d",
            "--provider", "agent-123",
            "--file", "/tmp/a.pdf",
            "--file", "/tmp/b.png",
        ]);
        match cli.cmd {
            super::super::DraftCommand::Create {
                title, description, budget, max_budget,
                currency, deadline_open, deadline_submit,
                provider, attachments, ..
            } => {
                assert_eq!(title, "full task");
                assert_eq!(description.as_deref(), Some("a long description here"));
                assert_eq!(budget, Some(50.5));
                assert_eq!(max_budget, Some(100.0));
                assert_eq!(currency.as_deref(), Some("USDT"));
                assert_eq!(deadline_open.as_deref(), Some("3d"));
                assert_eq!(deadline_submit.as_deref(), Some("7d"));
                assert_eq!(provider.as_deref(), Some("agent-123"));
                assert_eq!(attachments, Some(vec!["/tmp/a.pdf".to_string(), "/tmp/b.png".to_string()]));
            }
            _ => panic!("expected Create"),
        }
    }

    #[test]
    fn cli_create_missing_title_fails() {
        assert!(TestCli::try_parse_from(["test", "create"]).is_err());
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

    #[test]
    fn cli_update_deadline_flags() {
        let cli = TestCli::parse_from([
            "test", "update", "j1",
            "--deadline-open", "1h",
            "--deadline-submit", "2d",
        ]);
        match cli.cmd {
            super::super::DraftCommand::Update { deadline_open, deadline_submit, .. } => {
                assert_eq!(deadline_open.as_deref(), Some("1h"));
                assert_eq!(deadline_submit.as_deref(), Some("2d"));
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
    fn publish_expire_config_missing_entirely() {
        let mut d = full_draft_json();
        d.as_object_mut().unwrap().remove("expireConfig");
        let err = validate_draft_for_publish(&d).unwrap_err().to_string();
        assert!(err.contains("acceptDeadline"), "{err}");
        assert!(err.contains("submittedDeadline"), "{err}");
    }

    #[test]
    fn publish_accept_deadline_exact_min_ok() {
        let mut d = full_draft_json();
        d["expireConfig"]["acceptDeadline"] = serde_json::json!(ACCEPT_MIN);
        assert!(validate_draft_for_publish(&d).is_ok());
    }

    #[test]
    fn publish_accept_deadline_exact_max_ok() {
        let mut d = full_draft_json();
        d["expireConfig"]["acceptDeadline"] = serde_json::json!(ACCEPT_MAX);
        assert!(validate_draft_for_publish(&d).is_ok());
    }

    #[test]
    fn publish_accept_deadline_below_min_rejected() {
        let mut d = full_draft_json();
        d["expireConfig"]["acceptDeadline"] = serde_json::json!(ACCEPT_MIN - 1);
        let err = validate_draft_for_publish(&d).unwrap_err().to_string();
        assert!(err.contains("acceptDeadline"), "{err}");
    }

    #[test]
    fn publish_accept_deadline_above_max_rejected() {
        let mut d = full_draft_json();
        d["expireConfig"]["acceptDeadline"] = serde_json::json!(ACCEPT_MAX + 1);
        let err = validate_draft_for_publish(&d).unwrap_err().to_string();
        assert!(err.contains("acceptDeadline"), "{err}");
    }

    #[test]
    fn publish_submit_deadline_exact_min_ok() {
        let mut d = full_draft_json();
        d["expireConfig"]["submittedDeadline"] = serde_json::json!(SUBMIT_MIN);
        assert!(validate_draft_for_publish(&d).is_ok());
    }

    #[test]
    fn publish_submit_deadline_exact_max_ok() {
        let mut d = full_draft_json();
        d["expireConfig"]["submittedDeadline"] = serde_json::json!(SUBMIT_MAX);
        assert!(validate_draft_for_publish(&d).is_ok());
    }

    #[test]
    fn publish_submit_deadline_below_min_rejected() {
        let mut d = full_draft_json();
        d["expireConfig"]["submittedDeadline"] = serde_json::json!(SUBMIT_MIN - 1);
        let err = validate_draft_for_publish(&d).unwrap_err().to_string();
        assert!(err.contains("submittedDeadline"), "{err}");
    }

    #[test]
    fn publish_submit_deadline_above_max_rejected() {
        let mut d = full_draft_json();
        d["expireConfig"]["submittedDeadline"] = serde_json::json!(SUBMIT_MAX + 1);
        let err = validate_draft_for_publish(&d).unwrap_err().to_string();
        assert!(err.contains("submittedDeadline"), "{err}");
    }

    #[test]
    #[allow(non_snake_case)]
    fn publish_accepts_tokenAmount_alias() {
        let d = serde_json::json!({
            "title": "Test",
            "description": "A sufficiently long description for testing purposes here.",
            "tokenSymbol": "USDT",
            "tokenAmount": "10",
            "paymentMostTokenAmount": "20",
            "expireConfig": { "acceptDeadline": 3600, "submittedDeadline": 3600 }
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
    // Additional format parsing
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn deadline_open_days_format() {
        let result = validate_deadline_open_opt(Some("7d")).unwrap();
        assert_eq!(result, Some(7 * 86400));
    }

    #[test]
    fn deadline_submit_hours_format() {
        let result = validate_deadline_submit_opt(Some("2h")).unwrap();
        assert_eq!(result, Some(7200));
    }

    // ═══════════════════════════════════════════════════════════════════
    // make_summary — CJK and edge cases
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn summary_cjk_truncates_by_chars() {
        let text: String = "中".repeat(MAX_SUMMARY_CHARS + 10);
        let result = make_summary(Some(&text)).unwrap();
        assert_eq!(result.chars().count(), MAX_SUMMARY_CHARS);
    }

    #[test]
    fn summary_empty_string_returns_some_empty() {
        assert_eq!(make_summary(Some("")), Some(String::new()));
    }

    // ═══════════════════════════════════════════════════════════════════
    // CLI — additional edge cases
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn cli_create_single_attachment() {
        let cli = TestCli::parse_from([
            "test", "create", "--title", "t", "--file", "/tmp/a.pdf",
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
        let cli = TestCli::parse_from(["test", "create", "--title", "t"]);
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
            "--deadline-open", "1d",
            "--deadline-submit", "3d",
            "--provider", "prov-456",
        ]);
        match cli.cmd {
            super::super::DraftCommand::Update {
                job_id, title, description, budget, max_budget,
                currency, deadline_open, deadline_submit, provider,
            } => {
                assert_eq!(job_id, "j-99");
                assert_eq!(title.as_deref(), Some("new"));
                assert_eq!(description.as_deref(), Some("updated description text"));
                assert_eq!(budget, Some(25.0));
                assert_eq!(max_budget, Some(50.0));
                assert_eq!(currency.as_deref(), Some("USDG"));
                assert_eq!(deadline_open.as_deref(), Some("1d"));
                assert_eq!(deadline_submit.as_deref(), Some("3d"));
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
                currency, deadline_open, deadline_submit, provider, ..
            } => {
                assert!(title.is_none());
                assert!(description.is_none());
                assert!(budget.is_none());
                assert!(max_budget.is_none());
                assert!(currency.is_none());
                assert!(deadline_open.is_none());
                assert!(deadline_submit.is_none());
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
}
