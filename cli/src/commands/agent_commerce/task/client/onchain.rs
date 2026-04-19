//! 签名写操作（需要链上签名）
//!
//! 单签: close, set-public, claim, apply
//! 双签: confirm-accept(escrow), complete, reject
//! 自定义: create（内联签名流程）

use anyhow::{bail, Result};

use crate::commands::agentic_wallet::transfer::{build_broadcast_body, resolve_address};
use crate::commands::agent_commerce::mock_identity::{self as identity, AgentRole, AccountBalance};
use crate::commands::agent_commerce::task::common::{
    PAYMENT_MODE_ESCROW, PAYMENT_MODE_NON_ESCROW,
    XLAYER_CHAIN_ID, XLAYER_CHAIN_INDEX, XLAYER_CHAIN_NAME,
};
use crate::commands::agent_commerce::task::signing;
use crate::wallet_api::UnsignedInfoResponse;

// ─── 校验函数 ────────────────────────────────────────────────────────────

/// 单次任务预算上限
const MAX_BUDGET: f64 = 10_000_000.0;

/// 解析 "72h" / "30m" / "3600" → 秒
fn parse_duration_secs(s: &str) -> Result<u64> {
    let s = s.trim();
    if let Some(h) = s.strip_suffix('h') {
        Ok(h.parse::<u64>()? * 3600)
    } else if let Some(m) = s.strip_suffix('m') {
        Ok(m.parse::<u64>()? * 60)
    } else {
        Ok(s.parse::<u64>()?)
    }
}

/// 校验货币符号
fn validate_currency(currency: &str) -> Result<()> {
    match currency.to_uppercase().as_str() {
        "USDT" | "USDG" => Ok(()),
        other => bail!("不支持的代币: {other}，仅支持 USDT 和 USDG"),
    }
}

/// 校验预算金额
fn validate_budget(budget: f64) -> Result<()> {
    if budget <= 0.0 {
        bail!("预算金额必须大于 0");
    }
    if budget > MAX_BUDGET {
        bail!("单次任务预算不得超过 {} USDT/USDG", MAX_BUDGET as u64);
    }
    Ok(())
}

/// 余额不足时输出提示（仅警告，不阻断流程）
fn warn_insufficient_balance(bal: &AccountBalance, budget: f64, currency: &str) {
    let available = match currency.to_uppercase().as_str() {
        "USDT" => bal.usdt,
        "USDG" => bal.usdg,
        _ => return,
    };
    if available < budget {
        println!(
            "⚠ 当前账户 {} 余额不足: {} {} (任务预算 {} {})，请在上链前充值",
            bal.address, available, currency.to_uppercase(),
            budget, currency.to_uppercase()
        );
    }
}

// ─── 创建任务（自定义签名流程）────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub async fn handle_create(
    http: &reqwest::Client,
    api: &str,
    description: String,
    description_summary: Option<String>,
    budget: f64,
    max_budget: Option<f64>,
    currency: String,
    deadline_open: String,
    deadline_submit: String,
    title: Option<String>,
) -> Result<()> {
    validate_currency(&currency)?;
    validate_budget(budget)?;

    let max_budget_val = max_budget.unwrap_or(budget);
    if max_budget_val < budget {
        bail!("--max-budget ({max_budget_val}) 不能小于 --budget ({budget})");
    }
    validate_budget(max_budget_val)?;

    let open_secs = parse_duration_secs(&deadline_open)
        .map_err(|_| anyhow::anyhow!("--deadline-open 格式错误，例如 72h 或 3600"))?;
    let submit_secs = parse_duration_secs(&deadline_submit)
        .map_err(|_| anyhow::anyhow!("--deadline-submit 格式错误，例如 48h 或 3600"))?;

    let title_str = title.unwrap_or_else(|| description.chars().take(30).collect());
    let summary = description_summary
        .unwrap_or_else(|| description.chars().take(200).collect());

    // ── Step 0: 身份检查 + 余额提示 ───────────────────────────
    let wallets = crate::wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!("未登录，请先执行 onchainos wallet auth"))?;

    let selected_account_id = &wallets.selected_account_id;
    let (_, selected_addr) = resolve_address(&wallets, None, XLAYER_CHAIN_NAME)?;

    let (account_id, addr_info) = if identity::has_role(
        selected_account_id,
        &selected_addr.address,
        AgentRole::Buyer,
    ).await? {
        println!("✓ 当前账户已具有买家身份 (account: {selected_account_id})");

        let bal = identity::get_account_balance(
            selected_account_id, &selected_addr.address,
        ).await?;
        warn_insufficient_balance(&bal, budget, &currency);

        (selected_account_id.clone(), selected_addr)
    } else {
        let buyer_accounts = identity::list_accounts_with_role(
            &wallets,
            XLAYER_CHAIN_NAME,
            AgentRole::Buyer,
        ).await?;

        if buyer_accounts.is_empty() {
            println!("当前无任何账户具有买家身份");
            println!("正在为当前账户注册买家身份...");
            let _agent_id = identity::register_identity(
                selected_account_id,
                &selected_addr.address,
                AgentRole::Buyer,
            ).await?;
            (selected_account_id.clone(), selected_addr)
        } else {
            let acct_pairs: Vec<(&str, &str)> = buyer_accounts
                .iter()
                .map(|a| (a.account_id.as_str(), a.address.as_str()))
                .collect();
            let balances = identity::get_accounts_balance(&acct_pairs).await?;

            println!("当前账户未注册买家身份，以下账户可用：");
            for (i, acct) in buyer_accounts.iter().enumerate() {
                let bal = balances.iter().find(|b| b.account_id == acct.account_id);
                let (usdt, usdg) = bal
                    .map(|b| (b.usdt, b.usdg))
                    .unwrap_or((0.0, 0.0));
                println!(
                    "  {}. account: {}  address: {}  agent: {}  USDT: {}  USDG: {}",
                    i + 1, acct.account_id, acct.address, acct.agent_id, usdt, usdg
                );
            }
            let chosen = &buyer_accounts[0];
            println!("使用账户: {} ({})", chosen.account_id, chosen.address);
            let (_, addr) = resolve_address(&wallets, Some(&chosen.address), XLAYER_CHAIN_NAME)?;
            (chosen.account_id.clone(), addr)
        }
    };

    // ── Step 1: 生成 calldata ────────
    let body = serde_json::json!({
        "title":              title_str,
        "description":        description,
        "description_summary": summary,
        "paymentTokenSymbol": currency.to_uppercase(),
        "paymentTokenAmount": budget.to_string(),
        "maxPaymentTokenAmount": max_budget_val.to_string(),
        "chainId":            XLAYER_CHAIN_ID,
        "expireConfig": {
            "acceptDeadline":    open_secs,
            "submittedDeadline": submit_secs
        },
        "paymentMode":        0,
        "visibility":         0
    });

    let resp: serde_json::Value = http
        .post(format!("{api}/priapi/v1/aieco/task/create"))
        .json(&body)
        .send().await
        .map_err(|e| anyhow::anyhow!("无法连接后端: {e}"))?
        .json().await?;

    if resp["code"] != 0 {
        bail!("创建失败: {}", resp["msg"].as_str().unwrap_or("unknown"));
    }

    let job_id = resp["data"]["jobId"].as_str().unwrap_or("?").to_string();
    let uop_data = &resp["data"]["uopData"];

    println!("✓ Calldata 已生成 (jobId: {job_id})");

    // ── Step 2: 签名 uopHash ─────────────
    let unsigned: UnsignedInfoResponse = serde_json::from_value(uop_data.clone())
        .map_err(|e| anyhow::anyhow!("解析 uopData 失败: {e}"))?;

    let broadcast_body = build_broadcast_body(
        &unsigned,
        &account_id,
        &addr_info.address,
        XLAYER_CHAIN_INDEX,
        true,
        false,
        false,
    )
    .await?;

    println!("✓ 签名完成");

    // ── Step 3: 广播上链 ──────────
    let bc_resp: serde_json::Value = http
        .post(format!("{api}/priapi/v1/aieco/task/broadcast"))
        .json(&broadcast_body)
        .send().await
        .map_err(|e| anyhow::anyhow!("广播失败: {e}"))?
        .json().await?;

    if bc_resp["code"] != 0 {
        bail!("广播失败: {}", bc_resp["msg"].as_str().unwrap_or("unknown"));
    }

    let tx_hash = bc_resp["data"][0]["txHash"].as_str().unwrap_or("pending");
    println!("✓ 任务已上链");
    println!("  jobId:  {job_id}");
    println!("  txHash: {tx_hash}");
    println!("  状态:   open（等待 Provider 报名）");
    println!();
    println!("下一步: onchainos agent recommend {job_id}");
    Ok(())
}

// ─── 单签写操作 ──────────────────────────────────────────────────────────

/// close — 关闭任务
pub async fn handle_close(
    http: &reqwest::Client,
    api: &str,
    job_id: &str,
) -> Result<()> {
    let (account_id, address) = signing::resolve_wallet_for_task(http, api, job_id).await?;
    let endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/close");
    let broadcast = format!("{api}/priapi/v1/aieco/task/broadcast");
    let body = serde_json::json!({});

    let result = signing::task_sign_and_broadcast(
        http, &endpoint, &body, &broadcast, &account_id, &address,
    ).await?;

    println!("✓ 任务已关闭，状态 → close");
    println!("  txHash: {}", result.tx_hash);
    Ok(())
}

/// set-public — 转为公开任务
pub async fn handle_set_public(
    http: &reqwest::Client,
    api: &str,
    job_id: &str,
) -> Result<()> {
    let (account_id, address) = signing::resolve_wallet_for_task(http, api, job_id).await?;
    let endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/setVisibility");
    let broadcast = format!("{api}/priapi/v1/aieco/task/broadcast");
    let body = serde_json::json!({"visibility": 1});

    let result = signing::task_sign_and_broadcast(
        http, &endpoint, &body, &broadcast, &account_id, &address,
    ).await?;

    println!("✓ 任务已转为公开，其他卖家可以看到并报名");
    println!("  txHash: {}", result.tx_hash);
    Ok(())
}

/// claim — 仲裁奖金领取
pub async fn handle_claim(
    http: &reqwest::Client,
    api: &str,
    job_id: &str,
) -> Result<()> {
    let (account_id, address) = signing::resolve_wallet_for_task(http, api, job_id).await?;
    let endpoint = format!("{api}/priapi/v1/aieco/task/claim");
    let broadcast = format!("{api}/priapi/v1/aieco/task/broadcast");
    let body = serde_json::json!({ "jobId": job_id });

    let result = signing::task_sign_and_broadcast(
        http, &endpoint, &body, &broadcast, &account_id, &address,
    ).await?;

    println!("✓ 仲裁奖金已领取");
    println!("  txHash: {}", result.tx_hash);
    Ok(())
}

/// apply — Provider 申请接单（TODO: 需改为签名流程）
pub async fn handle_apply(
    http: &reqwest::Client,
    api: &str,
    job_id: &str,
) -> Result<()> {
    let (account_id, address) = signing::resolve_wallet(None, None)?;
    let agent_id = std::env::var("AGENT_ID").unwrap_or_default();
    let endpoint  = format!("{api}/priapi/v1/aieco/task/{job_id}/apply");
    let broadcast = format!("{api}/priapi/v1/aieco/task/broadcast");
    let body = serde_json::json!({});
    let result = signing::task_sign_and_broadcast_with_headers(
        http, &endpoint, &body, &broadcast, &account_id, &address, &agent_id,
    ).await?;
    println!("✓ 已申请任务 {job_id}，等待买家确认");
    println!("  txHash: {}", result.tx_hash);
    Ok(())
}

/// confirm — Provider 协商完成后提交接单申请（单签）
///
/// - `token_amount`: 协商后价格字符串，"0" 表示接受原价
/// - `token_symbol`: 支付币种（USDT / USDG），为 None 时从任务读取
pub async fn handle_confirm(
    http: &reqwest::Client,
    api: &str,
    job_id: &str,
    token_amount: &str,
    token_symbol: Option<&str>,
    agent_id: Option<&str>,
) -> Result<()> {
    // 若未指定 token_symbol，从任务详情读取
    let symbol = if let Some(s) = token_symbol {
        validate_currency(s)?;
        s.to_uppercase()
    } else {
        let task_resp: serde_json::Value = http
            .get(format!("{api}/priapi/v1/aieco/task/{job_id}"))
            .send().await
            .map_err(|e| anyhow::anyhow!("无法连接后端: {e}"))?
            .json().await?;
        if task_resp["code"] != 0 {
            bail!("任务不存在: {job_id}");
        }
        task_resp["data"]["task"]["tokenSymbol"]
            .as_str()
            .unwrap_or("USDT")
            .to_string()
    };

    let amount = token_amount.trim().to_string();
    let (account_id, address) = signing::resolve_wallet(None, None)?;
    let agent_id = agent_id
        .map(|s| s.to_string())
        .or_else(|| std::env::var("AGENT_ID").ok())
        .unwrap_or_default();
    let endpoint  = format!("{api}/priapi/v1/aieco/task/{job_id}/apply");
    let broadcast = format!("{api}/priapi/v1/aieco/task/broadcast");
    let body = serde_json::json!({
        "tokenSymbol": symbol,
        "tokenAmount": amount,
    });

    let result = signing::task_sign_and_broadcast_with_headers(
        http, &endpoint, &body, &broadcast, &account_id, &address, &agent_id,
    ).await?;

    println!("✓ 已提交接单申请 jobId={job_id}");
    if amount != "0" {
        println!("  议价: {amount} {symbol}");
    } else {
        println!("  接受原价: {symbol}");
    }
    println!("  txHash: {}", result.tx_hash);
    Ok(())
}

// ─── 双签写操作 ──────────────────────────────────────────────────────────

/// confirm-accept — 确认接受卖家（担保双签 / 非担保单签）
pub async fn handle_confirm_accept(
    http: &reqwest::Client,
    api: &str,
    job_id: &str,
    provider: &str,
    payment_mode: &str,
) -> Result<()> {
    let (account_id, address) = signing::resolve_wallet_for_task(http, api, job_id).await?;
    let broadcast = format!("{api}/priapi/v1/aieco/task/broadcast");

    if payment_mode == PAYMENT_MODE_NON_ESCROW {
        // 非担保：标准单签 direct/accept
        let endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/direct/accept");
        let body = serde_json::json!({
            "providerAddress": provider,
            "providerAgentId": provider,
        });
        let result = signing::task_sign_and_broadcast(
            http, &endpoint, &body, &broadcast, &account_id, &address,
        ).await?;
        println!("✓ 已接受卖家 {provider}（非担保支付），任务状态 → accepted");
        println!("  注意：任务完成后需手动转账给卖家");
        println!("  txHash: {}", result.tx_hash);
    } else {
        // 担保：双签 pre-accept → 签 digest → accept → 签 uopHash → broadcast
        let pre_endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/pre-accept");
        let main_endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/accept");
        let pre_body = serde_json::json!({
            "providerAddress": provider,
            "providerAgentId": provider,
        });
        let provider_owned = provider.to_string();
        let result = signing::task_dual_sign_and_broadcast(
            http,
            &pre_endpoint,
            &pre_body,
            &main_endpoint,
            move |signature| serde_json::json!({
                "providerAddress": provider_owned,
                "providerAgentId": provider_owned,
                "paymentMode": PAYMENT_MODE_ESCROW,
                "signature": signature,  // 【待确认】字段名
            }),
            &broadcast,
            &account_id,
            &address,
        ).await?;
        println!("✓ 已接受卖家 {provider}（担保支付），任务状态 → accepted");
        println!("  txHash: {}", result.tx_hash);
    }
    Ok(())
}

/// complete — 验收通过
pub async fn handle_complete(
    http: &reqwest::Client,
    api: &str,
    job_id: &str,
) -> Result<()> {
    let (account_id, address) = signing::resolve_wallet_for_task(http, api, job_id).await?;
    let pre_endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/pre-complete");
    let main_endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/complete");
    let broadcast = format!("{api}/priapi/v1/aieco/task/broadcast");
    let pre_body = serde_json::json!({});

    let result = signing::task_dual_sign_and_broadcast(
        http,
        &pre_endpoint,
        &pre_body,
        &main_endpoint,
        |signature| serde_json::json!({
            "signature": signature,  // 【待确认】字段名
        }),
        &broadcast,
        &account_id,
        &address,
    ).await?;

    println!("✓ 任务验收通过，状态 → complete，款项已释放");
    println!("  txHash: {}", result.tx_hash);
    Ok(())
}

/// reject/refuse — 拒绝验收
pub async fn handle_reject(
    http: &reqwest::Client,
    api: &str,
    job_id: &str,
    reason: &str,
) -> Result<()> {
    let (account_id, address) = signing::resolve_wallet_for_task(http, api, job_id).await?;
    let pre_endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/pre-refuse");
    let main_endpoint = format!("{api}/priapi/v1/aieco/task/{job_id}/refuse");
    let broadcast = format!("{api}/priapi/v1/aieco/task/broadcast");
    let pre_body = serde_json::json!({});

    let reason_owned = reason.to_string();
    let result = signing::task_dual_sign_and_broadcast(
        http,
        &pre_endpoint,
        &pre_body,
        &main_endpoint,
        move |signature| serde_json::json!({
            "signature": signature,  // 【待确认】字段名
            "reason": reason_owned,
        }),
        &broadcast,
        &account_id,
        &address,
    ).await?;

    println!("✓ 已拒绝验收（原因：{reason}），状态 → refused");
    println!("  卖家有 24 小时内可申请仲裁");
    println!("  txHash: {}", result.tx_hash);
    Ok(())
}
