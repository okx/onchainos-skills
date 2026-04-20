/// Trading Competition commands (mock data — real APIs pending backend deployment).
///
/// Public endpoints (no auth):
///   GET /priapi/v1/agentic/competition/list     status: 0=active,1=ended,2=all
///   GET /priapi/v1/agentic/competition/detail
///   GET /priapi/v1/agentic/competition/rank
///   GET /priapi/v1/agentic/competition/userStatus
///
/// Authenticated endpoints (JWT required — Authorization: Bearer <accessToken>):
///   POST /priapi/v5/wallet/agentic/competition/join
///   POST /priapi/v5/wallet/agentic/competition/claim
use anyhow::{bail, Result};
use clap::Subcommand;
use serde_json::{json, Value};

use crate::keyring_store;
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
        /// Status filter: 0=active, 1=ended, 2=all (omit for all)
        #[arg(long)]
        status: Option<u32>,
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
        /// Max leaderboard entries (default 20, max 100)
        #[arg(long, default_value = "20")]
        limit: u32,
    },
    /// Get user participation and reward status
    UserStatus {
        /// Activity ID
        #[arg(long)]
        activity_id: String,
        /// User wallet address
        #[arg(long)]
        wallet: String,
    },
    /// Join a trading competition (requires wallet login). Nickname auto-set to "Agentic...{last4}"
    Join {
        /// Activity ID
        #[arg(long)]
        activity_id: String,
        /// Wallet address to register
        #[arg(long)]
        wallet: String,
    },
    /// Claim competition rewards (requires wallet login)
    Claim {
        /// Activity ID
        #[arg(long)]
        activity_id: String,
        /// Winning wallet address
        #[arg(long)]
        wallet: String,
    },
}

pub async fn execute(command: CompetitionCommand) -> Result<()> {
    let data = match command {
        CompetitionCommand::List {
            page_size,
            page_num,
            status,
        } => list_inner(page_size, page_num, status)?,
        CompetitionCommand::Detail { activity_id } => detail_inner(&activity_id)?,
        CompetitionCommand::Rank {
            activity_id,
            wallet,
            sort_type,
            limit,
        } => rank_inner(&activity_id, &wallet, sort_type, limit)?,
        CompetitionCommand::UserStatus {
            activity_id,
            wallet,
        } => user_status_inner(&activity_id, &wallet)?,
        CompetitionCommand::Join {
            activity_id,
            wallet,
        } => join_inner(&activity_id, &wallet)?,
        CompetitionCommand::Claim {
            activity_id,
            wallet,
        } => claim_inner(&activity_id, &wallet)?,
    };
    output::success(data);
    Ok(())
}

// ── MCP-callable public wrappers ──────────────────────────────────────

pub fn cmd_list_mcp(page_size: u32, page_num: u32, status: Option<u32>) -> Result<Value> {
    list_inner(page_size, page_num, status)
}

pub fn cmd_detail_mcp(activity_id: &str) -> Result<Value> {
    detail_inner(activity_id)
}

pub fn cmd_rank_mcp(activity_id: &str, wallet: &str, sort_type: i32, limit: u32) -> Result<Value> {
    rank_inner(activity_id, wallet, sort_type, limit)
}

pub fn cmd_user_status_mcp(activity_id: &str, wallet: &str) -> Result<Value> {
    user_status_inner(activity_id, wallet)
}

pub fn cmd_join_mcp(activity_id: &str, wallet: &str) -> Result<Value> {
    join_inner(activity_id, wallet)
}

pub fn cmd_claim_mcp(activity_id: &str, wallet: &str) -> Result<Value> {
    claim_inner(activity_id, wallet)
}

// ── helpers ───────────────────────────────────────────────────────────

/// Check wallet login and return the stored access token for JWT auth.
/// Used by join/claim which require `Authorization: Bearer <token>`.
fn read_access_token() -> Result<String> {
    // Verify wallet session exists
    match wallet_store::load_wallets() {
        Ok(Some(w)) if !w.selected_account_id.is_empty() => {}
        _ => bail!("not logged in — please run: onchainos wallet login"),
    }
    // Read access_token from keyring (stored by wallet login/verify)
    keyring_store::get_opt("access_token")
        .filter(|t| !t.is_empty())
        .ok_or_else(|| anyhow::anyhow!("not logged in — please run: onchainos wallet login"))
}

fn addr_suffix(addr: &str) -> &str {
    let n = addr.len();
    if n >= 4 { &addr[n - 4..] } else { addr }
}

/// Build the default nickname: "Agentic....{last4}"
fn default_nickname(wallet: &str) -> String {
    format!("Agentic....{}", addr_suffix(wallet))
}

// ── inner implementations (return Value, shared by CLI and MCP) ───────

fn list_inner(page_size: u32, page_num: u32, status: Option<u32>) -> Result<Value> {
    // TODO: GET /priapi/v1/agentic/competition/list
    // status: 0=active(进行中), 1=ended(已结束), 2=all(全部); omit → all
    let now = 1743000000_u64;
    let all = vec![
        json!({
            "id": 100, "shortName": "hippo",
            "name": "HIPPO Trading Competition",
            "rewards": "50000 HIPPO",
            "startTime": now - 86400, "endTime": now + 6 * 86400,
            "chainId": 42161, "chainName": "Arbitrum One", "status": 0
        }),
        json!({
            "id": 101, "shortName": "vsn",
            "name": "VSN Trading Competition",
            "rewards": "300000 VSN",
            "startTime": now - 3 * 86400, "endTime": now + 4 * 86400,
            "chainId": 42161, "chainName": "Arbitrum One", "status": 0
        }),
        json!({
            "id": 99, "shortName": "btc-lunar",
            "name": "Bitcoin Lunar New Year Competition",
            "rewards": "1000 USDT",
            "startTime": now - 30 * 86400, "endTime": now - 23 * 86400,
            "chainId": 1, "chainName": "Ethereum", "status": 1
        }),
    ];

    let status_filtered: Vec<Value> = all
        .into_iter()
        .filter(|c| match status {
            None | Some(2) => true,                                        // all
            Some(s) => c["status"].as_u64() == Some(s as u64),
        })
        .collect();

    let total = status_filtered.len();
    let skip = page_num.saturating_sub(1) as usize * page_size as usize;
    let page: Vec<Value> = status_filtered
        .into_iter()
        .skip(skip)
        .take(page_size as usize)
        .collect();

    Ok(json!({
        "availableCompetitions": page,
        "totalCount": total,
        "pageNum": page_num,
        "pageSize": page_size
    }))
}

fn detail_inner(activity_id: &str) -> Result<Value> {
    // TODO: GET /priapi/v1/agentic/competition/detail?activityId=<id>
    let now = 1743000000_u64;
    Ok(json!({
        "id": activity_id,
        "name": "HIPPO Trading Competition",
        "chainId": 42161,
        "chainName": "Arbitrum One",
        "startTime": now - 86400,
        "endTime": now + 6 * 86400,
        "status": 0,
        "tabConfigs": [{
            "tab": 1,
            "tabDetails": [
                {
                    "title": "Ranking Rules",
                    "desc": "Trade HIPPO pairs during the activity to accumulate volume. Top 500 traders share the prize pool.\nLeaderboard updates every 10 min. Final list published within 3 business days after activity ends."
                },
                {
                    "title": "Participation Requirements",
                    "desc": "Agentic Wallet users only. Volume must exceed $100 to appear on leaderboard.\nOnly Arbitrum One HIPPO/USDT, HIPPO/USDC, HIPPO/ETH pairs count."
                }
            ],
            "prizePoolDistribution": [{
                "rewardType": 5, "rewardTypeName": "Volume",
                "rewardUnit": "HIPPO", "totalReward": 50000,
                "rules": [
                    {"interval": "1",      "reward": "10000"},
                    {"interval": "2",      "reward": "6000"},
                    {"interval": "3",      "reward": "4000"},
                    {"interval": "4-10",   "reward": "2000"},
                    {"interval": "11-50",  "reward": "500"},
                    {"interval": "51-200", "reward": "100"},
                    {"interval": "201-500","reward": "50"}
                ]
            }],
            "rankFieldConfig": [{
                "key": "volume", "title": "Volume",
                "format": 1, "sorter": true,
                "defaultSortOrder": "descend",
                "sortDirections": ["descend", "ascend"],
                "sortValueMap": {"ascend": 0, "descend": 5}
            }, {
                "key": "expectedRewards", "title": "Est. Reward",
                "format": 3, "sorter": false,
                "defaultSortOrder": "",
                "sortDirections": [],
                "sortValueMap": {"ascend": 0, "descend": 0}
            }]
        }]
    }))
}

fn rank_inner(activity_id: &str, wallet: &str, sort_type: i32, limit: u32) -> Result<Value> {
    // TODO: GET /priapi/v1/agentic/competition/rank
    let _ = activity_id;
    let count = (limit.min(100)) as usize;
    let suffix = addr_suffix(wallet);
    let my_rank = json!({
        "currentRank": 42,
        "nickName": format!("Agentic....{}", suffix),
        "userTotal": "1250.500000000000000000",
        "expectedRewards": "100",
        "format": 1,
        "rewardUnit": "HIPPO"
    });
    let all_ranks: Vec<Value> = (1..=count.min(20))
        .map(|i| {
            let vol = format!("{:.18}", (20000.0_f64 - i as f64 * 500.0).max(100.0));
            let reward = if i == 1 {
                "10000"
            } else if i == 2 {
                "6000"
            } else if i == 3 {
                "4000"
            } else if i <= 10 {
                "2000"
            } else if i <= 50 {
                "500"
            } else {
                "100"
            };
            json!({
                "currentRank": i,
                "nickName": format!("Agentic....{:04X}", (i * 1337) % 0xFFFF),
                "userTotal": vol,
                "expectedRewards": reward,
                "format": 1,
                "rewardUnit": "HIPPO"
            })
        })
        .collect();

    Ok(json!({
        "myRankInfo": my_rank,
        "allRankInfos": all_ranks,
        "rankUpdateTime": 1743001200,
        "sortType": sort_type
    }))
}

fn user_status_inner(activity_id: &str, wallet: &str) -> Result<Value> {
    // TODO: GET /priapi/v1/agentic/competition/userStatus
    let _ = (activity_id, wallet);
    Ok(json!({
        "joinStatus": 0,
        "joinTime": null,
        "rewardStatus": 0,
        "claimTime": null,
        "rewardAmount": null,
        "rewardUnit": null
    }))
}

fn join_inner(activity_id: &str, wallet: &str) -> Result<Value> {
    // TODO: POST /priapi/v5/wallet/agentic/competition/join
    // Real call: POST with Authorization: Bearer <access_token>
    // Body: { activityId, walletAddress, nickname }
    let _access_token = read_access_token()?;
    // TODO: pass _access_token as Authorization header when wiring real HTTP call
    let nickname = default_nickname(wallet);
    Ok(json!({
        "joined": true,
        "activityId": activity_id,
        "walletAddress": wallet,
        "nickname": nickname
    }))
}

fn claim_inner(activity_id: &str, wallet: &str) -> Result<Value> {
    // TODO: POST /priapi/v5/wallet/agentic/competition/claim
    // Real call: POST with Authorization: Bearer <access_token>
    // Body: { activityId, walletAddress }
    let _access_token = read_access_token()?;
    // TODO: pass _access_token as Authorization header when wiring real HTTP call
    let note = format!("MOCK DATA — activityId={} wallet={}", activity_id, wallet);
    Ok(json!([{
        "contractAddress": "0x1234567890abcdef1234567890abcdef12345678",
        "chain": 42161,
        "input": "0xa9059cbb0000000000000000000000000000000000000000000000000000000000000000",
        "tokenSymbol": "HIPPO",
        "tokenAmount": "10000000000000000000000",
        "tokenAddress": "0xabcdefabcdefabcdefabcdefabcdefabcdefabcd",
        "value": "0",
        "tx": null,
        "v0": null,
        "blockhashData": null,
        "suiCallData": null,
        "base58CallData": null,
        "_note": note
    }]))
}
