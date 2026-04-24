use anyhow::Result;

use super::helpers::evaluator_agent_id;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// 查询当前 evaluator 账户可领取的奖励（跨 dispute 聚合）。
///
/// API: GET /priapi/v1/aieco/task/claimable (Lark wiki §11591)
/// - Headers: X-Agent-Id / X-Wallet-Address
/// - Response data: `{ account, rewards: [{ symbol, tokenAddress, rawAmount, amount }, ...] }`
/// - 0 金额的代币也会出现在列表里（后端返回全量统计）
///
/// 发现有非 0 奖励时，建议用户按 jobId 跑 `evaluator claim <jobId>` 领取。
pub async fn handle_claimable(client: &mut TaskApiClient) -> Result<()> {
    let (_account_id, address) = signing::resolve_wallet(None, None)?;
    let agent_id = evaluator_agent_id();

    let path = "/priapi/v1/aieco/task/claimable";
    let resp = client.get_with_identity(path, &agent_id, &address).await?;

    let account = resp["account"].as_str().unwrap_or(address.as_str());
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

    if has_nonzero {
        println!(
            "\nnext: 对每个有奖励的任务跑 `onchainos agent evaluator claim <jobId>` 领取；\n\
             成功后会收到 `reward_claimed` 事件确认入账。"
        );
    } else {
        println!("\n(当前无待领奖励)");
    }
    Ok(())
}
