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

// ─── Canonical fail messages (one 提示文案 per rule group) ──────────────────
//
// Requirement ① (身份侧校验规则): when any CLI mechanical rule's fail-criterion is
// hit, the returned `message` is UNIFIED to that rule's canonical 提示文案 — so
// every sub-check under one rule group surfaces the SAME user-facing text. The
// fine-grained internal `code` (U1/N1/S1/T1/P1/D1…) is kept for diagnostics and
// tests; the user-facing string is `message`.
mod fe {
    pub const FE03: &str = "The Agent name doesn't meet the naming rules: it may contain a test marker, an ordinal suffix, or special symbols, or its length or bilingual format is invalid. Use a clean brand name instead: 2\u{2013}12 characters in Chinese or 3\u{2013}25 in English, with no test markers, ordinals, or special symbols; for a bilingual name, use the \"Chinese name \u{00B7} English name\" format. Then resubmit.";
    pub const FE05: &str = "The Agent description has an issue: it contains a URL, exceeds 500 characters, or is empty. Remove any links, trim it to 500 characters or fewer, and make sure it's filled in. Then resubmit.";
    pub const FE06: &str = "The service name doesn't meet the rules: its length is out of range, it duplicates the Agent name, or it contains pricing or a test marker. Keep the name to 5\u{2013}30 characters and different from the Agent name, move any pricing to the fee field, and remove test markers. Then resubmit.";
    pub const FE10: &str = "The service type must be exactly A2A or A2MCP; the current value is invalid. Select A2A or A2MCP from the menu, then resubmit.";
    pub const FE11: &str = "The endpoint configuration is invalid: A2MCP requires an endpoint while A2A must not have one, and it must be a publicly accessible HTTPS URL \u{2014} not a private-network address or one starting with 0x. For A2MCP, enter a publicly accessible https URL; for A2A, remove the endpoint. Then resubmit.";
    pub const FE12: &str = "The A2MCP fee must be a plain number (enter 0 for free), but the current value contains units or non-numeric text. Enter the fee as a number only (e.g., 10; 0 for free) \u{2014} it's denominated in USDT by default, so no symbols or extra text. Then resubmit.";
    pub const FE13: &str = "The service data isn't a valid JSON array, or the create/update/delete operations don't match the id rules. Follow the sample format \u{2014} omit the id when creating, and include the id when updating or deleting \u{2014} then resubmit.";
    pub const FE17: &str = "The subscription billing setup is invalid: A2MCP doesn't support subscriptions, and an A2A service must use exactly one of pay-per-use or monthly subscription \u{2014} you can't leave both empty or fill in both. Pick one billing mode for your A2A service: set a pay-per-use fee, or set a monthly subscription (leave fee as an empty string \"\" when using subscription); for A2MCP, remove the subscription field. Then resubmit.";
    pub const FE18: &str = "Subscriptions currently support monthly billing only, but a different interval was provided. Set the subscription tier's interval to \"month\" (weekly, yearly, and other intervals aren't supported yet), then resubmit.";
    pub const FE19: &str = "The subscription price must be a plain number, but the current value contains units, symbols, or non-numeric text. Enter each tier's price as a number only (e.g., 10) \u{2014} denominated in USDT by default, up to 6 decimal places, no symbols or extra text. Then resubmit.";
    pub const FE20: &str = "The free-trial setup is invalid: freeTrial can only be configured on monthly-subscription services and must be a positive integer number of hours; A2MCP and pay-per-use services can't offer a trial. To enable a trial on a monthly subscription, set freeTrial to \"72\" (a fixed 3 days); otherwise omit the freeTrial field entirely (don't set \"\" or \"0\"). Then resubmit.";
    pub const FE21: &str = "The service description is empty or its total length exceeds 1000 CJK characters. Fill it in, put each part on its own line, and keep the total within 1000 CJK characters. Then resubmit.";
    pub const FE22: &str = "The service description contains prohibited content: a URL, promised returns (e.g., \"guaranteed profit\", \"double your money\", \"guaranteed returns\"), or a test marker. Remove all links, test markers, and any promises of returns, guaranteed principal, or \"no losses\" \u{2014} describe what the service can do without promising outcomes. Then resubmit.";
}

// ─── Output model ───────────────────────────────────────────────────────────

#[derive(Serialize)]
struct Finding {
    field: String,
    /// Fine-grained internal code (diagnostics / tests): U1/N1/S1/T1/P1/D1…
    code: String,
    severity: &'static str,
    /// Unified user-facing 提示文案 (English-canonical; skill translates).
    /// This is the ONLY rule-message field exposed to the caller.
    message: String,
}

#[derive(Serialize)]
pub(crate) struct ValidationResult {
    pub(crate) pass: bool,
    findings: Vec<Finding>,
}

impl Finding {
    /// A blocking finding. `code` is the internal diagnostic code; `message` is
    /// the unified user-facing 提示文案 for the rule group (pass a `fe::FExx`).
    fn block(field: impl Into<String>, code: &str, message: &str) -> Finding {
        Finding {
            field: field.into(),
            code: code.to_string(),
            severity: "block",
            message: message.to_string(),
        }
    }

    /// An advisory (suggestion-only) finding. It surfaces to the caller with the
    /// same `code`/`message` semantics as `block`, but `severity: "suggest"` means
    /// it does NOT flip `pass` to false (see `run_validation`, which only counts
    /// `severity == "block"`). Used for the A2A description length finding (FE-21
    /// D2) and the profit/return-guarantee finding (FE-22 D9).
    fn suggest(field: impl Into<String>, code: &str, message: &str) -> Finding {
        Finding {
            field: field.into(),
            code: code.to_string(),
            severity: "suggest",
            message: message.to_string(),
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
    // Universal U1/U3 apply to a supplied non-empty description for every
    // role. The 3-part structure (D1) is asp-service-only and is
    // NEVER applied to the agent-level description. Agent-level description for
    // ASPs additionally gets D6 (and U1/U3 already above).
    if !description.is_empty() {
        check_universal_text("description", description, &mut findings);
        if role == "asp" {
            check_description_url_and_addr("description", description, &mut findings);
            if description.chars().count() > 500 {
                findings.push(Finding::block("description", "D8", fe::FE05));
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
                    Err(()) => findings.push(Finding::block("service", "PARSE", fe::FE13)),
                }
            }
        }
    }
    // For user / evaluator: --service is ignored silently (no findings).

    // NOTE: on `service[i].servicedescription` only these are ADVISORY
    // (severity "suggest" — they surface but never flip `pass`):
    //   • FE-21 D2 — A2A total display width over the cap.
    //   • FE-22 D9 — profit/return-guarantee wording (any service type).
    // Still BLOCKING: the empty/blank A2A description (D1 — a missing-required-field,
    // consistent with the normalize_service bail) and the remaining FE-22 prohibited
    // content (URL D6, test marker U1). There is NO paragraph-count rule at all:
    // the buyer-facing paragraph layout is guidance in the skill's collection prompts
    // (register.md §3 Step 2c) and is never mechanically counted here, so the
    // subscription / non-subscription split no longer affects validation. A2MCP is
    // unchanged — only prohibited content runs for it. The SEMANTIC
    // service-description quality check (FE-23) stays advisory in the skill layer.

    let pass = !findings.iter().any(|f| f.severity == "block");
    ValidationResult { pass, findings }
}

// ─── Name rules (N1, N2, N3, N6, N8) + Universal U1/U3 on the name ──────────

fn check_name(name: &str, findings: &mut Vec<Finding>) {
    if name.is_empty() {
        // Absent/empty name: skip silently (rule doesn't require presence at
        // this layer; presence is enforced by create/update).
        return;
    }

    // All name rules share one unified 提示文案.
    // U1 (= N7) test/env marker on the name.
    if has_test_marker(name) {
        findings.push(Finding::block("name", "U1", fe::FE03));
    }
    // U3 negative-capability phrase.
    if contains_negative_capability(name) {
        findings.push(Finding::block("name", "U3", fe::FE03));
    }

    // N1 length: pure-CJK → 2..=12 chars; mixed (CJK + Latin, e.g. the
    // N6-encouraged "CJK · English" form) and Latin → 3..=25 chars. Only a
    // purely-CJK name uses the dense 12-char bound, so a bilingual name is not
    // wrongly rejected for length by the CJK cap.
    let char_count = name.chars().count();
    if contains_cjk(name) && !contains_latin_letter(name) {
        if !(2..=12).contains(&char_count) {
            findings.push(Finding::block("name", "N1", fe::FE03));
        }
    } else if !(3..=25).contains(&char_count) {
        findings.push(Finding::block("name", "N1", fe::FE03));
    }

    // N2 embedded agent id.
    if has_embedded_agent_id(name) {
        findings.push(Finding::block("name", "N2", fe::FE03));
    }

    // N3 ordinal suffix.
    if has_ordinal_suffix(name) {
        findings.push(Finding::block("name", "N3", fe::FE03));
    }

    // N6 bilingual separator.
    if contains_cjk(name) && contains_latin_letter(name) && !name.contains(" \u{00B7} ") {
        findings.push(Finding::block("name", "N6", fe::FE03));
    }

    // N8 decorative symbols.
    if has_decorative_symbols(name) {
        findings.push(Finding::block("name", "N8", fe::FE03));
    }

    // NOTE: public-figure / celebrity-name checking is deliberately NOT done
    // here — it lives in the skill's semantic QA layer (register.md §4 step 4),
    // not as a CLI mechanical rule. Do not add a hard-coded name blocklist.
}

// ─── Universal text rules (U1/U3) for a generic field ───────────────────────

// Only caller is the agent-level `description`.
fn check_universal_text(field: &str, text: &str, findings: &mut Vec<Finding>) {
    if has_test_marker(text) {
        findings.push(Finding::block(field, "U1", fe::FE05));
    }
    if contains_negative_capability(text) {
        findings.push(Finding::block(field, "U3", fe::FE05));
    }
}

fn check_description_url_and_addr(field: &str, text: &str, findings: &mut Vec<Finding>) {
    if contains_url(text) {
        findings.push(Finding::block(field, "D6", fe::FE05));
    }
}

// ─── Service rules (T, S, U4, U5, P, D) ─────────────────────────────────────

fn check_service(index: usize, svc: &AgentService, agent_name: &str, findings: &mut Vec<Finding>) {
    let f = |sub: &str| format!("service[{index}].{sub}");
    let stype = svc.service_type.to_ascii_uppercase();
    let is_a2mcp = stype == "A2MCP";
    let is_a2a = stype == "A2A";

    // U3 negative-capability: on the service NAME → name message; on the service
    // DESCRIPTION it is a prohibited-content issue → description message.
    if !svc.service_name.is_empty() && contains_negative_capability(&svc.service_name) {
        findings.push(Finding::block(f("name"), "U3", fe::FE06));
    }
    if !svc.service_description.is_empty()
        && contains_negative_capability(&svc.service_description)
    {
        findings.push(Finding::block(f("servicedescription"), "U3", fe::FE22));
    }

    // ── ServiceType (T1) ──────────────────────────────────────────────────
    if !is_a2mcp && !is_a2a {
        findings.push(Finding::block(f("servicetype"), "T1", fe::FE10));
    }
    // ── Endpoint (T2/T3/T4) ───────────────────────────────────────────────
    let endpoint_empty = svc.endpoint.as_deref().map(str::trim).unwrap_or("").is_empty();
    if is_a2mcp && endpoint_empty {
        findings.push(Finding::block(f("endpoint"), "T2", fe::FE11));
    }
    if is_a2a && !endpoint_empty {
        findings.push(Finding::block(f("endpoint"), "T3", fe::FE11));
    }

    // ── T4: endpoint URL security (A2MCP only) ────────────────────────────
    if is_a2mcp && !endpoint_empty {
        let ep = svc.endpoint.as_deref().unwrap_or("").trim();
        if !ep.starts_with("https://") {
            findings.push(Finding::block(f("endpoint"), "T4", fe::FE11));
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
                findings.push(Finding::block(f("endpoint"), "T4", fe::FE11));
            }
        }
    }

    // ── U5 contradicting standalone A2A / A2MCP token in name/description ──
    if !stype.is_empty() && (is_a2mcp || is_a2a) {
        for (sub, text) in [
            ("name", svc.service_name.as_str()),
            ("servicedescription", svc.service_description.as_str()),
        ] {
            if contradicting_type_token(text, &stype).is_some() {
                findings.push(Finding::block(f(sub), "U5", fe::FE10));
            }
        }
    }

    // ── ServiceName (S1/S3/S4/S6) ─────────────────────────────────────────
    if !svc.service_name.is_empty() {
        let name_chars = svc.service_name.chars().count();
        if !(5..=30).contains(&name_chars) {
            findings.push(Finding::block(f("name"), "S1", fe::FE06));
        }
        if !agent_name.is_empty()
            && svc.service_name.trim().eq_ignore_ascii_case(agent_name.trim())
        {
            findings.push(Finding::block(f("name"), "S3", fe::FE06));
        }
        if contains_price_info(&svc.service_name) {
            findings.push(Finding::block(f("name"), "S4", fe::FE06));
        }
        if has_test_marker(&svc.service_name) {
            findings.push(Finding::block(f("name"), "S6", fe::FE06));
        }
    }

    // ── Pricing (U4/P1..P6) — single fee XOR subscription ────────────────
    check_pricing(index, svc, is_a2mcp, is_a2a, findings);

    // ── Description on servicedescription: FE-21 (required/length) + FE-22
    // (prohibited content). Always run: an empty / blank A2A description is a
    // missing required field (handled inside check_service_description).
    // The length cap is A2A-only — an A2MCP description is the request
    // description governed by FE-16 (skill rule), so it is not length-checked
    // here; only FE-22 prohibited-content runs for A2MCP.
    check_service_description(index, &svc.service_description, is_a2mcp, findings);
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
        findings.push(Finding::block(&fee_field, "P1", fe::FE12));
    };

    let trial = svc.free_trial.as_deref().map(str::trim).unwrap_or("");
    let trial_field = format!("service[{index}].freeTrial");

    if is_a2mcp {
        if has_subscription {
            findings.push(Finding::block(&sub_field, "P3", fe::FE17));
        }
        if !trial.is_empty() {
            findings.push(Finding::block(&trial_field, "P7", fe::FE20));
        }
        if !fee_present {
            findings.push(Finding::block(&fee_field, "U4", fe::FE12));
            findings.push(Finding::block(&fee_field, "P1", fe::FE12));
        } else if !is_plain_number(fee) {
            bad_fee(findings);
        }
        return;
    }

    if is_a2a {
        // A2A must be priced by EXACTLY ONE model — a single fee XOR a subscription.
        if !fee_present && !has_subscription {
            findings.push(Finding::block(&fee_field, "P2", fe::FE17));
        }
        if fee_present && has_subscription {
            findings.push(Finding::block(&fee_field, "P6", fe::FE17));
        }
        if fee_present && !is_plain_number(fee) {
            bad_fee(findings);
        }
        for tier in &svc.subscription {
            if !tier.interval.trim().eq_ignore_ascii_case("month") {
                findings.push(Finding::block(&sub_field, "P4", fe::FE18));
            }
            if !is_plain_number(tier.fee.trim()) {
                findings.push(Finding::block(&sub_field, "P5", fe::FE19));
            }
        }
        // Free trial is subscription-only; must be a positive integer (hours).
        if !trial.is_empty() {
            if !has_subscription {
                findings.push(Finding::block(&trial_field, "P7", fe::FE20));
            }
            if !is_positive_integer(trial) {
                findings.push(Finding::block(&trial_field, "P8", fe::FE20));
            }
        }
        return;
    }

    // Unknown serviceType (T1 already flags the type) — validate fee format only.
    if fee_present && !is_plain_number(fee) {
        bad_fee(findings);
    }
}

// Service description checks split across two doc rules:
//   • FE-21 (必填/长度) — A2A-only (an A2MCP description is the request
//     description, FE-16, skill layer):
//       - empty / blank → D1, BLOCKING (missing required field).
//       - total length over the cap → D2, ADVISORY. Length uses EAST-ASIAN
//         DISPLAY WIDTH (`display_width`: CJK = 2, ASCII = 1): total ≤ 2000 —
//         the spec's "≤ 1000 CJK total". No per-paragraph limit.
//     There is deliberately NO paragraph-count rule: the buyer-facing paragraph
//     layout is guidance in the skill's collection prompts (register.md §3 Step 2c)
//     and is never counted here, so subscription and non-subscription services are
//     validated identically.
//   • FE-22 (禁用内容) — URL (D6) and test/env marker (U1) are BLOCKING;
//     profit/return-guarantee wording (D9: "稳赚 / 保证收益 / 翻倍" …) is
//     ADVISORY. Applies to EVERY service including A2MCP.
// The purely SEMANTIC quality checks (unclear wording, tech-stack leak,
// disclaimers) are FE-23 and live in the skill layer (register.md §4), never in
// this mechanical validator. A2A has no declared-market and no signal-example
// requirement in either layer.
fn check_service_description(
    index: usize,
    desc: &str,
    is_a2mcp: bool,
    findings: &mut Vec<Finding>,
) {
    let field = |sub: &str| format!("service[{index}].{sub}");
    let fd = field("servicedescription");

    // ── Prohibited content — applies to EVERY service (A2A + A2MCP) ────────
    if contains_url(desc) {
        findings.push(Finding::block(&fd, "D6", fe::FE22));
    }
    // D9 is ADVISORY: the hardcoded guarantee-phrase list can only ever be a
    // partial backstop, so it suggests a rewrite instead of blocking registration.
    // The skill layer flags guarantee wording in any language as the same
    // suggestion (register.md §4 step 4).
    if contains_profit_guarantee(desc) {
        findings.push(Finding::suggest(&fd, "D9", fe::FE22));
    }
    if has_test_marker(desc) {
        findings.push(Finding::block(&fd, "U1", fe::FE22));
    }

    // ── Required / length — A2A only ──────────────────────────────────────
    // An A2MCP `serviceDescription` is the request description (skill rule);
    // the buyer-facing length limit does NOT apply here.
    if is_a2mcp {
        return;
    }

    // D1 (empty): a blank A2A description is a missing required field. BLOCKING.
    if desc.trim().is_empty() {
        findings.push(Finding::block(&fd, "D1", fe::FE21));
        return;
    }

    // D2 total display width <= 2000 (= 1000 CJK characters). ADVISORY.
    if display_width(desc) > 2000 {
        findings.push(Finding::suggest(&fd, "D2", fe::FE21));
    }

    // NO paragraph-count check: the 3-part (per-call) / 2-part (subscription)
    // layout is collection-time guidance in the skill (register.md §3 Step 2c),
    // not a mechanical rule — a non-empty A2A description of any shape is valid.
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
