//! ASP-side task flow driver.
//!
//! Based on the current system notification type received (event), outputs the prompt
//! for the next action to take. The goal: consolidate the Scene steps scattered across
//! the ASP role leaves under `references/a2a/provider/` into code so the agent can simply run
//! `exec onchainos agent next-action ...` to fetch the prompt and execute it directly,
//! without having to reason over the entire document.

use crate::commands::agent_commerce::task::common::util::short_job_id;

#[derive(Clone, Copy)]
enum ProviderAssignmentType {
    Single,
    Subscription,
}

fn task_params_request_command(job_id: &str, buyer_agent_id: &str, task_type: &str) -> String {
    format!(
        "okx-a2a xmtp-send --job-id {job_id} --to-agent-id {buyer_agent_id} --message \"<natural-language request>\\n\\n[intent:task_params_request]\\n{{\\\"version\\\":1,\\\"jobId\\\":\\\"{job_id}\\\",\\\"taskType\\\":\\\"{task_type}\\\",\\\"requestId\\\":\\\"<unique-request-id>\\\",\\\"round\\\":<1-3>,\\\"missing\\\":[\\\"<field>\\\"]}}\" --json"
    )
}

async fn provider_assignment_playbook(
    job_id: &str,
    agent_id: &str,
    assignment_type: ProviderAssignmentType,
    prefetched: Option<&crate::commands::agent_commerce::task::common::PreFetchedTaskContext>,
    message: Option<&serde_json::Value>,
) -> String {
    let task_type = match assignment_type {
        ProviderAssignmentType::Single => "single",
        ProviderAssignmentType::Subscription => "subscription",
    };
    let event_name = match assignment_type {
        ProviderAssignmentType::Single => "job_asp_selected",
        ProviderAssignmentType::Subscription => "sub_open",
    };
    let accept_command = match assignment_type {
        ProviderAssignmentType::Single => "accept-job-by-provider",
        ProviderAssignmentType::Subscription => "accept-subscription",
    };
    let decline_command = match assignment_type {
        ProviderAssignmentType::Single => "decline-job-by-provider",
        ProviderAssignmentType::Subscription => "decline-subscription",
    };
    let p = match prefetched {
        Some(value) => value,
        None => {
            return format!(
                "[Current state] {event_name}\n[Role] ASP\n\n\
                 Latest task detail could not be fetched. Stop with an error; do NOT accept, decline, or send task_params_request.\n\
                 jobId={job_id}\n"
            );
        }
    };
    match p.status {
        Some(0) => {}
        Some(1) => {
            return format!(
                "[Current state] {event_name}\n[Role] ASP\n\n\
                 Latest backend status is ACCEPTED/ACTIVE. This is a duplicate trigger: end idempotently.\n\
                 Do NOT repeat the mutation or broadcast. jobId={job_id}\n"
            );
        }
        Some(status) => {
            return format!(
                "[Current state] {event_name}\n[Role] ASP\n\n\
                 Latest backend status is {status}, not CREATED(0). End idempotently; do NOT mutate or broadcast.\n\
                 jobId={job_id}\n"
            );
        }
        None => {
            return format!(
                "[Current state] {event_name}\n[Role] ASP\n\n\
                 Latest backend detail has no status. Stop with an error; do NOT accept or decline.\n\
                 jobId={job_id}\n"
            );
        }
    }

    let msg_str = |key: &str| {
        message
            .and_then(|value| value.get(key))
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
    };
    let service_id = msg_str("serviceId")
        .or_else(|| p.service_id.as_deref().filter(|value| !value.is_empty()))
        .unwrap_or("");
    if service_id.is_empty() {
        return format!(
            "[Current state] {event_name}\n[Role] ASP\n\n\
             No serviceId is present. Run the v2 decline command (reason is required, ≤512 Unicode characters):\n\
             ```bash\n\
             onchainos agent {decline_command} {job_id} --agent-id {agent_id} --reason \"designated serviceId is missing\"\n\
             ```\n"
        );
    }

    let service = match crate::commands::agent_commerce::task::common::find_service(
        agent_id, service_id,
    )
    .await
    {
        Ok(Some(service)) => service,
        Ok(None) => {
            return format!(
                "[Current state] {event_name}\n[Role] ASP\n\n\
                 `onchainos agent service-list --agent-id {agent_id} --service-id {service_id}` completed but returned no matching service.\n\
                 Run exactly:\n\
                 ```bash\n\
                 onchainos agent {decline_command} {job_id} --agent-id {agent_id} --reason \"designated service is not registered\"\n\
                 ```\n"
            );
        }
        Err(error) => {
            return format!(
                "[Current state] {event_name}\n[Role] ASP\n\n\
                 Service lookup failed: {error:#}\n\
                 Stop with an error. Do NOT decline: a timeout, malformed response, or temporary service-list failure is not a capability rejection.\n\
                 jobId={job_id}\n"
            );
        }
    };

    let service_name = service
        .get("serviceName")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let service_description = service
        .get("serviceDescription")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let buyer_agent_id = p.user_agent_id.as_deref().unwrap_or("<buyerAgentId>");
    let service_params = p.service_params.as_deref().unwrap_or("{}");
    let request_command = task_params_request_command(job_id, buyer_agent_id, task_type);

    format!(
        "[Current state] {event_name}; latest backend status=CREATED(0)\n\
         [Role] ASP\n\n\
         Evaluate ONCE using only these four inputs:\n\
         - task description: {description}\n\
         - serviceParams: {service_params}\n\
         - attachments: inspect the attachments already forwarded into this job session\n\
         - registered service: {service_name} (`{service_id}`): {service_description}\n\n\
         Output exactly one internal conclusion: `ACCEPT`, `NEED_PARAMS`, or `REJECT`. Do not invent a fourth result.\n\n\
         **ACCEPT** — immediately before mutation, rely on the latest detail above (CREATED). Run:\n\
         ```bash\n\
         onchainos agent {accept_command} {job_id} --agent-id {agent_id}\n\
         ```\n\
         The command calls the documented {task_type} provider-accept endpoint, signs uopData, broadcasts bizType {accept_type}, and requires a full receipt. End the turn; duplicate accepted events must not repeat it.\n\n\
         **REJECT** — generate one concrete reason (required, ≤512 Unicode characters), then run:\n\
         ```bash\n\
         onchainos agent {decline_command} {job_id} --agent-id {agent_id} --reason \"<reason>\"\n\
         ```\n\
         The reason is placed in broadcast bizContext; do not use legacy `asp-reject`.\n\n\
         **NEED_PARAMS** — send one natural-language question followed by the structured block below to the Buyer through peer transport:\n\
         ```bash\n\
         {request_command}\n\
         ```\n\
         Count only a response for which the buyer successfully updated the backend as a successful round. Ignore duplicate requestId/response messages. Maximum: 3 successful update/response rounds. After the third successful update, fetch current detail and evaluate once more; if still NEED_PARAMS, decline.\n\n\
         When `[intent:task_params_response]` arrives: fetch latest detail again. If status is not CREATED, stop. If CREATED, evaluate the updated complete serviceParams again. The buyer-side required ordering is:\n\
         ```bash\n\
         onchainos agent service-param-update {job_id} --agent-id {buyer_agent_id} --task-type {task_type} --request-id '<request-id>' --round <same-round> --service-params '<complete JSON>'\n\
         # only after exit 0 and backendUpdated=true:\n\
         okx-a2a session send --job-id {job_id} --to-agent-id {agent_id} --content \"[intent:task_params_response]\\n{{\\\"version\\\":1,\\\"jobId\\\":\\\"{job_id}\\\",\\\"requestId\\\":\\\"<request-id>\\\",\\\"round\\\":<same-round>,\\\"backendUpdated\\\":true}}\" --json\n\
         ```\n",
        description = p.description,
        accept_type = 203,
    )
}

fn arbitration_decision_result(
    source_event: &str,
    job_id: &str,
    job_title: Option<&str>,
    prefetched: Option<&crate::commands::agent_commerce::task::common::PreFetchedTaskContext>,
    message: Option<&serde_json::Value>,
) -> serde_json::Value {
    use crate::commands::agent_commerce::task::arbitration::{
        build_decision_result, scalar_string,
    };

    let message_field = |keys: &[&str]| {
        keys.iter()
            .find_map(|key| scalar_string(message.and_then(|value| value.get(*key))))
    };
    let name = message_field(&["jobTitle", "title", "serviceName"])
        .or_else(|| {
            job_title
                .map(str::to_string)
                .filter(|value| !value.is_empty())
        })
        .or_else(|| {
            prefetched.and_then(|value| {
                value
                    .service_name
                    .clone()
                    .filter(|name| !name.is_empty())
                    .or_else(|| (!value.title.is_empty()).then(|| value.title.clone()))
            })
        });
    let amount = message_field(&["tokenAmount", "serviceTokenAmount"]).or_else(|| {
        prefetched.and_then(|value| {
            value
                .service_token_amount
                .clone()
                .filter(|amount| !amount.is_empty())
                .or_else(|| (!value.token_amount.is_empty()).then(|| value.token_amount.clone()))
        })
    });
    let token_symbol = message_field(&["tokenSymbol", "paymentTokenSymbol"]).or_else(|| {
        prefetched
            .map(|value| value.token_symbol.clone())
            .filter(|value| !value.is_empty() && value != "?")
    });
    let mut decision_context = message.cloned().unwrap_or_else(|| serde_json::json!({}));
    if decision_context.get("expireTime").is_none() {
        if let Some(expire_time) = prefetched.and_then(|value| value.expire_time) {
            decision_context["expireTime"] = serde_json::Value::Number(expire_time.into());
        }
    }
    if scalar_string(decision_context.get("serviceName")).is_none() {
        if let Some(service_name) = prefetched
            .and_then(|value| value.service_name.as_deref())
            .filter(|value| !value.trim().is_empty())
        {
            decision_context["serviceName"] = serde_json::Value::String(service_name.to_string());
        }
    }
    if scalar_string(decision_context.get("refundReason")).is_none() {
        if let Some(reason) = prefetched
            .and_then(|value| value.refund_reason.as_deref())
            .filter(|value| !value.trim().is_empty())
        {
            decision_context["refundReason"] = serde_json::Value::String(reason.to_string());
        }
    }
    for (key, value) in [
        (
            "subStartTime",
            prefetched.and_then(|value| value.period_start_time),
        ),
        (
            "subEndTime",
            prefetched.and_then(|value| value.period_end_time),
        ),
    ] {
        if decision_context.get(key).is_none() {
            if let Some(value) = value {
                decision_context[key] = serde_json::Value::Number(value.into());
            }
        }
    }
    build_decision_result(
        source_event,
        job_id,
        name,
        amount,
        token_symbol,
        Some(&decision_context),
    )
}

fn arbitration_decision_json(
    source_event: &str,
    job_id: &str,
    job_title: Option<&str>,
    prefetched: Option<&crate::commands::agent_commerce::task::common::PreFetchedTaskContext>,
    message: Option<&serde_json::Value>,
) -> String {
    serde_json::to_string(&arbitration_decision_result(
        source_event,
        job_id,
        job_title,
        prefetched,
        message,
    ))
    .unwrap_or_else(|_| "{}".to_string())
}

fn arbitration_decision_playbook(
    source_event: &str,
    job_id: &str,
    agent_id: &str,
    job_title: Option<&str>,
    prefetched: Option<&crate::commands::agent_commerce::task::common::PreFetchedTaskContext>,
    message: Option<&serde_json::Value>,
) -> String {
    use crate::commands::agent_commerce::task::{arbitration, common};

    let result = arbitration_decision_result(source_event, job_id, job_title, prefetched, message);
    if result["decision"] != "requires_user_input" {
        return serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string());
    }

    let payload = &result["payload"];
    let required = |key: &str| payload.get(key).and_then(serde_json::Value::as_str);
    let (
        Some(service_name),
        Some(task_type),
        Some(requested_refund),
        Some(buyer_reason),
        Some(response_deadline),
        Some(decision_id),
        Some(refund_display_b64),
    ) = (
        required("serviceName"),
        required("taskType"),
        required("requestedRefund"),
        required("buyerReason"),
        required("responseDeadline"),
        required("decisionId"),
        required("refundDisplayB64"),
    )
    else {
        return arbitration::blocked_result(
            "missing_required_facts",
            job_id,
            serde_json::json!({"sourceEvent": source_event}),
        );
    };
    let is_subscription = source_event == arbitration::SUB_USER_REJECT;
    let current_period = if is_subscription {
        required("currentPeriod")
    } else {
        None
    };
    if is_subscription && current_period.is_none() {
        return arbitration::blocked_result(
            "missing_required_facts",
            job_id,
            serde_json::json!({"sourceEvent": source_event, "missingFields": ["currentPeriod"]}),
        );
    }
    let Some(expires_at) = payload["responseDeadlineTimestamp"].as_i64() else {
        return arbitration::blocked_result(
            "missing_required_facts",
            job_id,
            serde_json::json!({"sourceEvent": source_event, "missingFields": ["responseDeadline"]}),
        );
    };

    let template_vars_b64 = common::pending_v2::encode_refund_decision_vars(
        service_name,
        job_id,
        task_type,
        current_period,
        requested_refund,
        buyer_reason,
        response_deadline,
    );
    let source_template = super::content::asp_refund_decision_source_template(is_subscription);
    let choices = arbitration::default_choices(source_event, job_id);
    let choices_json = serde_json::to_string(&choices)
        .unwrap_or_else(|_| "[]".to_string())
        .replace('\'', "'\"'\"'");
    let short_id = short_job_id(job_id);
    let to_flag = prefetched
        .and_then(|value| value.user_agent_id.as_deref())
        .filter(|value| !value.is_empty())
        .map(|value| format!(" --to-agent-id {value}"))
        .unwrap_or_default();
    let label_placeholder = common::template_vars::REFUND_SERVICE_NAME_PLACEHOLDER;

    format!(
        "[Current state] {source_event} (buyer refund request requires the ASP owner's decision)\n\
         [Role] ASP\n\n\
         Render and push Product Template 6.4 exactly once as a single-record field list. The card itself is the refund-or-evaluation confirmation; do not add a third confirmation.\n\n\
         Localize the complete source template below to the current conversation language before passing it to `--user-content`. Translate the title, field labels, explanatory text, and action wording. Preserve every reserved `{{__OKX_...__}}` placeholder byte-for-byte; the CLI replaces those placeholders in-process after shell parsing. Do not expose or decode `--template-vars-b64`.\n\n\
         ```bash\n\
         onchainos agent pending-decisions-v2 request-prompt \\\n\
         \x20\x20--job-id {job_id} --role asp --agent-id {agent_id}{to_flag} \\\n\
         \x20\x20--user-content \"<localized complete Template 6.4 source below>\" \\\n\
         \x20\x20--list-label \"[Decision {short_id}] {label_placeholder} — refund or evaluation\" \\\n\
         \x20\x20--source-event {source_event} \\\n\
         \x20\x20--decision-id \"{decision_id}\" \\\n\
         \x20\x20--choices-json '{choices_json}' \\\n\
         \x20\x20--expires-at {expires_at} \\\n\
         \x20\x20--refund-display-b64 \"{refund_display_b64}\" \\\n\
         \x20\x20--template-vars-b64 \"{template_vars_b64}\"\n\
         ```\n\n\
         === BEGIN TEMPLATE 6.4 SOURCE ===\n\
         {source_template}\n\
         === END TEMPLATE 6.4 SOURCE ===\n\n\
         After `request-prompt` succeeds, end this turn and wait for the ASP owner's reply.",
    )
}

/// Extract the decision deadline (unix seconds) from a `job_rejected` event
/// `message` JSON. Returns `None` when the field is absent, non-numeric, or
/// `<= 0` (FR-4 / FR-5 graceful no-op).
fn reject_expire_time(message: Option<&serde_json::Value>) -> Option<i64> {
    message
        .and_then(|m| m.get("expireTime"))
        .and_then(|v| v.as_i64())
        .filter(|&t| t > 0)
}

/// Generate the structured next-action prompt for the ASP based on event.
///
/// `event_str` accepts either an event name (provider_applied / job_accepted / ...)
/// or a status name (created / accepted / ...) — internally normalized via state_machine
/// into an `Event`; unrecognized strings fall through as `Event::Other(s)`.
pub async fn generate_next_action(
    job_id: &str,
    event_str: &str,
    agent_id: &str,
    job_title: Option<&str>,
    data: Option<&str>,
    prefetched: Option<&crate::commands::agent_commerce::task::common::PreFetchedTaskContext>,
    message: Option<&serde_json::Value>,
) -> String {
    let _ = message; // currently used only by event handlers that opt in (see JobAspSelected below); silence the unused-arg warning when no scene reads it.
    use crate::commands::agent_commerce::task::common::state_machine::{
        parse_status_or_event, Event,
    };

    // Protocol compatibility is enforced by preflight before task execution,
    // not by tagging individual A2A messages with a legacy payload field.

    // Short jobId, used as the `[Job <shortId> — you are the ASP]` prefix on the first
    // When multiple prompts run concurrently it provides the user and the user agent a
    // dual disambiguation anchor. See SKILL.md Session Communication Contract §5.
    let short_id = short_job_id(job_id);

    // jobTitle carried by the envelope — when present, inlined directly into the
    // playbook (saves the agent an extra API query). When absent, agent must fetch
    // via `common context`. Used in --list-label so the reprompt notification can
    // show the task name (e.g. "Data Analysis Report · Approve / Reject").
    let title_display = job_title.unwrap_or("<title>");

    // Per-scene helper — render the pre-fetched task fields inline, or fall back to
    // the "call common context" CLI instruction when prefetched is None / a field is
    // missing. `fields` is the ordered subset of: title / tokenAmount / tokenSymbol /
    // buyerAgentId / description / paymentMode / providerAgentId / status /
    // serviceId / serviceTokenAddress / serviceTokenAmount / serviceParams.
    // Output goes directly into the playbook where Step 1 used to instruct the LLM
    // to run `onchainos agent common context …`.
    let inline_task_fields = |fields: &[&str]| -> String {
        use crate::commands::agent_commerce::task::common::PreFetchedTaskContext;
        let render = |p: &PreFetchedTaskContext| -> Option<String> {
            let mut out = String::from("**Task fields** (pre-fetched; use directly — skip the `common context` call unless a value below is empty / null):\n");
            let mut any = false;
            for f in fields {
                let line = match *f {
                    "title" if !p.title.is_empty() => {
                        Some(format!("\x20\x20- title: {}\n", p.title))
                    }
                    "description" if !p.description.is_empty() => {
                        Some(format!("\x20\x20- description: {}\n", p.description))
                    }
                    "tokenAmount" if !p.token_amount.is_empty() => {
                        Some(format!("\x20\x20- tokenAmount: {}\n", p.token_amount))
                    }
                    "tokenSymbol" if !p.token_symbol.is_empty() && p.token_symbol != "?" => {
                        Some(format!("\x20\x20- tokenSymbol: {}\n", p.token_symbol))
                    }
                    "buyerAgentId" => p
                        .user_agent_id
                        .as_deref()
                        .filter(|s| !s.is_empty())
                        .map(|v| format!("\x20\x20- buyerAgentId: {v}\n")),
                    "providerAgentId" => p
                        .provider_agent_id
                        .as_deref()
                        .filter(|s| !s.is_empty())
                        .map(|v| format!("\x20\x20- providerAgentId: {v}\n")),
                    "paymentMode" => p.payment_mode.map(|v| {
                        format!(
                            "\x20\x20- paymentMode: {v} ({})\n",
                            match v {
                                1 => "escrow",
                                3 => "x402",
                                _ => "unknown",
                            }
                        )
                    }),
                    "serviceId" => p
                        .service_id
                        .as_deref()
                        .filter(|s| !s.is_empty())
                        .map(|v| format!("\x20\x20- serviceId: {v}\n")),
                    "serviceTokenAddress" => p
                        .service_token_address
                        .as_deref()
                        .filter(|s| !s.is_empty())
                        .map(|v| format!("\x20\x20- serviceTokenAddress: {v}\n")),
                    "serviceTokenAmount" => p
                        .service_token_amount
                        .as_deref()
                        .filter(|s| !s.is_empty())
                        .map(|v| format!("\x20\x20- serviceTokenAmount: {v}\n")),
                    "serviceParams" => p
                        .service_params
                        .as_deref()
                        .filter(|s| !s.is_empty())
                        .map(|v| format!("\x20\x20- serviceParams: {v}\n")),
                    _ => None,
                };
                if let Some(l) = line {
                    out.push_str(&l);
                    any = true;
                }
            }
            if any {
                Some(out)
            } else {
                None
            }
        };
        match prefetched.and_then(render) {
            Some(s) => s,
            None => format!(
                "**Load task context first**:\n\
                 ```bash\n\
                 onchainos agent common context {job_id} --role asp --agent-id {agent_id}\n\
                 ```\n\
                 Extract {} (needed below).\n",
                fields.join(" + "),
            ),
        }
    };

    // ──────────────────────────────────────────────────────────────────────
    // Communication mechanics (how to send, whether you can send, form whitelist) —
    // all defined in SKILL.md Session Communication Contract. This file only tells
    // the agent **what content to send where** at each step; it does not re-explain
    // tool usage.
    //
    // NOTE: `send_to_peer` helper was removed — the deliver CLI now handles
    // XMTP peer send internally (upload + [intent:deliver] message + on-chain submit).
    // Other events that need peer messaging construct the command inline.

    // V2 accepted-task execution is anchored to the designated registered Service.
    // The provider may use that Service's existing tools internally, but must not
    // replace it with an unrelated ad-hoc workflow.
    let execute_task = format!(
        "Reuse the designated registered Service's existing AI/Skill workflow. Feed it the authoritative description, complete serviceParams, and forwarded attachments from the Task fields/session; do not substitute an unrelated workflow and do not re-run provider acceptance.\n\n\
        ⚠️ If a new question about task details / acceptance criteria is still required, use the existing A2A session (resolve `<buyerAgentId>` from the Task fields above):\n\
        \x20\x20\x20\x20```bash\n\
        \x20\x20\x20\x20okx-a2a xmtp-send \\\n\
        \x20\x20\x20\x20\x20\x20--job-id {job_id} \\\n\
        \x20\x20\x20\x20\x20\x20--to-agent-id <buyerAgentId> \\\n\
        \x20\x20\x20\x20\x20\x20--message \"<plain natural-language question to the User Agent>\" --json\n\
        \x20\x20\x20\x20```\n\
        End this turn after sending, wait for the reply; once you have the answer, start the work. Do not guess and produce a deliverable that misses the mark."
    );

    // Terminal-state (completed / refunded / close / dispute_resolved, etc.) session
    // retain-vs-release policy is governed by common::config::KEEP_CONVERSATION_ON_TERMINAL —
    // change the default by modifying that const.
    let terminal_session_hint = format!("\
ℹ️ Task is in terminal state — run the cleanup command (handles pending-decision cancellation automatically):\n\
         ```bash\n\
         onchainos agent session-cleanup --job-id {job_id}\n\
         ```\n\
         Then follow the command's output to close conversations (if applicable).");
    let event = parse_status_or_event(event_str);
    match &event {
        Event::JobRejected => {
            if let Some(task) = prefetched.filter(|task| {
                task.provider_agent_id.as_deref() == Some(agent_id)
                    && task.job_type == Some(0)
                    && task.status == Some(9)
                    && crate::commands::agent_commerce::task::user::refund::is_zero_decimal(
                        &task.token_amount,
                    )
            }) {
                return super::v2::notification::free_job_rejected_failed(job_id, task, message);
            }
            return arbitration_decision_playbook(
                crate::commands::agent_commerce::task::arbitration::JOB_REJECTED,
                job_id,
                agent_id,
                job_title,
                prefetched,
                message,
            );
        }
        Event::SubUserReject => {
            return arbitration_decision_playbook(
                crate::commands::agent_commerce::task::arbitration::SUB_USER_REJECT,
                job_id,
                agent_id,
                job_title,
                prefetched,
                message,
            );
        }
        Event::Other(event_name) if event_name.starts_with("user_decision_") => {
            let source_event = &event_name["user_decision_".len()..];
            if crate::commands::agent_commerce::task::arbitration::is_decision_source(source_event)
            {
                let expected_prefix = format!("{job_id}:{source_event}:");
                let decision_is_bound = message
                    .and_then(|value| value.get("decisionId"))
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|value| {
                        value.starts_with(&expected_prefix) && value.len() > expected_prefix.len()
                    });
                if !decision_is_bound {
                    return crate::commands::agent_commerce::task::arbitration::blocked_result(
                        "decision_metadata_missing",
                        job_id,
                        serde_json::json!({"sourceEvent": source_event}),
                    );
                }
                let selected_action = message
                    .and_then(|value| value.get("selectedActionId"))
                    .and_then(serde_json::Value::as_str);
                let params = message.and_then(|value| value.get("params"));
                let Some(action_id) = selected_action else {
                    return crate::commands::agent_commerce::task::arbitration::blocked_result(
                        "decision_metadata_missing",
                        job_id,
                        serde_json::json!({"sourceEvent": source_event}),
                    );
                };
                return match crate::commands::agent_commerce::task::arbitration::resolved_action(
                    source_event,
                    action_id,
                    job_id,
                    params,
                ) {
                    Ok(resolved) => serde_json::to_string(
                        &crate::commands::agent_commerce::task::arbitration::build_selected_result(
                            job_id, &resolved,
                        ),
                    )
                    .unwrap_or_else(|_| "{}".to_string()),
                    Err(error) => {
                        crate::commands::agent_commerce::task::arbitration::blocked_result(
                            error.reason_code(),
                            job_id,
                            serde_json::json!({"sourceEvent": source_event}),
                        )
                    }
                };
            }
        }
        _ => {}
    }
    match event {
        // ─── Scene 3: Apply has been recorded on-chain (escrow path; the User Agent issues the payment) ──
        Event::ProviderApplied => {
            let user_notify = super::content::provider_applied_user_notify(job_id, agent_id);
            format!(
            "[Current state] provider_applied (apply has been recorded on-chain)\n\
             [Role] ASP (Agent Service ASP)\n\n\
             ❌ Do NOT communicate with the User Agent. ❌ Do NOT deliver directly.\n\n\
             **Step 1 — Use `onchainos agent user-notify` to push the apply-submitted notification to the user**:\n\n\
             🌐 **Localize first** — rewrite the content below in the user's language before sending. Do NOT pass the English template verbatim to a non-English user.\n\
             ```bash\n\
             onchainos agent user-notify --content \"<localized content shown below>\"\n\
             ```\n\
             content (only the lines between `=== BEGIN ===` and `=== END ===` — do NOT add / drop fields, do NOT include the markers themselves, do NOT include anything below END):\n\
             === BEGIN ===\n\
             {user_notify}\n\
             === END ===\n\n\
             [Follow-up events]\n\
             - job_accepted → User Agent has confirm-accepted, escrow funding complete.\n"
            )
        },

        // ─── §1.5: designated-provider acceptance confirmed; execute and deliver ──
        Event::JobAccepted => {
            let user_notify = super::content::job_accepted_user_notify(job_id, agent_id);
            let task_fields = inline_task_fields(&["title", "description", "tokenAmount", "tokenSymbol", "serviceId", "serviceParams", "buyerAgentId"]);
            format!(
            "[Current state] job_accepted (your provider acceptance is confirmed)\n\
             [Role] ASP (Agent Service ASP)\n\n\
             [Your next action (strict order, do not skip steps)]\n\n\
             {task_fields}\n\
             **Step 1 — Notify the ASP owner (acceptance succeeded) via `onchainos agent user-notify`**:\n\n\
             🌐 **Localize first** — rewrite the content below in the user's language before sending. Do NOT pass the English template verbatim to a non-English user.\n\
             ```bash\n\
             onchainos agent user-notify --content \"<localized content shown below>\"\n\
             ```\n\
             content:\n\
             {user_notify}\n\n\
             Fill the `<title>` / `<description>` / `<tokenAmount>` / `<tokenSymbol>` placeholders from the **Task fields** block above.\n\
             ⚠️ Do NOT send any A2A acceptance filler to the Buyer Agent — both sides receive the authoritative `job_accepted` system event.\n\n\
             **Step 2 — Start the designated Service workflow and prepare the deliverable**:\n\
             {execute_task}\n\n\
             **Step 3 — Deliver** (single CLI command — handles file upload, peer notification, on-chain submit, and local save internally):\n\n\
             ⚠️ Do NOT call `okx-a2a file upload` or `okx-a2a xmtp-send` yourself — the `deliver` CLI handles all of this internally:\n\
             \x20\x20- file upload (when needed) → XMTP-send `[intent:deliver]` to the User Agent → on-chain submit → local persistent save.\n\
             \x20\x20- Text deliverables over 500 Unicode characters are auto-converted to a `.md` file and sent as a file attachment; if conversion/upload fails, the CLI falls back to inline text.\n\
             \x20\x20- A2A delivery must succeed before a single task can be submitted on-chain. Subscription delivery never calls the single-task submit API.\n\n\
             ▸ **File deliverable** — pass `--file` with the local file path:\n\
             ```bash\n\
             onchainos agent deliver {job_id} --file \"<local file path>\" --agent-id {agent_id}\n\
             ```\n\n\
             ▸ **Text deliverable** — pass only the heredoc-wrapped `--deliverable-text` (exactly one delivery-input flag):\n\
             ```bash\n\
             onchainos agent deliver {job_id} --agent-id {agent_id} \\\n\
             \x20\x20--deliverable-text \"$(cat <<'OKX_TEXT_EOF'\n\
             <full text deliverable content>\n\
             OKX_TEXT_EOF\n\
             )\"\n\
             ```\n\n\
             **Step 4 — After Step 3 ends this turn immediately** (do NOT send any filler `okx-a2a xmtp-send` / `onchainos agent user-notify` — the CLI already notified the User Agent).\n\n\
             The backend now opens the Buyer review after successful submission. The ASP does **not** wait for `job_submitted`; end this turn and wait for the terminal result.\n\n\
             [Follow-up events]\n\
             - `job_completed` (User Agent reviewed and accepted) — auto-rate the User Agent + notify the user\n\
             - `job_rejected` with a zero-price one-time task at Failed(9) — notify the ASP owner that the task failed, then clean up; no refund/evaluation decision\n\
             - `job_rejected` for a paid task — push the refund-vs-evaluation decision to the user\n"
            )
        }

        // Optional compatibility event: the Buyer is the required recipient.
        // `onchainos agent deliver` already sent the deliverable to the User Agent.
        // When job_submitted reaches this sub, never send it to the peer again.
        Event::JobSubmitted => {
            let user_notify = super::content::job_submitted_user_notify(job_id);
            format!(
            "[System notification] job_submitted (deliverable confirmed on-chain; task state is now submitted)\n\
             [Role] ASP (Agent Service ASP)\n\n\
             ⚠️ The deliverable was already sent by `onchainos agent deliver`; this event **must NOT trigger a second A2A send** to the User Agent. The notification below targets the ASP owner only.\n\n\
             **Step 1 — Notify the user of the submit milestone via `onchainos agent user-notify`**:\n\n\
             🌐 **Localize first** — rewrite the content below in the user's language before sending. Do NOT pass the English template verbatim to a non-English user.\n\
             ```bash\n\
             onchainos agent user-notify --content \"<localized content shown below>\"\n\
             ```\n\
             content:\n\
             {user_notify}\n\n\
             **Step 2 — End this turn.** Wait for `job_completed` / `job_rejected` to drive the next action.\n\n\
             When `job_completed` or `job_rejected` arrives, use the fresh task status and price to select the terminal or paid-dispute path.\n\n\
             [Follow-up events]\n\
             - `job_completed` (review passed) — auto-rate the User Agent + notify the user\n\
             - `job_rejected` with a zero-price one-time task at Failed(9) — notify Failed and clean up; no refund/evaluation decision\n\
             - `job_rejected` for a paid task — push the refund-vs-evaluation decision to the user\n"
            )
        },

        // ─── Scene 6: User Agent rejected the deliverable ─────────────────────────────────
        Event::JobRejected => {
            // FR-4: thread the decision deadline from the inbound event message
            // into the ASP evaluation card (unix seconds; filtered `> 0`).
            let expire_time = reject_expire_time(message);
            let user_prompt = super::content::job_rejected_user_decision_prompt(&short_id, expire_time);
            let to_flag = prefetched
                .and_then(|p| p.user_agent_id.as_deref())
                .filter(|s| !s.is_empty())
                .map(|b| format!(" --to-agent-id {b}"))
                .unwrap_or_default();
            format!(
            "[Current state] job_rejected (User Agent rejected the deliverable)\n\
             [Role] ASP (Agent Service ASP)\n\n\
             🛑 **MUST push the evaluation/refund decision via `pending-decisions-v2 request-prompt`** — `onchainos agent user-notify` is one-way (no reply relay) and a plain text reply doesn't reach the user-session; either path = 24h timeout → auto-refund.\n\
             ⚠️ Do NOT send `okx-a2a session send` `received the rejection` filler to the User Agent — they just rejected; they know. Go straight to the user-decision flow.\n\
             ⚠️ **24h hard deadline** — if the user does not decide within 24h, funds are auto-refunded to the User Agent. (Agent-side context; do NOT include in `--user-content` unless the localized template already mentions it.)\n\n\
             **Step 1 — Push the decision to the user via `pending-decisions-v2 request-prompt`**:\n\n\
             🌐 **Localize first** — translate the source template below to the user's language before passing to `--user-content`. Keep `[Job <shortId>]`, the `A.` / `B.` letters, the shortId hex.\n\
             ```bash\n\
             onchainos agent pending-decisions-v2 request-prompt \\\n\
             \x20\x20--job-id {job_id} --role asp --agent-id {agent_id}{to_flag} \\\n\
             \x20\x20--user-content \"<localized content shown below>\" \\\n\
             \x20\x20--list-label \"[Decision {short_id}] {title_display} evaluation decision\" \\\n\
             \x20\x20--source-event job_rejected\n\
             ```\n\
             content (only the lines between `=== BEGIN ===` and `=== END ===` — translate before passing; do NOT include the markers themselves, do NOT append anything else):\n\
             === BEGIN ===\n\
             {user_prompt}\n\
             === END ===\n",
            )
        }

        // Legacy direct pseudo-events cannot prove which active decision the
        // user answered. The bound `user_decision_job_rejected` relay is the
        // sole entry for either write action.
        Event::Other(ref s)
            if matches!(
                s.as_str(),
                "raise_arbitration" | "dispute_raise" | "agree_refund"
            ) => {
                crate::commands::agent_commerce::task::arbitration::blocked_result(
                    "decision_metadata_missing",
                    job_id,
                    serde_json::json!({
                        "sourceEvent": crate::commands::agent_commerce::task::arbitration::JOB_REJECTED,
                        "receivedEvent": s,
                    }),
                )
            }

        // Compatibility receipt from the retired two-stage path. Evaluation
        // creation is already complete in the combined request, so this event
        // has no write action.
        Event::DisputeApproved => format!(
            "[Current state] dispute_approved (compatibility receipt)\n\
             [Role] ASP\n\n\
             Evaluation creation is already submitted by `approveAndCreateDispute`. End this turn and continue when the matching evaluation event or fresh status arrives.\n\
             jobId={job_id}\n"
        ),

        // ─── Scene 6.2: User chose to agree to refund (user-instruction pseudo-event) ───
        Event::Other(ref s) if s == "agree_refund" => format!(
            "[Current action] Agree to refund\n\
             [Role] ASP\n\n\
             **Step 1 — Call the CLI (on-chain):**\n\
             ```bash\n\
             onchainos agent agree-refund {job_id} --agent-id {agent_id}\n\
             ```\n\n\
             After Step 1 → **end this turn**.\n\
             ⚠️ Do NOT send `okx-a2a session send` `agreed to refund` filler to the User Agent — both sides receive the `job_refunded` system event.\n\
             ⚠️ Do NOT push to the user with `onchainos agent user-notify`.\n"
        ),

        // ─── Subscription Scene: buyer rejected the current period → ASP decides refund/evaluation ──
        // `sub_user_reject` is a first-class Event (state_machine → SubStatus::Rejected). The ASP
        // owns this scene per the design doc: push a refund/evaluation decision to the user, mirroring
        // job_rejected but routing to the SUBSCRIPTION endpoints. ~1-day window before the backend
        // auto-refunds this period. (Removed from the "not handled in this slice" notify group.)
        Event::SubUserReject => {
            use crate::commands::agent_commerce::task::common::{pending_v2, template_vars};
            let to_flag = prefetched
                .and_then(|p| p.user_agent_id.as_deref())
                .filter(|s| !s.is_empty())
                .map(|b| format!(" --to-agent-id {b}"))
                .unwrap_or_default();
            let msg_str = |k: &str| -> Option<&str> {
                message.and_then(|m| m.get(k)).and_then(|v| v.as_str()).filter(|s| !s.is_empty())
            };
            let msg_i64 = |k: &str| message.and_then(|m| m.get(k)).and_then(|v| v.as_i64());
            // Keep the buyer-controlled subscription
            // title out of the emitted shell. The title appears in TWO visible
            // fields whose BASE sources differ and must NOT be collapsed:
            //   * decision copy — `jobTitle` → `title` → `title_display`;
            //   * list-label    — `title_display` (falls back to the literal `<title>`).
            // Each field carries its own reserved placeholder; the raw titles travel
            // only in the shell-safe Base64 `--template-vars-b64` payload and are
            // substituted in-process by `request-prompt` after clap parse, before any
            // push. The public `request-prompt` (direct-push) semantic
            // is preserved — this is NOT changed to `request`.
            let copy_ph = template_vars::TITLE_PLACEHOLDER;
            let label_ph = template_vars::LABEL_TITLE_PLACEHOLDER;
            let copy_title = msg_str("jobTitle").or_else(|| msg_str("title")).unwrap_or(title_display);
            let title_b64 = pending_v2::encode_title_vars(copy_title, title_display);
            let decision_copy = super::content::sub_user_reject_asp_decision_copy(
                copy_ph,
                msg_i64("subStartTime"),
                msg_i64("subEndTime"),
                msg_i64("rejectWindowEndsAt"),
                msg_str("tokenAmount"),
                msg_str("tokenSymbol"),
            );
            format!(
            "[Current state] sub_user_reject (the buyer rejected the current subscription period)\n\
             [Role] ASP (subscription)\n\n\
             🛑 **Push the refund/evaluation decision via `pending-decisions-v2 request-prompt`** — `onchainos agent user-notify` is one-way (no reply relay); a plain reply doesn't reach the user-session, so either path lets the ~1-day window lapse into an auto-refund.\n\
             ⚠️ Limited reaction window (about 1 day). Let the USER choose — do NOT decide autonomously; do NOT `okx-a2a session send` the buyer (they just rejected — they know).\n\n\
             **Step 1 — push the decision to the user**:\n\n\
             🌐 **Localize first** — translate the content between the markers to the user's language; keep the `A.` / `B.` letters and the `[Decision {short_id}]` label. Do NOT translate, move, or re-inline the reserved `{copy_ph}` / `{label_ph}` tokens or the `--template-vars-b64` value — they are substituted in-process.\n\
             ```bash\n\
             onchainos agent pending-decisions-v2 request-prompt \\\n\
             \x20\x20--job-id {job_id} --role asp --agent-id {agent_id}{to_flag} \\\n\
             \x20\x20--user-content \"<localized content shown below>\" \\\n\
             \x20\x20--list-label \"[Decision {short_id}] {label_ph} — refund or evaluation\" \\\n\
             \x20\x20--source-event sub_user_reject \\\n\
             \x20\x20--template-vars-b64 \"{title_b64}\"\n\
             ```\n\
             content (only the lines between the markers — translate before passing; do NOT include the markers).\n\
             Canonical copy — ASP-3 subscription rejection decision (localize to the user's language at push time):\n\
             === BEGIN ===\n\
             {decision_copy}\n\
             === END ===\n",
            )
        }

        // Legacy subscription pseudo-events use the same bound-decision gate.
        Event::Other(ref s)
            if matches!(
                s.as_str(),
                "raise_subscription_arbitration" | "sub_dispute" | "sub_agree_refund"
            ) => {
                crate::commands::agent_commerce::task::arbitration::blocked_result(
                    "decision_metadata_missing",
                    job_id,
                    serde_json::json!({
                        "sourceEvent": crate::commands::agent_commerce::task::arbitration::SUB_USER_REJECT,
                        "receivedEvent": s,
                    }),
                )
            }

        // ─── Scene 7: Task completed (review passed / evaluation won) ────────────────
        Event::JobCompleted => super::v2::job_completed::handle(job_id, agent_id).await,

        // ─── Scene 6.5: Evaluation ruling (won / lost branches distinguished by jobStatus in the inbound envelope) ─
        Event::DisputeResolved => {
            let dispute_won_claim = super::content::dispute_won_with_claim_user_notify(job_id);
            let dispute_won_no_claim = super::content::dispute_won_no_claim_user_notify(job_id);
            let dispute_lost = super::content::dispute_lost_user_notify(job_id);
            let rating_notify = super::content::rating_submitted_user_notify(job_id);
            let task_fields = inline_task_fields(&["title", "tokenAmount", "tokenSymbol", "buyerAgentId"]);
            format!(
            "[Current state] dispute_resolved (evaluation ruling delivered)\n\
             [Role] ASP (Agent Service ASP)\n\n\
             ⚠️ **Determining win/loss**: read `message.jobStatus` from the system notification envelope you just received:\n\
             - `jobStatus = \"complete\"` → **you (ASP) won**; funds released to you\n\
             - `jobStatus = \"failed\"` → **you (ASP) lost**; funds refunded to the User Agent\n\
             [Your next action (branch by win/loss)]\n\n\
             ⚠️ Do NOT send `okx-a2a session send` `ruling supports party X` filler to the User Agent — both sides receive the `dispute_resolved` system event.\n\n\
             {task_fields}\n\
             ━━━━━━━━━━━━━ Branch A: jobStatus=complete (ASP won) ━━━━━━━━━━━━━\n\n\
             **A-Step 1 — Check claimable rewards (account-pull)**:\n\
             ```bash\n\
             onchainos agent asp-claimable --agent-id {agent_id}\n\
             ```\n\
             Lines with a `•` marker in stdout indicate a non-zero claimable amount for that token.\n\n\
             **A-Step 2 — Claim everything in one shot when amounts are non-zero** (skip if claimable output is all zero):\n\
             ```bash\n\
             onchainos agent asp-claim-rewards --agent-id {agent_id}\n\
             ```\n\
             Record stdout's txHash + the actual amount / token claimed (used to notify the user in the next step).\n\n\
             **A-Step 3 — Notify the user of the win + claim result via `onchainos agent user-notify`**:\n\n\
             Field values for the content template come from the **Task fields** block above.\n\
             ⚠️ content is the **chat the user will see** — plain natural language; **do NOT use** skill names / event names / state names / CLI flags or other technical jargon.\n\
             🌐 **Localize first** — rewrite the content below in the user's language before sending. Do NOT pass the English template verbatim to a non-English user.\n\
             ```bash\n\
             onchainos agent user-notify --content \"<localized content shown below>\"\n\
             ```\n\
             content (choose based on whether A-Step 2 actually claimed):\n\
             \x20\x20\x20\x20Claimed:\n\
             {dispute_won_claim}\n\
             \x20\x20\x20\x20Nothing to claim:\n\
             {dispute_won_no_claim}\n\n\
             🛑 Do NOT end this turn — A-Step 4 (auto-rate) and A-Step 4.5 (notify rating) below are MANDATORY.\n\n\
             **A-Step 4 — 🛑 Auto-rate the User Agent (MANDATORY):**\n\
             Based on the task description, requirements clarity, communication, and dispute outcome (you won), generate:\n\
             \x20\x20- Score: 0.00–5.00 (two decimal places). Guide: ASP won dispute → User Agent was likely at fault; 0.00–3.00 depending on severity. If the dispute was a misunderstanding, score higher.\n\
             \x20\x20- Comment: one sentence, ≤100 characters, evaluating how well the deliverable matches the description.\n\
             Then execute:\n\
             ```bash\n\
             onchainos agent feedback-submit --agent-id <buyerAgentId> --creator-id {agent_id} --score <X.XX> --task-id {job_id} --description \"<comment, ≤100 chars>\"\n\
             ```\n\
             ⚠️ `--agent-id` is the User Agent being rated (buyerAgentId from the **Task fields** block at the top); `--creator-id` is the ASP's own agent id ({agent_id}).\n\n\
             **A-Step 4.5 — Notify the user of the submitted rating**:\n\
             🌐 **Localize first** — rewrite the content below in the user's language before sending. Do NOT pass the English template verbatim to a non-English user.\n\
             After feedback-submit, run `onchainos agent user-notify` to notify the user:\n\
             - ✅ **Success** (output contains `txHash`):\n\
             ```bash\n\
             onchainos agent user-notify --content \"<localized content shown below>\"\n\
             ```\n\
             content (fill `<score>` with the X.XX value and `<description>` with the comment you just used in A-Step 4; fill `<title>` from task context):\n\
             {rating_notify}\n\
             - ❌ **Failure** (error / non-zero exit code) → silently skip; do NOT notify the user, do NOT retry.\n\n\
             ━━━━━━━━━━━━━ Branch B: jobStatus=failed (ASP lost) ━━━━━━━━━━━━━\n\n\
             **B-Step 1 — Notify the user of the loss via `onchainos agent user-notify`**:\n\n\
             Field values for the content template come from the **Task fields** block above (same fields as Branch A).\n\
             ⚠️ Same as A-Step 3 — content plain natural language; no technical jargon.\n\
             🌐 **Localize first** — rewrite the content below in the user's language before sending. Do NOT pass the English template verbatim to a non-English user.\n\
             ```bash\n\
             onchainos agent user-notify --content \"<localized content shown below>\"\n\
             ```\n\
             content:\n\
             {dispute_lost}\n\n\
             🛑 Do NOT end this turn — B-Step 2 (auto-rate) and B-Step 2.5 (notify rating) below are MANDATORY.\n\n\
             **B-Step 2 — 🛑 Auto-rate the User Agent (MANDATORY):**\n\
             Based on the task description, requirements clarity, and dispute outcome (you lost — User Agent's rejection was upheld), generate:\n\
             \x20\x20- Score: 0.00–5.00 (two decimal places). Guide: ASP lost dispute → User Agent was likely right; 3.00–5.00. Adjust based on whether the dispute felt fair.\n\
             \x20\x20- Comment: one sentence, ≤100 characters, evaluating how well the deliverable matches the description.\n\
             Then execute:\n\
             ```bash\n\
             onchainos agent feedback-submit --agent-id <buyerAgentId> --creator-id {agent_id} --score <X.XX> --task-id {job_id} --description \"<comment, ≤100 chars>\"\n\
             ```\n\
             ⚠️ `--agent-id` is the User Agent being rated (buyerAgentId from the **Task fields** block at the top); `--creator-id` is the ASP's own agent id ({agent_id}).\n\n\
             **B-Step 2.5 — Notify the user of the submitted rating**:\n\
             🌐 **Localize first** — rewrite the content below in the user's language before sending. Do NOT pass the English template verbatim to a non-English user.\n\
             After feedback-submit, run `onchainos agent user-notify` to notify the user:\n\
             - ✅ **Success** (output contains `txHash`):\n\
             ```bash\n\
             onchainos agent user-notify --content \"<localized content shown below>\"\n\
             ```\n\
             content (fill `<score>` with the X.XX value and `<description>` with the comment you just used in B-Step 2; fill `<title>` from task context):\n\
             {rating_notify}\n\
             - ❌ **Failure** (error / non-zero exit code) → silently skip; do NOT notify the user, do NOT retry.\n\n\
             ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\n\
             {terminal_session_hint}\n"
            )
        }

        // ─── Scene 6.5b: ASP agreed to refund / dispute refund on-chain ─────────────────
        Event::JobRefunded => format!(
            "[Current state] job_refunded (funds refunded to the User Agent)\n\
             [Role] ASP (Agent Service ASP)\n\n\
             [Your next action]\n\n\
             ⚠️ Do NOT send `okx-a2a session send` `refund on-chain` filler to the User Agent — both sides already receive the `job_refunded` system event.\n\
             {terminal_session_hint}\n\n\
             **End this turn directly**; the refund flow is fully complete.\n"
        ),

        // ─── Scene 6.4: Evaluation on-chain; CLI auto-submits evidence ─────────────────────
        Event::JobDisputed => {
            let task_fields = inline_task_fields(&["buyerAgentId"]);
            format!(
            "[Current state] job_disputed (evaluation is on-chain; CLI auto-submits evidence on this event)\n\
             [Role] ASP (Agent Service ASP)\n\n\
             🛑 **This event triggers an AUTOMATIC evidence upload — no user interaction**.\n\
             The agent does NOT ask the user for evidence; it recovers the exact evaluation reason and pulls the full chat history from this sub\n\
             session, calls `dispute upload` (which also auto-attaches the deliverable copy saved under\n\
             `~/.onchainos/deliverables/asp/{job_id}/`), and then notifies the user via\n\
             `onchainos agent user-notify`. **Do NOT** use `pending-decisions-v2 request` for this event.\n\
             **Do NOT** `okx-a2a session send` anything to the User Agent — both sides see the evaluation via on-chain events.\n\n\
             {task_fields}\n\
             **Step 1 — Recover the evaluation reason:**\n\
             Find the latest `[ARBITRATION_REASON_CONTEXT]` message in this task conversation whose `jobId` is `{job_id}`, `providerAgentId` is `{agent_id}`, `taskType` is `one_time`, and `resumeEvent` is `job_disputed`. Preserve its `reason` exactly.\n\
             The matching context is required for this evidence upload. When it is unavailable, return `arbitration_reason_context_missing` and end this turn.\n\n\
             **Step 2 — Pull this sub session's chat history** (use `buyerAgentId` from the **Task fields** block above):\n\n\
             ```bash\n\
             okx-a2a session history --job-id {job_id} --to-agent-id <buyerAgentId> --json\n\
             ```\n\n\
             **Step 3 — Format the evaluation reason and chat history as the `--text` body**:\n\n\
             ```\n\
             ==== ASP evaluation reason (from ARBITRATION_REASON_CONTEXT) ====\n\
             <exact reason>\n\
             ==== Negotiation / delivery chat history (from okx-a2a session history) ====\n\
             [time] User Agent(<agentId>): ...\n\
             [time] ASP(<agentId>): ...\n\
             ... (chronological; key checkpoints: ASP's cold-start opener / task scope clarifications / ASP's capability confirmation / your deliver message / each side's key contention points)\n\
             ```\n\n\
             ⚠️ **`--text` is capped at 16 KB** — if the chat history is long, **keep only** the key checkpoints (opener / scope clarifications / capability confirmation / deliverable / each side's key contention points) and prepend `(key checkpoints extracted)`; do NOT blindly drop the first N entries.\n\
             If history is genuinely empty, pass a minimal placeholder like `(no chat history available)` so `--text` is non-empty.\n\n\
             **Step 4 — Upload (off-chain multipart):**\n\
             ```bash\n\
             onchainos agent dispute upload {job_id} --role asp --agent-id {agent_id} --text \"<evaluation reason + chat history block>\"\n\
             ```\n\
             The CLI auto-attaches every entry under `~/.onchainos/deliverables/asp/{job_id}/manifest.json` as multipart `files[]` parts — **do NOT pass `--file`**; the manifest covers the deliverable copy saved at `deliver` time. If the upload fails, retry up to 3 times; if it keeps failing, still proceed to Step 5 — the on-chain evaluation will continue with the available evidence.\n\n\
             **Step 5 — Notify the user (after upload returns):**\n\n\
             content:\n\
             \x20\x20\x20\x20[Evaluation opened] Evaluation for job `{job_id}` is on-chain.\n\
             \x20\x20\x20\x20- Evaluation status: Evidence preparation\n\
             \x20\x20\x20\x20- Status description: Evidence was submitted and the evidence stage is in progress.\n\
             \x20\x20\x20\x20Awaiting the evaluator's verdict.\n\n\
             **Step 6 — End this turn.** Do NOT `okx-a2a session send` anything to the User Agent.\n\n\
             [Follow-up events]\n\
             - job_completed → won, funds released to the ASP\n\
             - dispute_resolved → lost, funds refunded to the User Agent\n"
            )
        }

        // ─── Scene 1: task is on-chain (job_created) — ASP takes no proactive
        // action on this raw event. Designated tasks arrive as a `JobAspSelected`
        // event (User Agent-designated). ────
        Event::JobCreated => "[System notification] job_created (task is on-chain; no ASP-side action)\n\
             [Role] ASP (Agent Service ASP)\n\n\
             Silently ignore; end this turn.\n\
             Designated tasks arrive via a `job_asp_selected` event when the User Agent designates this ASP.\n".to_string(),

        // ─── Scene 1.5: User Agent designated this ASP for a private task ──────────
        Event::JobAspSelected => provider_assignment_playbook(
            job_id,
            agent_id,
            ProviderAssignmentType::Single,
            prefetched,
            message,
        )
        .await,

        // ─── Job notifications (structured; terminal timeouts also clean up) ───
        Event::JobAspAcceptExpire => match prefetched {
            Some(task) => super::v2::notification::job_asp_accept_expire(job_id, task, message),
            None => super::v2::notification::authoritative_context_required(
                job_id,
                "job_asp_accept_expire",
                &["taskDetail"],
            ),
        },
        Event::JobAspRejectClosed => match prefetched {
            Some(task) => super::v2::notification::job_asp_reject_closed(job_id, task, message),
            None => super::v2::notification::authoritative_context_required(
                job_id,
                "job_asp_reject_closed",
                &["taskDetail"],
            ),
        },
        Event::JobAspRejectExpire => match prefetched {
            Some(task) => super::v2::notification::job_asp_reject_expire(job_id, task, message),
            None => super::v2::notification::authoritative_context_required(
                job_id,
                "job_asp_reject_expire",
                &["taskDetail"],
            ),
        },
        Event::JobExpired | Event::SubmitExpired => match prefetched {
            Some(task) => {
                super::v2::notification::job_delivery_expired(job_id, task, event.as_str())
            }
            None => super::v2::notification::authoritative_context_required(
                job_id,
                event.as_str(),
                &["taskDetail"],
            ),
        },
        Event::SubAspClaimNotify => {
            super::v2::notification::sub_asp_claim_notify(job_id, message)
        }

        // ─── User Agent-driven tx receipt notifications; no ASP action needed ─────
        Event::JobClosed
        | Event::JobPaymentModeChanged => format!(
            "[System notification] {event} (User Agent-side tx receipt; not the ASP's concern)\n\
             [Role] ASP (Agent Service ASP)\n\n\
             Silently ignore; end this turn. \n",
            event = event.as_str()
        ),

        // ─── User Agent-driven timeout events; no ASP action needed ─────
        Event::RejectExpired | Event::ReviewDeadlineWarn => format!(
            "[System notification] {event} (User Agent-side timeout event; not the ASP's concern)\n\
             [Role] ASP (Agent Service ASP)\n\n\
             Silently ignore; end this turn.\n",
            event = event.as_str()
        ),

        // ─── review_expired: review window timed out; ASP actively claims the payment ─────────────
        Event::ReviewExpired => format!(
            "[System notification] review_expired (review window expired; the User Agent did not accept in time)\n\
             [Role] ASP (Agent Service ASP)\n\n\
             ⚠️ **review_expired is just a window-timeout event; the task state is still submitted; funds are NOT auto-released**.\n\
             You need to actively call claimAutoComplete to pull the funds out of the escrow contract; only after on-chain confirmation does the state become completed.\n\n\
             [Your next action (strict order)]\n\n\
             **Step 1 — Call the CLI to claim the payment (on-chain):**\n\
             ```bash\n\
             onchainos agent claim-auto-complete {job_id} --agent-id {agent_id}\n\
             ```\n\
             CLI internals: POST /claimAutoComplete → uopData → sign uopHash → broadcast. Wait for the on-chain `job_completed` notification.\n\n\
             ⚠️ **After claim-auto-complete, end the turn directly**:\n\
             - Do NOT send any okx-a2a session send to the User Agent (filler in between; wait until the job_completed on-chain receipt arrives)\n\
             - Do NOT push to the user with `onchainos agent user-notify`\n\n\
             [Follow-up events]\n\
             - `job_completed` (success) → next-action provides the funds-received script (push to user; conversation retained)\n\
             - `job_completed` (failed)  → retry claim-auto-complete per errorCode\n"
        ),

        // ─── ASP's own deadline reminder ─────────────────────────────────────
        Event::SubmitDeadlineWarn => {
            let user_prompt = super::content::submit_deadline_warn_user_prompt(&short_id);
            let request_block = crate::commands::agent_commerce::task::common::pending_v2::request_command_block(
                job_id,
                "asp",
                agent_id,
                prefetched.and_then(|p| p.user_agent_id.as_deref()),
                &user_prompt,
                &format!("[Decision {short_id}] {title_display} submit decision"),
                "submit_deadline_warn",
            );
            format!(
            "[System notification] submit_deadline_warn (deadline for submitting the deliverable is approaching)\n\
             [Role] ASP (Agent Service ASP)\n\n\
             🛑 **MUST push the submit-now/let-timeout decision via `pending-decisions-v2 request`** — `onchainos agent user-notify` is one-way (no reply relay) and a plain text reply doesn't reach the user-session; either path = the deadline silently expires → auto-refund to the User Agent.\n\
             ❌ Do NOT `okx-a2a session send` the User Agent — the deadline warning is between the ASP and the user, not the User Agent's business.\n\n\
             **Push the decision to the user (3-substep protocol; read ALL 3 before running any command)**:\n\n\
             {request_block}\n\
             ⚠️ **Do NOT auto-run `onchainos agent deliver` later** — only the user knows whether the deliverable is actually ready; the agent must not decide \"deliverable is ready\" on the user's behalf.\n",
            )
        }

        // ─── Evaluation sub-state-machine events — ASP cares about dispute_resolved (already has a dedicated arm); other evaluator-internal events are observed silently ─────
        Event::EvaluatorSelected
        | Event::RevealStarted
        | Event::VoteCommitted
        | Event::VoteRevealed
        | Event::RoundFailed
        | Event::VoteCommitDeadlineWarn
        | Event::VoteRevealDeadlineWarn => format!(
            "[System notification] {event} (evaluation-internal event; handled by the evaluator)\n\
             [Role] ASP (Agent Service ASP)\n\n\
             [Recommendation] Observe silently. After the `dispute_resolved` notification arrives, call next-action to wrap up.\n",
            event = event.as_str()
        ),

        // ─── User Agent attachment received — download + save, no reply ─────
        Event::UserAttachmentReceived => {
            user_attachment_received_cli(job_id, agent_id, &short_id, message)
        }

        // ─── Staking / reward / slash lifecycle tx receipts — irrelevant when ASP is not an evaluator ─────
        Event::Staked
        | Event::UnstakeRequested
        | Event::UnstakeClaimed
        | Event::UnstakeCancelled
        | Event::StakeStopped
        | Event::CooldownEntered => format!(
            "[System notification] {event} (evaluator staking lifecycle tx receipt; not the ASP's concern)\n\
             [Role] ASP (Agent Service ASP)\n\n\
             Silently ignore; end this turn.\n",
            event = event.as_str()
        ),

        // reward_claimed — own claim tx receipt (ASP may also claim evaluation rewards)
        Event::RewardClaimed => {
            let failed_notify = super::content::reward_claim_failed_user_notify(job_id);
            let claimed_notify = super::content::reward_claimed_user_notify(job_id);
            format!(
            "[System notification] reward_claimed (claimRewards tx receipt)\n\
             [Role] ASP (Agent Service ASP)\n\n\
             **Step 1 — Check the envelope's `message.code` field:**\n\
             - `code` non-zero (failed) → run `onchainos agent user-notify` to notify the user, then end the turn:\n\
             \x20\x20```bash\n\
             \x20\x20onchainos agent user-notify --content \"{failed_notify}\"\n\
             \x20\x20```\n\n\
             - `code` = 0 (success) → continue to Step 2.\n\n\
             **Step 2 — Notify the user that the reward has arrived via `onchainos agent user-notify`:**\n\
             \x20\x20```bash\n\
             \x20\x20onchainos agent user-notify --content \"{claimed_notify}\"\n\
             \x20\x20```\n"
            )
        }

        // job_auto_refunded — buyer/backend Refund V2 settlement receipt; not the ASP's concern
        Event::JobAutoRefunded => "[System notification] job_auto_refunded (buyer/backend Refund V2 settlement receipt; not the ASP's concern)\n\
             [Role] ASP (Agent Service ASP)\n\n\
             Silently ignore; end this turn.\n".to_string(),

        Event::WakeupNotify => {
            format!(
            "[System notification] wakeup_notify (task wake-up after network / machine reboot)\n\
             [Role] ASP (Agent Service ASP)\n\n\
             ⚠️ This is a wake-up heartbeat event, **NOT** a business-driving event. The real business state is in the envelope.message.jobStatus field.\n\
             You should NOT use `wakeup_notify` as --event to run the script — this script is just for guidance.\n\n\
             [Your next action (strict order)]\n\n\
             **Step 1 — Read the real status from the envelope**:\n\
             From the wakeup_notify envelope that triggered this turn, read the `message.jobStatus` field (e.g. `accepted` / `submitted` / `rejected` / `disputed` / `completed` / `failed`, etc. — the real status string).\n\n\
             **Step 2 — Use the real status to call next-action and fetch the current script**:\n\
             ```bash\n\
             onchainos agent next-action --role asp --agentId {agent_id} --message '{{\"event\":\"<value of the message.jobStatus field>\",\"jobId\":\"{job_id}\"}}'\n\
             ```\n\
             Follow the returned script for what to do in the current status.\n\n\
             ⚠️ **Do NOT** okx-a2a session send the User Agent something like `I'm back online` — the peer does not care about your connection status.\n\
             ⚠️ If the Step 2 script is a passive-wait kind (e.g. status=accepted: ASP is working / status=submitted: waiting for User Agent review), only emit a `task resumed` notification and end the turn; do not proactively run business actions.\n"
            )
        }

        // Negotiation relay events are only used by the User Agent side; ASP ignores
        Event::NegotiateReply => "[System notification] negotiate_reply (User Agent-side negotiation relay event; not the ASP's concern)\n\
             [Recommendation] Ignore; no action needed.\n".to_string(),

        Event::AttachmentAdded | Event::DeliverableReceived => "[System notification] User Agent-side event; not the ASP's concern.\n\
             [Recommendation] Ignore; no action needed.\n".to_string(),

        // ─── user_decision_* relay router (ASP-side scenes) ───
        // User-decision relays arrive as system-shaped envelopes with
        // `event = "user_decision_<source_event>"` and `message.data = <user's verbatim reply>`.
        // CLI returns a routing playbook that lists the candidate pseudo-events with
        // natural-language descriptions; the sub agent's LLM decides which one the
        // user actually meant — no hardcoded keyword tables, pure semantic mapping.
        Event::Other(ref s) if s.starts_with("user_decision_") => {
            let source = &s["user_decision_".len()..];
            let reply = data.unwrap_or("").trim();
            match source {
                "submit_deadline_warn" => format!(
                    "[User decision relay] source_event=`submit_deadline_warn`, user's verbatim reply: `{reply}`\n\n\
                     **Semantic mapping** — decide which intent the user's reply means:\n\n\
                     \x20\x20• **Submit now** — user wants to deliver immediately (typical intents: 立即提交 / 我提交 / submit now / I'll deliver / ready / 现在交). Route: call `onchainos agent next-action --role asp --agentId {agent_id} --message '{{\"event\":\"job_accepted\",\"jobId\":\"{job_id}\"}}'` and run its Step 2-3 (skip Step 1 apply-accepted notification — user already knows).\n\
                     \x20\x20• **Let it timeout** — user lets the deadline pass (typical intents: silence / 算了 / 不交了 / let it timeout / skip / 放弃). Route: end the turn; the chain will fire `submit_expired` and the backend automatically refunds the User Agent without a client-side claim.\n\n\
                     If ambiguous: re-ask via `pending-decisions-v2 request` (`--source-event submit_deadline_warn`).\n"
                ),
                "cli_failed" => format!(
                    "[User decision relay] source_event=`cli_failed`, user's verbatim reply: `{reply}`\n\n\
                     The original `onchainos agent <cmd>` failed and you asked the user how to proceed. **Semantic mapping** — decide what the user means and act accordingly (no on-chain action by default):\n\n\
                     \x20\x20• **Retry** — user wants you to re-run the same CLI command (typical intents: A / 选A / retry / 重试 / try again / 再来一次 / 再试一次). Action: re-execute the **exact same** CLI you previously ran (same args, same job_id). If it fails again, do NOT loop — enqueue **one more** `pending-decisions-v2 request --source-event cli_failed` and end the turn.\n\
                     \x20\x20• **Dismiss** — user takes manual control of this step (typical intents: B / 选B / dismiss / 不再提示 / skip prompts / 我自己处理 / let me handle it). Action: end the turn. Do not re-prompt; the user owns this step now.\n\
                     \x20\x20• **New instruction** — user gives a corrective instruction in natural language (e.g. `把 token-symbol 改成 USDT 再试` / `change --token-symbol to USDT and retry` / `用 endpoint https://... 重试`). Action: parse the modification, rebuild the CLI invocation with the user's adjustment, and execute once. Treat the result as a fresh attempt (success → continue the original scene; failure → enqueue another `cli_failed` decision).\n\n\
                     ⚠️ Do NOT execute any on-chain action that wasn't part of the original failed command — the user reply only authorizes retry/edit of the failed step, not unrelated new actions.\n\
                     ⚠️ If the reply is truly ambiguous (e.g. unrelated chitchat / a non-committal `hmm` / `got it`), re-ask via `pending-decisions-v2 request` with the same `--to-agent-id` as the incoming relay's `[to: …]` header (OMIT it for `[to: backup]` / backup subs — NEVER your own agentId) and `--source-event cli_failed`. **`--user-content` must be localized to the user's language** (detect from the user's verbatim reply / prior turn) before sending. Reference (English): \"I didn't catch your reply, please clarify: A=retry  B=stop prompting  C=tell me what to change\".\n"
                ),
                _ => format!(
                    "[User decision relay] source_event=`{source}` (no specific routing rule defined for this scene), user's verbatim reply: `{reply}`\n"
                ),
            }
        }

        // job_provider_reject: off-chain receipt confirming this ASP's own asp-reject;
        // no ASP-side action needed (the User Agent side handles the re-route). Terminal.
        Event::JobProviderReject => format!(
            "[System notification] job_provider_reject (your decline was registered; no further action).\n\
             {terminal_session_hint}\n"
        ),
        Event::JobUserReject => {
            let user_notify = super::content::job_user_reject_notify(job_id);
            let l10n = super::content::L10N_DISPATCH_SHORT;
            format!(
                "[Current state] job_user_reject (User Agent declined to fund / confirm-accept)\n\
                 [Role] ASP (Agent Service ASP)\n\n\
                 **Notify the user, then end the turn** (🌐 translate template to user's language first):\n\
                 {user_notify}\n\
                 {l10n}\n\n\
                 ```bash\n\
                 onchainos agent user-notify --content \"<translated text>\"\n\
                 ```\n\
                 ❌ Do NOT okx-a2a session send the User Agent. ❌ Do NOT retry apply.\n\n\
                 {terminal_session_hint}\n"
            )
        }
        // ─── Subscription notifications (display-class) ──────────────────────
        Event::SubAspSelected => {
            let msg_str = |k: &str| -> Option<&str> {
                message.and_then(|m| m.get(k)).and_then(|v| v.as_str()).filter(|s| !s.is_empty())
            };
            let msg_i64 = |k: &str| -> Option<i64> {
                message.and_then(|m| m.get(k)).and_then(|v| v.as_i64())
            };
            // Envelope title first, then the prefetched task title. When both miss, the
            // content layer omits the service-name subclause — a literal `<title>`
            // placeholder must never reach the notification body.
            let title = msg_str("jobTitle")
                .or_else(|| msg_str("title"))
                .or_else(|| {
                    prefetched.map(|p| p.title.as_str()).filter(|s| !s.is_empty())
                });
            let buyer_agent_id = msg_str("buyerAgentId")
                .or_else(|| prefetched.and_then(|p| p.user_agent_id.as_deref()))
                .filter(|s| !s.is_empty());
            let token_amount = msg_str("tokenAmount")
                .or_else(|| prefetched.map(|p| p.token_amount.as_str()))
                .filter(|s| !s.is_empty());
            let token_symbol = msg_str("tokenSymbol")
                .or_else(|| prefetched.map(|p| p.token_symbol.as_str()))
                .filter(|s| !s.is_empty() && *s != "?");
            // Trial subscribers charge nothing on selection — the ASP must not be told a
            // payment was received (mirrors the buyer-side sub_created trialType branch).
            // trail* is the pre-rename field spelling kept as a read fallback (AC-17).
            let content = if msg_i64("trialType") == Some(1) {
                super::content::sub_asp_selected_trial_asp_notify(
                    title,
                    buyer_agent_id,
                    job_id,
                    token_amount,
                    token_symbol,
                    msg_i64("trialStartTime").or_else(|| msg_i64("trailStartTime")),
                    msg_i64("trialEndTime").or_else(|| msg_i64("trailEndTime")),
                )
            } else {
                super::content::sub_asp_selected_asp_notify(
                    title,
                    buyer_agent_id,
                    job_id,
                    token_amount,
                    token_symbol,
                    msg_i64("subStartTime"),
                    msg_i64("subEndTime"),
                )
            };
            let task_fields = inline_task_fields(&[
                "title",
                "description",
                "serviceParams",
                "buyerAgentId",
                "serviceId",
            ]);
            sub_asp_accepted_start(
                "sub_asp_selected (subscription acceptance confirmed)",
                &content,
                &task_fields,
                job_id,
                agent_id,
            )
        }
        Event::SubCompleteNotify => {
            let title = message
                .and_then(|m| m.get("jobTitle"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .or_else(|| prefetched.map(|task| task.title.as_str()).filter(|s| !s.is_empty()));
            let period_end = message.and_then(|m| m.get("subEndTime")).and_then(|v| v.as_i64());
            super::v2::sub_complete_notify::handle(job_id, title, period_end)
        }
        Event::SubCloseNotify => {
            let title = message
                .and_then(|m| m.get("jobTitle").or_else(|| m.get("title")))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty());
            let asp_reject_reason = message
                .and_then(|m| m.get("aspRejectReason"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty());
            display_notify(
                "sub_close_notify (subscription closed)",
                &super::content::sub_close_notify_asp_notify(title, job_id, asp_reject_reason),
                Some(terminal_session_hint.as_str()),
            )
        }
        Event::SubFailedNotify => {
            // The unchanged backend shares Failed(9) across refund and
            // charge/conversion failures, while the inbound event JSON has no
            // trusted system provenance. Fresh provider ownership/status is
            // checked by the outer gate, but caller title/reason fields still
            // cannot establish a cause or a terminal cleanup decision.
            let title = prefetched
                .map(|detail| detail.title.trim())
                .filter(|title| !title.is_empty())
                .unwrap_or("Subscription title unavailable");
            let content = format!(
                "[Subscription Result Needs Reconciliation] {title} (`{job_id}`) is in fresh Failed(9) status, but the authoritative backend detail does not expose whether this was a refund or a charge/conversion failure. The caller-provided `sub_failed_notify` title and reason fields are not trusted result evidence. Do not report either outcome, take a settlement action, or close the ASP session from this event. Wait for an authoritative lifecycle result or inspect the latest subscription status read-only."
            );
            display_notify(
                "sub_failed_notify (result cause unverified)",
                &content,
                None,
            )
        }
        // ─── Subscription evaluation: ASP auto-submits evidence ───────────────────
        Event::SubAspDispute => {
            let task_fields = inline_task_fields(&["buyerAgentId"]);
            format!(
            "[Current state] sub_asp_dispute (subscription evaluation on-chain; CLI auto-submits evidence on this event)\n\
             [Role] ASP (Agent Service ASP)\n\n\
             🛑 **This event triggers an AUTOMATIC evidence upload — no user interaction**.\n\
             The agent does NOT ask the user for evidence; it recovers the exact evaluation reason and pulls the full chat history from this sub\n\
             session, calls `dispute upload` (which also auto-attaches the most recent 20 deliverables saved under\n\
             `~/.onchainos/deliverables/asp/{job_id}/`), and then notifies the user via\n\
             `onchainos agent user-notify`. **Do NOT** use `pending-decisions-v2 request` for this event.\n\
             **Do NOT** `okx-a2a session send` anything to the User Agent — both sides see the evaluation via on-chain events.\n\n\
             {task_fields}\n\
             **Step 1 — Recover the evaluation reason:**\n\
             Find the latest `[ARBITRATION_REASON_CONTEXT]` message in this task conversation whose `jobId` is `{job_id}`, `providerAgentId` is `{agent_id}`, `taskType` is `subscription`, and `resumeEvent` is `sub_asp_dispute`. Preserve its `reason` exactly.\n\
             The matching context is required for this evidence upload. When it is unavailable, return `arbitration_reason_context_missing` and end this turn.\n\n\
             **Step 2 — Pull this sub session's chat history** (use `buyerAgentId` from the **Task fields** block above):\n\n\
             ```bash\n\
             okx-a2a session history --job-id {job_id} --to-agent-id <buyerAgentId> --json\n\
             ```\n\n\
             **Step 3 — Format the evaluation reason and chat history as the `--text` body**:\n\n\
             ```\n\
             ==== ASP evaluation reason (from ARBITRATION_REASON_CONTEXT) ====\n\
             <exact reason>\n\
             ==== Negotiation / delivery chat history (from okx-a2a session history) ====\n\
             [time] User Agent(<agentId>): ...\n\
             [time] ASP(<agentId>): ...\n\
             ... (chronological; key checkpoints: subscription scope / deliverable messages / each side's key contention points)\n\
             ```\n\n\
             ⚠️ **`--text` is capped at 16 KB** — if the chat history is long, **keep only** the key checkpoints and prepend `(key checkpoints extracted)`; do NOT blindly drop the first N entries.\n\
             If history is genuinely empty, pass a minimal placeholder like `(no chat history available)` so `--text` is non-empty.\n\n\
             **Step 4 — Upload (off-chain multipart):**\n\
             ```bash\n\
             onchainos agent dispute upload {job_id} --role asp --agent-id {agent_id} --max-files 20 --text \"<evaluation reason + chat history block>\"\n\
             ```\n\
             The CLI auto-attaches the most recent 20 entries under `~/.onchainos/deliverables/asp/{job_id}/manifest.json` as multipart `files[]` parts — **do NOT pass `--file`**; the manifest covers the deliverable copies saved at delivery time. If the upload fails, retry up to 3 times; if it keeps failing, still proceed to Step 5 — the on-chain evaluation will continue with the available evidence.\n\n\
             **Step 5 — Notify the user (after upload returns):**\n\n\
             content:\n\
             \x20\x20\x20\x20[Evaluation opened] Subscription evaluation for job `{job_id}` is on-chain.\n\
             \x20\x20\x20\x20- Evaluation status: Evidence preparation\n\
             \x20\x20\x20\x20- Status description: Evidence was submitted and the evidence stage is in progress.\n\
             \x20\x20\x20\x20Awaiting the evaluator's verdict.\n\n\
             **Step 6 — End this turn.** Do NOT `okx-a2a session send` anything to the User Agent.\n\n\
             [Follow-up events]\n\
             - job_completed → won, funds released to the ASP\n\
             - dispute_resolved → lost, funds refunded to the User Agent\n"
            )
        }

        // sub_renew: the subscription renewed — the PREVIOUS period's income became
        // claimable (Subscribe API §2.9 aspClaim, bizType 107 SUB_ASP_CLAIM; the
        // backend's stated trigger is exactly this renewal notification). Claiming
        // moves the ASP's OWN funds — no counterparty decision — so route straight
        // to the deterministic claim command instead of silently dropping the event
        // (which left accrued income sitting unclaimed in the contract).
        Event::SubRenew => format!(
            "[System notification] sub_renew — the subscription renewed; the previous period's income is now claimable.\n\
             [Role] ASP (Agent Service ASP)\n\n\
             **Step 1 — Claim the accrued income (your own funds; run as-is):**\n\
             ```bash\n\
             onchainos agent subscribe-asp-claim {job_id} --agent-id {agent_id}\n\
             ```\n\
             CLI internals: POST /subscribe/{{subId}}/aspClaim (subId == jobId) → uopData → sign → broadcast. It claims everything outstanding for this subscription in one shot.\n\
             **Step 2 — Report:** on success push a short localized note via `onchainos agent user-notify --content \"<claim submitted, tx …>\"` — a background session's reply text never reaches the operator. If the CLI reports nothing claimable / already claimed, end the turn silently.\n\
             Do NOT `okx-a2a session send` anything to the User Agent — this involves no buyer action.\n"
        ),

        // sub_asp_agree is the ASP's OWN action (agree refund); the existing action-command
        // flow (subscribe-agree-refund) owns that lifecycle, not this notification path.
        Event::SubOpen => provider_assignment_playbook(
            job_id,
            agent_id,
            ProviderAssignmentType::Subscription,
            prefetched,
            message,
        )
        .await,

        Event::SubCreated
        | Event::SubCancel
        | Event::SubTrialIntoActive
        | Event::SubExpireWarn
        | Event::SubRejectRefundNotify
        | Event::SubAspAgree => format!(
            "[System notification] {event} (obsolete or not handled on the ASP side in this slice)\n\
             [Role] ASP (Agent Service ASP)\n\n\
             Silently ignore; end this turn.\n",
            event = event.as_str()
        ),

        Event::Other(ref other) => format!("[Unknown state] {other}\n"),
    }
}

// ── user_attachment_received helpers ────────────────────────────────

/// Render an ASP-side display notification: the localize-then-user-notify scaffold wrapping the
/// canonical English `content`, optionally followed by the terminal session-cleanup hint. Used by
/// display-only arms, which have no state transition or on-chain action.
fn display_notify(header: &str, content: &str, terminal_hint: Option<&str>) -> String {
    let tail = match terminal_hint {
        Some(h) => format!("\n{h}\n"),
        None => String::new(),
    };
    format!(
        "[System notification] {header}\n\
         [Role] ASP (Agent Service ASP)\n\n\
         **Notify the user, then end the turn** (🌐 **Localize first** — rewrite the content below in the user's language before sending; do NOT pass the English template verbatim to a non-English user. If the content still contains `<...>` placeholders such as `<title>`, fill them from the task context — `onchainos agent common context` — before sending; never send a literal placeholder):\n\
         ```bash\n\
         onchainos agent user-notify --content \"<localized content shown below>\"\n\
         ```\n\
         content:\n\
         {content}\n{tail}"
    )
}

/// Acceptance is not display-only: after notifying the ASP owner, hand the
/// active subscription into the existing service execution/skill flow.
fn sub_asp_accepted_start(
    header: &str,
    content: &str,
    task_fields: &str,
    job_id: &str,
    agent_id: &str,
) -> String {
    format!(
        "[System notification] {header}\n\
         [Role] ASP (Agent Service ASP)\n\n\
         {task_fields}\n\
         **Step 1 — Notify the ASP owner** (localize the fixed template first; fill any `<...>` value from the task context and never send a literal placeholder):\n\
         ```bash\n\
         onchainos agent user-notify --content \"<localized content shown below>\"\n\
         ```\n\
         content:\n\
         {content}\n\n\
         **Step 2 — Start service execution now.** Reuse the registered Service's existing AI/Skill workflow with the authoritative description, serviceParams, and attachments above. Do not re-run provider acceptance and do not send filler to the Buyer Agent.\n\n\
         - If this execution produces a deliverable now, hand it to the §1.6 delivery command for job `{job_id}` as ASP `{agent_id}`.\n\
         - If the Service is schedule/event driven, initialize its existing schedule/listener and then end the turn; do not invent an empty deliverable.\n"
    )
}

fn user_attachment_received_cli(
    job_id: &str,
    agent_id: &str,
    short_id: &str,
    message: Option<&serde_json::Value>,
) -> String {
    use crate::commands::agent_commerce::task::common::okx_a2a;
    use crate::commands::agent_commerce::task::user::attachments::{attachments_dir, dedup_dest};

    let msg_str = |key: &str| {
        message
            .and_then(|m| m.get(key))
            .and_then(|v| v.as_str())
            .unwrap_or("")
    };

    let file_key = msg_str("fileKey");
    let digest = msg_str("digest");
    let salt = msg_str("salt");
    let nonce = msg_str("nonce");
    let secret = msg_str("secret");
    let filename = message
        .and_then(|m| m.get("filename"))
        .and_then(|v| v.as_str());

    if file_key.is_empty()
        || digest.is_empty()
        || salt.is_empty()
        || nonce.is_empty()
        || secret.is_empty()
    {
        let mut missing = Vec::new();
        if file_key.is_empty() {
            missing.push("fileKey");
        }
        if digest.is_empty() {
            missing.push("digest");
        }
        if salt.is_empty() {
            missing.push("salt");
        }
        if nonce.is_empty() {
            missing.push("nonce");
        }
        if secret.is_empty() {
            missing.push("secret");
        }
        let fields = missing.join(", ");
        return format!(
            "[user_attachment_received_cli] ERROR: encryption metadata incomplete — missing: {fields}. \
             The caller must include all 6 fields (fileKey/digest/salt/nonce/secret/filename) in --message JSON.\n\n\
             [Your next action] Notify the user that the attachment could not be downloaded.\n\n\
             ```bash\n\
             onchainos agent user-notify --content \"<translate: [Job {short_id}] User Agent attachment download failed — encryption metadata incomplete. The User Agent may need to re-send.>\"\n\
             ```\n\n\
             ❌ Do NOT reply to the User Agent via okx-a2a session send.\n\
             **End this turn.**\n"
        );
    }

    let local_path = match okx_a2a::file_download(
        file_key, agent_id, digest, salt, nonce, secret, filename,
    ) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[user_attachment_received_cli] download failed: {e}");
            return format!(
                "[user_attachment_received_cli] ERROR: file download failed: {e}\n\n\
                 [Your next action] Notify the user that the attachment could not be downloaded.\n\n\
                 ```bash\n\
                 onchainos agent user-notify --content \"<translate: [Job {short_id}] User Agent attachment download failed. Please check network and retry.>\"\n\
                 ```\n\n\
                 ❌ Do NOT reply to the User Agent via okx-a2a session send.\n\
                 **End this turn.**\n"
            );
        }
    };

    let save_path = match (|| -> Result<String, String> {
        let src = std::path::Path::new(&local_path);
        if !src.exists() {
            return Err(format!("downloaded file not found: {local_path}"));
        }
        let dir = attachments_dir(job_id).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir failed: {e}"))?;
        let file_name = src
            .file_name()
            .ok_or_else(|| format!("invalid file path: {local_path}"))?;
        let dest = dedup_dest(&dir, file_name);
        if std::fs::rename(src, &dest).is_err() {
            std::fs::copy(src, &dest).map_err(|e| format!("copy failed: {e}"))?;
            let _ = std::fs::remove_file(src);
        }
        Ok(dest.display().to_string())
    })() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[user_attachment_received_cli] save-to-job-dir failed: {e}");
            local_path
        }
    };

    let att_notify = super::content::user_attachment_received_user_notify(job_id);
    let _ = short_id;
    format!(
        "[user_attachment_received_cli] ✓ Attachment downloaded and saved: {save_path}\n\n\
         [Your next action] Translate the notification below to the user's language, then dispatch it. End the turn after notifying.\n\n\
         Canonical content:\n\
         \x20\x20{att_notify}\n\n\
         ```bash\n\
         onchainos agent user-notify --content \"<your translated content>\"\n\
         ```\n\n\
         ❌ Do NOT reply to the User Agent via okx-a2a session send.\n\
         **End this turn.**\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── reject_expire_time extraction (FR-4) ─────────────────────────────

    #[test]
    fn reject_expire_time_extracts_positive() {
        let now = chrono::Local::now().timestamp();
        let msg = json!({ "event": "job_rejected", "expireTime": now + 86_400 });
        assert_eq!(reject_expire_time(Some(&msg)), Some(now + 86_400));
    }

    #[test]
    fn reject_expire_time_absent_is_none() {
        let msg = json!({ "event": "job_rejected" });
        assert_eq!(reject_expire_time(Some(&msg)), None);
        assert_eq!(reject_expire_time(None), None);
    }

    #[test]
    fn reject_expire_time_nonpositive_is_none() {
        let msg = json!({ "expireTime": 0 });
        assert_eq!(reject_expire_time(Some(&msg)), None);
        let msg = json!({ "expireTime": -1 });
        assert_eq!(reject_expire_time(Some(&msg)), None);
    }

    #[test]
    fn refund_list_metadata_uses_prefetched_service_name() {
        let prefetched =
            crate::commands::agent_commerce::task::common::PreFetchedTaskContext::from_api_response(
                &json!({
                    "title": "Task",
                    "jobType": 0,
                    "serviceName": "Audit Service",
                    "tokenAmount": "2.5",
                    "tokenSymbol": "USDT",
                    "expireTime": 2_000_000_000i64,
                    "refundReason": "Delivery did not match the request",
                }),
            );
        let output = arbitration_decision_json(
            crate::commands::agent_commerce::task::arbitration::JOB_REJECTED,
            "job-1",
            None,
            Some(&prefetched),
            Some(&json!({"eventId": "event-1"})),
        );
        let output: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(output["payload"]["serviceName"], "Audit Service");
        assert!(output["payload"]["refundDisplayB64"].is_string());
    }

    const ASP_JOB_ID: &str = "0xsub01";
    const ASP_AGENT_ID: &str = "864";

    async fn run_asp(event: &str, msg: serde_json::Value) -> String {
        generate_next_action(
            ASP_JOB_ID,
            event,
            ASP_AGENT_ID,
            Some("My Sub"),
            None,
            None,
            Some(&msg),
        )
        .await
    }

    fn notification_task(
        title: &str,
        job_type: i64,
        token_amount: &str,
        token_symbol: &str,
        status: i64,
    ) -> crate::commands::agent_commerce::task::common::PreFetchedTaskContext {
        let mut context =
            crate::commands::agent_commerce::task::common::PreFetchedTaskContext::from_api_response(
                &json!({
                    "title": title,
                    "jobType": job_type,
                    "paymentTokenAmount": token_amount,
                    "tokenSymbol": token_symbol,
                    "status": status,
                    "providerAgentId": ASP_AGENT_ID,
                }),
            );
        if job_type == 1 {
            context.trial_type = Some(0);
        }
        context
    }

    async fn run_asp_with_task(
        event: &str,
        msg: serde_json::Value,
        task: &crate::commands::agent_commerce::task::common::PreFetchedTaskContext,
    ) -> String {
        generate_next_action(
            ASP_JOB_ID,
            event,
            ASP_AGENT_ID,
            Some("caller title must not be used"),
            None,
            Some(task),
            Some(&msg),
        )
        .await
    }

    #[tokio::test]
    async fn free_one_time_job_rejected_routes_to_terminal_failed_not_arbitration() {
        let task = notification_task("Daily forecast", 0, "0", "", 9);
        let output = run_asp_with_task(
            "job_rejected",
            json!({
                "event": "job_rejected",
                "jobId": ASP_JOB_ID,
                "reason": "The result did not meet my requirements"
            }),
            &task,
        )
        .await;
        let progression: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(progression["payload"]["statusLabel"], "Failed");
        assert_eq!(
            progression["nextAction"][0]["id"],
            "notify_and_cleanup_subscription"
        );
        assert_eq!(progression["payload"]["cleanup"]["jobId"], ASP_JOB_ID);
        assert!(!output.contains("pending-decisions-v2 request-prompt"));
        assert!(!output.contains("raise_arbitration"));
    }

    #[tokio::test]
    async fn one_time_dispute_approved_is_a_write_free_compatibility_receipt() {
        let task = notification_task("One-time work", 0, "1", "USDT", 3);
        let output = run_asp_with_task(
            "dispute_approved",
            json!({"event": "dispute_approved"}),
            &task,
        )
        .await;
        assert!(output.contains("compatibility receipt"));
        assert!(!output.contains("onchainos agent dispute confirm"));
    }

    #[tokio::test]
    async fn dispute_approved_is_a_write_free_compatibility_receipt() {
        let task = notification_task("Subscription", 1, "1", "USDT", 4);
        let output = run_asp_with_task(
            "dispute_approved",
            json!({"event": "dispute_approved"}),
            &task,
        )
        .await;
        assert!(output.contains("compatibility receipt"));
        assert!(output.contains("approveAndCreateDispute"));
        assert!(!output.contains("onchainos agent dispute confirm"));
    }

    #[tokio::test]
    async fn dispute_approved_does_not_require_job_type() {
        let output = run_asp("dispute_approved", json!({"event": "dispute_approved"})).await;
        assert!(output.contains("compatibility receipt"));
        assert!(!output.contains("onchainos agent dispute confirm"));
    }

    #[tokio::test]
    async fn job_submitted_notifies_only_the_asp_owner() {
        let output = run_asp("job_submitted", json!({"event":"job_submitted"})).await;
        assert!(output.contains("onchainos agent user-notify"));
        assert!(output.contains("Waiting for the User Agent's review"));
        assert!(output.contains("must NOT trigger a second A2A send"));
        assert!(output.contains("Wait for `job_completed` / `job_rejected`"));
    }

    #[tokio::test]
    async fn one_time_job_disputed_uses_handed_off_reason_in_evidence() {
        let task = notification_task("One-time work", 0, "1", "USDT", 4);
        let approved = run_asp_with_task(
            "dispute_approved",
            json!({"event":"dispute_approved", "code":0}),
            &task,
        )
        .await;
        assert!(approved.contains("compatibility receipt"));
        assert!(!approved.contains("onchainos agent dispute confirm"));

        let disputed = run_asp_with_task(
            "job_disputed",
            json!({"event":"job_disputed", "buyerAgentId":"8315"}),
            &task,
        )
        .await;
        assert!(disputed.contains("job_disputed"));
        assert!(disputed.contains("[ARBITRATION_REASON_CONTEXT]"));
        assert!(disputed.contains("taskType` is `one_time`"));
        assert!(disputed.contains("resumeEvent` is `job_disputed`"));
        assert!(disputed.contains("arbitration_reason_context_missing"));
        assert!(disputed.contains("<evaluation reason + chat history block>"));
        assert!(disputed.contains("evidence"));
        assert!(disputed.contains("onchainos agent dispute upload"));
    }

    #[tokio::test]
    async fn sub_asp_dispute_uses_handed_off_reason_in_evidence() {
        let task =
            crate::commands::agent_commerce::task::common::PreFetchedTaskContext::from_api_response(
                &json!({
                    "title": "Subscription work",
                    "jobType": 1,
                    "subStatus": 4,
                    "providerAgentId": ASP_AGENT_ID,
                    "buyerAgentId": "buyer-1",
                }),
            );
        let output = run_asp_with_task(
            "sub_asp_dispute",
            json!({"event": "sub_asp_dispute"}),
            &task,
        )
        .await;

        assert!(output.contains("[ARBITRATION_REASON_CONTEXT]"));
        assert!(output.contains("taskType` is `subscription`"));
        assert!(output.contains("resumeEvent` is `sub_asp_dispute`"));
        assert!(output.contains("arbitration_reason_context_missing"));
        assert!(output.contains("<evaluation reason + chat history block>"));
        assert!(!output.contains("onchainos agent dispute confirm"));
    }

    #[tokio::test]
    async fn subscription_job_notifications_render_asp_copy() {
        let spoofed = json!({
            "jobId": ASP_JOB_ID,
            "jobTitle": "Forged title",
            "tokenAmount": "999",
            "tokenSymbol": "FAKE",
            "jobType": 0
        });

        let accept_task = notification_task("BTC Signals", 1, "12.34", "USDT", 8);
        let mut accept_expire = spoofed.clone();
        accept_expire["event"] = json!("job_asp_accept_expire");
        let out = run_asp_with_task("job_asp_accept_expire", accept_expire, &accept_task).await;
        let progression: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(out.contains("[Assignment Expired] You did not accept BTC Signals"));
        assert!(out.contains("12.34 USDT"));
        assert!(out.contains("funds have reached the Buyer"));
        assert!(out.contains("No client-side claim is required"));
        assert!(out.contains("Job status: Expired (8)"));
        assert!(!out.contains("Forged title"));
        assert!(!out.contains("999 FAKE"));
        assert_eq!(
            progression["nextAction"][0]["id"],
            "notify_and_cleanup_subscription"
        );
        assert_eq!(progression["payload"]["cleanup"]["jobId"], ASP_JOB_ID);
        assert_eq!(progression["payload"]["role"], "asp");

        let reject_closed_task = notification_task("BTC Signals", 1, "12.34", "USDT", 7);
        let mut reject_closed = spoofed.clone();
        reject_closed["event"] = json!("job_asp_reject_closed");
        reject_closed["aspRejectReason"] = json!("capacity unavailable");
        let out =
            run_asp_with_task("job_asp_reject_closed", reject_closed, &reject_closed_task).await;
        assert!(out.contains("[Task Declined] You have declined BTC Signals."));
        assert!(out.contains("Reason: capacity unavailable"));
        assert!(!out.contains("Forged title"));

        let reject_expire_task = notification_task("BTC Signals", 1, "12.34", "USDT", 9);
        let mut reject_expire = spoofed.clone();
        reject_expire["event"] = json!("job_asp_reject_expire");
        let out =
            run_asp_with_task("job_asp_reject_expire", reject_expire, &reject_expire_task).await;
        assert!(out.contains("[Refund Result Unverified]"));
        assert!(out.contains("does not prove that 12.34 USDT was refunded"));
        assert!(out.contains("Job status: Failed (9)"));
        assert!(out.contains("Verify the authoritative settlement result"));
        assert!(!out.contains("[Automatic Refund Completed]"));
        assert!(!out.contains("Job status: Closed"));
        assert!(!out.contains("Job status: Expired"));
        assert!(!out.contains("is pending"));
        assert!(!out.contains("999 FAKE"));

        let mut claim_notify = json!({
            "jobId": ASP_JOB_ID,
            "jobTitle": "BTC Signals",
            "tokenAmount": "12.34",
            "tokenSymbol": "USDT",
            "jobType": 1
        });
        claim_notify["event"] = json!("sub_asp_claim_notify");
        claim_notify["txHash"] = json!("0xreceive");
        let out = run_asp("sub_asp_claim_notify", claim_notify).await;
        let progression: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(out.contains("[Income Collected]"));
        assert!(!out.contains("Subscription income collected\n"));
        assert!(out.contains("12.34 USDT for BTC Signals"));
        assert!(out.contains("Transaction: 0xreceive"));
        assert_eq!(progression["nextAction"][0]["id"], "notify_user");
        assert_eq!(progression["payload"]["event"], "sub_asp_claim_notify");
    }

    #[tokio::test]
    async fn ordinary_job_notifications_split_free_and_paid_copy() {
        let spoofed = json!({
            "jobId": ASP_JOB_ID,
            "jobTitle": "Forged subscription",
            "tokenAmount": "999",
            "tokenSymbol": "FAKE",
            "jobType": 1
        });

        let free_task = notification_task("One-off analysis", 0, "0", "USDT", 8);
        let mut free = spoofed.clone();
        free["event"] = json!("job_asp_accept_expire");
        let out = run_asp_with_task("job_asp_accept_expire", free, &free_task).await;
        assert!(out.contains("[Assignment Expired]"));
        assert!(out.contains("No refundable funds were collected"));
        assert!(!out.contains("funds have reached the Buyer"));
        let progression: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            progression["nextAction"][0]["id"],
            "notify_and_cleanup_subscription"
        );
        assert_eq!(progression["payload"]["cleanup"]["jobId"], ASP_JOB_ID);

        let paid_task = notification_task("One-off analysis", 0, "5", "USDT", 8);
        let mut paid = spoofed.clone();
        paid["event"] = json!("job_asp_accept_expire");
        let out = run_asp_with_task("job_asp_accept_expire", paid, &paid_task).await;
        assert!(out.contains("Escrowed amount: 5 USDT"));
        assert!(out.contains("funds have reached the Buyer"));
        assert!(out.contains("No client-side claim is required"));
        assert!(!out.contains("999 FAKE"));
        let progression: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            progression["nextAction"][0]["id"],
            "notify_and_cleanup_subscription"
        );
        assert_eq!(progression["payload"]["cleanup"]["jobId"], ASP_JOB_ID);

        for event in ["job_expired", "submit_expired"] {
            let out = run_asp_with_task(
                event,
                json!({"event": event, "jobId": ASP_JOB_ID}),
                &paid_task,
            )
            .await;
            let progression: serde_json::Value = serde_json::from_str(&out).unwrap();
            assert!(out.contains("[Delivery Expired]"));
            assert!(out.contains("funds have reached the Buyer"));
            assert_eq!(
                progression["nextAction"][0]["id"],
                "notify_and_cleanup_subscription"
            );
            assert_eq!(progression["payload"]["cleanup"]["jobId"], ASP_JOB_ID);
        }

        let declined_task = notification_task("One-off analysis", 0, "0", "USDT", 7);
        let mut declined = spoofed.clone();
        declined["event"] = json!("job_asp_reject_closed");
        declined["aspRejectReason"] = json!("policy");
        let out = run_asp_with_task("job_asp_reject_closed", declined, &declined_task).await;
        assert!(out.contains("[Job Declined]"));
        assert!(out.contains("Job status: Closed"));

        let free_refund_task = notification_task("One-off analysis", 0, "0.000", "USDT", 9);
        let mut free_refund = spoofed;
        free_refund["event"] = json!("job_asp_reject_expire");
        let out = run_asp_with_task("job_asp_reject_expire", free_refund, &free_refund_task).await;
        assert!(out.contains("[Refund Response Expired]"));
        assert!(out.contains("Job status: Failed (9)"));
        assert!(out.contains("No further service delivery or refund response is required."));
        assert!(!out.contains("Job status: Expired"));
        assert!(!out.contains("Job status: Closed"));
    }

    #[tokio::test]
    async fn provider_assignment_duplicate_accept_is_idempotent() {
        let single =
            crate::commands::agent_commerce::task::common::PreFetchedTaskContext::from_api_response(
                &json!({"status": 1}),
            );
        let output = provider_assignment_playbook(
            ASP_JOB_ID,
            ASP_AGENT_ID,
            ProviderAssignmentType::Single,
            Some(&single),
            None,
        )
        .await;
        assert!(output.contains("duplicate trigger"));
        assert!(output.contains("Do NOT repeat the mutation or broadcast"));
        assert!(!output.contains("accept-job-by-provider 0xsub01"));
    }

    #[tokio::test]
    async fn sub_open_starts_provider_decision() {
        let output = run_asp(
            "sub_open",
            json!({ "event": "sub_open", "jobId": ASP_JOB_ID }),
        )
        .await;
        assert!(output.contains("[Current state] sub_open"));
        assert!(output.contains("Latest task detail could not be fetched"));
        assert!(!output.contains("obsolete"));
    }

    #[tokio::test]
    async fn provider_assignment_requires_fresh_detail() {
        let output = provider_assignment_playbook(
            ASP_JOB_ID,
            ASP_AGENT_ID,
            ProviderAssignmentType::Single,
            None,
            None,
        )
        .await;
        assert!(output.contains("could not be fetched"));
        assert!(output.contains("do NOT accept, decline"));

        let subscription = provider_assignment_playbook(
            ASP_JOB_ID,
            ASP_AGENT_ID,
            ProviderAssignmentType::Subscription,
            None,
            None,
        )
        .await;
        assert!(subscription.contains("[Current state] sub_open"));
        assert!(!subscription.contains("[Current state] sub_created"));
    }

    #[test]
    fn provider_need_params_uses_peer_transport() {
        let command = task_params_request_command(ASP_JOB_ID, "buyer-1", "single");
        assert!(command.contains("[intent:task_params_request]"));
        assert!(command.starts_with("okx-a2a xmtp-send"));
        assert!(!command.contains("okx-a2a session send"));
    }

    #[tokio::test]
    async fn asp_handled_subscription_events_render_notify() {
        for evt in ["sub_asp_selected", "sub_close_notify", "sub_failed_notify"] {
            let out = run_asp(evt, json!({ "event": evt, "jobId": ASP_JOB_ID })).await;
            assert!(
                out.contains("onchainos agent user-notify"),
                "{evt}: ASP must render a notify body"
            );
            assert!(
                !out.contains("pending-decisions"),
                "{evt}: display-only — no pending-decisions"
            );
        }
    }

    #[tokio::test]
    async fn asp_terminal_subscription_events_carry_cleanup_hint() {
        for evt in ["sub_close_notify"] {
            let out = run_asp(evt, json!({ "event": evt, "jobId": ASP_JOB_ID })).await;
            assert!(
                out.contains("session-cleanup"),
                "{evt}: terminal ASP event must append the cleanup hint"
            );
        }
        // sub_asp_selected starts service execution and is non-terminal → no cleanup hint.
        for evt in ["sub_asp_selected"] {
            let out = run_asp(evt, json!({ "event": evt, "jobId": ASP_JOB_ID })).await;
            assert!(
                !out.contains("session-cleanup"),
                "{evt}: non-terminal ASP event must NOT append the cleanup hint"
            );
        }
        let unverified = run_asp(
            "sub_failed_notify",
            json!({"event": "sub_failed_notify", "jobId": ASP_JOB_ID}),
        )
        .await;
        assert!(
            unverified.contains("result cause unverified"),
            "{unverified}"
        );
        assert!(!unverified.contains("session-cleanup"), "{unverified}");
    }

    #[tokio::test]
    async fn asp_sub_complete_routes_structured_progression() {
        let out = run_asp(
            "sub_complete_notify",
            json!({ "event": "sub_complete_notify", "jobId": ASP_JOB_ID }),
        )
        .await;
        let progression: serde_json::Value = serde_json::from_str(&out).unwrap();

        assert_eq!(progression["decision"], "ready");
        assert_eq!(
            progression["nextAction"][0]["id"],
            "notify_and_cleanup_subscription"
        );
        assert_eq!(progression["payload"]["cleanup"]["jobId"], ASP_JOB_ID);
    }

    #[tokio::test]
    async fn asp_selected_renders_terms_verbatim() {
        let out = run_asp(
            "sub_asp_selected",
            json!({ "event": "sub_asp_selected", "jobId": ASP_JOB_ID, "tokenSymbol": "USDT", "tokenAmount": "5.5" }),
        )
        .await;
        assert!(out.contains("5.5 USDT"), "ASP terms echoed verbatim: {out}");
        assert!(
            out.contains("Start service execution now"),
            "accepted subscription must enter the existing service workflow: {out}"
        );
        assert!(
            out.contains("§1.6 delivery command"),
            "immediate output must hand off to delivery: {out}"
        );
        assert!(!out.contains("Notify the user, then end the turn"));
    }

    #[tokio::test]
    async fn asp_job_accepted_notifies_then_executes_and_delivers() {
        let out = run_asp(
            "job_accepted",
            json!({ "event": "job_accepted", "jobId": ASP_JOB_ID }),
        )
        .await;
        assert!(out.contains("your provider acceptance is confirmed"));
        assert!(out.contains("Notify the ASP owner"));
        assert!(out.contains("registered Service's existing AI/Skill workflow"));
        assert!(out.contains("serviceId"));
        assert!(out.contains("okx-a2a xmtp-send"));
        assert!(!out.contains("okx-a2a session send"));
        assert!(out.contains("onchainos agent deliver"));
        assert!(!out.contains("--file \"\""));
        assert!(out.contains("exactly one delivery-input flag"));
        assert!(!out.contains("User Agent has confirmed the apply"));
    }

    #[tokio::test]
    async fn asp_subscription_startup_falls_back_to_authoritative_detail() {
        let prefetched =
            crate::commands::agent_commerce::task::common::PreFetchedTaskContext::from_api_response(
                &json!({
                    "title": "Authoritative Service",
                    "description": "Service description",
                    "buyerAgentId": "buyer-9",
                    "tokenAmount": "7.5",
                    "tokenSymbol": "USDT",
                    "serviceId": "service-1",
                    "serviceParams": "{\"symbol\":\"SOL\"}",
                    "subStatus": 1
                }),
            );
        let out = generate_next_action(
            ASP_JOB_ID,
            "sub_asp_selected",
            ASP_AGENT_ID,
            None,
            None,
            Some(&prefetched),
            Some(&json!({"event": "sub_asp_selected"})),
        )
        .await;
        assert!(out.contains("Authoritative Service"));
        assert!(out.contains("Buyer: buyer-9"));
        assert!(out.contains("7.5 USDT"));
        assert!(out.contains("serviceId: service-1"));
        assert!(out.contains("Start service execution now"));
    }

    #[tokio::test]
    async fn asp_terminal_events_render_product_copy_verbatim() {
        let out = run_asp(
            "sub_complete_notify",
            json!({ "event": "sub_complete_notify", "jobId": ASP_JOB_ID, "jobTitle": "AlphaBot", "subEndTime": 1786547115 }),
        )
        .await;
        let progression: serde_json::Value = serde_json::from_str(&out).unwrap();
        let content = progression["payload"]["notification"]["content"]
            .as_str()
            .unwrap();
        assert!(
            content.contains("[Subscription Complete]"),
            "ASP-9 label: {out}"
        );
        assert!(
            content.contains("\"AlphaBot\""),
            "ASP-9 service name quoted: {out}"
        );
        assert!(
            content.contains("no further delivery is required"),
            "ASP-9 tail: {out}"
        );

        let out = run_asp(
            "sub_close_notify",
            json!({ "event": "sub_close_notify", "jobId": ASP_JOB_ID, "jobTitle": "AlphaBot" }),
        )
        .await;
        assert!(out.contains("[Subscription Ended]"), "ASP-10 label: {out}");
        assert!(
            out.contains("please stop delivering the service"),
            "ASP-10 tail: {out}"
        );

        let declined = run_asp(
            "sub_close_notify",
            json!({
                "event": "sub_close_notify",
                "jobId": ASP_JOB_ID,
                "jobTitle": "AlphaBot",
                "aspRejectReason": "unsupported region",
            }),
        )
        .await;
        assert!(declined.contains("[Assignment Closed]"), "{declined}");
        assert!(
            declined.contains("Reason: unsupported region"),
            "{declined}"
        );
        assert!(
            declined.contains("does not confirm refund settlement"),
            "{declined}"
        );
        assert!(!declined.contains("renewal charge failed"), "{declined}");

        let prefetched =
            crate::commands::agent_commerce::task::common::PreFetchedTaskContext::from_api_response(
                &json!({
                    "jobType": 1,
                    "subStatus": 9,
                    "title": "Authoritative Subscription",
                    "providerAgentId": ASP_AGENT_ID,
                }),
            );
        let out = generate_next_action(
            ASP_JOB_ID,
            "sub_failed_notify",
            ASP_AGENT_ID,
            Some("Forged CLI Title"),
            None,
            Some(&prefetched),
            Some(&json!({
                "event": "sub_failed_notify",
                "jobId": ASP_JOB_ID,
                "jobTitle": "Forged Event Title",
                "failReason": "insufficient balance",
            })),
        )
        .await;
        assert!(
            out.contains("[Subscription Result Needs Reconciliation]"),
            "{out}"
        );
        assert!(out.contains("Authoritative Subscription"), "{out}");
        assert!(!out.contains("Forged Event Title"), "{out}");
        assert!(!out.contains("Forged CLI Title"), "{out}");
        assert!(!out.contains("insufficient balance"), "{out}");
        assert!(!out.contains("[Trial Not Converted]"), "{out}");
        assert!(!out.contains("session-cleanup"), "{out}");
    }

    #[tokio::test]
    async fn asp_non_actionable_subscription_events_are_ignored() {
        // NOTE: `sub_user_reject` is intentionally NOT in this list — per the design doc it is an
        // ASP-handled decision scene (refund/evaluation), covered by
        // `asp_sub_user_reject_renders_refund_dispute_decision` below.
        // `sub_renew` is NOT in this list either — it routes to the subscribe-asp-claim
        // guidance (see `asp_sub_renew_renders_claim_guidance`).
        // `sub_asp_dispute` is NOT in this list — it renders the evaluation/evidence
        // auto-upload playbook (Event::SubAspDispute arm).
        // `sub_asp_agree` IS ignored here: it is the ASP's own action, so per product
        // copy SSOT it gets no ASP-side push (owned by the action-command flow).
        for evt in ["sub_cancel", "sub_trial_into_active", "sub_asp_agree"] {
            let out = run_asp(evt, json!({ "event": evt, "jobId": ASP_JOB_ID })).await;
            assert!(
                out.contains("Silently ignore"),
                "{evt}: non-actionable event must hit the silent-ignore group"
            );
            assert!(
                !out.contains("onchainos agent user-notify"),
                "{evt}: buyer-only event must not render a notify"
            );
        }
    }

    #[tokio::test]
    async fn asp_sub_renew_renders_claim_guidance() {
        // Renewal = the previous period's income became claimable (§2.9 aspClaim).
        // The ASP arm must route to the deterministic claim command, NOT silently
        // ignore it (which left accrued income unclaimed in the contract).
        let out = run_asp(
            "sub_renew",
            json!({ "event": "sub_renew", "jobId": ASP_JOB_ID }),
        )
        .await;
        assert!(
            out.contains("subscribe-asp-claim"),
            "sub_renew must guide the ASP to claim: {out}"
        );
        assert!(
            out.contains(ASP_JOB_ID) && out.contains(ASP_AGENT_ID),
            "got: {out}"
        );
        assert!(!out.contains("Silently ignore"), "got: {out}");
        // No buyer involvement: never instruct an XMTP send toward the User Agent.
        assert!(out.contains("Do NOT `okx-a2a session send`"), "got: {out}");
    }

    #[tokio::test]
    async fn asp_sub_user_reject_renders_refund_arbitration_decision() {
        let out = run_asp(
            "sub_user_reject",
            json!({
                "event": "sub_user_reject", "jobId": ASP_JOB_ID, "jobTitle": "My Sub",
                "subStartTime": 1_700_000_000, "subEndTime": 1_700_500_000,
                "rejectWindowEndsAt": 1_700_600_000,
                "tokenAmount": "0.0005", "tokenSymbol": "USDT",
                "refundReason": "Delivery did not match the request"
            }),
        )
        .await;
        assert!(out.contains("pending-decisions-v2 request-prompt"), "{out}");
        assert!(out.contains("=== BEGIN TEMPLATE 6.4 SOURCE ==="), "{out}");
        assert!(
            out.contains("- Service Name: {{__OKX_REFUND_SERVICE_NAME__}}"),
            "{out}"
        );
        assert!(
            out.contains("- Current Period: {{__OKX_REFUND_CURRENT_PERIOD__}}"),
            "{out}"
        );
        assert!(!out.contains("| Service Name |"), "{out}");
        assert!(
            out.contains("--decision-id \"0xsub01:sub_user_reject:"),
            "{out}"
        );
        assert!(out.contains("--choices-json"), "{out}");
        assert!(out.contains("--expires-at 1700600000"), "{out}");
        assert!(out.contains("--refund-display-b64"), "{out}");
        assert!(out.contains("--template-vars-b64"), "{out}");
        let vars =
            crate::commands::agent_commerce::task::common::template_vars::decode_emitted_vars(&out);
        assert_eq!(vars["__OKX_REFUND_SERVICE_NAME__"], "My Sub");
        assert_eq!(vars["__OKX_REFUND_JOB_ID__"], ASP_JOB_ID);
        assert_eq!(vars["__OKX_REFUND_TASK_TYPE__"], "Subscription");
        assert_eq!(vars["__OKX_REFUND_AMOUNT__"], "0.0005 USDT");
        assert_eq!(
            vars["__OKX_REFUND_BUYER_REASON__"],
            "Delivery did not match the request"
        );
        assert!(vars.contains_key("__OKX_REFUND_CURRENT_PERIOD__"));

        let degraded = run_asp(
            "sub_user_reject",
            json!({ "event": "sub_user_reject", "jobId": ASP_JOB_ID, "jobTitle": "My Sub" }),
        )
        .await;
        let degraded: serde_json::Value = serde_json::from_str(&degraded).unwrap();
        assert_eq!(degraded["decision"], "blocked");
        assert_eq!(degraded["reason"], "missing_required_facts");
        assert_eq!(degraded["nextAction"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn asp_job_rejected_pushes_one_time_template_6_4_without_current_period() {
        let out = run_asp(
            "job_rejected",
            json!({
                "event": "job_rejected",
                "jobId": ASP_JOB_ID,
                "jobTitle": "One-time service",
                "expireTime": 1_700_600_000,
                "tokenAmount": "1.25",
                "tokenSymbol": "USDT",
                "refundReason": "Delivery did not match the request"
            }),
        )
        .await;

        assert!(out.contains("pending-decisions-v2 request-prompt"), "{out}");
        assert!(out.contains("=== BEGIN TEMPLATE 6.4 SOURCE ==="), "{out}");
        assert!(
            out.contains("- Service Name: {{__OKX_REFUND_SERVICE_NAME__}}"),
            "{out}"
        );
        assert!(!out.contains("- Current Period:"), "{out}");
        assert!(!out.contains("| Service Name |"), "{out}");
        assert!(out.contains("do not add a third confirmation"), "{out}");
        let vars =
            crate::commands::agent_commerce::task::common::template_vars::decode_emitted_vars(&out);
        assert_eq!(vars["__OKX_REFUND_TASK_TYPE__"], "One-time");
        assert!(!vars.contains_key("__OKX_REFUND_CURRENT_PERIOD__"));
    }

    /// A hostile title payload. In zsh, `${(e)}`
    /// forces eval and `${(#):-96}` yields a backtick, so this reconstructs and
    /// runs `id>&2` IF any byte of it ever reaches a zsh command line. The whole
    /// point of the hotfix is that it never does — it travels only inside the
    /// shell-safe Base64 `--template-vars-b64` payload.
    const HOSTILE_ZSH_TITLE: &str = "x${(e):-${(#):-96}id>&2${(#):-96}}";

    // Exercise the ACTUAL `Event::SubUserReject` renderer (not a hand-assembled
    // command) with the hostile zsh payload, through the production title
    // extraction path. This closes the composition gap the integration test flags:
    // the real renderer must emit the placeholder-carrying `request-prompt` with a
    // single shell-safe Base64 payload. Neither the raw attacker title nor the
    // buyer-authored reason may appear in emitted shell source; both must decode
    // back byte-for-byte only inside `request-prompt`.
    #[tokio::test]
    async fn asp_sub_user_reject_hostile_payload_stays_out_of_shell() {
        // Production (`agent_commerce/mod.rs`) reads ONLY `message.jobTitle` as the
        // 4th arg (title_ref); mirror that so this is a real production combination.
        let msg = json!({
            "event": "sub_user_reject",
            "jobId": ASP_JOB_ID,
            "jobTitle": HOSTILE_ZSH_TITLE,
            "subStartTime": 1_700_000_000,
            "subEndTime": 1_700_500_000,
            "rejectWindowEndsAt": 1_700_600_000,
            "tokenAmount": "0.0005",
            "tokenSymbol": "USDT",
            "refundReason": "$(touch /tmp/asp-refund-reason-must-not-run)"
        });
        let title_ref = production_title_ref(&msg);
        let out = generate_next_action(
            ASP_JOB_ID,
            "sub_user_reject",
            ASP_AGENT_ID,
            title_ref,
            None,
            None,
            Some(&msg),
        )
        .await;

        assert!(out.contains("pending-decisions-v2 request-prompt"));
        assert!(out.contains("--template-vars-b64"));
        assert!(!out.contains(HOSTILE_ZSH_TITLE));
        assert!(!out.contains("$(touch /tmp/asp-refund-reason-must-not-run)"));
        let vars =
            crate::commands::agent_commerce::task::common::template_vars::decode_emitted_vars(&out);
        assert_eq!(vars["__OKX_REFUND_SERVICE_NAME__"], HOSTILE_ZSH_TITLE);
        assert_eq!(
            vars["__OKX_REFUND_BUYER_REASON__"],
            "$(touch /tmp/asp-refund-reason-must-not-run)"
        );
    }

    // Reproduce the EXACT production title extraction from `agent_commerce/mod.rs`
    // so the precedence cases below are the ones the real caller can actually
    // produce (title precedence tests must mirror the production
    // caller"). Production reads ONLY `message.jobTitle`:
    //   - `mod.rs` `let job_title = msg_str("jobTitle");` — this layer's `msg_str`
    //     does NOT filter empty strings, so `jobTitle: ""` becomes `Some("")`;
    //   - `mod.rs` `let title_ref = job_title.as_deref();` is the 4th arg passed to
    //     `generate_next_action`;
    //   - `flow.rs` `let title_display = job_title.unwrap_or("<title>");`.
    // So `title_display` is a pure function of `message.jobTitle`; a test that sets
    // `title_display` independently of `jobTitle` is an impossible production
    // combination. This helper returns exactly what production passes as the 4th
    // arg (the `jobTitle` value, borrow-checked against `message`).
    fn production_title_ref(message: &serde_json::Value) -> Option<&str> {
        message.get("jobTitle").and_then(|v| v.as_str())
    }

    #[tokio::test]
    async fn sub_user_reject_service_name_falls_back_to_task_title() {
        let msg = json!({
            "event": "sub_user_reject",
            "jobId": ASP_JOB_ID,
            "title": "Fallback task title",
            "subStartTime": 1_700_000_000,
            "subEndTime": 1_700_500_000,
            "rejectWindowEndsAt": 1_700_600_000,
            "tokenAmount": "0.0005",
            "tokenSymbol": "USDT",
            "refundReason": "Delivery did not match the request"
        });
        let out = generate_next_action(
            ASP_JOB_ID,
            "sub_user_reject",
            ASP_AGENT_ID,
            production_title_ref(&msg),
            None,
            None,
            Some(&msg),
        )
        .await;

        assert!(out.contains("pending-decisions-v2 request-prompt"), "{out}");
        let vars =
            crate::commands::agent_commerce::task::common::template_vars::decode_emitted_vars(&out);
        assert_eq!(vars["__OKX_REFUND_SERVICE_NAME__"], "Fallback task title");
    }

    #[tokio::test]
    async fn direct_arbitration_pseudo_events_require_bound_decision_metadata() {
        for event in [
            "raise_arbitration",
            "dispute_raise",
            "agree_refund",
            "raise_subscription_arbitration",
            "sub_dispute",
            "sub_agree_refund",
        ] {
            let out = run_asp(event, json!({ "event": event, "jobId": ASP_JOB_ID })).await;
            let result: serde_json::Value = serde_json::from_str(&out).unwrap();
            assert_eq!(result["decision"], "blocked", "{event}: {out}");
            assert_eq!(
                result["reason"], "decision_metadata_missing",
                "{event}: {out}"
            );
            assert_eq!(result["nextAction"], json!([]), "{event}: {out}");
        }
    }

    #[tokio::test]
    async fn bound_arbitration_relay_preserves_reason_in_next_action() {
        let out = run_asp(
            "user_decision_job_rejected",
            json!({
                "event": "user_decision_job_rejected",
                "jobId": ASP_JOB_ID,
                "data": "B 交付物符合预期",
                "decisionId": format!("{ASP_JOB_ID}:job_rejected:event-1"),
                "selectedActionId": "raise_arbitration",
                "params": {
                    "jobId": ASP_JOB_ID,
                    "reason": "交付物符合预期"
                }
            }),
        )
        .await;
        let result: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(result["decision"], "ready");
        assert_eq!(result["nextAction"][0]["id"], "raise_arbitration");
        assert_eq!(
            result["nextAction"][0]["params"]["reason"],
            "交付物符合预期"
        );
    }

    #[tokio::test]
    async fn bound_arbitration_relay_requires_reason_for_write_action() {
        let out = run_asp(
            "user_decision_job_rejected",
            json!({
                "event": "user_decision_job_rejected",
                "jobId": ASP_JOB_ID,
                "decisionId": format!("{ASP_JOB_ID}:job_rejected:event-1"),
                "selectedActionId": "raise_arbitration",
                "params": {"jobId": ASP_JOB_ID}
            }),
        )
        .await;
        let result: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(result["decision"], "blocked");
        assert_eq!(result["reason"], "arbitration_reason_required");
        assert_eq!(result["nextAction"], json!([]));
    }

    #[tokio::test]
    async fn asp_notify_scaffold_instructs_placeholder_fill() {
        // The notify scaffold must tell the sub session to fill `<...>` placeholders
        // from task context so a missing envelope title never leaks a literal "<title>".
        let out = run_asp("sub_asp_selected", json!({ "event": "sub_asp_selected" })).await;
        assert!(
            out.contains("never send a literal placeholder"),
            "scaffold carries the placeholder-fill instruction: {out}"
        );
    }

    #[tokio::test]
    async fn asp_epoch_tolerates_millisecond_timestamps() {
        // A millisecond-scale subEndTime must render a sane date, not a five-digit year.
        let out = run_asp(
            "sub_complete_notify",
            json!({ "event": "sub_complete_notify", "subEndTime": 1_790_000_000_000i64 }),
        )
        .await;
        assert!(
            out.contains("2026-"),
            "ms timestamp rendered as seconds date: {out}"
        );
        assert!(!out.contains("+58692"), "no five-digit year: {out}");
    }
    #[tokio::test]
    async fn sub_complete_notify_ignores_legacy_title_field() {
        let out = run_asp(
            "sub_complete_notify",
            json!({ "event": "sub_complete_notify", "title": "Legacy title" }),
        )
        .await;

        assert!(
            !out.contains("Legacy title"),
            "legacy title must be ignored: {out}"
        );
    }

    // ── FR-3: price gate test_flag short-circuit (sandbox ASP review) ────
}
