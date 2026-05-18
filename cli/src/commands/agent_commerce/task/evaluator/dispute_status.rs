//! `dispute/status` 硬门 helper —— 不是独立 CLI 子命令，是 `evidence-info`
//! (`handle_info`) 内联调用的前置门。
//!
//! 用途：evaluator 收到 `evaluator_selected` 等系统通知时，envelope 可能是旧轮
//! 的（agent 重启、网络滞后、commit 窗口已关、本轮已重抽、任务已结算……）。
//! 直接按 stale envelope 走 commit/reveal 会被罚 stake，所以 `handle_info` 在
//! 下载证据前先跑 [`precheck_round_gate`] 把所有 stale 场景兜在一起判一遍，
//! 任一不过就早返回不下载。
//!
//! API：`GET /priapi/v1/aieco/task/{jobId}/dispute/status`，返回
//! `{ jobId, currentRound, selectedVoter, taskStatus, disputeStatus }`。后端按
//! 调用者 `agenticId` 个性化（非选中陪审时 `selectedVoter=null`）。
//!
//! 四条硬门（与 / AND）：
//! 1. `taskStatus` 不能是终态 — 6 Completed / 7 Close / 8 Expired / 9 Rejected
//! 2. 入参 `round_num` 必须等于 `currentRound`（envelope 滞后于真实链上轮次 = stale）
//! 3. `disputeStatus` 必须是 3 (CommitPhase)（commit 窗口已关 / 未开 → 投了就罚）
//! 4. `selectedVoter` 必须非空（本账户不是本轮选中陪审）
//!
//! [`precheck_round_gate`] 自己负责诊断输出 + 稳定标记行：
//! - 全过 → 打印 `selected: yes`，返回 `true`（`handle_info` 继续下载证据）
//! - 任一不过 → 打印 `reason: ...` + `selected: no`，返回 `false`（`handle_info` 早返回）

use anyhow::{Context, Result};
use serde::de::IgnoredAny;
use serde::Deserialize;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::state_machine::{DisputeStatus, Status};

/// `dispute/status` 接口的原始响应承载体。
///
/// 命名上故意 `Response` 后缀和 [`crate::commands::agent_commerce::task::common::state_machine::DisputeStatus`]
/// 枚举区分开——一个是 HTTP DTO，一个是仲裁子状态机阶段枚举（响应里的
/// `dispute_round_status: i32` 字段才映射到那个枚举）。
/// **可空字段说明**：后端在任务终态 / 无 active dispute 时返回
/// `{currentRound:null, disputeStatus:null, selectedVoter:null, taskStatus:9}`，
/// 故 `current_round` / `dispute_round_status` / `selected_voter` 必须用 `Option`，
/// 不能裸 i64 / i32 + `#[serde(default)]`——后者只兜 missing 不兜 null，
/// 会触发 `invalid type: null, expected i64` deserialize 失败。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisputeStatusResponse {
    pub job_id: String,
    #[serde(default)]
    pub current_round: Option<i64>,
    /// 后端按调用者 agentId 个性化：非空 = 命中，null = 未命中（含 stale 通知 / 无 active dispute）。
    /// 内部字段（voterAddress / voterAgentId）在命中时一定就是调用者自己，零增量信息，所以
    /// 用 `IgnoredAny` consume 而不解出来——硬门只需要 `is_none()` 判断。
    #[serde(default)]
    pub selected_voter: Option<IgnoredAny>,
    /// task 主状态机当前状态。样本里始终为整数（终态时也给数字如 9 Rejected），不为 null，仍用裸 i32 + default。
    #[serde(default)]
    pub task_status: i32,
    /// 仲裁子状态机当前阶段（state_machine::DisputeStatus）。任务终态 / 无 dispute 时为 null。
    /// `rename` + `alias` 兼容后端两种 JSON key：`disputeStatus` / `disputeRoundStatus`，
    /// 哪个不同步都不破。
    #[serde(rename = "disputeStatus", alias = "disputeRoundStatus", default)]
    pub dispute_round_status: Option<i32>,
}

pub async fn get_dispute_status(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
) -> Result<DisputeStatusResponse> {
    let path = client.endpoint(job_id, "dispute/status");
    let data = client.get_with_identity(&path, agent_id).await?;
    serde_json::from_value(data).context("failed to parse dispute/status response")
}

/// 跑 4 条 AND 硬门：通过返回 `true` 并打印 `selected: yes`；任一不过返回 `false`
/// 并打印 `reason: ...` + `selected: no`。`agent_id` 由调用方（`handle_info`）负责
/// resolve，本函数不再二次 resolve（同一个 evaluator 流程内重复 resolve 无意义）。
pub async fn precheck_round_gate(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
    round_num: &str,
) -> Result<bool> {
    let s = get_dispute_status(client, job_id, agent_id).await?;

    // 把后端的裸 int 提前升成枚举，下游硬门校验 + 打印都走 enum，杜绝裸数字比较。
    // 两个枚举都对 unknown 值做了容错（Status::Other / DisputeStatus::Other）。
    // disputeStatus 字段后端在终态 / 无 dispute 时返 null，所以 dispute_status 整个是 Option。
    let task_status = Status::from_int(s.task_status);
    let dispute_status = s.dispute_round_status.map(DisputeStatus::from_int);

    // Option 字段打印：None 显示成 "null"，避免读者把缺省 0 误判成 round 0 / NONE 状态。
    let fmt_opt = |n: Option<i64>| n.map(|v| v.to_string()).unwrap_or_else(|| "null".into());
    let fmt_opt_i32 = |n: Option<i32>| n.map(|v| v.to_string()).unwrap_or_else(|| "null".into());

    println!("dispute status (jobId={})", s.job_id);
    println!("  currentRound : {}", fmt_opt(s.current_round));
    println!("  taskStatus   : {} ({})", s.task_status, task_status.as_str());
    println!(
        "  disputeStatus: {} ({})",
        fmt_opt_i32(s.dispute_round_status),
        dispute_status.as_ref().map(DisputeStatus::as_str).unwrap_or("null"),
    );
    println!(
        "  selectedVoter: {}",
        match &s.selected_voter {
            Some(_) => "present (this account is selected as juror for current round)",
            None => "null (not selected for current round / notification expired / no active dispute)",
        },
    );

    // 硬门（AND）：任一不过都是 stale，输出第一条失败原因即可。
    // 顺序：先验任务是否终态（最强信号——终态时 currentRound/disputeStatus 都会是 null，
    // 必须先短路掉，否则下面的 None 分支会给出误导性的 reason）→ 再验 round_num 可解析
    // → 再验链上 currentRound 不为 null → 再验 req_round == currentRound → 再验
    // disputeStatus 不为 null → 再验 disputeStatus == CommitPhase → 最后验本账户命中。
    let reason: Option<String> = if task_status.is_terminal() {
        Some(format!(
            "taskStatus={} ({}) is terminal — task finished, dispute window closed",
            s.task_status, task_status.as_str(),
        ))
    } else {
        match round_num.parse::<i64>() {
            Err(e) => Some(format!("--round-num cannot be parsed as integer: {round_num:?} ({e})")),
            Ok(req_round) => match (s.current_round, dispute_status.as_ref()) {
                (None, _) => Some(
                    "currentRound=null — no active dispute (task not in dispute / already ended / backend has not advanced round)".into(),
                ),
                (Some(cur), _) if req_round != cur => Some(format!(
                    "round mismatch: envelope round_num={req_round} != on-chain currentRound={cur} (stale envelope)",
                )),
                (Some(_), None) => Some(
                    "disputeStatus=null — dispute sub-state-machine not started / already settled (commit window guaranteed closed)".into(),
                ),
                (Some(_), Some(ds)) if *ds != DisputeStatus::CommitPhase => Some(format!(
                    "disputeStatus={} ({}) is not {} — commit window not open / already closed",
                    fmt_opt_i32(s.dispute_round_status),
                    ds.as_str(),
                    DisputeStatus::CommitPhase.as_str(),
                )),
                (Some(_), Some(_)) if s.selected_voter.is_none() => {
                    Some("selectedVoter=null — this account is not the selected juror for the current round".into())
                }
                (Some(_), Some(_)) => None,
            },
        }
    };

    // 稳定标记行 + reason 行：flow.rs 剧本按 `selected: yes/no` 判定走向，
    // reason 行用于诊断（任一硬门不过时输出，紧贴 selected 行上方）。
    match reason {
        None => {
            println!("\nselected: yes");
            Ok(true)
        }
        Some(r) => {
            println!("\nreason: {r}");
            println!("selected: no");
            Ok(false)
        }
    }
}
