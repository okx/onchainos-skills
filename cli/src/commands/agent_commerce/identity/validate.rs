//! Pure-local (no HTTP, no network) validator that checks an agent listing's
//! fields against mechanical marketplace rules. Invoked explicitly by the skill
//! as a single-pass QA gate during registration and update (ASP only);
//! `activate` does NOT re-run it.
//!
//! Scope (deliberately narrow): only MECHANICAL rules — length / format /
//! forbidden-marker / structural checks decidable without semantic judgment.

use anyhow::Result;
use serde::Serialize;

use crate::commands::Context;

use super::args::ValidateListingArgs;
use super::models::AgentService;
use super::utils::{display_width, is_plain_number, is_positive_integer, normalize_role};

// ─── CLI entry point (hidden — not shown in --help) ─────────────────────────

pub async fn validate_listing(args: ValidateListingArgs, _ctx: &Context) -> Result<()> {
    let role = args
        .role
        .as_deref()
        .and_then(|r| normalize_role(r).ok())
        .unwrap_or_else(|| "asp".to_string());

    let result = run_validation(
        &role,
        args.name.as_deref(),
        args.description.as_deref(),
        args.service.as_deref(),
    );
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

// ─── Output model ───────────────────────────────────────────────────────────

#[derive(Serialize)]
struct Finding {
    field: String,
    code: String,
    severity: String,
    issue: String,
    fix: String,
}

#[derive(Serialize)]
pub(crate) struct ValidationResult {
    pub(crate) pass: bool,
    findings: Vec<Finding>,
}

impl Finding {
    fn block(field: impl Into<String>, code: &str, issue: &str, fix: &str) -> Finding {
        Finding {
            field: field.into(),
            code: code.to_string(),
            severity: "block".to_string(),
            issue: issue.to_string(),
            fix: fix.to_string(),
        }
    }
}

// ─── Service parsing (no hard-error; rules report findings instead) ─────────
//
// We deserialize into the SAME `AgentService` struct that create/update use,
// so the element shape (field renames, optional endpoint) is identical. We do
// NOT call `utils::normalize_service` because that bails on missing fields —
// validate-listing must surface those as findings, not abort. We only trim.
fn parse_services_lenient(raw: &str) -> std::result::Result<Vec<AgentService>, ()> {
    let parsed: std::result::Result<Vec<AgentService>, _> = serde_json::from_str(raw);
    match parsed {
        Ok(mut services) => {
            for s in &mut services {
                s.service_name = s.service_name.trim().to_string();
                s.service_description = s.service_description.trim().to_string();
                s.fee = s.fee.trim().to_string();
                s.service_type = s.service_type.trim().to_string();
                s.endpoint = s.endpoint.as_ref().map(|e| e.trim().to_string());
                s.free_trial = s.free_trial.as_ref().map(|t| t.trim().to_string());
            }
            Ok(services)
        }
        Err(_) => Err(()),
    }
}

pub(crate) fn run_validation(
    role: &str,
    name: Option<&str>,
    description: Option<&str>,
    service: Option<&str>,
) -> ValidationResult {
    let mut findings: Vec<Finding> = Vec::new();

    let name = name.map(str::trim).unwrap_or("");
    let description = description.map(str::trim).unwrap_or("");

    // ── Name checks (all roles) ──────────────────────────────────────────
    check_name(name, &mut findings);

    // ── Description checks ────────────────────────────────────────────────
    // Universal U1/U2/U3 apply to a supplied non-empty description for every
    // role. The 3-part structure (D1/D3/D4/D5) is asp-service-only and is
    // NEVER applied to the agent-level description. Agent-level description for
    // ASPs additionally gets D6/D7 (and U2/U3 already above).
    if !description.is_empty() {
        check_universal_text("description", description, &mut findings);
        if role == "asp" {
            check_description_url_and_addr("description", description, &mut findings);
            if description.chars().count() > 500 {
                findings.push(Finding::block(
                    "description",
                    "D8",
                    "Agent description exceeds 500 characters.",
                    "Shorten the description to 500 characters or fewer.",
                ));
            }
        }
    }

    // ── Service checks (ASP only) ───────────────────────────────────
    if role == "asp" {
        if let Some(raw) = service {
            let raw = raw.trim();
            if !raw.is_empty() {
                match parse_services_lenient(raw) {
                    Ok(services) => {
                        for (i, svc) in services.iter().enumerate() {
                            check_service(i, svc, name, &mut findings);
                        }
                    }
                    Err(()) => findings.push(Finding::block(
                        "service",
                        "PARSE",
                        "--service is not a valid JSON array of service objects.",
                        "Provide a JSON array, e.g. [{\"name\":\"...\",\"servicedescription\":\"...\",\"servicetype\":\"A2MCP\",\"fee\":\"0\",\"endpoint\":\"https://...\"}].",
                    )),
                }
            }
        }
    }
    // For user / evaluator: --service is ignored silently (no findings).

    let pass = !findings.iter().any(|f| f.severity == "block");
    ValidationResult { pass, findings }
}

// ─── Name rules (N1, N2, N3, N6, N8) + Universal U1/U2/U3 on the name ───────

fn check_name(name: &str, findings: &mut Vec<Finding>) {
    if name.is_empty() {
        // Absent/empty name: skip silently (rule doesn't require presence at
        // this layer; presence is enforced by create/update).
        return;
    }

    // U1 (= N7) test/env marker on the name.
    if has_test_marker(name) {
        findings.push(Finding::block(
            "name",
            "U1",
            "Contains a test/environment marker.",
            "Remove the test/environment marker.",
        ));
    }
    // U2 hex address.
    if contains_hex_address(name) {
        findings.push(Finding::block(
            "name",
            "U2",
            "Contains a 0x hex address.",
            "Remove the 0x address.",
        ));
    }
    // U3 negative-capability phrase.
    if contains_negative_capability(name) {
        findings.push(Finding::block(
            "name",
            "U3",
            "Contains a negative-capability phrase.",
            "Describe what the agent does, not what it cannot do.",
        ));
    }

    // N1 length: pure-CJK → 2..=12 chars; mixed (CJK + Latin, e.g. the
    // N6-encouraged "CJK · English" form) and Latin → 3..=25 chars. Only a
    // purely-CJK name uses the dense 12-char bound, so a bilingual name is not
    // wrongly rejected for length by the CJK cap.
    let char_count = name.chars().count();
    if contains_cjk(name) && !contains_latin_letter(name) {
        if !(2..=12).contains(&char_count) {
            findings.push(Finding::block(
                "name",
                "N1",
                "CJK name must be 2-12 characters.",
                "Use a 2-12 character name.",
            ));
        }
    } else if !(3..=25).contains(&char_count) {
        findings.push(Finding::block(
            "name",
            "N1",
            "Name must be 3-25 characters.",
            "Use a 3-25 character name.",
        ));
    }

    // N2 embedded agent id.
    if has_embedded_agent_id(name) {
        findings.push(Finding::block(
            "name",
            "N2",
            "Contains an embedded agent id / trailing number.",
            "Remove the embedded id or trailing number from the name.",
        ));
    }

    // N3 ordinal suffix.
    if has_ordinal_suffix(name) {
        findings.push(Finding::block(
            "name",
            "N3",
            "Ends with an ordinal/version suffix.",
            "Remove the ordinal suffix (e.g. _v2, (2), #3).",
        ));
    }

    // N6 bilingual separator.
    if contains_cjk(name) && contains_latin_letter(name) && !name.contains(" \u{00B7} ") {
        findings.push(Finding::block(
            "name",
            "N6",
            "Mixed CJK + Latin name must use ' \u{00B7} ' (space middle-dot space) as separator.",
            "Separate the CJK and Latin parts with ' \u{00B7} '.",
        ));
    }

    // N8 decorative symbols.
    if has_decorative_symbols(name) {
        findings.push(Finding::block(
            "name",
            "N8",
            "Contains decorative or disallowed symbols.",
            "Use only letters, digits, spaces, a middle dot, and at most a single internal hyphen.",
        ));
    }

    // NOTE: public-figure / celebrity-name checking is deliberately NOT done
    // here — it lives in the skill's semantic QA layer (register.md §4 step 4),
    // not as a CLI mechanical rule. Do not add a hard-coded name blocklist.
}

// ─── Universal text rules (U1/U2/U3) for a generic field ────────────────────

fn check_universal_text(field: &str, text: &str, findings: &mut Vec<Finding>) {
    if has_test_marker(text) {
        findings.push(Finding::block(
            field,
            "U1",
            "Contains a test/environment marker.",
            "Remove the test/environment marker.",
        ));
    }
    if contains_hex_address(text) {
        findings.push(Finding::block(
            field,
            "U2",
            "Contains a 0x hex address.",
            "Remove the 0x address.",
        ));
    }
    if contains_negative_capability(text) {
        findings.push(Finding::block(
            field,
            "U3",
            "Contains a negative-capability phrase.",
            "Describe what the agent does, not what it cannot do.",
        ));
    }
}

fn check_description_url_and_addr(field: &str, text: &str, findings: &mut Vec<Finding>) {
    if contains_url(text) {
        findings.push(Finding::block(
            field,
            "D6",
            "Contains a URL.",
            "Remove URLs from the description.",
        ));
    }
    // D7 is the 0x check scoped to a description; U2 already covers agent-level
    // description, but the service-description path calls this with code D7. To
    // avoid a duplicate U2 + D7 on the same agent-level text we only emit D7
    // here when U2 has not been added for the same field. Simplest: emit D7
    // only for the service path (handled in check_service). For agent-level we
    // skip D7 (U2 covers it). So nothing to do here for the address.
}

// ─── Service rules (T, S, U4, U5, P, D) ─────────────────────────────────────

fn check_service(index: usize, svc: &AgentService, agent_name: &str, findings: &mut Vec<Finding>) {
    let f = |sub: &str| format!("service[{index}].{sub}");
    let stype = svc.service_type.to_ascii_uppercase();
    let is_a2mcp = stype == "A2MCP";
    let is_a2a = stype == "A2A";

    // ── Universal on every non-empty service field ───────────────────────
    // U2 hex address on any service field EXCEPT `servicedescription`: the
    // hex-address check on the description is emitted once as D7 by
    // `check_service_description` (the description-scoped code), so excluding
    // it here avoids a duplicate U2 + D7 on the same text.
    for (sub, text) in [
        ("name", svc.service_name.as_str()),
        ("fee", svc.fee.as_str()),
        ("servicetype", svc.service_type.as_str()),
        ("endpoint", svc.endpoint.as_deref().unwrap_or("")),
    ] {
        if !text.is_empty() && contains_hex_address(text) {
            findings.push(Finding::block(
                f(sub),
                "U2",
                "Contains a 0x hex address.",
                "Remove the 0x address.",
            ));
        }
    }
    // U3 negative-capability on name + description.
    for (sub, text) in [
        ("name", svc.service_name.as_str()),
        ("servicedescription", svc.service_description.as_str()),
    ] {
        if !text.is_empty() && contains_negative_capability(text) {
            findings.push(Finding::block(
                f(sub),
                "U3",
                "Contains a negative-capability phrase.",
                "Describe what the service does, not what it cannot do.",
            ));
        }
    }

    // ── ServiceType (T1/T2/T3) ───────────────────────────────────────────
    if !is_a2mcp && !is_a2a {
        findings.push(Finding::block(
            f("servicetype"),
            "T1",
            "servicetype must be exactly A2A or A2MCP.",
            "Set servicetype to A2A or A2MCP.",
        ));
    }
    let endpoint_empty = svc.endpoint.as_deref().map(str::trim).unwrap_or("").is_empty();
    if is_a2mcp && endpoint_empty {
        findings.push(Finding::block(
            f("endpoint"),
            "T2",
            "A2MCP service must have an endpoint.",
            "Provide the MCP endpoint URL.",
        ));
    }
    if is_a2a && !endpoint_empty {
        findings.push(Finding::block(
            f("endpoint"),
            "T3",
            "A2A service must not have an endpoint.",
            "Remove the endpoint field for A2A services.",
        ));
    }

    // ── T4: endpoint URL security (A2MCP only) ────────────────────────────
    if is_a2mcp && !endpoint_empty {
        let ep = svc.endpoint.as_deref().unwrap_or("").trim();
        if !ep.starts_with("https://") {
            findings.push(Finding::block(
                f("endpoint"),
                "T4",
                "Endpoint must use HTTPS.",
                "Change the URL scheme to https://.",
            ));
        } else {
            let host = ep.strip_prefix("https://")
                .and_then(|s| s.split('/').next())
                .map(|h| h.split(':').next().unwrap_or(h))
                .unwrap_or("")
                .to_lowercase();
            let is_private = host == "localhost"
                || host == "127.0.0.1"
                || host == "0.0.0.0"
                || host.starts_with("10.")
                || host.starts_with("192.168.")
                || host.ends_with(".local")
                || host.ends_with(".internal")
                || host.strip_prefix("172.").and_then(|r| r.split('.').next()?.parse::<u8>().ok()).map(|n| (16..=31).contains(&n)).unwrap_or(false);
            if is_private {
                findings.push(Finding::block(
                    f("endpoint"),
                    "T4",
                    "Endpoint must be a publicly reachable HTTPS URL (not localhost, 127.0.0.1, or a private network address).",
                    "Deploy the service to a public host and provide its https:// URL.",
                ));
            }
        }
    }

    // ── U5 contradicting standalone A2A / A2MCP token in name/description ──
    if !stype.is_empty() && (is_a2mcp || is_a2a) {
        for (sub, text) in [
            ("name", svc.service_name.as_str()),
            ("servicedescription", svc.service_description.as_str()),
        ] {
            if let Some(token) = contradicting_type_token(text, &stype) {
                findings.push(Finding::block(
                    f(sub),
                    "U5",
                    &format!("Mentions '{token}' but servicetype is {stype}."),
                    "Make the text and the servicetype agree.",
                ));
            }
        }
    }

    // ── ServiceName (S1/S3/S4/S6) ────────────────────────────────────────
    if !svc.service_name.is_empty() {
        let name_chars = svc.service_name.chars().count();
        if !(5..=30).contains(&name_chars) {
            findings.push(Finding::block(
                f("name"),
                "S1",
                "Service name must be 5-30 characters.",
                "Use a 5-30 character service name.",
            ));
        }
        if !agent_name.is_empty()
            && svc.service_name.trim().eq_ignore_ascii_case(agent_name.trim())
        {
            findings.push(Finding::block(
                f("name"),
                "S3",
                "Service name duplicates the agent name.",
                "Give the service a distinct name from the agent.",
            ));
        }
        if contains_price_info(&svc.service_name) {
            findings.push(Finding::block(
                f("name"),
                "S4",
                "Service name contains price information.",
                "Move price into the fee field; keep it out of the name.",
            ));
        }
        if has_test_marker(&svc.service_name) {
            findings.push(Finding::block(
                f("name"),
                "S6",
                "Service name contains a test/environment marker.",
                "Remove the test/environment marker.",
            ));
        }
    }

    // ── Pricing (U4/P1..P6) — single fee XOR subscription ────────────────
    check_pricing(index, svc, is_a2mcp, is_a2a, findings);

    // ── Description (D1-D7) on servicedescription ────────────────────────
    // Always run: an empty / blank description is itself a D1 (handled by the
    // empty branch inside check_service_description). Gating on non-empty here
    // would skip that branch and let a blank description pass silently.
    check_service_description(index, &svc.service_description, findings);
}

// Pricing QA. A2MCP: single-purchase `fee` required (plain number), no
// subscription. A2A: EXACTLY ONE of a single-purchase `fee` XOR a
// `subscription` — never neither (P2) and never both (P6); the two models are
// mutually exclusive. Every fee (single or per-tier) is a plain number, and
// the only supported interval today is `month`. A subscription-priced A2A
// carries an EMPTY single `fee` (`""`) — that is the "no single price" marker.
// USDT is the implicit, only currency, so ANY extra text — a symbol, a
// parenthetical, or negotiation wording — makes a fee non-numeric and is
// rejected (P1/P5).
fn check_pricing(
    index: usize,
    svc: &AgentService,
    is_a2mcp: bool,
    is_a2a: bool,
    findings: &mut Vec<Finding>,
) {
    let fee_field = format!("service[{index}].fee");
    let sub_field = format!("service[{index}].subscription");
    let fee = svc.fee.trim();
    // Empty `fee` is the subscription "no single price" marker.
    let fee_present = !fee.is_empty();
    let has_subscription = !svc.subscription.is_empty();
    let bad_fee = |findings: &mut Vec<Finding>| {
        findings.push(Finding::block(
            &fee_field,
            "P1",
            "Fee must be a plain number.",
            "Use a plain number, e.g. 10 — USDT is the default currency; do not add a currency symbol, parenthetical, or any other text.",
        ));
    };

    let trial = svc.free_trial.as_deref().map(str::trim).unwrap_or("");
    let trial_field = format!("service[{index}].freeTrial");

    if is_a2mcp {
        // Subscription is not allowed on A2MCP.
        if has_subscription {
            findings.push(Finding::block(
                &sub_field,
                "P3",
                "A2MCP services do not support subscription pricing.",
                "Remove the subscription field for A2MCP services.",
            ));
        }
        // Free trial is a subscription-only concept → not allowed on A2MCP.
        if !trial.is_empty() {
            findings.push(Finding::block(
                &trial_field,
                "P7",
                "A2MCP services do not support a free trial.",
                "Remove the freeTrial field for A2MCP services.",
            ));
        }
        if !fee_present {
            // U4 + P1 for empty A2MCP fee.
            findings.push(Finding::block(
                &fee_field,
                "U4",
                "A2MCP service has an empty fee.",
                "Set an explicit fee, e.g. 10, or 0 for a free service.",
            ));
            findings.push(Finding::block(
                &fee_field,
                "P1",
                "A2MCP fee is required.",
                "Provide a plain number, e.g. 10 (USDT is the default currency).",
            ));
        } else if !is_plain_number(fee) {
            bad_fee(findings);
        }
        return;
    }

    if is_a2a {
        // A2A must be priced by EXACTLY ONE model — a single fee XOR a
        // subscription. Never neither (P2), never both (P6).
        if !fee_present && !has_subscription {
            findings.push(Finding::block(
                &fee_field,
                "P2",
                "A2A service has no pricing.",
                "Provide a single-purchase fee or a monthly subscription fee (exactly one).",
            ));
        }
        if fee_present && has_subscription {
            findings.push(Finding::block(
                &fee_field,
                "P6",
                "A2A service sets both a single-purchase fee and a subscription.",
                "Choose one billing model — a single-purchase fee or a monthly subscription, not both.",
            ));
        }
        if fee_present && !is_plain_number(fee) {
            bad_fee(findings);
        }
        for tier in &svc.subscription {
            if !tier.interval.trim().eq_ignore_ascii_case("month") {
                findings.push(Finding::block(
                    &sub_field,
                    "P4",
                    &format!("Unsupported subscription interval '{}'.", tier.interval.trim()),
                    "Only monthly subscription is supported — set interval to 'month'.",
                ));
            }
            if !is_plain_number(tier.fee.trim()) {
                findings.push(Finding::block(
                    &sub_field,
                    "P5",
                    "Subscription fee must be a plain number.",
                    "Use a plain number, e.g. 10 — USDT is the default currency; do not add a currency symbol or any other text.",
                ));
            }
        }
        // Free trial (subscription-only): valid only alongside a subscription,
        // and must be a positive integer number of hours.
        if !trial.is_empty() {
            if !has_subscription {
                findings.push(Finding::block(
                    &trial_field,
                    "P7",
                    "freeTrial is only valid on a subscription-priced service.",
                    "Remove freeTrial, or price this service with a monthly subscription.",
                ));
            }
            if !is_positive_integer(trial) {
                findings.push(Finding::block(
                    &trial_field,
                    "P8",
                    "freeTrial must be a positive integer number of hours.",
                    "Use a whole number of hours, e.g. 72; do not use decimals, 0, or any extra text.",
                ));
            }
        }
        return;
    }

    // Unknown serviceType (T1 already flags the type) — validate fee format only.
    if fee_present && !is_plain_number(fee) {
        bad_fee(findings);
    }
}

// Service description is a THREE-part structure per the okx.ai display spec
// (field 5):
//   part 1 — core-capability summary        — REQUIRED
//   part 2 — what the user must provide      — REQUIRED
//   part 3 — delivery note (交付物说明)       — OPTIONAL here. The spec makes it
//            REQUIRED for trading-signal services, but "is this a trading-signal
//            service?" is a SEMANTIC judgment left to the skill's QA layer
//            (register.md §4), never decided by this mechanical validator.
// Parts are the non-empty lines positionally: part 1 = line 1, part 2 = line 2,
// part 3 = every remaining non-empty line joined. At least parts 1 and 2 must be
// present (fewer → D1). Lengths are measured in EAST-ASIAN DISPLAY WIDTH
// (`display_width`: CJK = 2, ASCII = 1). Limits (width units): each part ≤ 400,
// total ≤ 1200 — i.e. the spec's "≤ 200 chars per part / ≤ 600 chars total"
// counted in CJK characters. D9 additionally blocks deterministic profit /
// return-guarantee wording ("稳赚 / 保证收益 / 翻倍" …); the trading-signal
// requirements (declared markets, signal example, no abbreviations, in-scope
// markets) are semantic and live in the skill layer, not here.
fn check_service_description(index: usize, desc: &str, findings: &mut Vec<Finding>) {
    let field = |sub: &str| format!("service[{index}].{sub}");
    let fd = field("servicedescription");

    // D1 (empty): a blank description has no parts at all.
    let trimmed = desc.trim();
    if trimmed.is_empty() {
        findings.push(Finding::block(
            &fd,
            "D1",
            "Service description must have at least 2 parts: a core-capability summary and what the user must provide, on separate lines (a delivery note is a recommended 3rd line).",
            "Put the core-capability summary on line 1 and what the user must provide on line 2 (optionally a delivery note on line 3).",
        ));
        return;
    }

    // D2 total display width <= 1200 (= 600 CJK characters).
    if display_width(desc) > 1200 {
        findings.push(Finding::block(
            &fd,
            "D2",
            "Service description is too long (limit: 600 CJK characters, i.e. 1200 half-width).",
            "Trim the description to 600 CJK characters (1200 half-width) or fewer.",
        ));
    }

    // D6 URL.
    if contains_url(desc) {
        findings.push(Finding::block(
            &fd,
            "D6",
            "Service description contains a URL.",
            "Remove URLs from the description.",
        ));
    }
    // D7 hex address (description scope).
    if contains_hex_address(desc) {
        findings.push(Finding::block(
            &fd,
            "D7",
            "Service description contains a 0x hex address.",
            "Remove the 0x address.",
        ));
    }
    // D9 profit / return-guarantee wording (deterministic forbidden phrases).
    if contains_profit_guarantee(desc) {
        findings.push(Finding::block(
            &fd,
            "D9",
            "Service description contains profit / return-guarantee wording (e.g. 稳赚 / 保证收益 / 翻倍 / guaranteed returns).",
            "Remove any guaranteed-profit or guaranteed-return claims — describe the capability, not a promised outcome.",
        ));
    }
    // U1 test/environment marker — the spec's universal ban ("所有字段不允许
    // 包含 (pre)、(test)") applies to EVERY field, service description included.
    if has_test_marker(desc) {
        findings.push(Finding::block(
            &fd,
            "U1",
            "Service description contains a test/environment marker.",
            "Remove the test/environment marker.",
        ));
    }

    // Three-part structure, positional: part 1 = 1st non-empty line, part 2 =
    // 2nd, part 3 = every remaining non-empty line joined. Parts 1 and 2 are
    // both required; fewer than 2 non-empty lines → D1.
    let lines: Vec<&str> = desc
        .split('\n')
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();

    if lines.len() < 2 {
        findings.push(Finding::block(
            &fd,
            "D1",
            "Service description must have at least 2 parts: a core-capability summary and what the user must provide, on separate lines (a delivery note is a recommended 3rd line).",
            "Put the core-capability summary on line 1 and what the user must provide on line 2 (optionally a delivery note on line 3).",
        ));
        return;
    }

    let part1 = lines[0];
    let part2 = lines[1];
    // part 3 = the 3rd non-empty line onward, joined — so no trailing content
    // escapes the per-part length gate. Absent when there are only 2 lines.
    let part3: Option<String> = if lines.len() > 2 {
        Some(lines[2..].join("\n"))
    } else {
        None
    };

    // D3 part 1 (core-capability summary) display width <= 400 (= 200 CJK characters).
    if display_width(part1) > 400 {
        findings.push(Finding::block(
            &fd,
            "D3",
            "Description part 1 (core-capability summary) is too long (limit: 200 CJK characters, i.e. 400 half-width).",
            "Shorten the core-capability summary to 200 CJK characters (400 half-width) or fewer.",
        ));
    }
    // D4 part 2 (what the user must provide) display width <= 400 (= 200 CJK characters).
    if display_width(part2) > 400 {
        findings.push(Finding::block(
            &fd,
            "D4",
            "Description part 2 (what the user must provide) is too long (limit: 200 CJK characters, i.e. 400 half-width).",
            "Shorten what-the-user-must-provide to 200 CJK characters (400 half-width) or fewer.",
        ));
    }
    // D5 part 3 (delivery note) display width <= 400 (= 200 CJK characters).
    if let Some(p3) = &part3 {
        if display_width(p3) > 400 {
            findings.push(Finding::block(
                &fd,
                "D5",
                "Description part 3 (delivery note) is too long (limit: 200 CJK characters, i.e. 400 half-width).",
                "Shorten the delivery note to 200 CJK characters (400 half-width) or fewer.",
            ));
        }
    }
}

// ─── Pure predicate helpers (no regex crate; plain string ops) ──────────────

/// CJK ideograph check (covers the common CJK Unified Ideographs block).
fn contains_cjk(s: &str) -> bool {
    s.chars().any(is_cjk_char)
}

fn is_cjk_char(c: char) -> bool {
    matches!(c,
        '\u{4E00}'..='\u{9FFF}'      // CJK Unified Ideographs
        | '\u{3400}'..='\u{4DBF}'    // CJK Extension A
        | '\u{F900}'..='\u{FAFF}'    // CJK Compatibility Ideographs
        | '\u{3000}'..='\u{303F}'    // CJK symbols & punctuation
    )
}

fn contains_latin_letter(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_alphabetic())
}

/// U1: delimited test/env markers (case-insensitive). Must be delimited so
/// real words like `Predict` / `protest` do NOT match.
fn has_test_marker(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();

    // Bracketed / braced / parenthesized forms.
    const BRACKETED: &[&str] = &[
        "(pre)", "(test)", "(dev)", "(beta)", "(alpha)", "(staging)", "(uat)", "(sandbox)",
        "[pre]", "[test]", "[dev]", "[beta]", "{pre}", "{test}",
    ];
    for m in BRACKETED {
        if lower.contains(m) {
            return true;
        }
    }

    // Delimiter-suffix forms: -X / _X / .X (delimiter immediately before the
    // marker word, and the marker word must be terminated by a non-alphanumeric
    // boundary or end-of-string so `_predict` doesn't match `_pre`).
    const DELIM_MARKERS: &[(char, &str)] = &[
        ('-', "pre"), ('-', "test"), ('-', "dev"), ('-', "beta"), ('-', "staging"),
        ('_', "pre"), ('_', "test"), ('_', "dev"), ('_', "beta"), ('_', "staging"),
        ('.', "pre"), ('.', "test"),
    ];
    for (delim, word) in DELIM_MARKERS {
        if delimited_marker_present(&lower, *delim, word) {
            return true;
        }
    }

    // Trailing space-suffix forms at END of value.
    const TRAILING: &[&str] = &[" pre", " test", " dev", " beta", " staging"];
    for m in TRAILING {
        if lower.ends_with(m) {
            return true;
        }
    }

    false
}

/// True if `lower` contains `{delim}{word}` where the char right after `word`
/// is a non-alphanumeric boundary or end-of-string.
fn delimited_marker_present(lower: &str, delim: char, word: &str) -> bool {
    let needle: String = std::iter::once(delim).chain(word.chars()).collect();
    let mut search_from = 0usize;
    while let Some(rel) = lower[search_from..].find(&needle) {
        let start = search_from + rel;
        let after = start + needle.len();
        let boundary = lower[after..]
            .chars()
            .next()
            .map(|c| !c.is_ascii_alphanumeric())
            .unwrap_or(true);
        if boundary {
            return true;
        }
        search_from = start + 1;
    }
    false
}

/// U2 / D7: a `0x` hex address — `0x` followed by >= 6 hex digits.
fn contains_hex_address(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        if bytes[i] == b'0' && (bytes[i + 1] == b'x' || bytes[i + 1] == b'X') {
            let mut j = i + 2;
            while j < bytes.len() && bytes[j].is_ascii_hexdigit() {
                j += 1;
            }
            if j - (i + 2) >= 6 {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// U3: negative-capability phrases (case-insensitive substring). Scoped to the
/// spec's "目前不支持"-style negative info — a capability GAP / "not yet" framing
/// that signals an incomplete product. It deliberately does NOT match a bare
/// "不支持": the field-5 part-3 delivery note states whether copy-trading is
/// supported ("支持跟单" / "不支持跟单"), and that permanent delivery attribute —
/// used verbatim in the spec's own correct example — must pass QA.
fn contains_negative_capability(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    const EN: &[&str] = &[
        "currently not supported",
        "not supported yet",
        "not yet supported",
    ];
    if EN.iter().any(|p| lower.contains(p)) {
        return true;
    }
    // CJK: match only the temporal "currently / for-now not supported" gap
    // phrasing — never a bare "不支持" (which false-positives on "不支持跟单").
    s.contains("暂不支持") || s.contains("目前不支持")
}

/// D9: profit / return-guarantee wording (deterministic phrase check). Blocks
/// the spec's forbidden "收益承诺" wording — 稳赚 / 保证收益 / 翻倍 and close
/// equivalents — in either language. Kept to unambiguous guarantee phrases so a
/// legitimate capability description is not falsely blocked.
fn contains_profit_guarantee(s: &str) -> bool {
    // CJK phrases (match on raw — not ASCII-lowercased).
    const CJK: &[&str] = &[
        "稳赚", "稳赚不赔", "保证收益", "收益保证", "保本", "包赚", "必赚", "翻倍", "零风险",
    ];
    if CJK.iter().any(|p| s.contains(p)) {
        return true;
    }
    let lower = s.to_ascii_lowercase();
    const EN: &[&str] = &[
        "guaranteed return",
        "guaranteed returns",
        "guaranteed profit",
        "guaranteed income",
        "guaranteed gains",
        "risk-free",
        "risk free",
    ];
    EN.iter().any(|p| lower.contains(p))
}


/// N2: embedded agent id — `#\d+` or `_\d+` anywhere, OR a bare trailing number
/// after a space (e.g. `Bot 3`).
fn has_embedded_agent_id(name: &str) -> bool {
    if marker_digit_run(name, '#') || marker_digit_run(name, '_') {
        return true;
    }
    // Trailing " <digits>" at end.
    if let Some(idx) = name.rfind(' ') {
        let tail = &name[idx + 1..];
        if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) {
            return true;
        }
    }
    false
}

/// True if `name` contains `marker` immediately followed by >= 1 ASCII digits.
fn marker_digit_run(name: &str, marker: char) -> bool {
    let chars: Vec<char> = name.chars().collect();
    for i in 0..chars.len() {
        if chars[i] == marker {
            if let Some(&next) = chars.get(i + 1) {
                if next.is_ascii_digit() {
                    return true;
                }
            }
        }
    }
    false
}

/// N3: ordinal suffix at the END — `_v?\d+$`, `\(\d+\)$`, `#\d+$`,
/// `No\.?\d+$` (case-insensitive).
fn has_ordinal_suffix(name: &str) -> bool {
    let trimmed = name.trim_end();
    let lower = trimmed.to_ascii_lowercase();

    // (\d+)$  e.g. "(2)"
    if lower.ends_with(')') {
        if let Some(open) = lower.rfind('(') {
            let inner = &lower[open + 1..lower.len() - 1];
            if !inner.is_empty() && inner.chars().all(|c| c.is_ascii_digit()) {
                return true;
            }
        }
    }

    // trailing digits with a recognized prefix.
    let digits_len = lower
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .count();
    if digits_len == 0 {
        return false;
    }
    let prefix = &lower[..lower.len() - digits_len];
    // #\d+$
    if prefix.ends_with('#') {
        return true;
    }
    // _\d+$  and  _v\d+$
    if prefix.ends_with("_v") || prefix.ends_with('_') {
        return true;
    }
    // No.\d+$ / No\d+$
    if prefix.ends_with("no.") || prefix.ends_with("no") {
        return true;
    }
    false
}

/// N8: decorative / disallowed symbols. Allowed: CJK, Latin letters, digits,
/// spaces, the `·` middle dot, and a SINGLE internal hyphen joining word parts.
fn has_decorative_symbols(name: &str) -> bool {
    const DECOR: &[char] = &['!', '?', '@', '#', '$', '%', '*', '~', '/', '\\', '|', '+', '='];
    if name.chars().any(|c| DECOR.contains(&c)) {
        return true;
    }
    // Hyphen handling: a leading / trailing / standalone hyphen is not ok.
    if name.contains('-') {
        let trimmed = name.trim();
        if trimmed.starts_with('-') || trimmed.ends_with('-') {
            return true;
        }
        // standalone hyphen (surrounded by spaces) is not an internal joiner.
        if name.contains(" - ") {
            return true;
        }
    }
    false
}

fn contains_url(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    lower.contains("http://") || lower.contains("https://") || lower.contains("github.com")
}

/// S4: price info — a number immediately/space-followed by USDT/USDG
/// (case-insensitive) OR the standalone word `free` (and the CJK equivalent `免费`).
fn contains_price_info(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    if standalone_word(&lower, "free") || s.contains("免费") {
        return true;
    }
    // number followed (optionally by space) by usdt/usdg.
    for cur in ["usdt", "usdg"] {
        let mut from = 0usize;
        while let Some(rel) = lower[from..].find(cur) {
            let pos = from + rel;
            // look back over optional spaces then require >= 1 digit.
            let before = lower[..pos].trim_end();
            if before.chars().last().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                return true;
            }
            from = pos + cur.len();
        }
    }
    false
}

/// True if `lower` contains `word` as a whole word (non-alphanumeric boundaries).
fn standalone_word(lower: &str, word: &str) -> bool {
    let mut from = 0usize;
    while let Some(rel) = lower[from..].find(word) {
        let start = from + rel;
        let end = start + word.len();
        let left_ok = start == 0
            || !lower[..start]
                .chars()
                .next_back()
                .map(|c| c.is_ascii_alphanumeric())
                .unwrap_or(false);
        let right_ok = lower[end..]
            .chars()
            .next()
            .map(|c| !c.is_ascii_alphanumeric())
            .unwrap_or(true);
        if left_ok && right_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

/// U5: standalone `A2A` / `A2MCP` token (case-insensitive, word-boundary) that
/// contradicts the actual `stype`. Returns the contradicting token if found.
fn contradicting_type_token(text: &str, stype: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    // Check the OTHER type's token. Order matters: check a2mcp before a2a so we
    // don't match the "a2a" prefix inside "a2mcp".
    let stype = stype.to_ascii_uppercase();
    let candidates: &[&str] = match stype.as_str() {
        "A2A" => &["a2mcp"],
        "A2MCP" => &["a2a"],
        _ => return None,
    };
    for tok in candidates {
        if standalone_word(&lower, tok) {
            return Some(tok.to_ascii_uppercase());
        }
    }
    None
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "tests/validate_tests.rs"]
mod tests;
