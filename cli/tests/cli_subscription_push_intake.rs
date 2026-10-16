//! Cross-layer contract test — new-version `a2a-subscription-push` reuses the
//! existing User delivery intake instead of routing straight into
//! `subscription-signal.md`.
//!
//! ─── Contract under test (frozen) ─────────────────────────────────────────────
//!   This is the E2E-equivalent for the subscription-push-intake change: no web
//!   UI exists, so Playwright does not apply, and a live black-box binary run of
//!   `deliverable_received` through subscription-active determination is not
//!   feasible offline (`TaskApiClient::fetch_subscription` has no test-mock hook
//!   and would issue a real authenticated HTTP call). Instead this test asserts
//!   the end-to-end WIRING contract across the `skills/okx-ai` markdown and the
//!   CLI Rust source by `include_str!`-ing both layers and checking the pinned
//!   string invariants below (frozen contract IDs; copied verbatim from
//!   `oli-docs/subscription-push-intake/implementation-plan.md` § E2E contract):
//!
//!   - C-MSGTYPE          — top-level `msgType` string exactly `a2a-subscription-push`.
//!   - C-LEGACY-MSGTYPE   — legacy envelope `msgType` string `a2a-agent-chat` (unchanged).
//!   - C-ROUTE-TARGET     — SKILL.md top-level row for `a2a-subscription-push` →
//!                          links `references/a2a/user/intake.md` (not
//!                          `subscription-signal.md` directly).
//!   - C-EVENT            — CLI intake event name `deliverable_received` (reused).
//!   - C-CMD              — intake command: `onchainos agent next-action --role user
//!                          --agentId <id> --message '{"event":"deliverable_received",
//!                          "jobId":"<jobId>"}' --a2a-file "<0600 path>"`.
//!   - C-NOFRAME          — new-version `content` is raw ASP signal text; NOT
//!                          required to end with `[intent:deliver]`, NO embedded
//!                          `jobId:` line, NO `deliverableType`.
//!   - C-BIND             — binding validated against trusted ctx: envelope
//!                          top-level `jobId==message_job_id` AND
//!                          `userAgentId==agent_id`. CORRECTED: the okx-a2a
//!                          daemon's real per-task delivery for this protocol
//!                          carries `jobId`/`userAgentId`/`aspAgentId`/
//!                          `content` — there is NO `receiverAgentId` field
//!                          (that name is specific to the legacy
//!                          `a2a-agent-chat` envelope only). An earlier
//!                          revision of this contract and its tests
//!                          incorrectly pinned `receiverAgentId` for this
//!                          protocol, which rejected every real daemon
//!                          delivery; see `binding_tests` below and the
//!                          `subscription_push_binding_failure` gate in
//!                          `flow_lifecycle/core.rs` for the follow-on
//!                          trusted-task-context check (non-subscription
//!                          task / User mismatch / ASP mismatch / unknown
//!                          task type), which must run before any
//!                          persistence or review side effect.
//!   - C-RESULT-ACTIVE    — Active subscription → CLI prompt marker
//!                          `active_subscription_signal` via `direct_model_route_prompt`.
//!   - C-RESULT-DISPLAY   — non-Active / lookup-off → marker
//!                          `active_subscription_signal_notify_only` via `signal_only_prompt`.
//!   - C-SIGNAL-LEAF      — `subscription-signal.md` reached only via the CLI
//!                          intake result, not the top-level table.
//!   - C-LEGACY-INTACT    — legacy `a2a-agent-chat` + `[intent:deliver]` validation
//!                          in `validate_a2a_file_arg` / `parse_a2a_envelope`
//!                          unchanged (regression guard).
//!
//! ─── Existing-E2E impact ───────────────────────────────────────────────────────
//!   `cli/tests/task_action_routing_contract.rs` was read for an assertion that
//!   pins the OLD `subscription-signal.md` target for the `a2a-subscription-push`
//!   top-level row. No such assertion exists there (that file does not reference
//!   `a2a-subscription-push` at all), so no existing-E2E fix is owned by this
//!   file; this test is the sole new coverage for the routing-target change.
//!
//! ─── Conventions ──────────────────────────────────────────────────────────────
//!   Mirrors `cli/tests/task_action_routing_contract.rs`: `include_str!` the
//!   skill markdown + CLI source as module-level `const`s and assert plain
//!   string invariants — no process spawn, no network, compiles and runs fully
//!   offline.

const OKX_AI_SKILL: &str = include_str!("../../skills/okx-ai/SKILL.md");
const INTAKE: &str = include_str!("../../skills/okx-ai/references/a2a/user/intake.md");
const SUBSCRIPTION_SIGNAL: &str =
    include_str!("../../skills/okx-ai/references/a2a/user/subscription-signal.md");
const AGENT_COMMERCE_MOD_SOURCE: &str = include_str!("../src/commands/agent_commerce/mod.rs");
const FLOW_LIFECYCLE_CORE_SOURCE: &str =
    include_str!("../src/commands/agent_commerce/task/user/flow_lifecycle/core.rs");

// ── E1 (FR-1, C-ROUTE-TARGET) ──────────────────────────────────────────────────
// The top-level Skill routing row whose match is `a2a-subscription-push` links
// `references/a2a/user/intake.md`, and the table no longer sends that row
// directly at `subscription-signal.md`.
#[test]
fn e1_top_level_route_target_is_intake_not_signal() {
    let row = OKX_AI_SKILL
        .lines()
        .find(|line| line.contains("a2a-subscription-push"))
        .unwrap_or_else(|| {
            panic!("SKILL.md must contain a top-level row matching a2a-subscription-push")
        });
    assert!(
        row.contains("references/a2a/user/intake.md"),
        "the a2a-subscription-push row must route to references/a2a/user/intake.md, got: {row}"
    );
    assert!(
        !row.contains("subscription-signal.md"),
        "the a2a-subscription-push row must NOT point directly at subscription-signal.md, got: {row}"
    );
}

// ── E2 (FR-1/FR-3, C-CMD/C-NOFRAME) ────────────────────────────────────────────
// `intake.md` carries a new-version `a2a-subscription-push` section that reuses
// the `next-action … --event deliverable_received … --a2a-file` command and
// explicitly forbids reframing `content` as `[intent:deliver]`.
#[test]
fn e2_intake_has_new_version_section_reusing_deliver_command_without_reframing() {
    assert!(
        INTAKE.contains("New-version `a2a-subscription-push` envelope"),
        "intake.md must document the new-version a2a-subscription-push envelope section"
    );
    let new_version_section = INTAKE
        .split_once("New-version `a2a-subscription-push` envelope")
        .expect("new-version section header")
        .1;
    assert!(
        new_version_section.contains("onchainos agent next-action"),
        "new-version section must reuse the next-action command"
    );
    assert!(
        new_version_section.contains("--role user"),
        "new-version section must keep --role user"
    );
    assert!(
        new_version_section.contains(r#""event":"deliverable_received""#),
        "new-version section must use the deliverable_received event"
    );
    assert!(
        new_version_section.contains("--a2a-file"),
        "new-version section must pass --a2a-file"
    );
    assert!(
        new_version_section.contains("Never reframe, wrap, or forge it into a legacy")
            || new_version_section.contains("[intent:deliver]"),
        "new-version section must explicitly state content is not reframed as [intent:deliver]"
    );
    assert!(
        new_version_section.contains("not a `[intent:deliver]` frame")
            || new_version_section.contains("Never reframe"),
        "new-version section must state content is not a [intent:deliver] frame"
    );
}

// ── E3 (FR-3, C-SIGNAL-LEAF) ───────────────────────────────────────────────────
// `subscription-signal.md` states entry for the new-version envelope is via the
// CLI delivery-intake result (precondition: CLI proved Active + saved), and
// keeps the "never inferred from content" rule.
#[test]
fn e3_signal_leaf_entered_only_via_cli_intake_result() {
    assert!(
        SUBSCRIPTION_SIGNAL.contains("reaches this leaf only via the"),
        "subscription-signal.md must state the new-version envelope reaches this leaf only via the CLI intake result"
    );
    assert!(
        SUBSCRIPTION_SIGNAL.contains("CLI delivery-intake result"),
        "subscription-signal.md must name the CLI delivery-intake result as the entry path"
    );
    assert!(
        SUBSCRIPTION_SIGNAL.contains("never directly from the top-level Skill table"),
        "subscription-signal.md must disclaim direct top-level entry"
    );
    assert!(
        SUBSCRIPTION_SIGNAL.contains("the CLI has already proven")
            && SUBSCRIPTION_SIGNAL.contains("Active")
            && SUBSCRIPTION_SIGNAL.contains("saved"),
        "subscription-signal.md must restate the CLI-proved-Active-and-saved precondition"
    );
}

// ── E4 (FR-2, C-MSGTYPE/C-BIND/C-NOFRAME) ──────────────────────────────────────
// `validate_a2a_file_arg` references `a2a-subscription-push` and the new branch
// does not require `[intent:deliver]`.
#[test]
fn e4_validate_a2a_file_arg_has_subscription_push_branch_without_intent_deliver_requirement() {
    let validate_fn = AGENT_COMMERCE_MOD_SOURCE
        .split_once("fn validate_a2a_file_arg(")
        .expect("validate_a2a_file_arg function")
        .1;
    // Scope to the function body only (up to the next top-level fn/mod item) so
    // this assertion is specific to the new branch, not incidental matches
    // elsewhere in the file.
    let next_item = validate_fn
        .find("\n#[cfg(")
        .or_else(|| validate_fn.find("\nfn "))
        .unwrap_or(validate_fn.len());
    let validate_fn_body = &validate_fn[..next_item];

    assert!(
        validate_fn_body.contains("a2a-subscription-push"),
        "validate_a2a_file_arg must reference a2a-subscription-push, got body: {validate_fn_body}"
    );
    assert!(
        AGENT_COMMERCE_MOD_SOURCE.contains("a2a-subscription-push"),
        "mod.rs must reference a2a-subscription-push somewhere for the new branch"
    );
}

// ── E5 (FR-2, C-NOFRAME) ───────────────────────────────────────────────────────
// `core.rs` has a `parse_a2a_subscription_push_envelope` (or equivalent
// new-version branch) that routes `a2a-subscription-push` to a
// `DeliverPayload::Text` path without `parse_deliver_content`.
#[test]
fn e5_core_has_subscription_push_envelope_parser_bypassing_legacy_deliver_frame() {
    assert!(
        FLOW_LIFECYCLE_CORE_SOURCE.contains("a2a-subscription-push"),
        "core.rs must reference a2a-subscription-push for the new-version parse branch"
    );
    assert!(
        FLOW_LIFECYCLE_CORE_SOURCE.contains("parse_a2a_subscription_push_envelope"),
        "core.rs must define parse_a2a_subscription_push_envelope (or the equivalent new-version branch)"
    );

    let new_fn = FLOW_LIFECYCLE_CORE_SOURCE
        .split_once("fn parse_a2a_subscription_push_envelope")
        .expect("parse_a2a_subscription_push_envelope function")
        .1;
    let next_item = new_fn
        .find("\n#[cfg(")
        .or_else(|| new_fn.find("\nfn "))
        .unwrap_or(new_fn.len());
    let new_fn_body = &new_fn[..next_item];

    assert!(
        new_fn_body.contains("DeliverPayload::Text"),
        "parse_a2a_subscription_push_envelope must produce DeliverPayload::Text, got body: {new_fn_body}"
    );
    assert!(
        !new_fn_body.contains("parse_deliver_content"),
        "parse_a2a_subscription_push_envelope must NOT call parse_deliver_content (no legacy frame parsing), got body: {new_fn_body}"
    );
}

// ── E6 (FR-3/NFR-1, C-RESULT-ACTIVE/C-RESULT-DISPLAY) ──────────────────────────
// Both result markers remain reachable from the (reused, unchanged)
// subscription delivery route — this is a reuse guard, not a new branch.
#[test]
fn e6_both_result_markers_remain_reachable_from_subscription_delivery_route() {
    assert!(
        FLOW_LIFECYCLE_CORE_SOURCE.contains("active_subscription_signal"),
        "core.rs must still carry the active_subscription_signal marker"
    );
    assert!(
        FLOW_LIFECYCLE_CORE_SOURCE.contains("active_subscription_signal_notify_only"),
        "core.rs must still carry the active_subscription_signal_notify_only marker"
    );
    assert!(
        FLOW_LIFECYCLE_CORE_SOURCE.contains("direct_model_route_prompt"),
        "core.rs must still carry direct_model_route_prompt (Active path)"
    );
    assert!(
        FLOW_LIFECYCLE_CORE_SOURCE.contains("signal_only_prompt"),
        "core.rs must still carry signal_only_prompt (non-Active/display-only path)"
    );
    assert!(
        FLOW_LIFECYCLE_CORE_SOURCE.contains("deliverable_task_route"),
        "core.rs must still route through deliverable_task_route after save"
    );
    assert!(
        FLOW_LIFECYCLE_CORE_SOURCE.contains("determine_active_delivery"),
        "core.rs must still gate on determine_active_delivery (decide_active)"
    );
}

// ── E7 (FR-3-AC-3, C-LEGACY-INTACT) ────────────────────────────────────────────
// The legacy `a2a-agent-chat` + `[intent:deliver]` requirements still appear in
// `validate_a2a_file_arg` / `parse_a2a_envelope` (regression guard).
#[test]
fn e7_legacy_a2a_agent_chat_intent_deliver_requirements_unchanged() {
    assert!(
        AGENT_COMMERCE_MOD_SOURCE.contains("a2a-agent-chat"),
        "mod.rs must still reference the legacy a2a-agent-chat msgType"
    );
    assert!(
        AGENT_COMMERCE_MOD_SOURCE.contains("--a2a-file content must end with [intent:deliver]"),
        "validate_a2a_file_arg must still require the legacy [intent:deliver] trailer"
    );
    assert!(
        AGENT_COMMERCE_MOD_SOURCE.contains("payload.msgType must be a2a-agent-chat")
            || AGENT_COMMERCE_MOD_SOURCE.contains("msgType must be a2a-agent-chat"),
        "validate_a2a_file_arg must still reject a non a2a-agent-chat legacy msgType"
    );

    assert!(
        FLOW_LIFECYCLE_CORE_SOURCE.contains(r#"Some("a2a-agent-chat")"#),
        "parse_a2a_envelope must still match the legacy a2a-agent-chat msgType"
    );
    assert!(
        FLOW_LIFECYCLE_CORE_SOURCE.contains("fn parse_deliver_content"),
        "core.rs must still carry the legacy parse_deliver_content function unchanged"
    );
    assert!(
        FLOW_LIFECYCLE_CORE_SOURCE.contains(r#"!= Some("[intent:deliver]")"#),
        "parse_deliver_content must still require the legacy [intent:deliver] trailer"
    );
}

// ── Regression guard — C-MSGTYPE / C-LEGACY-MSGTYPE byte-exact strings ────────
// The two msgType literal strings this whole change keys on must be present
// byte-exact in both layers; this is the shared value both prod owners (A, B)
// and this E2E owner (C) pin verbatim.
#[test]
fn msgtype_literals_are_byte_exact_across_layers() {
    assert!(OKX_AI_SKILL.contains("a2a-subscription-push"));
    assert!(INTAKE.contains("a2a-subscription-push"));
    assert!(AGENT_COMMERCE_MOD_SOURCE.contains("a2a-agent-chat"));
    assert!(FLOW_LIFECYCLE_CORE_SOURCE.contains("a2a-agent-chat"));
}

// ── Real-field-contract regression guard (C-BIND fix) ─────────────────────
//
// This crate ships no `[lib]` target, so these `tests/` integration files
// cannot link against the private `validate_a2a_subscription_push_payload` /
// `parse_a2a_subscription_push_envelope` functions directly — that is why
// this whole file is a source-string contract check rather than a call-level
// behavior test. The actual behavior tests (constructing real daemon-shape
// JSON and calling the functions) live as `#[cfg(test)] mod tests` inside
// `cli/src/commands/agent_commerce/mod.rs` (`escape_control_chars_tests`,
// e.g. `rejects_subscription_push_without_receiver_agent_id_field`) and
// `cli/src/commands/agent_commerce/task/user/flow_lifecycle/core.rs`
// (`tests::parse_a2a_envelope_subscription_push_has_no_receiver_agent_id_field`,
// `tests::binding_gate_*`). This test only pins the string-level invariant
// that both production functions key the new protocol's receiver identity on
// `userAgentId`, not `receiverAgentId`.
#[test]
fn subscription_push_functions_key_receiver_identity_on_user_agent_id_not_receiver_agent_id() {
    let validate_fn = AGENT_COMMERCE_MOD_SOURCE
        .split_once("fn validate_a2a_subscription_push_payload(")
        .expect("validate_a2a_subscription_push_payload function")
        .1;
    let next_item = validate_fn
        .find("\n#[cfg(")
        .or_else(|| validate_fn.find("\nfn "))
        .unwrap_or(validate_fn.len());
    let validate_fn_body = &validate_fn[..next_item];
    assert!(
        validate_fn_body.contains(r#"get("userAgentId")"#),
        "validate_a2a_subscription_push_payload must bind the new protocol's \
         receiver identity on userAgentId (the real okx-a2a daemon field), got body: {validate_fn_body}"
    );
    assert!(
        !validate_fn_body.contains("receiverAgentId"),
        "validate_a2a_subscription_push_payload must NOT require receiverAgentId — \
         the real daemon delivery for this protocol has no such field, got body: {validate_fn_body}"
    );
    assert!(
        validate_fn_body.contains(r#"get("aspAgentId")"#),
        "validate_a2a_subscription_push_payload must require aspAgentId so the CLI \
         intake layer can cross-check it against the trusted task context"
    );

    let parse_fn = FLOW_LIFECYCLE_CORE_SOURCE
        .split_once("fn parse_a2a_subscription_push_envelope(")
        .expect("parse_a2a_subscription_push_envelope function")
        .1;
    let next_item = parse_fn
        .find("\n#[cfg(")
        .or_else(|| parse_fn.find("\nfn "))
        .unwrap_or(parse_fn.len());
    let parse_fn_body = &parse_fn[..next_item];
    assert!(
        parse_fn_body.contains(r#"get("userAgentId")"#),
        "parse_a2a_subscription_push_envelope must bind on userAgentId, got body: {parse_fn_body}"
    );
    assert!(
        !parse_fn_body.contains("receiverAgentId"),
        "parse_a2a_subscription_push_envelope must NOT require receiverAgentId, got body: {parse_fn_body}"
    );

    // The legacy a2a-agent-chat branch must still require receiverAgentId,
    // unchanged (C-LEGACY-INTACT).
    let legacy_validate_fn = AGENT_COMMERCE_MOD_SOURCE
        .split_once("fn validate_a2a_agent_chat_payload(")
        .expect("validate_a2a_agent_chat_payload function")
        .1;
    let next_item = legacy_validate_fn
        .find("\n#[cfg(")
        .or_else(|| legacy_validate_fn.find("\nfn "))
        .unwrap_or(legacy_validate_fn.len());
    assert!(
        legacy_validate_fn[..next_item].contains(r#"get("receiverAgentId")"#),
        "the legacy a2a-agent-chat branch must keep requiring receiverAgentId unchanged"
    );
}

// ── Business-binding gate must run before persistence/review side effects ──
//
// String-level regression guard: `subscription_push_binding_failure` must be
// consulted inside `deliverable_received_cli` BEFORE the save/download block
// runs, so a non-subscription / identity-mismatched new-protocol delivery
// never reaches `handle_save` or a review card. Full behavior coverage for
// the gate itself (job_type / user_agent_id / provider_agent_id combinations)
// is in `core.rs::tests::binding_gate_*` — this only pins call-site ordering.
#[test]
fn deliverable_received_cli_runs_binding_gate_before_save_block() {
    let func = FLOW_LIFECYCLE_CORE_SOURCE
        .split_once("pub(crate) async fn deliverable_received_cli(")
        .expect("deliverable_received_cli function")
        .1;
    let gate_pos = func
        .find("subscription_push_binding_failure(")
        .expect("deliverable_received_cli must call subscription_push_binding_failure");
    let save_block_pos = func
        .find("Execute: download (file) or write tmp (text)")
        .expect("deliverable_received_cli must still contain the save/download block");
    assert!(
        gate_pos < save_block_pos,
        "the binding gate must run before the download/save block, \
         gate_pos={gate_pos} save_block_pos={save_block_pos}"
    );
}
