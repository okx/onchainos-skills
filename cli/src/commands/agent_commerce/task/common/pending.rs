//! 待用户决策列表（pending-decisions）本地缓存
//!
//! 文件：`~/.onchainos/pending-decisions.json`
//!
//! 配套 `xmtp_prompt_user` / `[USER_DECISION_RELAY]` 的 sub agent 工具配对规则使用，
//! 给 user session agent 一个**确定的**「当前有几条 pending」状态源，
//! 避免靠扫聊天历史推断（不可靠且会被上下文裁剪影响）。
//!
//! 三个子命令（在顶层以 `agent pending-decisions <add|remove|list>` 暴露）：
//! - `add`：sub agent 调 `xmtp_prompt_user` **之前**调一次，登记一条 pending
//! - `remove`：sub agent 解析完 `[USER_DECISION_RELAY]` **之前**调一次，删一条
//! - `list`：user session agent 进入「展示中 / 待用户回复」状态时调一次，拿当前列表
//!
//! 唯一键 = `(job_id, role, agent_id)` 三元组：
//! - 同 `(job_id, role)` 但不同 `agent_id`（典型场景：单钱包多 provider agent
//!   同时盯同一 public 任务）→ 各占一条不会互覆
//! - 重复 add 时按三元组替换旧条，避免漏调 remove 后再 add 造成重复

use anyhow::{bail, Result};
use chrono::Utc;
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const DEFAULT_TTL_SECS: i64 = 86400;

#[derive(Subcommand)]
pub enum PendingDecisionsCommand {
    /// 登记一条待用户决策（sub agent 在调 xmtp_prompt_user **之前**调用）
    Add {
        /// sub session sessionKey 整串（先调 session_status 工具拿到）
        #[arg(long = "sub-key")]
        sub_key: String,
        /// 任务 jobId
        #[arg(long = "job-id")]
        job_id: String,
        /// sub session 角色：buyer / provider / evaluator
        #[arg(long)]
        role: String,
        /// sub session 自己的 agentId（多 agent 钱包必填，唯一键的第三维度）
        #[arg(long = "agent-id")]
        agent_id: String,
        /// 一句话摘要（场景 1：新 prompt 末尾"另有 N 条待决策"简列时用）
        #[arg(long)]
        summary: String,
        /// 完整 userContent 原文（场景 2：反问聚合详细列表时 verbatim 渲染）
        #[arg(long = "user-content")]
        user_content: String,
        /// 过期时间（秒），默认 86400（24h）；过期条目下次 list 时自动清理
        #[arg(long, default_value_t = DEFAULT_TTL_SECS)]
        ttl: i64,
    },
    /// 按 (job_id, role, agent_id) 删除一条 pending（sub agent 在解析
    /// [USER_DECISION_RELAY] **之前**调用，避免 user agent 看到僵尸条目）
    Remove {
        #[arg(long = "job-id")]
        job_id: String,
        #[arg(long)]
        role: String,
        /// sub session 自己的 agentId（多 agent 钱包必填）
        #[arg(long = "agent-id")]
        agent_id: String,
    },
    /// 列出当前 pending（自动清理过期条目）。可按 --agent-id 过滤。
    /// `--format json` 输出 `{ ok, data: { pending: [...], count } }`，
    /// `--format text` 输出人类可读列表，每行 `<idx>. [任务 <短ID> 你作为<角色>(#<agentId>)] <summary>`。
    List {
        #[arg(long, default_value = "json")]
        format: String,
        /// 仅列出指定 agentId 的 pending（可选）。缺省返回全部。
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PendingEntry {
    pub sub_key: String,
    pub job_id: String,
    pub short_job_id: String,
    pub role: String,
    pub agent_id: String,
    pub summary: String,
    pub user_content: String,
    pub created_at: i64,
    pub expires_at: i64,
}

#[derive(Serialize, Deserialize, Default)]
struct PendingFile {
    pending: Vec<PendingEntry>,
}

fn pending_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("无法获取 HOME 目录"))?;
    let dir = home.join(".onchainos");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("pending-decisions.json"))
}

fn read_pending() -> Result<PendingFile> {
    let path = pending_path()?;
    if !path.exists() {
        return Ok(PendingFile::default());
    }
    let raw = std::fs::read_to_string(&path)?;
    if raw.trim().is_empty() {
        return Ok(PendingFile::default());
    }
    match serde_json::from_str::<PendingFile>(&raw) {
        Ok(pf) => Ok(pf),
        Err(e) => {
            // 容错：文件损坏时备份并重置（沿用 wallet_store 的容错风格）
            let backup = path.with_file_name(format!(
                "pending-decisions.broken-{}.json",
                Utc::now().timestamp()
            ));
            let _ = std::fs::copy(&path, &backup);
            eprintln!(
                "[pending] pending-decisions.json 解析失败 ({e})，已备份到 {} 并重置",
                backup.display()
            );
            Ok(PendingFile::default())
        }
    }
}

/// 原子写：先写 `.tmp` 再 rename（POSIX 上 rename 是原子操作）
fn write_pending_atomic(pf: &PendingFile) -> Result<()> {
    let path = pending_path()?;
    let tmp = path.with_extension("tmp");
    let json = serde_json::to_string_pretty(pf)?;
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

fn cleanup_expired(pf: &mut PendingFile) -> usize {
    let now = Utc::now().timestamp();
    let before = pf.pending.len();
    pf.pending.retain(|e| e.expires_at > now);
    before - pf.pending.len()
}

/// 短 jobId：前 6 + … + 后 4 字符。0x... hex 形式得到 `0x1b76…1be1`，
/// 长字符串 ID 得到 `task-0…long`。≤ 12 字符原样返回。
pub fn short_job_id(job_id: &str) -> String {
    if job_id.chars().count() <= 12 {
        return job_id.to_string();
    }
    let chars: Vec<char> = job_id.chars().collect();
    let head: String = chars.iter().take(6).collect();
    let tail: String = chars.iter().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    format!("{head}…{tail}")
}

fn validate_role(role: &str) -> Result<()> {
    if !["buyer", "provider", "evaluator"].contains(&role) {
        bail!("--role 必须是 buyer / provider / evaluator，当前: {role}");
    }
    Ok(())
}

fn role_zh(role: &str) -> &'static str {
    match role {
        "buyer" => "买家",
        "provider" => "卖家",
        "evaluator" => "仲裁者",
        _ => "未知角色",
    }
}

pub async fn run(cmd: PendingDecisionsCommand) -> Result<()> {
    match cmd {
        PendingDecisionsCommand::Add {
            sub_key, job_id, role, agent_id, summary, user_content, ttl,
        } => {
            validate_role(&role)?;
            if sub_key.trim().is_empty() {
                bail!("--sub-key 不能为空");
            }
            if job_id.trim().is_empty() {
                bail!("--job-id 不能为空");
            }
            if agent_id.trim().is_empty() {
                bail!("--agent-id 不能为空（唯一键第三维度，多 agent 钱包必填）");
            }
            if ttl <= 0 {
                bail!("--ttl 必须是正数（秒），当前: {ttl}");
            }

            let mut pf = read_pending()?;
            cleanup_expired(&mut pf);

            // 同 (job_id, role, agent_id) 已存在则替换，避免漏 remove 后再 add 造成重复
            let replaced = pf
                .pending
                .iter()
                .any(|e| e.job_id == job_id && e.role == role && e.agent_id == agent_id);
            pf.pending.retain(|e| {
                !(e.job_id == job_id && e.role == role && e.agent_id == agent_id)
            });

            let now = Utc::now().timestamp();
            let entry = PendingEntry {
                short_job_id: short_job_id(&job_id),
                sub_key,
                job_id,
                role,
                agent_id,
                summary,
                user_content,
                created_at: now,
                expires_at: now + ttl,
            };
            pf.pending.push(entry);
            write_pending_atomic(&pf)?;

            crate::output::success(serde_json::json!({
                "added": true,
                "replaced": replaced,
                "pending_count": pf.pending.len(),
            }));
            Ok(())
        }
        PendingDecisionsCommand::Remove { job_id, role, agent_id } => {
            validate_role(&role)?;
            if agent_id.trim().is_empty() {
                bail!("--agent-id 不能为空（唯一键第三维度，多 agent 钱包必填）");
            }
            let mut pf = read_pending()?;
            cleanup_expired(&mut pf);
            let before = pf.pending.len();
            pf.pending.retain(|e| {
                !(e.job_id == job_id && e.role == role && e.agent_id == agent_id)
            });
            let removed = before - pf.pending.len();
            write_pending_atomic(&pf)?;
            crate::output::success(serde_json::json!({
                "removed": removed,
                "pending_count": pf.pending.len(),
            }));
            Ok(())
        }
        PendingDecisionsCommand::List { format, agent_id } => {
            let mut pf = read_pending()?;
            let dropped = cleanup_expired(&mut pf);
            // 把过期清理后的状态写回（防止下次又跑一遍）
            if dropped > 0 {
                write_pending_atomic(&pf)?;
            }

            // 按 agent_id 过滤(可选)
            let filtered: Vec<&PendingEntry> = match &agent_id {
                Some(aid) if !aid.is_empty() => {
                    pf.pending.iter().filter(|e| &e.agent_id == aid).collect()
                }
                _ => pf.pending.iter().collect(),
            };

            match format.as_str() {
                "text" => {
                    if filtered.is_empty() {
                        println!("(当前无待决策)");
                    } else {
                        for (i, e) in filtered.iter().enumerate() {
                            println!(
                                "{}. [任务 {} 你作为{}(#{})] {}",
                                i + 1,
                                e.short_job_id,
                                role_zh(&e.role),
                                e.agent_id,
                                e.summary
                            );
                        }
                    }
                }
                "json" => {
                    let owned: Vec<PendingEntry> = filtered.into_iter().cloned().collect();
                    let count = owned.len();
                    crate::output::success(serde_json::json!({
                        "pending": owned,
                        "count": count,
                    }));
                }
                other => bail!("--format 必须是 json 或 text，当前: {other}"),
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_job_id_hex_64() {
        assert_eq!(
            short_job_id("0x1b76dabd3bf884626184e3b36b7c65b54929a827a8a26e223c4b8aa868d41be1"),
            "0x1b76…1be1"
        );
    }

    #[test]
    fn short_job_id_passthrough() {
        assert_eq!(short_job_id("0x12"), "0x12");
        assert_eq!(short_job_id("task-1"), "task-1");
        assert_eq!(short_job_id("task-001-12"), "task-001-12");
    }

    #[test]
    fn short_job_id_long_string() {
        assert_eq!(short_job_id("task-001-very-long"), "task-0…long");
    }

    #[test]
    fn validate_role_accepts_canonical() {
        assert!(validate_role("buyer").is_ok());
        assert!(validate_role("provider").is_ok());
        assert!(validate_role("evaluator").is_ok());
        assert!(validate_role("seller").is_err());
        assert!(validate_role("").is_err());
    }

    #[test]
    fn pending_entry_serializes_with_new_fields() {
        let entry = PendingEntry {
            sub_key: "agent:main:xmtp:group:foo".to_string(),
            job_id: "0x3938abcdef".to_string(),
            short_job_id: "0x3938…cdef".to_string(),
            role: "buyer".to_string(),
            agent_id: "100".to_string(),
            summary: "测试摘要".to_string(),
            user_content: "[任务 0x3938…cdef 你作为买家] 测试内容".to_string(),
            created_at: 1700000000,
            expires_at: 1700086400,
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        assert!(json.contains("\"agent_id\":\"100\""));
        assert!(json.contains("\"user_content\":"));
        let back: PendingEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.agent_id, "100");
        assert_eq!(back.user_content, "[任务 0x3938…cdef 你作为买家] 测试内容");
    }
}
