use crate::commands::agent_commerce::task::common::state_machine::Status;

pub fn available_actions(status: &Status, job_id: &str) -> Vec<String> {
    let next_action = |evt: &str| {
        format!("**Next required action** → `onchainos agent next-action --jobid {job_id} --jobStatus {evt} --role evaluator --agentId <agentId>` (fetch the full playbook for the current status; **follow the playbook**, do not bypass next-action and call the CLI below directly).")
    };

    match status {
        Status::Disputed => vec![next_action("evaluator_selected")],
        Status::Completed | Status::Failed => vec![next_action("dispute_resolved")],
        _ => vec![
            format!("Current task status=`{}` → evaluator has no task-level action; just wait for the next relevant chain event.", status.as_str()),
            "→ **Do not** rerun `agent status` / `agent common context` (the result will be identical); end this turn.".to_string(),
        ],
    }
}

const LOCALIZATION_PREFIX: &str = "[Localization] All `content:` templates below are samples — translate to the user's language before `xmtp_dispatch_user`.\n\n";

pub fn generate_next_action(job_id: &str, job_status: &str, agent_id: &str) -> String {
    if let Some(s) = staking_next_action(job_id, job_status, agent_id) {
        return format!("{LOCALIZATION_PREFIX}{s}");
    }
    if let Some(s) = dispute_next_action(job_id, job_status, agent_id) {
        return format!("{LOCALIZATION_PREFIX}{s}");
    }
    format!(
        "[unknown event or status={job_status} at jobId={job_id} ignored.\n
         Do not pull context; do not guess other notifications.\n"
    )
}

fn staking_next_action(_job_id: &str, job_status: &str, _agent_id: &str) -> Option<String> {
    let body = match job_status {
        "staked" => "[Current Event] staked\n\n\
             [Step 1] Run `onchainos agent my-stake --agent-id <your agentId>` to get `activeStake`.\n\
             [Step 2] Use `xmtp_dispatch_user` to push the notification to the user:\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20Your stake is now active on-chain. Current activeStake is <my-stake.activeStake> OKB.\n\n\
             [my-stake failure fallback] Drop numeric fields and degrade to `Your stake is now active on-chain.`\n".to_string(),

        "unstake_requested" => "[Current Event] unstake_requested\n\n\
             [Step 1] Run `onchainos agent my-stake --agent-id <your agentId>` to get `pendingUnstake` and `unstakeAvailableAt` (already in local time).\n\
             [Step 2] Use `xmtp_dispatch_user` to push the notification to the user:\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20The unstake request has been recorded on-chain. Current cumulative pending unstake is <my-stake.pendingUnstake> OKB; the last claimable time is <unstakeAvailableAt local time>. You can cancel the unstake mid-way.\n\n\
             [my-stake failure fallback] Drop numeric fields and degrade to `The unstake request has been recorded on-chain. You can cancel the unstake before the cooldown ends.`\n".to_string(),

        "unstake_claimed" => "[Current Status] unstake_claimed\n\n\
             Use `xmtp_dispatch_user` to push the notification to the user:\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20Your unstake has been claimed; OKB has been credited to your wallet.\n".to_string(),

        "unstake_cancelled" => "[Current Status] unstake_cancelled\n\n\
             Use `xmtp_dispatch_user` to push the notification to the user:\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20Your unstake has been cancelled; the pending OKB is back in staked state.\n".to_string(),

        "stake_stopped" => "[Current Status] stake_stopped\n\n\
             Use `xmtp_dispatch_user` to push the notification to the user:\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20You have exited the voter pool and will no longer be selected as a juror.\n".to_string(),

        _ => return None,
    };
    Some(body)
}

fn dispute_next_action(job_id: &str, job_status: &str, _agent_id: &str) -> Option<String> {
    let body = match job_status {
        "evaluator_selected" => format!(
            "[Current Status] evaluator_selected\n\n\
             **Step 1 — Routing check:**\n\n\
             ⚠️ Immediately after the calls in 1.1 / 1.2, print the entire returned `sessionKey` verbatim in this turn's output (no truncation, no abbreviation); subsequent comparison MUST be based on the two printed lines.\n\n\
             **1.1** Call `xmtp_start_evaluate_conversation` with `myAgentId=<envelope top-level agentId>`, `jobId={job_id}`. Print:\n\
             `[evaluator-routing] arbKey=<entire sessionKey returned by this xmtp_start_evaluate_conversation call>`\n\n\
             **1.2** Call `session_status`. Print:\n\
             `[evaluator-routing] currentKey=<entire sessionKey returned by this session_status call>`\n\n\
             **1.3** Compare the two `[evaluator-routing]` lines above character-by-character (don't go by impression — base it on the two printed lines):\n\
             - Exact match → proceed to Step 2.\n\
             - Any character differs → call `xmtp_dispatch_session` (`sessionKey=arbKey`, `content=<the entire current inbound envelope as a JSON string>`, **insert all fields verbatim, no rewriting**), then **end this turn**.\n\n\
             **Step 2 — Extract `jobId`, top-level `agentId` (your evaluator agentId), and top-level `roundNum` from the inbound envelope.**\n\
             If any of `jobId` / top-level `agentId` / `roundNum` is missing, abort this turn immediately and output `missing jobId/agentId/roundNum in payload; abort` log.\n\
             **Step 3 — Notify the user that you've been selected as a juror:**\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20You've been selected as a juror for Job jobId={job_id}. Your agent will automatically review evidence, vote, and reveal in the background — no action needed from you.\n\n\
             **Step 4 — Fetch evidence:**\n\
             ```bash\n\
             onchainos agent evidence-info <jobId> --agent-id <envelope top-level agentId> --round-num <envelope top-level roundNum>\n\
             ```\n\n\
             Evidence JSON top-level: `{{ title, description, provider: {{texts[], images[]}}, client: {{texts[], images[]}} }}`. `description` / `title` is the task's original definition; `texts[]` is text evidence; `images[]` is already downloaded — each item has `localPath` (absolute path; use it to open the image).\n\n\
             **Post-evidence hard constraints**:\n\
             - An image item with a `downloadError` field = that evidence is **considered missing**\n\
             - **Do not** scan local disk for replacement files; a missing `localPath` means the CLI already knows the image is unavailable\n\
             - **Do not** retry `evidence-info` hoping it downloads next time (internally already retried 3 times) — mark this image as missing\n"
        ),

        "vote_committed" => format!(
            "[Current Status] vote_committed\n\n\
             Use `xmtp_dispatch_user` to push the notification to the user:\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20Your agent has committed its vote on-chain for Job jobId={job_id}. Waiting for the reveal phase to begin — no action needed from you.\n"
        ),

        "reveal_started" => format!(
            "[Current Status] reveal_started\n\n\
             **Step 1 — Extract `jobId` and `agentId` from the inbound envelope top level** (if `jobId` missing → output `missing jobId in payload; abort` log and end this turn).\n\n\
             **Step 2 — Notify the user that the reveal phase has started:**\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20The reveal phase has started for Job jobId={job_id}. Your agent is revealing the vote on-chain in the background — no action needed from you.\n\n\
             **Step 3 — Execute reveal:**\n\
             ```bash\n\
             onchainos agent vote-reveal <jobId> --agent-id <envelope top-level agentId>\n\
             ```\n\n\
             [Error mapping]\n\
             - `canReveal=false` → CLI has already pre-checked and rejected; no retry needed. This round may have settled already (wait for dispute_resolved) or you did not commit (normal skip).\n\
             - `voter has not committed` → you did not commit this round; skipping reveal is normal.\n\
             - Other failures: retry up to 3 times.\n"
        ),

        "vote_revealed" => format!(
            "[Current Status] vote_revealed\n\n\
             Use `xmtp_dispatch_user` to push the notification to the user:\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20Your agent has revealed its vote on-chain for Job jobId={job_id}. Waiting for the dispute resolution result — no action needed from you.\n"
        ),

        "dispute_resolved" => "[Current Status] dispute_resolved\n\n\
             [Step 1] Extract `agentId` and `jobId` from the envelope top level.\n\n\
             [Step 2] Call `arbitration-claimable` to check if this account has rewards to claim (pass envelope top-level `agentId`):\n\
             ```bash\n\
             onchainos agent arbitration-claimable --agent-id <envelope top-level agentId>\n\
             ```\n\
             The output's last line is a stable marker `hasClaimable: yes` or `hasClaimable: no`. **Decide based on this line only**; do not parse amount yourself.\n\
             - `hasClaimable: no` → skip Step 3 (you were not in the majority this round; you may receive a slashed event)\n\
             - `hasClaimable: yes` → proceed to Step 3 to claim\n\n\
             [Step 3] Immediately claim rewards:\n\
             ```bash\n\
             onchainos agent arbitration-claim --agent-id <envelope top-level agentId>\n\
             ```\n\
             ⚠️ Account-level pull mode: aside from `--agent-id`, do not pass any other business parameters; pull all pending rewards from all settled disputes at once (empty body).\n\
             Retry up to 3 times on failure. The actual credit confirmation will be communicated to the user via a later `reward_claimed` event.\n".to_string(),

        "slashed" => format!(
            "[Current Status] slashed\n\n\
             ⚠️ envelope.message only contains `event / jobId / timestamp / source / description` — no amount / reason. Do not fabricate or guess from other fields.\n\n\
             [Step 1] Run `onchainos agent my-stake --agent-id <your agentId>` to get the post-slash `activeStake`.\n\
             [Step 2] Use `xmtp_dispatch_user` to push the notification to the user:\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20Your stake has been slashed on Job jobId={job_id}. Remaining activeStake is <my-stake.activeStake> OKB.\n\n\
             [my-stake failure fallback] Drop numeric fields and degrade to `Your stake has been slashed on Job jobId={job_id}.`\n"
        ),

        "cooldown_entered" => "[Current Status] cooldown_entered\n\n\
             [Step 1] Run `onchainos agent my-stake --agent-id <your agentId>` to get `cooldownEndsAt` (already in local time).\n\
             [Step 2] Use `xmtp_dispatch_user` to push the notification to the user:\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20You've entered the absence cooldown period; you won't be selected as a juror before <my-stake.cooldownEndsAt local time>.\n\n\
             [my-stake failure fallback] Drop numeric fields and degrade to `You've entered the absence cooldown period and won't be selected as a juror during this period.`\n".to_string(),

        "round_failed" =>
            "[Current Status] round_failed\n\n\
             [Action] None; do not notify the user.\n".to_string(),

        "reward_claimed" => "[Current Status] reward_claimed\n\n\
             Use `xmtp_dispatch_user` to push the notification to the user:\n\n\
             tool: xmtp_dispatch_user\n\
             content:\n\
             \x20\x20\x20\x20Your arbitration reward has been credited.\n".to_string(),

        _ => return None,
    };
    Some(body)
}

/// Step 5-7 of the `evaluator_selected` playbook, intended to be appended to
/// `evidence-info` stdout instead of returned by `next-action`.
///
/// Rationale: when next-action's evaluator_selected body included the full
/// vote-commit / vote-record CLI templates, a weak LLM could pattern-match the
/// command line and skip Step 4 (evidence-info) + Step 5 (rubric read). By
/// printing these steps only after evidence has actually been fetched, the LLM
/// physically cannot see the vote-commit invocation template until it has
/// pulled the evidence.
pub fn evaluator_selected_post_evidence_steps(job_id: &str) -> String {
    format!(
        "**Step 5 — Render the verdict per `references/evaluator-decision-rubric.md`:**\n\
         - **Prerequisite — file readability check**: read `references/evaluator-decision-rubric.md`.\n\
         \x20\x20Read failure / file missing / empty content → **stop this turn immediately** (no commit, no fallback default rules, no search for replacement file). Push the user via `xmtp_dispatch_user` then end the turn:\n\n\
         tool: xmtp_dispatch_user\n\
         content:\n\
         \x20\x20\x20\x20Arbitration aborted for task jobId={job_id}: the decision rubric `references/evaluator-decision-rubric.md` is missing or unreadable; this round's vote is skipped.\n\
         \x20\x20\x20\x20⚠️ commit window timeout will slash your stake — please restore the file as soon as possible.\n\n\
         - Read success and evidence already output → produce the final `vote` and verdict per the rules therein.\n\n\
         **Step 6 — Execute commit (also pass envelope top-level `agentId` to `--agent-id`):**\n\
         ```bash\n\
         onchainos agent vote-commit <jobId> --vote <0|1> --agent-id <envelope top-level agentId>\n\
         ```\n\
         ⚠️ **Only 0 (Approve / Client wins) or 1 (Reject / Provider wins) — skip is forbidden**.\n\
         ⚠️ **The `<0|1>` value MUST come from Step 5** — it is the binary vote that Step 5 derived by applying `references/evaluator-decision-rubric.md` (whatever decision procedure that document defines) to the evidence. Do **not** commit a vote that bypassed Step 5 — guessing / pattern-matching / averaging a value here violates the rubric and produces an unfounded ruling.\n\
         Retry up to 3 times on failure (CRITICAL — closing of the commit window triggers timeout slashing). `voter has already committed` counts as success — proceed to Step 6.5.\n\
         Body only carries `vote`.\n\n\
         **Step 6.5 — Persist verdict to disk (local audit redundancy; run after commit):**\n\
         - Verdict generated per rubric §3 template → **flatten the entire verdict markdown into a single line** with `\\n` literal escapes (two characters: `\\` + `n`, not a real newline) replacing every real newline; then pass via `--verdict`:\n\
         \x20\x20```bash\n\
         \x20\x20onchainos agent vote-record <jobId> --agent-id <envelope top-level agentId> --verdict \"Verdict\\n\\nJob ID: <jobId>\\nvote: <0|1>\\nFindings of fact: 1. ...\\nEvidence citations: ...\\nReasoning: ...\"\n\
         \x20\x20```\n\
         \x20\x20CLI un-escapes `\\n` → newline, `\\t` → tab, `\\r` → CR, `\\\\` → `\\`, `\\\"` → `\"` before writing to disk; `verdict.md` stays human-readable multi-line markdown for later audit.\n\
         - **Character taboos inside the `--verdict` value** (otherwise the shell will corrupt the argument before the CLI even sees it):\n\
         \x20\x20- `\"` (double quote) → escape as `\\\"`\n\
         \x20\x20- `` ` `` (backtick) → either replace with `'` (single quote) or escape as `` \\` ``; an unescaped backtick triggers shell command substitution\n\
         \x20\x20- `$` → escape as `\\$` to prevent shell variable expansion\n\
         \x20\x20- Real newlines / tabs / CRs → **must** use `\\n` / `\\t` / `\\r` escapes; never embed a literal newline (the command will break across lines)\n\
         - User-customized rubric does not define §3 template, no verdict generated this round → omit `--verdict`; the CLI auto-writes a placeholder:\n\
         \x20\x20```bash\n\
         \x20\x20onchainos agent vote-record <jobId> --agent-id <envelope top-level agentId>\n\
         \x20\x20```\n\
         Failure: **do not retry, do not push user session, do not block** — go directly to Step 7 (vote is already on-chain; disk persistence is only local audit redundancy).\n\n\
         **Step 7 — Output one log line then end this turn:**\n\n\
         > Committed jobId=<jobId> vote=<0|1>\n"
    )
}
