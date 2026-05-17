//! Account-level reward claim — 公共 API 调用层
//!
//! 后端 `POST /priapi/v1/aieco/task/claim` 是 **account-pull** 接口：
//! 一次性领取该账户在所有已结算 dispute / job 中的全部待领奖励，body 为空，
//! 不按 jobId / token 切分。该接口与调用方角色（buyer / provider / evaluator）无关，
//! 只要 agentId + 钱包能签名就能调。
//!
//! 本模块只负责：API 调用 + 签名 + broadcast / JSON 解析 + 表格输出。
//! 角色专属的 wallet/agent 解析（不同角色用不同 `signing::resolve_*`）和上下文文案
//! （例如 evaluator 的"跟我说『领取奖励』"提示）由各自的薄壳保留。
//!
//! 当前接入：`evaluator/claim.rs`、`evaluator/claimable.rs`。
//! buyer / provider 接入时只需在自己的 handler 里 resolve 钱包后调用此处的两个函数。

use anyhow::Result;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// 调用 account-pull claim 接口，签名 uopData 并广播。返回 txHash。
///
/// 调用方需先自行解析出 `(account_id, address, agent_id)`（不同角色用不同 resolver）。
pub async fn submit_claim_and_broadcast(
    client: &mut TaskApiClient,
    account_id: &str,
    address: &str,
    agent_id: &str,
) -> Result<String> {
    let path = "/priapi/v1/aieco/task/claim";
    let resp = client
        .post_with_identity(path, &serde_json::json!({}), agent_id)
        .await?;

    signing::sign_uop_and_broadcast(
        client,
        &resp["uopData"],
        account_id,
        address,
        "",
        signing::extract_biz_type(&resp),
        agent_id,
    )
    .await
}

/// 拉取账户级 claimable 列表并直接 println 输出。返回 `has_nonzero`，调用方可据此决定
/// 是否给出"建议立刻 claim"之类的角色文案。
///
/// 表头展示的 `account` 字段直接取自后端响应；后端不回时为空串。
pub async fn fetch_and_print_claimable(
    client: &mut TaskApiClient,
    agent_id: &str,
) -> Result<bool> {
    let path = "/priapi/v1/aieco/task/claimable";
    let resp = client.get_with_identity(path, agent_id).await?;

    let account = resp["account"].as_str().unwrap_or_default();
    println!("claimable rewards (account={account}, agentId={agent_id})");

    let rewards = resp["rewards"].as_array();
    let mut has_nonzero = false;
    match rewards {
        Some(items) if !items.is_empty() => {
            for r in items {
                let symbol = r["symbol"].as_str().unwrap_or("?");
                let amount = r["amount"].as_str().unwrap_or("0");
                let token = r["tokenAddress"].as_str().unwrap_or("");
                let raw = r["rawAmount"].as_str().unwrap_or("0");
                let nonzero = raw != "0" && !raw.is_empty();
                if nonzero {
                    has_nonzero = true;
                }
                let marker = if nonzero { "•" } else { " " };
                println!("  {marker} {symbol:<8} {amount:>30}  (token={token})");
            }
        }
        _ => {
            println!("  (no rewards)");
        }
    }

    Ok(has_nonzero)
}
