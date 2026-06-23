use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::evaluator::staking_types::{self, MyStake};
use chrono::TimeZone;
use serde_json::Value;

pub async fn generate_next_action(job_id: &str, event: &str, agent_id: &str, message: Option<&Value>) -> String {
    if let Some(s) = staking_next_action(job_id, event, agent_id).await {
        return s;
    }
    if let Some(s) = dispute_next_action(job_id, event, agent_id, message).await {
        return s;
    }
    format!(
        "[unknown event={event} at jobId={job_id} ignored.\n\
         Do not pull context; do not guess other notifications.\n"
    )
}

/// Render unix seconds (>0) as `YYYY-MM-DD HH:MM:SS TZ` in local time, or `None`
/// for `0` / unparseable values.
fn fmt_local_time(ts: i64) -> Option<String> {
    if ts <= 0 {
        return None;
    }
    chrono::Local
        .timestamp_opt(ts, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S %Z").to_string())
}

async fn fetch_my_stake(agent_id: &str) -> Option<MyStake> {
    let mut client = TaskApiClient::new();
    staking_types::get_my_stake(&mut client, agent_id).await.ok()
}

fn notify_block(content: &str) -> String {
    format!(
        "Run `onchainos agent user-notify` to push the notification to the user. Translate the content below into the user's language first, then run:\n\n\
         ```bash\n\
         onchainos agent user-notify --content '<localized content>'\n\
         ```\n\n\
         Canonical English content:\n\
         \x20\x20\x20\x20{content}\n"
    )
}

fn notify_block_lines(lines: &[String]) -> String {
    let body = lines
        .iter()
        .map(|l| format!("    {l}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Run `onchainos agent user-notify` to push the notification to the user. Translate the content below into the user's language first, then run:\n\n\
         ```bash\n\
         onchainos agent user-notify --content '<localized content>'\n\
         ```\n\n\
         Canonical English content:\n\
         {body}\n"
    )
}

/// Extract a non-empty string field from `message`.
fn str_field(msg: &Value, key: &str) -> Option<String> {
    msg.get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Extract a signed integer field — accepts JSON number or numeric string.
fn i64_field(msg: &Value, key: &str) -> Option<i64> {
    msg.get(key).and_then(|v| match v {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.parse::<i64>().ok(),
        _ => None,
    })
}

/// Extract a field for verbatim display — accepts string or number.
fn display_field(msg: &Value, key: &str) -> Option<String> {
    msg.get(key).and_then(|v| match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    })
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Returns `None` when the deadline has already passed; otherwise renders
/// `"<N> hours"` for >=1h or `"less than 1 hour"` for sub-hour windows.
fn hours_left_text(deadline: i64) -> Option<String> {
    let now = now_secs();
    if deadline <= now {
        return None;
    }
    let hrs = (deadline - now) / 3600;
    if hrs >= 1 {
        Some(format!("{hrs} hours"))
    } else {
        Some("less than 1 hour".to_string())
    }
}

/// Returns `None` when the deadline has already passed; otherwise renders
/// `"<N> minutes remaining"` or `"less than 1 minute remaining"`.
fn minutes_left_text(deadline: i64) -> Option<String> {
    let now = now_secs();
    if deadline <= now {
        return None;
    }
    let mins = (deadline - now) / 60;
    if mins >= 1 {
        Some(format!("{mins} minutes remaining"))
    } else {
        Some("less than 1 minute remaining".to_string())
    }
}

async fn staking_next_action(_job_id: &str, event: &str, agent_id: &str) -> Option<String> {
    let body = match event {
        "staked" => {
            let content = match fetch_my_stake(agent_id).await {
                Some(s) => format!(
                    "Your stake is now active on-chain. Current activeStake is {} OKB.",
                    s.active_stake_okb
                ),
                None => "Your stake is now active on-chain.".to_string(),
            };
            format!("[Current Event] staked\n\n{}", notify_block(&content))
        }

        "unstake_requested" => {
            let content = match fetch_my_stake(agent_id).await {
                Some(s) => match fmt_local_time(s.unstake_available_at) {
                    Some(local) => format!(
                        "The unstake request has been recorded on-chain. Current cumulative pending unstake is {} OKB; the last claimable time is {}. You can cancel the unstake mid-way.",
                        s.pending_unstake_okb, local
                    ),
                    None => format!(
                        "The unstake request has been recorded on-chain. Current cumulative pending unstake is {} OKB. You can cancel the unstake before the cooldown ends.",
                        s.pending_unstake_okb
                    ),
                },
                None => "The unstake request has been recorded on-chain. You can cancel the unstake before the cooldown ends.".to_string(),
            };
            format!("[Current Event] unstake_requested\n\n{}", notify_block(&content))
        }

        "unstake_claimed" => format!(
            "[Current Status] unstake_claimed\n\n{}",
            notify_block("Your unstake has been claimed; OKB has been credited to your wallet.")
        ),

        "unstake_cancelled" => format!(
            "[Current Status] unstake_cancelled\n\n{}",
            notify_block("Your unstake has been cancelled; the pending OKB is back in staked state.")
        ),

        "stake_stopped" => format!(
            "[Current Status] stake_stopped\n\n{}",
            notify_block("You have exited the voter pool and will no longer be selected as a juror.")
        ),

        _ => return None,
    };
    Some(body)
}

async fn dispute_next_action(job_id: &str, event: &str, agent_id: &str, message: Option<&Value>) -> Option<String> {
    let body = match event {
        "evaluator_selected" => {
            let job_title = message.and_then(|m| str_field(m, "jobTitle")).unwrap_or_default();
            let agent_name = message.and_then(|m| str_field(m, "agentName"));
            let budget = message.and_then(|m| display_field(m, "budget"));
            let token_symbol = message.and_then(|m| str_field(m, "tokenSymbol"));
            let commit_deadline = message.and_then(|m| i64_field(m, "commitDeadline"));
            let round_num = message.and_then(|m| i64_field(m, "roundNum"));

            let mut lines = Vec::new();
            match &agent_name {
                Some(n) => lines.push(format!("【Your Agent {n} has been selected as juror for task [{job_title}]】")),
                None => lines.push(format!("You have been selected as juror for task [{job_title}]")),
            }
            lines.push(format!("Task title: {job_title}"));
            lines.push(format!("Task ID: #{job_id}"));
            if let (Some(b), Some(t)) = (&budget, &token_symbol) {
                lines.push(format!("Task Amount: {b} {t}"));
            }
            if let Some(d) = commit_deadline {
                if let Some(text) = hours_left_text(d) {
                    lines.push("⏰ Key deadline".to_string());
                    lines.push(format!("Your Agent must vote within {text}"));
                }
            }

            let step2 = match round_num {
                Some(n) => format!(
                    "**Step 2 — Fetch evidence:**\n\
                     ```bash\n\
                     onchainos agent evidence-info {job_id} --agent-id {agent_id} --round-num {n}\n\
                     ```\n\n\
                     Evidence JSON top-level: `{{ title, description, provider: {{reason, texts[], files[]}}, client: {{reason, texts[], files[]}} }}`. `description` / `title` is the task's original definition. Per side: `reason` is the party's stated motivation (`provider.reason` = why arbitration was raised; `client.reason` = why delivery was rejected); `texts[]` is free-text evidence; `files[]` is **any file type** (image / PDF / video / archive / unknown binary), already downloaded — each item has `localPath` (absolute path; **the local file has NO extension** — CLI deliberately leaves type detection to the agent).\n\n\
                     **Post-evidence hard constraints** (only the rules the agent could not infer on its own — tool choice / commands are the agent's call):\n\
                     - `files[]` items arrive **without extensions** by design; probe the type yourself (`file --mime-type`, hexdump, whatever) and use whatever tools you have to inspect each one. If you rename a file to give it an extension, **update the `localPath` you cite in the verdict**.\n\
                     - **Never vote blindly on an item you could not inspect.** If a file is unreadable for any reason (unsupported format, conversion failed, archive contents inaccessible, download error), cite it in the verdict as `<short reason> — contents unreviewable` and apply the rubric's evidence-missing rule for that item.\n\
                     - **Do not recurse into nested archives** (zip-in-tar-in-gz etc.). One extraction layer at most; deeper = treat as unreviewable.\n\
                     - A `files[]` item with `downloadError` set = CLI already gave up after 3 retries; treat as missing. Do not re-run `evidence-info` and do not scan local disk for replacements.\n"
                ),
                None => "**Step 2 aborted** — message envelope is missing `roundNum`; cannot fetch evidence. End this turn and wait for a fresh notification.\n".to_string(),
            };

            format!(
                "[Current Status] evaluator_selected\n\n\
                 **Step 1 — Notify the user that you've been selected as a juror:**\n\n\
                 {step1}\n\
                 → **Once Step 1 has attempted the `onchainos agent user-notify` call (whether it succeeds or errors), continue with Step 2 in this same turn.** Step 1 is a user-facing notification, not a precondition for Step 2.\n\n\
                 {step2}",
                step1 = notify_block_lines(&lines),
            )
        }

        "vote_committed" => {
            let job_title = message.and_then(|m| str_field(m, "jobTitle")).unwrap_or_default();
            let vote = message.and_then(|m| i64_field(m, "vote"));

            let mut lines = vec![
                format!("【Arbitration vote committed for task [{job_title}] · waiting for Reveal】"),
                format!("Task title: {job_title}"),
                format!("Task ID: #{job_id}"),
            ];
            if let Some(v) = vote {
                let label = if v == 0 { "User" } else { "ASP" };
                lines.push(format!("🗳️ Your Agent supports: {label}"));
            }
            format!(
                "[Current Status] vote_committed\n\n{}",
                notify_block_lines(&lines)
            )
        }

        "vote_commit_deadline_warn" => {
            let job_title = message.and_then(|m| str_field(m, "jobTitle")).unwrap_or_default();
            let commit_deadline = message.and_then(|m| i64_field(m, "commitDeadline"));
            let slash_timeout_bps = message.and_then(|m| str_field(m, "slashTimeoutBps"));
            let slashed_cooldown_seconds = message.and_then(|m| i64_field(m, "slashedCooldownSeconds"));

            let mut lines = vec![
                format!("【⏰ URGENT: Arbitration vote for task [{job_title}] is about to close】"),
                format!("Task title: {job_title}"),
                format!("Task ID: #{job_id}"),
            ];
            if let Some(d) = commit_deadline {
                if let (Some(local), Some(text)) = (fmt_local_time(d), minutes_left_text(d)) {
                    lines.push(format!("Commit deadline: {local} ({text})"));
                }
            }
            lines.push("Current Status: Agent has not committed yet".to_string());
            lines.push("🚨 Timeout consequences:".to_string());
            if let Some(bps) = &slash_timeout_bps {
                lines.push(format!("• Stake slashed {bps}"));
            }
            if let Some(cd) = slashed_cooldown_seconds {
                lines.push(format!(
                    "• Enter a {}h cooldown during which you cannot be selected",
                    cd / 3600
                ));
            }
            lines.push("• Miss the base validation fee".to_string());
            lines.push("⚡ Have the Agent vote immediately".to_string());
            format!(
                "[Current Status] vote_commit_deadline_warn\n\n{}",
                notify_block_lines(&lines)
            )
        }

        "vote_reveal_deadline_warn" => {
            let job_title = message.and_then(|m| str_field(m, "jobTitle")).unwrap_or_default();
            let reveal_deadline = message.and_then(|m| i64_field(m, "revealDeadline"));
            let slash_timeout_bps = message.and_then(|m| str_field(m, "slashTimeoutBps"));
            let slashed_cooldown_seconds = message.and_then(|m| i64_field(m, "slashedCooldownSeconds"));

            let mut lines = vec![
                format!("【⏰ URGENT: Arbitration reveal for task [{job_title}] is about to close】"),
                format!("Task title: {job_title}"),
                format!("Task ID: #{job_id}"),
            ];
            if let Some(d) = reveal_deadline {
                if let (Some(local), Some(text)) = (fmt_local_time(d), minutes_left_text(d)) {
                    lines.push(format!("Reveal deadline: {local} ({text})"));
                }
            }
            lines.push("Current Status: Agent has not revealed yet".to_string());
            lines.push("🚨 Timeout consequences:".to_string());
            if let Some(bps) = &slash_timeout_bps {
                lines.push(format!("• Stake slashed {bps}"));
            }
            if let Some(cd) = slashed_cooldown_seconds {
                lines.push(format!(
                    "• Enter a {}h cooldown during which you cannot be selected",
                    cd / 3600
                ));
            }
            lines.push("• Miss the base validation fee".to_string());
            lines.push("⚡ Have the Agent reveal immediately".to_string());
            format!(
                "[Current Status] vote_reveal_deadline_warn\n\n{}",
                notify_block_lines(&lines)
            )
        }

        "reveal_started" => format!(
            "[Current Status] reveal_started\n\n\
             **Step 1 — Execute reveal:**\n\
             ```bash\n\
             onchainos agent vote-reveal {job_id} --agent-id {agent_id}\n\
             ```\n\n\
             [Error mapping]\n\
             - `canReveal=false` → CLI has already pre-checked and rejected; no retry needed. This round may have settled already (wait for dispute_resolved) or you did not commit (normal skip). **End this turn; skip Step 2.**\n\
             - `voter has not committed` → you did not commit this round; skipping reveal is normal. **End this turn; skip Step 2.**\n\
             - Other failures: retry up to 3 times.\n\n\
             **Step 2 — Notify the user that the reveal has been submitted via `onchainos agent user-notify`.**\n\n\
             {}",
            notify_block(&format!(
                "Your agent has submitted the reveal transaction for Job jobId={job_id}. Waiting for chain confirmation — no action needed from you."
            ))
        ),

        "vote_revealed" => format!(
            "[Current Status] vote_revealed\n\n{}",
            notify_block(&format!(
                "Your agent has revealed its vote on-chain for Job jobId={job_id}. Waiting for the dispute resolution result — no action needed from you."
            ))
        ),

        "dispute_resolved" => {
            let job_title = message.and_then(|m| str_field(m, "jobTitle")).unwrap_or_default();
            let agent_name = message.and_then(|m| str_field(m, "agentName"));
            let vote = message.and_then(|m| i64_field(m, "vote"));
            let job_status = message.and_then(|m| str_field(m, "jobStatus"));
            let slash_minority_bps = message.and_then(|m| str_field(m, "slashMinorityBps"));
            let slash_timeout_bps = message.and_then(|m| str_field(m, "slashTimeoutBps"));
            let has_commit = message.and_then(|m| i64_field(m, "hasCommit")).unwrap_or(1);
            let has_reveal = message.and_then(|m| i64_field(m, "hasReveal")).unwrap_or(1);

            let your_vote = vote.map(|v| if v == 0 { "User" } else { "ASP" });
            let winning_side = match job_status.as_deref() {
                Some("complete") => Some("ASP"),
                Some("failed") => Some("User"),
                _ => None,
            };

            #[derive(Clone, Copy)]
            enum Branch { MissedCommit, MissedReveal, Won, Lost }
            let branch = if has_commit == 0 {
                Branch::MissedCommit
            } else if has_reveal == 0 {
                Branch::MissedReveal
            } else {
                match (your_vote, winning_side) {
                    (Some(y), Some(w)) if y == w => Branch::Won,
                    _ => Branch::Lost,
                }
            };

            match branch {
                Branch::MissedCommit | Branch::MissedReveal => {
                    let phase = if matches!(branch, Branch::MissedCommit) { "Commit" } else { "Reveal" };
                    let mut lines = Vec::new();
                    match &agent_name {
                        Some(n) => lines.push(format!("【⚖️ Your Agent {n} missed [{phase}] for task [{job_title}] arbitration — penalty incoming】")),
                        None => lines.push(format!("⚖️ You missed [{phase}] for task [{job_title}] arbitration — penalty incoming")),
                    }
                    lines.push(format!("Task title: {job_title}"));
                    lines.push(format!("Task ID: #{job_id}"));
                    lines.push(format!("You did not participate in [{phase}]"));
                    if let Some(bps) = &slash_timeout_bps {
                        lines.push("🚫 Penalty applied".to_string());
                        lines.push(format!("• Stake slashed {bps}"));
                    }
                    format!(
                        "[Current Status] dispute_resolved\n\n\
                         {}\n\
                         Missed-{} branch ends this turn; do not call `arbitration-claim`.\n",
                        notify_block_lines(&lines),
                        phase.to_lowercase()
                    )
                }
                Branch::Won => {
                    let mut lines = vec![
                        format!("【🎉 Arbitration result for task [{job_title}]: your vote aligned with the majority — reward eligible】"),
                        format!("Task title: {job_title}"),
                        format!("Task ID: #{job_id}"),
                    ];
                    if let Some(y) = your_vote {
                        lines.push(format!("Your vote: backed {y} ✓ aligned with majority"));
                    }
                    format!(
                        "[Current Status] dispute_resolved\n\n\
                         {}\n\
                         Pull claimable then claim:\n\
                         ```bash\n\
                         onchainos agent arbitration-claimable --agent-id {agent_id}\n\
                         ```\n\
                         The last line is the stable marker `hasClaimable: yes | no`. Decide on that line only; do not parse amounts.\n\
                         - `hasClaimable: no` → end this turn; do not call claim (reward may be pending settlement; a later `reward_claimed` event will close the loop).\n\
                         - `hasClaimable: yes` →\n\
                         \x20\x20```bash\n\
                         \x20\x20onchainos agent arbitration-claim --agent-id {agent_id}\n\
                         \x20\x20```\n\
                         \x20\x20⚠️ Account-level pull: aside from `--agent-id`, pass no other business params. Retry up to 3 times on failure. Final credit confirmation arrives via the later `reward_claimed` event.\n",
                        notify_block_lines(&lines)
                    )
                }
                Branch::Lost => {
                    let mut lines = vec![
                        format!("【⚠️ Arbitration result for task [{job_title}]: your vote disagreed with the majority — slash penalty incoming】"),
                        format!("Task title: {job_title}"),
                        format!("Task ID: #{job_id}"),
                    ];
                    if let Some(y) = your_vote {
                        lines.push(format!("Your vote: backed {y} ✗ opposed majority"));
                    }
                    if let Some(bps) = &slash_minority_bps {
                        lines.push("🚫 Penalty applied".to_string());
                        lines.push(format!("• Stake slashed {bps}"));
                    }
                    format!(
                        "[Current Status] dispute_resolved\n\n\
                         {}\n\
                         Lost branch ends this turn; do not call `arbitration-claim` (nothing to claim). The slash was conveyed in the notification above — no follow-up event will arrive.\n",
                        notify_block_lines(&lines)
                    )
                }
            }
        }

        "cooldown_entered" => {
            let content = match fetch_my_stake(agent_id).await.and_then(|s| fmt_local_time(s.cooldown_ends_at)) {
                Some(local) => format!(
                    "You've entered the absence cooldown period; you won't be selected as a juror before {local}."
                ),
                None => "You've entered the absence cooldown period and won't be selected as a juror during this period.".to_string(),
            };
            format!("[Current Status] cooldown_entered\n\n{}", notify_block(&content))
        }

        "round_failed" => {
            let job_title = message.and_then(|m| str_field(m, "jobTitle")).unwrap_or_default();
            let agent_name = message.and_then(|m| str_field(m, "agentName"));
            let abstain_count = message.and_then(|m| display_field(m, "abstainCount"));
            let total_slashed = message.and_then(|m| display_field(m, "totalSlashed"));
            let slash_timeout_bps = message.and_then(|m| str_field(m, "slashTimeoutBps"));
            let reveal_count = message.and_then(|m| display_field(m, "revealCount"));
            let has_commit = message.and_then(|m| i64_field(m, "hasCommit")).unwrap_or(1);
            let has_reveal = message.and_then(|m| i64_field(m, "hasReveal")).unwrap_or(1);

            #[derive(Clone, Copy)]
            enum Branch { MissedCommit, MissedReveal, Invalidated }
            let branch = if has_commit == 0 {
                Branch::MissedCommit
            } else if has_reveal == 0 {
                Branch::MissedReveal
            } else {
                Branch::Invalidated
            };

            match branch {
                Branch::MissedCommit | Branch::MissedReveal => {
                    let phase = if matches!(branch, Branch::MissedCommit) { "Commit" } else { "Reveal" };
                    let mut lines = Vec::new();
                    match &agent_name {
                        Some(n) => lines.push(format!("【⚖️ Your Agent {n} missed [{phase}] for task [{job_title}] arbitration — penalty incoming】")),
                        None => lines.push(format!("⚖️ You missed [{phase}] for task [{job_title}] arbitration — penalty incoming")),
                    }
                    lines.push(format!("Task title: {job_title}"));
                    lines.push(format!("Task ID: #{job_id}"));
                    lines.push(format!("You did not participate in [{phase}]"));
                    if let Some(bps) = &slash_timeout_bps {
                        lines.push("🚫 Penalty applied".to_string());
                        lines.push(format!("• Stake slashed {bps}"));
                    }
                    format!(
                        "[Current Status] round_failed\n\n\
                         {}\n\
                         Missed-{} branch ends this turn.\n",
                        notify_block_lines(&lines),
                        phase.to_lowercase()
                    )
                }
                Branch::Invalidated => {
                    let mut lines = vec![
                        format!("【⚖️ Task [{job_title}] arbitration round invalidated】"),
                        format!("Task title: {job_title}"),
                        format!("Task ID: #{job_id}"),
                        "Tally: no side reached ≥ 50%".to_string(),
                    ];
                    if let (Some(a), Some(t), Some(b), Some(r)) =
                        (&abstain_count, &total_slashed, &slash_timeout_bps, &reveal_count)
                    {
                        lines.push("💰 Abstain-slash pool distribution".to_string());
                        lines.push(format!("• Source: {a} abstainers × {b} = {t} OKB total"));
                        lines.push(format!("• Split evenly among {r} revealers"));
                    }
                    format!(
                        "[Current Status] round_failed\n\n{}",
                        notify_block_lines(&lines)
                    )
                }
            }
        }

        "reward_claimed" => format!(
            "[Current Status] reward_claimed\n\n{}",
            notify_block("Your arbitration reward has been credited.")
        ),

        _ => return None,
    };
    Some(body)
}

/// Step 3-4 of the `evaluator_selected` playbook, intended to be appended to
/// `evidence-info` stdout instead of returned by `next-action`.
///
/// Rationale: when next-action's evaluator_selected body included the full
/// vote-commit CLI template, a weak LLM could pattern-match the command line
/// and skip Step 2 (evidence-info) + Step 3 (rubric read). By printing these
/// steps only after evidence has actually been fetched, the LLM physically
/// cannot see the vote-commit invocation template until it has pulled the
/// evidence.
pub fn evaluator_selected_post_evidence_steps(job_id: &str, agent_id: &str) -> String {
    format!(
        "→ **Continue with Step 3 in this same turn — it is NOT event-driven.**\n\n\
         **Step 3 — Render the verdict per `references/evaluator-decision-rubric.md`:**\n\
         - **Prerequisite — file readability check**: read `references/evaluator-decision-rubric.md`.\n\
         \x20\x20Read failure / file missing / empty content → **stop this turn immediately** (no commit, no fallback default rules, no search for replacement file). Run `onchainos agent user-notify` (🌐 localize first), then end the turn:\n\n\
         ```bash\n\
         onchainos agent user-notify --content '<localized content>'\n\
         ```\n\n\
         Canonical English content (substitute placeholders first):\n\
         \x20\x20\x20\x20Arbitration aborted for task jobId={job_id}: the decision rubric `references/evaluator-decision-rubric.md` is missing or unreadable; this round's vote is skipped.\n\
         \x20\x20\x20\x20⚠️ commit window timeout will slash your stake — please restore the file as soon as possible.\n\n\
         - Read success and evidence already output → produce the final `vote` and the verdict text per the rubric's Verdict section (whichever heading defines the verdict template).\n\n\
         → **Once Step 3's verdict text is produced, continue with Step 4 in this same turn.**\n\n\
         **Step 4 — Execute commit:**\n\
         - **Flatten the entire verdict text into a single line** with `\\n` literal escapes (two characters: `\\` + `n`, not a real newline) replacing every real newline; pass via `--reason`.\n\
         - **Compress the verdict into a ≤30-character one-sentence summary** that captures the decision. Count is Unicode characters, not bytes — CJK and Latin characters each count as 1. The CLI hard-fails if the value is empty or exceeds 30 characters. Pass via `--reason-summary`.\n\
         ```bash\n\
         onchainos agent vote-commit {job_id} --vote <0|1> --reason \"<flattened verdict text from Step 3, with every real newline replaced by the two-character escape \\n>\" --reason-summary \"<≤30-char one-sentence summary>\" --agent-id {agent_id}\n\
         ```\n\
         ⚠️ **Only 0 (Approve / Client wins) or 1 (Reject / Provider wins) — skip is forbidden**.\n\
         ⚠️ **The `<0|1>` value MUST come from Step 3** — it is the binary vote that Step 3 derived by applying `references/evaluator-decision-rubric.md` (whatever decision procedure that document defines) to the evidence. Do **not** commit a vote that bypassed Step 3 — guessing / pattern-matching / averaging a value here violates the rubric and produces an unfounded ruling.\n\
         ⚠️ **`--reason` is the full verdict produced by Step 3**. Empty / whitespace-only values are rejected by the CLI. CLI un-escapes `\\n` → newline, `\\t` → tab, `\\r` → CR, `\\\\` → `\\`, `\\\"` → `\"` before sending to backend; the backend stores it as the human-readable on-chain audit trail. If the user-customized rubric (no verdict template defined), still pass a minimal one-line reason such as `\"Verdict not generated — rubric verdict missing.\"` \n\
         ⚠️ **`--reason-summary` is a ≤30-Unicode-character one-sentence headline** distilled from the same verdict — no markdown / line breaks / bullet markers. If you can't compress further, drop low-information words first; do not truncate mid-character to dodge the limit (the CLI counts after trim and rejects overflows).\n\
         - **Character taboos inside both `--reason` and `--reason-summary` values** (otherwise the shell will corrupt the argument before the CLI even sees it):\n\
         \x20\x20- `\"` (double quote) → escape as `\\\"`\n\
         \x20\x20- `` ` `` (backtick) → either replace with `'` (single quote) or escape as `` \\` ``; an unescaped backtick triggers shell command substitution\n\
         \x20\x20- `$` → escape as `\\$` to prevent shell variable expansion\n\
         \x20\x20- Real newlines / tabs / CRs → **must** use `\\n` / `\\t` / `\\r` escapes; never embed a literal newline (the command will break across lines)\n\
         Retry up to 3 times on failure (CRITICAL — closing of the commit window triggers timeout slashing).\n"
    )
}
