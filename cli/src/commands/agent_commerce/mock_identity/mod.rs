//! mock_identity — Agent 身份模块 Mock (ERC-8004)
//!
//! 管理链上 Agent 身份（买家 / 卖家 / 仲裁者）。
//! 当前为 mock 实现，后续由身份模块团队替换为真实链上查询。
//! 替换时新建 `identity` 模块，删除本文件即可。

use anyhow::Result;

// ─── 角色定义 ──────────────────────────────────────────────────────────────

/// Agent 角色类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRole {
    /// 买家（Client / Buyer）
    Buyer,
    /// 卖家（Provider / Seller）
    Provider,
    /// 仲裁者（Evaluator / Judge）
    Evaluator,
}

impl std::fmt::Display for AgentRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentRole::Buyer => write!(f, "buyer"),
            AgentRole::Provider => write!(f, "provider"),
            AgentRole::Evaluator => write!(f, "evaluator"),
        }
    }
}

// ─── 身份信息 ──────────────────────────────────────────────────────────────

/// 某个 account 的 Agent 身份信息
#[derive(Debug, Clone)]
pub struct AgentIdentity {
    /// 钱包 account ID
    pub account_id: String,
    /// 钱包地址
    pub address: String,
    /// 链上 Agent ID（ERC-8004 tokenId）
    pub agent_id: String,
    /// 已注册的角色列表
    pub roles: Vec<AgentRole>,
}

/// 账户余额信息（USDT / USDG on XLayer）
#[derive(Debug, Clone)]
pub struct AccountBalance {
    pub account_id: String,
    pub address: String,
    pub usdt: f64,
    pub usdg: f64,
}

// ─── Mock 数据 ─────────────────────────────────────────────────────────────

/// [MOCK] 查询指定 account 是否注册了某个角色
///
/// TODO: 替换为真实链上查询（ERC-8004 registry）
pub async fn has_role(account_id: &str, _address: &str, role: AgentRole) -> Result<bool> {
    // Mock: 默认第一个账户有 buyer 身份
    let mock_buyers = ["mock-account-buyer"];
    let mock_providers = ["mock-account-provider"];
    let mock_evaluators = ["mock-account-evaluator"];

    let result = match role {
        AgentRole::Buyer => mock_buyers.contains(&account_id),
        AgentRole::Provider => mock_providers.contains(&account_id),
        AgentRole::Evaluator => mock_evaluators.contains(&account_id),
    };
    Ok(result)
}

/// [MOCK] 查询指定 account 的完整身份信息
///
/// TODO: 替换为真实链上查询
pub async fn get_identity(account_id: &str, address: &str) -> Result<Option<AgentIdentity>> {
    let mut roles = Vec::new();
    if has_role(account_id, address, AgentRole::Buyer).await? {
        roles.push(AgentRole::Buyer);
    }
    if has_role(account_id, address, AgentRole::Provider).await? {
        roles.push(AgentRole::Provider);
    }
    if has_role(account_id, address, AgentRole::Evaluator).await? {
        roles.push(AgentRole::Evaluator);
    }

    if roles.is_empty() {
        return Ok(None);
    }

    Ok(Some(AgentIdentity {
        account_id: account_id.to_string(),
        address: address.to_string(),
        agent_id: format!("agent-{}", &account_id[..account_id.len().min(8)]),
        roles,
    }))
}

/// [MOCK] 列出所有拥有指定角色的 accounts
///
/// TODO: 替换为真实实现 — 遍历 wallet accounts，查询链上身份
pub async fn list_accounts_with_role(
    wallets: &crate::wallet_store::WalletsJson,
    chain_name: &str,
    role: AgentRole,
) -> Result<Vec<AgentIdentity>> {
    let mut result = Vec::new();

    for (account_id, entry) in &wallets.accounts_map {
        for addr in &entry.address_list {
            if addr.chain_name == chain_name
                && has_role(account_id, &addr.address, role).await?
            {
                if let Some(identity) = get_identity(account_id, &addr.address).await? {
                    result.push(identity);
                }
            }
        }
    }

    Ok(result)
}

/// [MOCK] 注册 Agent 身份（指定角色）
///
/// TODO: 替换为真实链上注册（ERC-8004 mint + role registration）
pub async fn register_identity(_account_id: &str, _address: &str, role: AgentRole) -> Result<String> {
    let agent_id = format!("agent-{}-new", role);
    println!("[mock] ✓ 已注册 {role} 身份，agent_id: {agent_id}");
    Ok(agent_id)
}

/// [MOCK] 查询单个账户在 XLayer 上的 USDT / USDG 余额
///
/// TODO: 替换为真实余额查询（调用 wallet balance API，过滤 chainIndex=196 + tokenAddress）
pub async fn get_account_balance(_account_id: &str, address: &str) -> Result<AccountBalance> {
    // Mock: 返回固定余额
    Ok(AccountBalance {
        account_id: _account_id.to_string(),
        address: address.to_string(),
        usdt: 500.0,
        usdg: 200.0,
    })
}

/// [MOCK] 批量查询多个账户在 XLayer 上的 USDT / USDG 余额
///
/// TODO: 替换为真实实现 — 调用 wallet balance batch API
pub async fn get_accounts_balance(
    accounts: &[(&str, &str)],  // [(account_id, address), ...]
) -> Result<Vec<AccountBalance>> {
    let mut result = Vec::new();
    for (account_id, address) in accounts {
        result.push(get_account_balance(account_id, address).await?);
    }
    Ok(result)
}
