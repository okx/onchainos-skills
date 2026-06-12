//! `onchainos agent validate-listing` — a PURE-LOCAL (no HTTP, no network)
//! validator that checks an agent listing's fields against mechanical
//! marketplace rules and prints a structured JSON result. This moves the
//! deterministic QA that used to live in the markdown skill into the CLI so
//! the checks are reproducible and testable.
//!
//! Scope (deliberately narrow): only MECHANICAL rules are implemented here —
//! length / format / forbidden-marker / structural checks that can be decided
//! without semantic judgment. Anything requiring meaning (is the description
//! actually accurate? is the capability claim plausible?) stays in the skill.
//!
//! Output is the exact `{ "pass": bool, "findings": [...] }` shape; `pass` is
//! true iff there are zero `block`-severity findings. This command never emits
//! `advisory` findings (the only advisory rule was logo/format, which is
//! image-only and we have no image bytes here).

use anyhow::Result;
use serde::Serialize;

use crate::commands::Context;

use super::args::ValidateListingArgs;
use super::models::AgentService;
use super::utils::normalize_role;

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

// ─── Command entry point ────────────────────────────────────────────────────

pub async fn validate_listing(args: ValidateListingArgs, _ctx: &Context) -> Result<()> {
    let role = args
        .role
        .as_deref()
        .and_then(|r| normalize_role(r).ok())
        .unwrap_or_else(|| "provider".to_string());

    let result = run_validation(
        &role,
        args.name.as_deref(),
        args.description.as_deref(),
        args.service.as_deref(),
    );
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
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
    // role. The 3-part structure (D1/D3/D4/D5) is provider-service-only and is
    // NEVER applied to the agent-level description. Agent-level description for
    // providers additionally gets D6/D7 (and U2/U3 already above).
    if !description.is_empty() {
        check_universal_text("description", description, &mut findings);
        if role == "provider" {
            check_description_url_and_addr("description", description, &mut findings);
        }
    }

    // ── Service checks (provider only) ───────────────────────────────────
    if role == "provider" {
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
                        "Provide a JSON array, e.g. [{\"name\":\"...\",\"servicedescription\":\"...\",\"servicetype\":\"A2MCP\",\"fee\":\"0 USDT\",\"endpoint\":\"https://...\"}].",
                    )),
                }
            }
        }
    }
    // For requester / evaluator: --service is ignored silently (no findings).

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
    // N6-encouraged "中文 · English" form) and Latin → 3..=25 chars. Only a
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

    // ── Fee (U4/P1/P2/P3/P4) ─────────────────────────────────────────────
    check_fee(index, svc, is_a2mcp, findings);

    // ── Description (D1-D7) on servicedescription ────────────────────────
    if !svc.service_description.is_empty() {
        check_service_description(index, &svc.service_description, findings);
    }
}

fn check_fee(index: usize, svc: &AgentService, is_a2mcp: bool, findings: &mut Vec<Finding>) {
    let field = format!("service[{index}].fee");
    let fee = svc.fee.trim();

    if fee.is_empty() {
        if is_a2mcp {
            // U4 + P1 for empty A2MCP fee.
            findings.push(Finding::block(
                &field,
                "U4",
                "A2MCP service has an empty fee.",
                "Set an explicit fee, e.g. 0 USDT for free.",
            ));
            findings.push(Finding::block(
                &field,
                "P1",
                "A2MCP fee is required.",
                "Provide a fee like '10 USDT' or a bare number.",
            ));
        }
        // A2A: fee optional → skip silently.
        return;
    }

    // P4 parenthetical after the price.
    if fee.contains('(') || fee.contains(')') {
        findings.push(Finding::block(
            &field,
            "P4",
            "Fee contains a parenthetical note.",
            "Remove the parenthetical; keep only the numeric amount + currency.",
        ));
    }

    // P3 negotiation language.
    if contains_negotiation_language(fee) {
        findings.push(Finding::block(
            &field,
            "P3",
            "Fee contains negotiation language.",
            "Set a concrete fee instead of TBD / negotiable.",
        ));
    }

    // Format + currency: strip a trailing parenthetical for the format check so
    // P4 is the only finding for the paren itself.
    let core = match fee.split_once('(') {
        Some((before, _)) => before.trim(),
        None => fee,
    };
    let (ok_format, currency) = parse_fee_core(core);
    if !ok_format {
        findings.push(Finding::block(
            &field,
            "P1",
            "Fee format is invalid.",
            "Use a number optionally followed by USDT or USDG, e.g. '10 USDT' or '10'.",
        ));
    }
    if let Some(cur) = currency {
        let cur_up = cur.to_ascii_uppercase();
        if cur_up != "USDT" && cur_up != "USDG" {
            findings.push(Finding::block(
                &field,
                "P2",
                "Fee currency must be USDT or USDG.",
                "Use USDT or USDG as the currency.",
            ));
        }
    }
}

fn check_service_description(index: usize, desc: &str, findings: &mut Vec<Finding>) {
    let field = |sub: &str| format!("service[{index}].{sub}");
    let fd = field("servicedescription");

    // D2 total length <= 400.
    if desc.chars().count() > 400 {
        findings.push(Finding::block(
            &fd,
            "D2",
            "Service description exceeds 400 characters.",
            "Trim the description to 400 characters or fewer.",
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

    // 3-part structure: split on newlines into non-empty parts.
    let parts: Vec<&str> = desc
        .split('\n')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();

    if parts.len() < 3 {
        findings.push(Finding::block(
            &fd,
            "D1",
            "Service description must have 3 parts (summary, capabilities, example prompts) separated by newlines.",
            "Provide a one-line summary, a capabilities paragraph, and 1-3 example prompts on separate lines.",
        ));
        return;
    }

    // D3 part1 <= 50.
    if parts[0].chars().count() > 50 {
        findings.push(Finding::block(
            &fd,
            "D3",
            "Description part 1 (summary) exceeds 50 characters.",
            "Shorten the summary to 50 characters or fewer.",
        ));
    }
    // D4 part2 <= 150.
    if parts[1].chars().count() > 150 {
        findings.push(Finding::block(
            &fd,
            "D4",
            "Description part 2 (capabilities) exceeds 150 characters.",
            "Shorten the capabilities part to 150 characters or fewer.",
        ));
    }
    // D5 part3: 1..=3 prompts, each <= 80 chars. Prompts are the remaining
    // lines (everything from part index 2 onward counts as prompt lines).
    let prompts: Vec<&str> = parts[2..].to_vec();
    if prompts.is_empty() || prompts.len() > 3 {
        findings.push(Finding::block(
            &fd,
            "D5",
            "Description part 3 must contain 1-3 example prompts.",
            "Provide between 1 and 3 example prompts.",
        ));
    } else if prompts.iter().any(|p| p.chars().count() > 80) {
        findings.push(Finding::block(
            &fd,
            "D5",
            "An example prompt exceeds 80 characters.",
            "Keep each example prompt to 80 characters or fewer.",
        ));
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

/// U3: negative-capability phrases (case-insensitive substring).
fn contains_negative_capability(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    const EN: &[&str] = &[
        "currently not supported",
        "does not support",
        "not supported yet",
    ];
    if EN.iter().any(|p| lower.contains(p)) {
        return true;
    }
    // CJK phrases are not ASCII-lowercased meaningfully; match on raw.
    s.contains("暂不支持") || s.contains("不支持")
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
/// (case-insensitive) OR the standalone word `free` / `免费`.
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

/// P3: negotiation language in a fee string.
fn contains_negotiation_language(fee: &str) -> bool {
    let lower = fee.to_ascii_lowercase();
    const EN: &[&str] = &["tbd", "negotiable", "flexible"];
    if EN.iter().any(|w| standalone_word(&lower, w)) {
        return true;
    }
    fee.contains("面议") || fee.contains("协商")
}

/// Parse the "core" fee (parenthetical already stripped). Returns
/// (format_ok, detected_currency_token). Accepts:
///   `^\d+(\.\d{1,6})?$`                 (bare numeric)
///   `^\d+(\.\d{1,6})?\s+[A-Za-z]+$`     (numeric + currency token)
/// The currency token (if present) is returned so P2 can validate it. A bare
/// numeric returns currency=None. Malformed → (false, None | Some(...)).
fn parse_fee_core(core: &str) -> (bool, Option<String>) {
    let core = core.trim();
    if core.is_empty() {
        return (false, None);
    }
    // Split into number part and optional currency part on whitespace.
    let mut it = core.split_whitespace();
    let num = it.next().unwrap_or("");
    let cur = it.next();
    let extra = it.next();

    let num_ok = is_valid_numeric(num);

    match (cur, extra) {
        (None, None) => (num_ok, None),
        (Some(c), None) => {
            // currency token must be alphabetic; report it for P2 either way.
            let cur_alpha = c.chars().all(|ch| ch.is_ascii_alphabetic()) && !c.is_empty();
            (num_ok && cur_alpha, Some(c.to_string()))
        }
        // Anything beyond "<num> <cur>" is malformed.
        _ => (false, cur.map(str::to_string)),
    }
}

fn is_valid_numeric(s: &str) -> bool {
    match s.split_once('.') {
        None => !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()),
        Some((int, frac)) => {
            !int.is_empty()
                && int.bytes().all(|b| b.is_ascii_digit())
                && (1..=6).contains(&frac.len())
                && frac.bytes().all(|b| b.is_ascii_digit())
        }
    }
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
