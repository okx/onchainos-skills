/// Trading Competition commands.
///
/// Public endpoints (no auth):
///   GET /priapi/v1/dapp/agentic/competition/list
///   GET /priapi/v1/dapp/agentic/competition/detail
///   GET /priapi/v1/dapp/agentic/competition/rank
///   GET /priapi/v1/dapp/agentic/competition/userStatus
///
/// Authenticated endpoints (JWT required — Authorization: Bearer <accessToken>):
///   POST /priapi/v5/wallet/agentic/competition/join
///   POST /priapi/v5/wallet/agentic/competition/claim
use anyhow::{bail, Result};
use clap::Subcommand;
use serde_json::{json, Value};

use super::Context;
use crate::client::ApiClient;
use crate::output;
use crate::wallet_store;

#[derive(Subcommand)]
pub enum CompetitionCommand {
    /// List Agentic Wallet exclusive trading competitions
    List {
        /// Page size (default 10)
        #[arg(long, default_value = "10")]
        page_size: u32,
        /// Page number starting from 1
        #[arg(long, default_value = "1")]
        page_num: u32,
        /// Request filter: 0=active, 1=ended, 2=all (default 0).
        /// NOTE: response activity.status uses a DIFFERENT set: 3=active, 4=ended.
        #[arg(long, default_value = "0")]
        status: u32,
    },
    /// Get competition details: rules, prize pools, chain, timeline
    Detail {
        /// Activity ID from `competition list`
        #[arg(long)]
        activity_id: String,
    },
    /// Get leaderboard and current user ranking
    Rank {
        /// Activity ID
        #[arg(long)]
        activity_id: String,
        /// User wallet address
        #[arg(long)]
        wallet: String,
        /// Sort type: 1=PnL% (realized ROI), 7=PnL (realized profit). The exact values for a
        /// given competition come from `competition detail` → `tabConfigs[].rankFieldConfig[].sortValueMap.descend`;
        /// future activities may add more. Default 1 matches the typical primary leaderboard.
        #[arg(long, default_value = "1")]
        sort_type: i32,
        /// Max leaderboard entries to return (default 20, max 100)
        #[arg(long, default_value = "20")]
        limit: u32,
    },
    /// Get user participation and reward status (omit --activity-id to check all activities)
    UserStatus {
        /// Activity ID (omit to check all activities including ended ones)
        #[arg(long)]
        activity_id: Option<String>,
        /// EVM wallet address
        #[arg(long)]
        evm_wallet: String,
        /// SOL wallet address
        #[arg(long)]
        sol_wallet: String,
    },
    /// Join a trading competition (requires wallet login)
    Join {
        /// Activity ID
        #[arg(long)]
        activity_id: String,
        /// EVM wallet address to register
        #[arg(long)]
        evm_wallet: String,
        /// SOL wallet address to register
        #[arg(long)]
        sol_wallet: String,
        /// Chain ID of the competition chain (e.g. "1" for Ethereum)
        #[arg(long)]
        chain_index: String,
    },
    /// Claim competition rewards: pre-checks rewardStatus, fetches calldata,
    /// signs each entry via TEE session, broadcasts, and returns txHash array.
    /// Requires wallet login.
    Claim {
        /// Activity ID
        #[arg(long)]
        activity_id: String,
        /// EVM wallet address
        #[arg(long)]
        evm_wallet: String,
        /// SOL wallet address
        #[arg(long)]
        sol_wallet: String,
    },
}

pub async fn execute(ctx: &Context, command: CompetitionCommand) -> Result<()> {
    let mut client = ctx.client_async().await?;
    let data = match command {
        CompetitionCommand::List {
            page_size,
            page_num,
            status,
        } => list(&mut client, page_size, page_num, Some(status)).await?,
        CompetitionCommand::Detail { activity_id } => detail(&mut client, &activity_id).await?,
        CompetitionCommand::Rank {
            activity_id,
            wallet,
            sort_type,
            limit,
        } => rank(&mut client, &activity_id, &wallet, sort_type, limit).await?,
        CompetitionCommand::UserStatus {
            activity_id,
            evm_wallet,
            sol_wallet,
        } => user_status_all(&mut client, activity_id.as_deref(), &evm_wallet, &sol_wallet).await?,
        CompetitionCommand::Join {
            activity_id,
            evm_wallet,
            sol_wallet,
            chain_index,
        } => join(&mut client, &activity_id, &evm_wallet, &sol_wallet, &chain_index).await?,
        // CLI invocation goes through the atomic `claim_and_submit` flow —
        // same path the MCP wrapper uses — so users (and AI shelling out to
        // Bash) get a one-shot claim that signs + broadcasts and returns
        // txHashes. The bare `claim()` API call returns only unsigned
        // calldata, which on Solana is delivered as a `tx.data` byte array
        // that needs base58 encoding before signing — too easy to get
        // wrong outside of this code path.
        CompetitionCommand::Claim {
            activity_id,
            evm_wallet,
            sol_wallet,
        } => claim_and_submit(&mut client, &activity_id, &evm_wallet, &sol_wallet).await?,
    };
    output::success(data);
    Ok(())
}

// ── Public API (shared by CLI and MCP) ───────────────────────────────

/// GET /priapi/v1/dapp/agentic/competition/list
pub async fn list(
    client: &mut ApiClient,
    page_size: u32,
    page_num: u32,
    status: Option<u32>,
) -> Result<Value> {
    let page_size_s = page_size.to_string();
    let page_num_s = page_num.to_string();
    let status_s = status.map(|s| s.to_string());

    let mut query: Vec<(&str, &str)> = vec![
        ("pageSize", &page_size_s),
        ("pageNum", &page_num_s),
    ];
    if let Some(ref s) = status_s {
        query.push(("status", s));
    }

    client
        .get("/priapi/v1/dapp/agentic/competition/list", &query)
        .await
}

/// GET /priapi/v1/dapp/agentic/competition/detail
pub async fn detail(client: &mut ApiClient, activity_id: &str) -> Result<Value> {
    client
        .get(
            "/priapi/v1/dapp/agentic/competition/detail",
            &[("activityId", activity_id)],
        )
        .await
}

/// GET /priapi/v1/dapp/agentic/competition/rank
/// `limit` is applied client-side by truncating `allRankInfos` (not a server param).
pub async fn rank(
    client: &mut ApiClient,
    activity_id: &str,
    wallet: &str,
    sort_type: i32,
    limit: u32,
) -> Result<Value> {
    let sort_type_s = sort_type.to_string();
    let mut data = client
        .get(
            "/priapi/v1/dapp/agentic/competition/rank",
            &[
                ("activityId", activity_id),
                ("walletAddress", wallet),
                ("sortType", &sort_type_s),
            ],
        )
        .await?;

    // Truncate allRankInfos client-side
    let cap = limit.min(100) as usize;
    if let Some(arr) = data["allRankInfos"].as_array() {
        let truncated: Vec<Value> = arr.iter().take(cap).cloned().collect();
        data["allRankInfos"] = json!(truncated);
    }

    Ok(data)
}

/// GET /priapi/v1/dapp/agentic/competition/userStatus
pub async fn user_status(
    client: &mut ApiClient,
    activity_id: &str,
    wallet: &str,
) -> Result<Value> {
    client
        .get(
            "/priapi/v1/dapp/agentic/competition/userStatus",
            &[("activityId", activity_id), ("walletAddress", wallet)],
        )
        .await
}

/// If activity_id is Some, query that single activity.
/// If None, fetch all activities (status=2) and query each one, returning an array.
/// Uses evm_wallet for EVM chains and sol_wallet for Solana chains.
pub async fn user_status_all(
    client: &mut ApiClient,
    activity_id: Option<&str>,
    evm_wallet: &str,
    sol_wallet: &str,
) -> Result<Value> {
    if let Some(id) = activity_id {
        let detail_data = detail(client, id).await?;
        let wallet = pick_wallet_by_chain(&detail_data, evm_wallet, sol_wallet);
        return user_status(client, id, wallet).await;
    }

    // Fetch all activities (active + ended)
    let list_data = list(client, 100, 1, Some(2)).await?;
    let activities = match list_data["availableCompetitions"].as_array() {
        Some(a) => a.clone(),
        None => return Ok(json!([])),
    };

    let mut results = Vec::new();
    for activity in &activities {
        let id = match activity["id"].as_u64() {
            Some(i) => i.to_string(),
            None => continue,
        };
        let wallet = pick_wallet_by_chain(activity, evm_wallet, sol_wallet);
        let status = user_status(client, &id, wallet).await?;
        // activityStatus: 3=active, 4=ended
        results.push(json!({
            "activityId": activity["id"],
            "activityName": activity["name"],
            "shortName": activity["shortName"],
            "chainName": activity["chainName"],
            "activityStatus": activity["status"],
            "userStatus": status,
        }));
    }

    Ok(json!(results))
}

/// Pick the EVM or Solana address for a single-chain competition entry,
/// based on the entry's `chainId` (preferred — passed through the unified
/// `chains::chain_family` helper) or `chainName` as a string fallback.
fn pick_wallet_by_chain<'a>(entry: &Value, evm_wallet: &'a str, sol_wallet: &'a str) -> &'a str {
    if is_solana_entry(entry) {
        sol_wallet
    } else {
        evm_wallet
    }
}

/// Whether a competition activity / detail entry is on Solana.
/// Single source of truth: prefer the numeric `chainId` (passed through
/// `crate::chains::chain_family`) so the project-wide chain registry stays
/// authoritative. Falls back to a `chainName` substring match only when the
/// numeric id is absent (older API responses).
fn is_solana_entry(entry: &Value) -> bool {
    if let Some(id) = entry["chainId"].as_u64() {
        return crate::chains::chain_family(&id.to_string()) == "solana";
    }
    if let Some(s) = entry["chainId"].as_str() {
        return crate::chains::chain_family(s) == "solana";
    }
    let chain_name = entry["chainName"].as_str().unwrap_or("");
    let lower = chain_name.to_lowercase();
    lower.contains("solana") || lower == "sol"
}

/// Resolve an activity by its `name` (or `shortName`) → numeric activityId.
/// Used by MCP-facing entry points so callers (the AI) never need to handle
/// raw activityId values, which would invite leaking them in user-facing
/// output.
pub async fn resolve_activity_id_by_name(
    client: &mut ApiClient,
    activity_name: &str,
) -> Result<String> {
    let list_data = list(client, 100, 1, Some(2)).await?;
    let activities = list_data["availableCompetitions"].as_array().cloned().unwrap_or_default();
    let needle = activity_name.trim().to_lowercase();
    let matched: Vec<&Value> = activities
        .iter()
        .filter(|a| {
            let name = a["name"].as_str().unwrap_or("").to_lowercase();
            let short = a["shortName"].as_str().unwrap_or("").to_lowercase();
            name == needle || short == needle
        })
        .collect();
    match matched.len() {
        0 => bail!("no competition matches name: {activity_name}"),
        1 => match matched[0]["id"].as_u64() {
            Some(id) => Ok(id.to_string()),
            None => bail!("matched competition has no numeric id"),
        },
        _ => bail!(
            "multiple competitions match name '{}'; please disambiguate",
            activity_name
        ),
    }
}

/// MCP-facing wrapper for `user_status_all`. Identical to the inner CLI
/// version, plus a `_render` hint that nudges the AI toward the correct
/// SKILL.md section so it doesn't paraphrase the response.
///
/// `activityId` is intentionally retained so downstream tools
/// (`competition_detail` / `competition_claim`) can chain to it. Display
/// safety is enforced by the SKILL.md Output Rules — never by stripping
/// the field at the data layer (doing so broke the join → detail chain).
pub async fn user_status_all_for_mcp(
    client: &mut ApiClient,
    activity_id: Option<&str>,
    evm_wallet: &str,
    sol_wallet: &str,
) -> Result<Value> {
    let mut data = user_status_all(client, activity_id, evm_wallet, sol_wallet).await?;
    inject_render_hint(
        &mut data,
        "User-facing message MUST follow okx-growth-competition SKILL.md Step 5 \
         'Check participation status' rules. NEVER show activityId / chainIndex / \
         accountId in the output (Output Rules <NEVER>). Identify activities by activityName only.",
    );
    Ok(data)
}

/// MCP-facing wrapper for `list` that adds a `_render` hint. The SKILL.md
/// Step 1 template is strict about column names, the hardcoded `Solana, …`
/// chain prefix, and never showing `activityId`; this hint reminds the AI
/// at the data-layer point of consumption.
pub async fn list_for_mcp(
    client: &mut ApiClient,
    page_size: u32,
    page_num: u32,
    status: Option<u32>,
) -> Result<Value> {
    let mut data = list(client, page_size, page_num, status).await?;
    inject_render_hint(
        &mut data,
        "User-facing message MUST follow okx-growth-competition SKILL.md Step 1 fixed table template \
         structure, rendered in the user's language (English canonical column headers: \
         Name / Chain / Time / Total Prize Pool / Details — Chinese: 活动名称 / 活动链 / 时间 / 总奖池 / 详情链接). \
         Do NOT add an ID column. Do NOT show activityId anywhere in the table or surrounding text. \
         The Chain cell MUST be 'Solana, {chainName}' when chainName is not Solana \
         (hardcoded Solana prefix until backend exposes a multi-chain field); if chainName is Solana, \
         write just 'Solana'. If results contain BOTH activityStatus=3 (active) and activityStatus=4 \
         (ended), split into two tables under bold subheadings — '**Active**' / '**Ended**' for English, \
         '**进行中**' / '**已结束**' for Chinese — in that order.",
    );
    Ok(data)
}

/// MCP-facing wrapper for `detail` that adds a `_render` hint pointing at
/// SKILL.md Step 2 (the four-section reward template with the leading
/// Basic-info block, rendered in the user's language).
pub async fn detail_for_mcp(client: &mut ApiClient, activity_id: &str) -> Result<Value> {
    let mut data = detail(client, activity_id).await?;
    inject_render_hint(
        &mut data,
        "User-facing message MUST follow okx-growth-competition SKILL.md Step 2 fixed display template \
         structure, rendered in the user's language. Required structure: a Basic-info block \
         (English 'Basic info:' / Chinese '基本信息：') with the chain line using the hardcoded \
         'Solana, {chainName}' prefix unless chainName is Solana, plus a numbered Reward-categories \
         list (English 'Reward categories:' / Chinese '奖励分类：') with items 1./2./3./4. \
         Sections 3 (Participation Reward / 参与奖) and 4 (Skill Quality Award / Skill 质量奖) have \
         specific required content (eligibility threshold, judging mechanism, top-N reward) — preserve \
         that meaning in any language. Sections 1 and 2 must include the rank breakdown markdown table \
         built from prizePoolDistribution[].rules[]. NEVER show activityId, chainIndex, or any internal \
         numeric id to the user.",
    );
    Ok(data)
}

/// MCP-facing wrapper for `rank` that adds a `_render` hint reminding the AI
/// of two critical Step 5 rules:
///   1. A user can be on multiple leaderboards simultaneously — call this
///      once per `sort_type` from `tabConfigs[].rankFieldConfig[].sortValueMap.descend`.
///   2. The user-facing summary must follow Step 5 CASE 1 / 2 / 3 fixed
///      template structure (rendered in the user's language).
pub async fn rank_for_mcp(
    client: &mut ApiClient,
    activity_id: &str,
    wallet: &str,
    sort_type: i32,
    limit: u32,
) -> Result<Value> {
    let mut data = rank(client, activity_id, wallet, sort_type, limit).await?;
    inject_render_hint(
        &mut data,
        "User-facing message MUST follow okx-growth-competition SKILL.md Step 5 'Check user's own rank' \
         CASE 1 / CASE 2 / CASE 3 fixed template structure, rendered in the user's language. \
         BEFORE rendering, you MUST first call competition_detail to enumerate \
         tabConfigs[].rankFieldConfig[].sortValueMap.descend, then call competition_rank ONCE PER \
         sort_type so you have data for every leaderboard the activity exposes. A user can rank on \
         multiple leaderboards (e.g. PnL% and PnL) at the same time — never assume one leaderboard \
         is enough. Classify outcome as CASE 1 (ranked on all), CASE 2 (ranked on some), or CASE 3 \
         (ranked on none) and emit the matching template. Never invent your own table layout, never \
         collapse multi-leaderboard sections into one.",
    );
    Ok(data)
}

/// Insert a `_render` field into a JSON response without disturbing real
/// payload fields. Works for both object and array shapes; for arrays the
/// hint is wrapped into a sibling object so it doesn't mutate row entries.
fn inject_render_hint(data: &mut Value, hint: &str) {
    if let Some(obj) = data.as_object_mut() {
        obj.insert("_render".to_string(), json!(hint));
    }
    // Note: we deliberately do NOT inject into bare arrays — wrapping the
    // shape would change the public response contract. The list and detail
    // APIs already return objects, so this is fine in practice.
}

const PROJECT_HEADER: &str = "4d156bf0c61130f2692d097ecb68dbe4";

/// POST /priapi/v5/wallet/agentic/competition/join — requires wallet login
pub async fn join(
    _client: &mut ApiClient,
    activity_id: &str,
    evm_wallet: &str,
    sol_wallet: &str,
    chain_index: &str,
) -> Result<Value> {
    let (account_id, mut auth_client) = ensure_logged_in_client().await?;
    let body = json!({
        "activityId": activity_id,
        "evmAddress": evm_wallet,
        "solAddress": sol_wallet,
        "chainIndex": chain_index,
        "accountId": account_id,
    });
    auth_client
        .post_with_headers(
            "/priapi/v5/wallet/agentic/competition/join",
            &body,
            Some(&[("OK-ACCESS-PROJECT", PROJECT_HEADER)]),
        )
        .await?;
    // API returns data: null on success — construct a useful confirmation object.
    // `activityId` is included for downstream tool calls (e.g. competition_detail
    // takes an id to fetch totalPrizePool / chainName for rendering the success
    // template). Output Rules apply to USER-FACING display only, not internal
    // data flow between tools. The AI must still follow the SKILL.md rule of
    // never showing this id in messages it produces for the user.
    //
    // The `_render` field is an inline reminder for the AI: if it didn't already
    // load SKILL.md Step 3, this nudges it to follow the fixed copy template
    // instead of paraphrasing.
    Ok(json!({
        "joined": true,
        "activityId": activity_id,
        "evmAddress": evm_wallet,
        "solAddress": sol_wallet,
        "chainIndex": chain_index,
        "_render": "Follow okx-growth-competition SKILL.md Step 3 'Successful registration' fixed template, rendered in the user's language (English, Chinese, etc.). Required content: lead phrase 'Registered successfully! / 报名成功！', dual-chain sentence ('runs on {chainName} and Solana / 同时在 {chainName} 和 Solana 两条链上进行'), total prize pool, the dual-axis PnL%/realized PnL ranking note, the participation+Skill awards mention, and a closing question. Required trailing line on its own line: the bracketed disclaimer ('[Disclaimer: ...]' for English / '[免责声明：...]' for Chinese). Use the returned `activityId` to call competition_detail and fetch chainName + totalPrizePool, but NEVER show activityId to the user.",
    }))
}

/// POST /priapi/v5/wallet/agentic/competition/claim — requires wallet login.
///
/// Returns the raw calldata array as-is. CLI users get this and must run
/// `onchainos wallet contract-call` themselves for each entry. MCP callers
/// should use `claim_and_submit` instead, which handles the sign+broadcast
/// loop in-process.
pub async fn claim(
    _client: &mut ApiClient,
    activity_id: &str,
    evm_wallet: &str,
    sol_wallet: &str,
) -> Result<Value> {
    let (account_id, mut auth_client) = ensure_logged_in_client().await?;
    let body = json!({
        "activityId": activity_id,
        "evmAddress": evm_wallet,
        "solAddress": sol_wallet,
        "accountId": account_id,
    });
    auth_client
        .post_with_headers(
            "/priapi/v5/wallet/agentic/competition/claim",
            &body,
            Some(&[("OK-ACCESS-PROJECT", PROJECT_HEADER)]),
        )
        .await
}

/// Atomic claim flow for MCP: pre-check reward eligibility → claim API →
/// for each calldata entry, run the full `execute_contract_call` flow (TEE
/// signing + broadcast) → return aggregate result with reward info, list of
/// successful txHashes, and list of failed entries.
///
/// This exists because the AI in MCP-only mode has no `wallet_contract_call`
/// MCP tool to chain to. Without an atomic wrapper it would either need to
/// shell out to the CLI or improvise (which led to a real bug where it
/// constructed a fake "signed tx" in Python and called `gateway_broadcast`).
///
/// Pre-check: blocks the claim before any signing if the user is not
/// eligible (rewardStatus 0), already claimed (2), or expired (3). This is
/// a defensive idempotency check — the backend would also reject these,
/// but a clean local error keeps us from broadcasting a doomed tx.
///
/// Partial failures: when the claim returns multiple calldata entries, each
/// is broadcast independently. If entry 2 fails after entry 1 succeeded,
/// entry 1's txHash is still surfaced under `succeeded`. The caller (AI)
/// can show the user what landed on-chain and what didn't.
pub async fn claim_and_submit(
    client: &mut ApiClient,
    activity_id: &str,
    evm_wallet: &str,
    sol_wallet: &str,
) -> Result<Value> {
    // ── Step 1: pre-check eligibility & capture reward metadata ──────────
    let detail_data = detail(client, activity_id).await?;
    let pre_check_wallet = pick_wallet_by_chain(&detail_data, evm_wallet, sol_wallet);
    let status = user_status(client, activity_id, pre_check_wallet).await?;
    let reward_status = status["rewardStatus"].as_i64().unwrap_or(-1);
    let reward_amount = status["rewardAmount"].as_str().unwrap_or("").to_string();
    let reward_unit = status["rewardUnit"].as_str().unwrap_or("").to_string();
    match reward_status {
        1 => { /* won, not yet claimed — proceed */ }
        0 => bail!("not eligible for reward — you did not win this competition"),
        2 => bail!("reward already claimed"),
        3 => bail!("reward has expired and can no longer be claimed"),
        _ => bail!("unexpected rewardStatus {} from user_status", reward_status),
    }

    // ── Step 2: fetch unsigned calldata ─────────────────────────────────
    let calldata = claim(client, activity_id, evm_wallet, sol_wallet).await?;
    let entries = calldata.as_array().cloned().unwrap_or_default();
    if entries.is_empty() {
        bail!("claim API returned no calldata to submit");
    }

    // ── Step 3: broadcast each entry, collecting per-entry outcomes ─────
    let total = entries.len();
    let mut succeeded = Vec::new();
    let mut failed = Vec::new();
    for (idx, entry) in entries.iter().enumerate() {
        match submit_one_calldata(entry).await {
            Ok(success) => succeeded.push(success),
            Err(e) => {
                let chain = entry_chain_string(entry);
                let contract = entry["contractAddress"].as_str().unwrap_or("").to_string();
                failed.push(json!({
                    "index": idx,
                    "contractAddress": contract,
                    "chain": chain,
                    "error": e.to_string(),
                }));
            }
        }
    }

    // If every entry failed, surface as Err so the AI doesn't mistakenly
    // tell the user "claim succeeded".
    if succeeded.is_empty() {
        let first_err = failed
            .first()
            .and_then(|f| f["error"].as_str())
            .unwrap_or("unknown error");
        bail!(
            "claim failed for all {} entries; first error: {}. \
             AI rendering hint: surface the error verbatim, then append the SKILL.md Step 6 \
             '建议' block (建议：领取过程需要支付Gas，请确认Gas是否充足 / 稍后再试一次（可能是暂时性网络问题） / \
             如果多次失败，请联系客服处理) UNLESS the error is a semantic pre-check rejection \
             (rewardStatus 0/2/3, code 11002, code 11008) — those are not transient and the suggestion is misleading.",
            total,
            first_err
        );
    }

    Ok(json!({
        "rewardAmount": reward_amount,
        "rewardUnit": reward_unit,
        "totalEntries": total,
        "succeeded": succeeded,
        "failed": failed,
        "_render": "User-facing message rendered in the user's language: for each entry in succeeded[] report 'Claimed {rewardAmount} {rewardUnit}, txHash: {txHash}' (Chinese: '已领取 {rewardAmount} {rewardUnit}，交易哈希: {txHash}'). \
                    If failed[] is non-empty (partial success), list the failed entries with their `error`, then append the SKILL.md Step 6 'Fixed failure-suggestion block' (English canonical: 'Suggestions: - The claim process requires Gas... - Try again later... - If it fails repeatedly, contact customer support.' / Chinese: '建议：领取过程需要支付Gas... 稍后再试一次... 如果多次失败，请联系客服处理'). \
                    Skip the suggestion block when the failures are pre-check semantic rejections (rewardStatus 0/2/3, code 11002, code 11008). \
                    Do NOT re-run claim blindly on partial success — already-broadcasted txs would hit the dedup guard.",
    }))
}

/// Submit one calldata entry from a competition claim response. Extracts
/// the transaction payload (EVM hex / Solana base58 / Solana byte array),
/// then delegates to `execute_contract_call` for the TEE sign + broadcast.
///
/// Field-name compatibility: the backend places fields differently across
/// chains and versions. We mirror what swap.rs and cross_chain.rs already
/// support so EVM/XLayer responses go through the same pattern they use:
///   - swap.rs reads `tx.to`, `tx.data`, `tx.value`, `tx.gas`
///   - cross_chain.rs reads `tx.to`, `tx.data`, `tx.value`, `tx.gasLimit`
/// We accept either gas key. Top-level legacy keys (`input`, `value`,
/// `contractAddress`, `gasLimit`) are also honored as a fallback.
async fn submit_one_calldata(entry: &Value) -> Result<Value> {
    // ── contract address: top-level `contractAddress`, fallback `tx.to` ──
    let contract_addr = entry["contractAddress"]
        .as_str()
        .or_else(|| entry["tx"]["to"].as_str())
        .unwrap_or("")
        .to_string();

    // ── value (native token, usually "0" for token rewards) ──────────────
    let value = entry["value"]
        .as_str()
        .or_else(|| entry["tx"]["value"].as_str())
        .unwrap_or("0")
        .to_string();

    // ── chain id (numeric string) ────────────────────────────────────────
    let chain = match &entry["chain"] {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        _ => bail!("calldata entry missing or invalid `chain` field"),
    };

    // ── Sui claim is not supported by the local TEE signing path.
    //    Backend returns rewards as a structured `suiCallData` object
    //    (packageObjectId / module / function / typeArguments / arguments),
    //    which is not a hex calldata or base58 binary tx — `wallet
    //    contract-call` cannot consume it. Fail fast with a clear message
    //    rather than letting the user see a confusing "no recognized
    //    transaction payload" error.
    if !entry["suiCallData"].is_null() {
        bail!(
            "Sui-chain reward claims are not yet supported by this client. \
             Please claim from the Sui-compatible wallet UI."
        );
    }

    // ── gas limit: try several keys to cover both swap / cross_chain
    //    naming conventions (`tx.gas` vs `tx.gasLimit`) and a top-level
    //    `gasLimit` if present. Internal fields are strings.
    let gas_limit: Option<String> = entry["gasLimit"]
        .as_str()
        .or_else(|| entry["tx"]["gas"].as_str())
        .or_else(|| entry["tx"]["gasLimit"].as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    // ── EVM calldata: 0x-prefixed hex. Search top-level then `tx.*`. ─────
    let input_data: Option<String> = ["input", "callData"]
        .iter()
        .find_map(|k| {
            entry[*k]
                .as_str()
                .filter(|s| !s.is_empty() && s.starts_with("0x"))
                .map(|s| s.to_string())
        })
        .or_else(|| {
            ["data", "input"].iter().find_map(|k| {
                entry["tx"][*k]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.starts_with("0x"))
                    .map(|s| s.to_string())
            })
        });

    // ── Solana payload: only attempt if no EVM hex was found.
    //    Try base58 string first; fall back to encoding a byte array. ────
    let unsigned_tx: Option<String> = if input_data.is_some() {
        None
    } else {
        ["base58CallData", "serializedTx", "unsignedTx"]
            .iter()
            .find_map(|k| {
                entry[*k]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
            })
            .or_else(|| encode_solana_byte_array(entry))
    };

    if input_data.is_none() && unsigned_tx.is_none() {
        let keys: Vec<&str> = entry
            .as_object()
            .map(|m| m.keys().map(|k| k.as_str()).collect())
            .unwrap_or_default();
        bail!(
            "claim calldata entry has no recognized transaction payload. \
             Tried: input/callData/tx.data/tx.input (EVM hex), \
             base58CallData/serializedTx/unsignedTx (Solana base58 string), \
             tx.data byte array (Solana). Available fields: {:?}",
            keys
        );
    }

    let resp = crate::commands::agentic_wallet::transfer::execute_contract_call(
        &contract_addr,
        &chain,
        &value,
        input_data.as_deref(),
        unsigned_tx.as_deref(),
        gas_limit.as_deref(),
        None,  // from — use selected account
        None,  // aa_dex_token_addr
        None,  // aa_dex_token_amount
        false, // mev_protection
        None,  // jito_unsigned_tx
        true,  // force — reward claim is non-interactive by design
        None,  // tx_source
        None,  // gas_token_address
        None,  // relayer_id
        false, // enable_gas_station
        Some("competition"),
        Some("competition_claim"),
    )
    .await?;

    Ok(json!({
        "contractAddress": contract_addr,
        "chain": chain,
        "txHash": resp.tx_hash,
        "orderId": resp.order_id,
    }))
}

fn entry_chain_string(entry: &Value) -> String {
    match &entry["chain"] {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        _ => String::new(),
    }
}

// ── helpers ───────────────────────────────────────────────────────────

/// If a Solana claim entry carries the unsigned transaction as a JSON byte
/// array instead of an already base58-encoded string, encode it into base58
/// ourselves so it can be passed to `wallet contract-call --unsigned-tx`.
///
/// The backend wraps the bytes in a Node.js Buffer JSON shape:
/// `{ "type": "Buffer", "data": [1, 0, 7, 12, ...] }`. The Buffer object
/// itself is nested under one of several keys depending on the backend
/// version:
///   - top level: `data` / `unsignedTx` / `serializedTx` / `rawTx`
///   - nested:    `tx` / `v0.tx` (preferred — these are the *unsigned* tx;
///                we avoid `*.txSigned` because despite the name those
///                bytes still have a zero signature placeholder).
///
/// Returns `None` if no plausible byte array is found.
fn encode_solana_byte_array(entry: &Value) -> Option<String> {
    let buffer_paths: [&[&str]; 6] = [
        &["tx"],            // preferred: unsigned tx
        &["v0", "tx"],      // versioned (v0) unsigned tx
        &["unsignedTx"],
        &["serializedTx"],
        &["rawTx"],
        &["data"],          // bare top-level array (no Buffer wrapper)
    ];

    for path in buffer_paths {
        let mut cursor = entry;
        for segment in path {
            cursor = &cursor[*segment];
        }
        if let Some(s) = bytes_from_buffer_or_array(cursor) {
            return Some(s);
        }
    }
    None
}

/// Decode a Node.js `Buffer` JSON object (`{ type: "Buffer", data: [...] }`)
/// or a bare integer array into a base58 string. Returns `None` if the
/// shape doesn't match or any element is out of byte range.
fn bytes_from_buffer_or_array(v: &Value) -> Option<String> {
    let arr = v
        .get("data")
        .and_then(|d| d.as_array())
        .or_else(|| v.as_array())?;
    if arr.is_empty() {
        return None;
    }
    let bytes: Vec<u8> = arr
        .iter()
        .map(|n| n.as_u64().filter(|x| *x <= 255).map(|x| x as u8))
        .collect::<Option<Vec<u8>>>()?;
    Some(bs58::encode(bytes).into_string())
}

/// Pick the chain-appropriate wallet address for a single-chain query
/// (e.g. `competition_rank`). Looks up the activity's `chainName` via
/// `competition_detail` and returns the SOL address for Solana activities,
/// the EVM address otherwise. Both addresses are read from the local
/// wallet_store via `resolve_default_addresses`.
pub async fn resolve_wallet_for_activity(
    client: &mut ApiClient,
    activity_id: &str,
) -> Result<String> {
    let detail_data = detail(client, activity_id).await?;
    let (evm, sol) = resolve_default_addresses()?;
    Ok(if is_solana_entry(&detail_data) { sol } else { evm })
}

/// Resolve the user's default EVM and Solana wallet addresses from the local
/// wallet_store. Used by MCP entry points so the AI does not need to call a
/// separate (non-existent) `wallet_status` MCP tool just to discover the
/// addresses required by competition tools.
///
/// Returns `(evm_address, sol_address)`. Errors out if the user is not
/// logged in, or if either an EVM or SOL address is missing from the
/// selected account (both are required by the competition backend).
pub fn resolve_default_addresses() -> Result<(String, String)> {
    let wallets = wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!("not logged in — please run: onchainos wallet login"))?;
    if wallets.selected_account_id.is_empty() {
        bail!("not logged in — please run: onchainos wallet login");
    }
    let account = wallets
        .accounts_map
        .get(&wallets.selected_account_id)
        .ok_or_else(|| anyhow::anyhow!("selected account has no address list — please re-login"))?;

    let mut evm: Option<String> = None;
    let mut sol: Option<String> = None;
    for addr in &account.address_list {
        // Solana is chainIndex "501". Anything else with a 0x-prefixed address is treated as EVM.
        if addr.chain_index == "501" {
            if sol.is_none() {
                sol = Some(addr.address.clone());
            }
        } else if addr.address.starts_with("0x") && evm.is_none() {
            evm = Some(addr.address.clone());
        }
    }

    let evm = evm.ok_or_else(|| {
        anyhow::anyhow!("could not find an EVM address in the selected account")
    })?;
    let sol = sol.ok_or_else(|| {
        anyhow::anyhow!("could not find a Solana address in the selected account")
    })?;
    Ok((evm, sol))
}

/// Pre-flight login check for authenticated competition endpoints.
///
/// Long-lived MCP server clients are constructed once via `ApiClient::new()`
/// (sync) and cache the JWT they had at startup — that token may have expired
/// by the time `join` / `claim` runs. To avoid sharing a stale token, we
/// always build a fresh `ApiClient::new_async()` here: it has the full JWT
/// lifecycle (expiry check + refresh + AK fallback) baked in.
async fn ensure_logged_in_client() -> Result<(String, ApiClient)> {
    let account_id = match wallet_store::load_wallets() {
        Ok(Some(w)) if !w.selected_account_id.is_empty() => w.selected_account_id.clone(),
        _ => bail!("not logged in — please run: onchainos wallet login"),
    };
    let auth_client = ApiClient::new_async(None).await?;
    Ok((account_id, auth_client))
}
