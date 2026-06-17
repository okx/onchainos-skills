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
        "Run `okx-a2a user notify` to push the notification to the user. Translate the content below into the user's language first, then run:\n\n\
         ```bash\n\
         okx-a2a user notify --content '<localized content>' --json\n\
         ```\n\n\
         Canonical English content:\n\
         \x20\x20\x20\x20{content}\n"
    )
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

        "unstake_claimed" => "[Current Status] unstake_claimed\n\n\
             Run `okx-a2a user notify` to push the notification to the user. Translate the content below into the user's language first, then run:\n\n\
             ```bash\n\
             okx-a2a user notify --content '<localized content>' --json\n\
             ```\n\n\
             Canonical English content:\n\
             \x20\x20\x20\x20Your unstake has been claimed; OKB has been credited to your wallet.\n".to_string(),

        "unstake_cancelled" => "[Current Status] unstake_cancelled\n\n\
             Run `okx-a2a user notify` to push the notification to the user. Translate the content below into the user's language first, then run:\n\n\
             ```bash\n\
             okx-a2a user notify --content '<localized content>' --json\n\
             ```\n\n\
             Canonical English content:\n\
             \x20\x20\x20\x20Your unstake has been cancelled; the pending OKB is back in staked state.\n".to_string(),

        "stake_stopped" => "[Current Status] stake_stopped\n\n\
             Run `okx-a2a user notify` to push the notification to the user. Translate the content below into the user's language first, then run:\n\n\
             ```bash\n\
             okx-a2a user notify --content '<localized content>' --json\n\
             ```\n\n\
             Canonical English content:\n\
             \x20\x20\x20\x20You have exited the voter pool and will no longer be selected as a juror.\n".to_string(),

        _ => return None,
    };
    Some(body)
}

async fn dispute_next_action(job_id: &str, event: &str, agent_id: &str, message: Option<&Value>) -> Option<String> {
    let body = match event {
        "evaluator_selected" => format!(
            "[Current Status] evaluator_selected\n\n\
             **Step 1 — Notify the user that you've been selected as a juror:**\n\n\
             Extract from `message`: `jobTitle`, `budget`, `tokenSymbol`, `commitDeadline` (epoch seconds), `agentName`. Render `commitDeadline` (epoch seconds) into the user's local time as `commitDeadlineLocal`, and compute `hoursLeft` = `floor((commitDeadline - now_epoch_seconds) / 3600)`. Render `hoursLeftText`: if `hoursLeft >= 1` use `<hoursLeft> hours`; else if the deadline has not passed (`commitDeadline > now_epoch_seconds`) use `less than 1 hour`; else treat as expired (drop the entire `⏰ Key deadline` block). **Substitute every `<message.jobTitle>` below with the actual value extracted from `message.jobTitle`.**\n\n\
             Run `okx-a2a user notify` to push the notification to the user. Translate the content below into the user's language first, then run:\n\n\
             ```bash\n\
             okx-a2a user notify --content '<localized content>' --json\n\
             ```\n\n\
             Canonical English content (substitute placeholders first):\n\
             \x20\x20\x20\x20【Your Agent <agentName> has been selected as juror for task [<message.jobTitle>]】\n\
             \x20\x20\x20\x20Task title: <message.jobTitle>\n\
             \x20\x20\x20\x20Task ID: #{job_id}\n\
             \x20\x20\x20\x20Task Amount: <budget> <tokenSymbol>\n\
             \x20\x20\x20\x20⏰ Key deadline\n\
             \x20\x20\x20\x20Your Agent must vote within <hoursLeftText>\n\n\
             [Field-missing fallbacks] Apply each independently — do **not** invent placeholders.\n\
             - `agentName` missing → degrade header to `You have been selected as juror for task [<message.jobTitle>]`.\n\
             - `budget` / `tokenSymbol` missing → drop the `Amount:` line.\n\
             - `commitDeadline` missing or deadline already passed → drop the entire `⏰ Key deadline` block.\n\n\
             → **Once Step 1 has attempted the `okx-a2a user notify` call (whether it succeeds or errors), continue with Step 2 in this same turn.** Step 1 is a user-facing notification, not a precondition for Step 2.\n\n\
             **Step 2 — Fetch evidence (`--round-num` comes from the envelope's top-level `roundNum`; if missing, abort this turn and log `missing roundNum in payload; abort`):**\n\
             ```bash\n\
             onchainos agent evidence-info {job_id} --agent-id {agent_id} --round-num <envelope top-level roundNum>\n\
             ```\n\n\
             Evidence JSON top-level: `{{ title, description, provider: {{reason, texts[], files[]}}, client: {{reason, texts[], files[]}} }}`. `description` / `title` is the task's original definition. Per side: `reason` is the party's stated motivation (`provider.reason` = why arbitration was raised; `client.reason` = why delivery was rejected); `texts[]` is free-text evidence; `files[]` is **any file type** (image / PDF / video / archive / unknown binary), already downloaded — each item has `localPath` (absolute path; **the local file has NO extension** — CLI deliberately leaves type detection to the agent).\n\n\
             **Post-evidence hard constraints** (only the rules the agent could not infer on its own — tool choice / commands are the agent's call):\n\
             - `files[]` items arrive **without extensions** by design; probe the type yourself (`file --mime-type`, hexdump, whatever) and use whatever tools you have to inspect each one. If you rename a file to give it an extension, **update the `localPath` you cite in the verdict**.\n\
             - **Never vote blindly on an item you could not inspect.** If a file is unreadable for any reason (unsupported format, conversion failed, archive contents inaccessible, download error), cite it in the verdict as `<short reason> — contents unreviewable` and apply the rubric's evidence-missing rule for that item.\n\
             - **Do not recurse into nested archives** (zip-in-tar-in-gz etc.). One extraction layer at most; deeper = treat as unreviewable.\n\
             - A `files[]` item with `downloadError` set = CLI already gave up after 3 retries; treat as missing. Do not re-run `evidence-info` and do not scan local disk for replacements.\n"
        ),

        "vote_committed" => format!(
            "[Current Status] vote_committed\n\n\
             Extract from `message`: `jobTitle`, `vote` (0 or 1). Render vote as text:\n\
             - `vote = 0` → `User`\n\
             - `vote = 1` → `ASP`\n\
             Run `okx-a2a user notify` to push the notification to the user. **Substitute `<message.jobTitle>` with the actual extracted value.** Translate the content below into the user's language first, then run:\n\n\
             ```bash\n\
             okx-a2a user notify --content '<localized content>' --json\n\
             ```\n\n\
             Canonical English content (substitute placeholders first):\n\
             \x20\x20\x20\x20【Arbitration vote committed for task [<message.jobTitle>] · waiting for Reveal】\n\
             \x20\x20\x20\x20Task title: <message.jobTitle>\n\
             \x20\x20\x20\x20Task ID: #{job_id}\n\
             \x20\x20\x20\x20🗳️ Your Agent supports: <ASP | User>\n\n\
             [Field-missing fallbacks]\n\
             - `vote` missing → drop the `🗳️ Your Agent supports:` line entirely; do NOT guess.\n"
        ),

        "vote_commit_deadline_warn" => format!(
            "[Current Status] vote_commit_deadline_warn\n\n\
             Extract from `message`: `jobTitle`, `commitDeadline`, `slashTimeoutBps`, `slashedCooldownSeconds`. Compute `commitDeadlineLocal` from `commitDeadline` (local time) and `minutesLeft` = `floor((commitDeadline - now_epoch_seconds) / 60)`. Render `minutesLeftText`: if `minutesLeft >= 1` use `<minutesLeft> minutes remaining`; else if the deadline has not passed (`commitDeadline > now_epoch_seconds`) use `less than 1 minute remaining`; else treat as expired (drop the `Commit deadline:` line). Compute `cooldownHours` = `slashedCooldownSeconds / 3600`.\n\
             Run `okx-a2a user notify` to push the notification to the user. **Substitute `<message.jobTitle>` with the actual extracted value.** Translate the content below into the user's language first, then run:\n\n\
             ```bash\n\
             okx-a2a user notify --content '<localized content>' --json\n\
             ```\n\n\
             Canonical English content (substitute placeholders first):\n\
             \x20\x20\x20\x20【⏰ URGENT: Arbitration vote for task [<message.jobTitle>] is about to close】\n\
             \x20\x20\x20\x20Task title: <message.jobTitle>\n\
             \x20\x20\x20\x20Task ID: #{job_id}\n\
             \x20\x20\x20\x20Commit deadline: <commitDeadlineLocal> (<minutesLeftText>)\n\
             \x20\x20\x20\x20Current Status: Agent has not committed yet\n\
             \x20\x20\x20\x20🚨 Timeout consequences:\n\
             \x20\x20\x20\x20• Stake slashed <slashTimeoutBps>\n\
             \x20\x20\x20\x20• Enter a <cooldownHours>h cooldown during which you cannot be selected\n\
             \x20\x20\x20\x20• Miss the base validation fee\n\
             \x20\x20\x20\x20⚡ Have the Agent vote immediately\n\n\
             [Field-missing fallbacks]\n\
             - `commitDeadline` missing or deadline already passed → drop the `Commit deadline:` line.\n\
             - `slashTimeoutBps` missing → drop the `• Stake slashed` bullet.\n\
             - `slashedCooldownSeconds` missing → drop the `• Enter a ... cooldown` bullet.\n"
        ),

        "vote_reveal_deadline_warn" => format!(
            "[Current Status] vote_reveal_deadline_warn\n\n\
             Extract from `message`: `jobTitle`, `revealDeadline`, `slashTimeoutBps`, `slashedCooldownSeconds`. Compute `revealDeadlineLocal` from `revealDeadline` (local time) and `minutesLeft` = `floor((revealDeadline - now_epoch_seconds) / 60)`. Render `minutesLeftText`: if `minutesLeft >= 1` use `<minutesLeft> minutes remaining`; else if the deadline has not passed (`revealDeadline > now_epoch_seconds`) use `less than 1 minute remaining`; else treat as expired (drop the `Reveal deadline:` line). Compute `cooldownHours` = `slashedCooldownSeconds / 3600`.\n\
             Run `okx-a2a user notify` to push the notification to the user. **Substitute `<message.jobTitle>` with the actual extracted value.** Translate the content below into the user's language first, then run:\n\n\
             ```bash\n\
             okx-a2a user notify --content '<localized content>' --json\n\
             ```\n\n\
             Canonical English content (substitute placeholders first):\n\
             \x20\x20\x20\x20【⏰ URGENT: Arbitration reveal for task [<message.jobTitle>] is about to close】\n\
             \x20\x20\x20\x20Task title: <message.jobTitle>\n\
             \x20\x20\x20\x20Task ID: #{job_id}\n\
             \x20\x20\x20\x20Reveal deadline: <revealDeadlineLocal> (<minutesLeftText>)\n\
             \x20\x20\x20\x20Current Status: Agent has not revealed yet\n\
             \x20\x20\x20\x20🚨 Timeout consequences:\n\
             \x20\x20\x20\x20• Stake slashed <slashTimeoutBps>\n\
             \x20\x20\x20\x20• Enter a <cooldownHours>h cooldown during which you cannot be selected\n\
             \x20\x20\x20\x20• Miss the base validation fee\n\
             \x20\x20\x20\x20⚡ Have the Agent reveal immediately\n\n\
             [Field-missing fallbacks]\n\
             - `revealDeadline` missing or deadline already passed → drop the `Reveal deadline:` line.\n\
             - `slashTimeoutBps` missing → drop the `• Stake slashed` bullet.\n\
             - `slashedCooldownSeconds` missing → drop the `• Enter a ... cooldown` bullet.\n"
        ),

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
             **Step 2 — Notify the user that the reveal has been submitted via `okx-a2a user notify`.** Translate the content below into the user's language first, then run:\n\n\
             ```bash\n\
             okx-a2a user notify --content '<localized content>' --json\n\
             ```\n\n\
             Canonical English content (substitute placeholders first):\n\
             \x20\x20\x20\x20Your agent has submitted the reveal transaction for Job jobId={job_id}. Waiting for chain confirmation — no action needed from you.\n"
        ),

        "vote_revealed" => format!(
            "[Current Status] vote_revealed\n\n\
             Run `okx-a2a user notify` to push the notification to the user. Translate the content below into the user's language first, then run:\n\n\
             ```bash\n\
             okx-a2a user notify --content '<localized content>' --json\n\
             ```\n\n\
             Canonical English content (substitute placeholders first):\n\
             \x20\x20\x20\x20Your agent has revealed its vote on-chain for Job jobId={job_id}. Waiting for the dispute resolution result — no action needed from you.\n"
        ),

        "dispute_resolved" => format!(
            "[Current Status] dispute_resolved\n\n\
             Extract from `message`: `jobTitle`, `vote` (0 or 1), `jobStatus` (`complete` or `failed`), `slashMinorityBps` (lost branch only), `agentName`, `slashTimeoutBps`, `hasCommit`, `hasReveal`. **Substitute `<message.jobTitle>` below with the extracted value.**\n\
             Render two text labels (pure text mapping, no semantic interpretation):\n\
             - `vote = 0` → `yourVote = User`; `vote = 1` → `yourVote = ASP`\n\
             - `jobStatus = complete` → `winningSide = ASP`; `jobStatus = failed` → `winningSide = User`\n\
             `hasCommit` / `hasReveal` missing → treat as `1` (participated).\n\n\
             **Routing (evaluate in order, first match wins):**\n\
             1. `hasCommit == 0` → Branch 0a (missed commit)\n\
             2. `hasReveal == 0` → Branch 0b (missed reveal)\n\
             3. `vote` missing → Branch B (lost / minority)\n\
             4. `yourVote == winningSide` → Branch A (won)\n\
             5. otherwise → Branch B (lost)\n\
             ━━━━━━━━━━━━━ Branch 0a: MISSED COMMIT ━━━━━━━━━━━━━\n\n\
             Run `okx-a2a user notify` (🌐 localize first):\n\n\
             ```bash\n\
             okx-a2a user notify --content '<localized content>' --json\n\
             ```\n\n\
             Canonical English content (substitute placeholders first):\n\
             \x20\x20\x20\x20【⚖️ Your Agent <agentName> missed [Commit] for task [<message.jobTitle>] arbitration — penalty incoming】\n\
             \x20\x20\x20\x20Task title: <message.jobTitle>\n\
             \x20\x20\x20\x20Task ID: #{job_id}\n\
             \x20\x20\x20\x20You did not participate in [Commit]\n\
             \x20\x20\x20\x20🚫 Penalty applied\n\
             \x20\x20\x20\x20• Stake slashed <slashTimeoutBps>\n\n\
             Missed-commit branch ends this turn; do not call `arbitration-claim`.\n\n\
             ━━━━━━━━━━━━━ Branch 0b: MISSED REVEAL ━━━━━━━━━━━━━\n\n\
             Run `okx-a2a user notify` (🌐 localize first):\n\n\
             ```bash\n\
             okx-a2a user notify --content '<localized content>' --json\n\
             ```\n\n\
             Canonical English content (substitute placeholders first):\n\
             \x20\x20\x20\x20【⚖️ Your Agent <agentName> missed [Reveal] for task [<message.jobTitle>] arbitration — penalty incoming】\n\
             \x20\x20\x20\x20Task title: <message.jobTitle>\n\
             \x20\x20\x20\x20Task ID: #{job_id}\n\
             \x20\x20\x20\x20You did not participate in [Reveal]\n\
             \x20\x20\x20\x20🚫 Penalty applied\n\
             \x20\x20\x20\x20• Stake slashed <slashTimeoutBps>\n\n\
             Missed-reveal branch ends this turn; do not call `arbitration-claim`.\n\n\
             ━━━━━━━━━━━━━ Branch A: WON ━━━━━━━━━━━━━\n\n\
             Run `okx-a2a user notify` (🌐 localize first):\n\n\
             ```bash\n\
             okx-a2a user notify --content '<localized content>' --json\n\
             ```\n\n\
             Canonical English content (substitute placeholders first):\n\
             \x20\x20\x20\x20【🎉 Arbitration result for task [<message.jobTitle>]: your vote aligned with the majority — reward eligible】\n\
             \x20\x20\x20\x20Task title: <message.jobTitle>\n\
             \x20\x20\x20\x20Task ID: #{job_id}\n\
             \x20\x20\x20\x20Your vote: backed <yourVote> ✓ aligned with majority\n\n\
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
             \x20\x20⚠️ Account-level pull: aside from `--agent-id`, pass no other business params. Retry up to 3 times on failure. Final credit confirmation arrives via the later `reward_claimed` event.\n\n\
             ━━━━━━━━━━━━━ Branch B: LOST ━━━━━━━━━━━━━\n\n\
             Run `okx-a2a user notify` (🌐 localize first):\n\n\
             ```bash\n\
             okx-a2a user notify --content '<localized content>' --json\n\
             ```\n\n\
             Canonical English content (substitute placeholders first):\n\
             \x20\x20\x20\x20【⚠️ Arbitration result for task [<message.jobTitle>]: your vote disagreed with the majority — slash penalty incoming】\n\
             \x20\x20\x20\x20Task title: <message.jobTitle>\n\
             \x20\x20\x20\x20Task ID: #{job_id}\n\
             \x20\x20\x20\x20Your vote: backed <yourVote> ✗ opposed majority\n\
             \x20\x20\x20\x20🚫 Penalty applied\n\
             \x20\x20\x20\x20• Stake slashed <slashMinorityBps>\n\n\
             Lost branch ends this turn; do not call `arbitration-claim` (nothing to claim). The slash was conveyed in the notification above — no follow-up event will arrive.\n\n\
             [Field-missing fallbacks]\n\
             - `slashMinorityBps` missing → drop the `🚫 Penalty applied` block (Branch B).\n\
             - `agentName` missing → degrade Branch 0a/0b header to `⚖️ You missed [Commit|Reveal] for task [<message.jobTitle>] arbitration — penalty incoming`.\n\
             - `slashTimeoutBps` missing → drop the entire `🚫 Penalty applied` block in Branch 0a/0b.\n"
        ),

        "cooldown_entered" => {
            let content = match fetch_my_stake(agent_id).await.and_then(|s| fmt_local_time(s.cooldown_ends_at)) {
                Some(local) => format!(
                    "You've entered the absence cooldown period; you won't be selected as a juror before {local}."
                ),
                None => "You've entered the absence cooldown period and won't be selected as a juror during this period.".to_string(),
            };
            format!("[Current Status] cooldown_entered\n\n{}", notify_block(&content))
        }

        "round_failed" => format!(
            "[Current Status] round_failed\n\n\
             Extract from `message`: `jobTitle`, `abstainCount`, `totalSlashed`, `slashTimeoutBps`, `revealCount`, `agentName`, `hasCommit`, `hasReveal`. **Substitute `<message.jobTitle>` below with the extracted value.**\n\
             `hasCommit` / `hasReveal` missing → treat as `1` (participated).\n\n\
             **Routing (evaluate in order, first match wins):**\n\
             1. `hasCommit == 0` → Branch 0a (missed commit)\n\
             2. `hasReveal == 0` → Branch 0b (missed reveal)\n\
             3. otherwise → Branch C (round invalidated)\n\n\
             ━━━━━━━━━━━━━ Branch 0a: MISSED COMMIT ━━━━━━━━━━━━━\n\n\
             Run `okx-a2a user notify` (🌐 localize first):\n\n\
             ```bash\n\
             okx-a2a user notify --content '<localized content>' --json\n\
             ```\n\n\
             Canonical English content (substitute placeholders first):\n\
             \x20\x20\x20\x20【⚖️ Your Agent <agentName> missed [Commit] for task [<message.jobTitle>] arbitration — penalty incoming】\n\
             \x20\x20\x20\x20Task title: <message.jobTitle>\n\
             \x20\x20\x20\x20Task ID: #{job_id}\n\
             \x20\x20\x20\x20You did not participate in [Commit]\n\
             \x20\x20\x20\x20🚫 Penalty applied\n\
             \x20\x20\x20\x20• Stake slashed <slashTimeoutBps>\n\n\
             Missed-commit branch ends this turn.\n\n\
             ━━━━━━━━━━━━━ Branch 0b: MISSED REVEAL ━━━━━━━━━━━━━\n\n\
             Run `okx-a2a user notify` (🌐 localize first):\n\n\
             ```bash\n\
             okx-a2a user notify --content '<localized content>' --json\n\
             ```\n\n\
             Canonical English content (substitute placeholders first):\n\
             \x20\x20\x20\x20【⚖️ Your Agent <agentName> missed [Reveal] for task [<message.jobTitle>] arbitration — penalty incoming】\n\
             \x20\x20\x20\x20Task title: <message.jobTitle>\n\
             \x20\x20\x20\x20Task ID: #{job_id}\n\
             \x20\x20\x20\x20You did not participate in [Reveal]\n\
             \x20\x20\x20\x20🚫 Penalty applied\n\
             \x20\x20\x20\x20• Stake slashed <slashTimeoutBps>\n\n\
             Missed-reveal branch ends this turn.\n\n\
             ━━━━━━━━━━━━━ Branch C: ROUND INVALIDATED ━━━━━━━━━━━━━\n\n\
             Run `okx-a2a user notify` to push the notification to the user. Translate the content below into the user's language first, then run:\n\n\
             ```bash\n\
             okx-a2a user notify --content '<localized content>' --json\n\
             ```\n\n\
             Canonical English content (substitute placeholders first):\n\
             \x20\x20\x20\x20【⚖️ Task [<message.jobTitle>] arbitration round invalidated】\n\
             \x20\x20\x20\x20Task title: <message.jobTitle>\n\
             \x20\x20\x20\x20Task ID: #{job_id}\n\
             \x20\x20\x20\x20Tally: no side reached ≥ 50%\n\
             \x20\x20\x20\x20💰 Abstain-slash pool distribution\n\
             \x20\x20\x20\x20• Source: <abstainCount> abstainers × <slashTimeoutBps> = <totalSlashed> OKB total\n\
             \x20\x20\x20\x20• Split evenly among <revealCount> revealers\n\n\
             [Field-missing fallbacks]\n\
             - Any of `abstainCount` / `totalSlashed` / `slashTimeoutBps` / `revealCount` missing → drop the `💰 Abstain-slash pool distribution` block (Branch C).\n\
             - `agentName` missing → degrade Branch 0a/0b header to `⚖️ You missed [Commit|Reveal] for task [<message.jobTitle>] arbitration — penalty incoming`.\n\
             - `slashTimeoutBps` missing → drop the entire `🚫 Penalty applied` block in Branch 0a/0b.\n"
        ),

        "reward_claimed" => "[Current Status] reward_claimed\n\n\
             Run `okx-a2a user notify` to push the notification to the user. Translate the content below into the user's language first, then run:\n\n\
             ```bash\n\
             okx-a2a user notify --content '<localized content>' --json\n\
             ```\n\n\
             Canonical English content:\n\
             \x20\x20\x20\x20Your arbitration reward has been credited.\n".to_string(),

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
         \x20\x20Read failure / file missing / empty content → **stop this turn immediately** (no commit, no fallback default rules, no search for replacement file). Run `okx-a2a user notify` (🌐 localize first), then end the turn:\n\n\
         ```bash\n\
         okx-a2a user notify --content '<localized content>' --json\n\
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
