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
        /// Sort type: 5=volume, 7=realized PnL, 8=boost token volume
        #[arg(long, default_value = "5")]
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
    /// Claim competition rewards (requires wallet login)
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
        CompetitionCommand::Claim {
            activity_id,
            evm_wallet,
            sol_wallet,
        } => claim(&mut client, &activity_id, &evm_wallet, &sol_wallet).await?,
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
        let chain_name = detail_data["chainName"].as_str().unwrap_or("");
        let wallet = if is_solana_chain(chain_name) { sol_wallet } else { evm_wallet };
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
        let chain_name = activity["chainName"].as_str().unwrap_or("");
        let wallet = if is_solana_chain(chain_name) { sol_wallet } else { evm_wallet };
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

fn is_solana_chain(chain_name: &str) -> bool {
    let lower = chain_name.to_lowercase();
    lower.contains("solana") || lower == "sol"
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
    // API returns data: null on success — construct a useful confirmation object
    Ok(json!({
        "joined": true,
        "activityId": activity_id,
        "evmAddress": evm_wallet,
        "solAddress": sol_wallet,
        "chainIndex": chain_index,
    }))
}

/// POST /priapi/v5/wallet/agentic/competition/claim — requires wallet login
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

// ── helpers ───────────────────────────────────────────────────────────

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
