use super::*;

fn svc(name: &str, desc: &str, stype: &str, fee: &str, endpoint: Option<&str>) -> String {
    let ep = match endpoint {
        Some(e) => format!(",\"endpoint\":\"{e}\""),
        None => String::new(),
    };
    // Escape actual newlines so the generated JSON string is always valid.
    // Original tests pass "\\n" (backslash+n) which is already a JSON escape; new tests
    // may pass "\n" (real newline char). Both end up as a JSON \n escape after this replace.
    let desc_json = desc.replace('\n', "\\n");
    format!(
            "[{{\"serviceName\":\"{name}\",\"serviceDescription\":\"{desc_json}\",\"serviceType\":\"{stype}\",\"fee\":\"{fee}\"{ep}}}]"
        )
}

fn codes(r: &ValidationResult) -> Vec<String> {
    r.findings.iter().map(|f| f.code.clone()).collect()
}

/// Severity of the first finding carrying `code` (the test module is a child of
/// `validate`, so the private `Finding.severity` / `ValidationResult.findings`
/// fields are readable — same access `codes()` uses for `.code`).
fn severity_of(r: &ValidationResult, code: &str) -> Option<&'static str> {
    r.findings.iter().find(|f| f.code == code).map(|f| f.severity)
}

#[test]
fn every_name_finding_unifies_to_same_message() {
    // Requirement ①: multiple sub-checks under one rule group (name) all return
    // the SAME unified 提示文案 in `message`, regardless of which internal code
    // (U1/N1/N3/…) fired. Bad name below trips several name sub-checks.
    let r = run_validation("asp", Some("Trump_v2(test)#3!"), None, None);
    let name_findings: Vec<&Finding> = r.findings.iter().filter(|f| f.field == "name").collect();
    assert!(name_findings.len() > 1, "expected several name findings, got {:?}", codes(&r));
    let expected_msg = super::fe::FE03;
    for f in &name_findings {
        assert_eq!(f.message, expected_msg, "name finding {:?} message not unified", f.code);
    }
}

#[test]
fn clean_asp_passes() {
    // 3-part description, good name, valid A2MCP service.
    let desc = "Summarizes text.\\nHandles long docs and articles.\\nSummarize this article";
    let service = svc(
        "Document Summarizer",
        desc,
        "A2MCP",
        "10",
        Some("https://example.com/mcp"),
    );
    let r = run_validation(
        "asp",
        Some("Summarizer Bot"),
        Some("A helpful agent."),
        Some(&service),
    );
    // Note: the literal \n above is an escaped newline in the JSON string, so
    // serde turns it into a real newline → 3 parts.
    assert!(r.pass, "expected pass, got {:?}", codes(&r));
}

#[test]
fn name_with_test_marker_fails_u1() {
    let r = run_validation("asp", Some("FitnessBot(test)"), None, None);
    assert!(codes(&r).contains(&"U1".to_string()));
    assert!(!r.pass);
}

#[test]
fn name_predict_does_not_fail_u1() {
    // "Predict" contains "pre" but is not a delimited marker.
    let r = run_validation("user", Some("Predict"), None, None);
    assert!(
        !codes(&r).contains(&"U1".to_string()),
        "got {:?}",
        codes(&r)
    );
}

#[test]
fn protest_does_not_fail_u1() {
    assert!(!has_test_marker("protest"));
    assert!(!has_test_marker("Predict"));
}

#[test]
fn a2mcp_empty_endpoint_fails_t2() {
    let service = svc(
        "Some MCP Service",
        "Does a thing.\\nMore detail here.\\nDo the thing",
        "A2MCP",
        "5",
        Some(""),
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"T2".to_string()), "got {:?}", codes(&r));
    assert!(!r.pass);
}

#[test]
fn a2a_with_endpoint_fails_t3() {
    let service = svc(
        "Some A2A Service",
        "Does a thing.\\nMore detail here.\\nDo the thing",
        "A2A",
        "5",
        Some("https://example.com"),
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"T3".to_string()), "got {:?}", codes(&r));
    assert!(!r.pass);
}

#[test]
fn fee_with_negotiation_or_paren_fails_p1() {
    // The spec no longer enumerates parenthetical / negotiation cases as
    // separate rules; any non-plain-number fee is rejected via P1 (and there
    // is no longer a P3 / P4 code). Covers ASCII parens, fullwidth parens, and
    // negotiation wording.
    for fee in &["0.2 (negotiable)", "0.05 USDT（支持 USDG 结算）", "按复杂度协商"] {
        let service = svc(
            "Pricing Service",
            "Does a thing.\\nMore detail here.\\nDo the thing",
            "A2A",
            fee,
            None,
        );
        let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
        let c = codes(&r);
        assert!(c.contains(&"P1".to_string()), "fee={fee} got {:?}", c);
        assert!(!c.contains(&"P3".to_string()), "P3 retired, fee={fee} got {:?}", c);
        assert!(!c.contains(&"P4".to_string()), "P4 retired, fee={fee} got {:?}", c);
        assert!(!r.pass);
    }
}

#[test]
fn fee_with_currency_token_fails_p1() {
    // Fee must be a plain number — USDT is implicit. Any currency token (even a
    // valid one) makes the fee non-numeric → P1. (The old USDT/USDG currency
    // check, P2, has been removed.)
    for fee in &["5 ETH", "5 USDT", "5 USDG"] {
        let service = svc(
            "Pricing Service",
            "Does a thing.\\nMore detail here.\\nDo the thing",
            "A2A",
            fee,
            None,
        );
        let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
        assert!(codes(&r).contains(&"P1".to_string()), "fee={fee} got {:?}", codes(&r));
        assert!(!r.pass);
    }
}

#[test]
fn a2a_with_both_fee_and_subscription_fails_p6() {
    // A2A billing models are mutually exclusive: a real single-purchase fee
    // AND a subscription together must be flagged P6 (choose exactly one).
    let desc = "Does a thing.\\nMore detail here.\\nDo the thing";
    let service = format!(
        "[{{\"serviceName\":\"Pricing Service\",\"serviceDescription\":\"{desc}\",\"serviceType\":\"A2A\",\"fee\":\"0.11\",\"subscription\":[{{\"interval\":\"month\",\"fee\":\"10\"}}]}}]"
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"P6".to_string()), "got {:?}", codes(&r));
    assert!(!r.pass);
}

#[test]
fn a2a_subscription_only_passes() {
    // Subscription-priced A2A (empty single fee) is a valid single model → no
    // pricing findings, no P2/P6.
    let desc = "Does a thing.\\nMore detail here.\\nDo the thing";
    let service = format!(
        "[{{\"serviceName\":\"Pricing Service\",\"serviceDescription\":\"{desc}\",\"serviceType\":\"A2A\",\"fee\":\"\",\"subscription\":[{{\"interval\":\"month\",\"fee\":\"10\"}}]}}]"
    );
    let r = run_validation("asp", Some("Agent Name"), Some("A helpful agent."), Some(&service));
    let c = codes(&r);
    assert!(!c.contains(&"P2".to_string()), "unexpected P2; got {:?}", c);
    assert!(!c.contains(&"P6".to_string()), "unexpected P6; got {:?}", c);
}

#[test]
fn a2a_subscription_with_valid_free_trial_passes() {
    // A subscription-priced A2A with a positive integer freeTrial (hours) is
    // valid → no P7/P8.
    let desc = "Does a thing.\\nMore detail here.\\nDo the thing";
    let service = format!(
        "[{{\"serviceName\":\"Pricing Service\",\"serviceDescription\":\"{desc}\",\"serviceType\":\"A2A\",\"fee\":\"\",\"subscription\":[{{\"interval\":\"month\",\"fee\":\"10\"}}],\"freeTrial\":\"72\"}}]"
    );
    let r = run_validation("asp", Some("Agent Name"), Some("A helpful agent."), Some(&service));
    let c = codes(&r);
    assert!(!c.contains(&"P7".to_string()), "unexpected P7; got {:?}", c);
    assert!(!c.contains(&"P8".to_string()), "unexpected P8; got {:?}", c);
}

#[test]
fn a2a_free_trial_without_subscription_fails_p7() {
    // freeTrial is subscription-only: on a single-purchase A2A it must be flagged
    // P7.
    let desc = "Does a thing.\\nMore detail here.\\nDo the thing";
    let service = format!(
        "[{{\"serviceName\":\"Pricing Service\",\"serviceDescription\":\"{desc}\",\"serviceType\":\"A2A\",\"fee\":\"0.11\",\"freeTrial\":\"72\"}}]"
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"P7".to_string()), "got {:?}", codes(&r));
    assert!(!r.pass);
}

#[test]
fn a2a_free_trial_non_integer_fails_p8() {
    // freeTrial must be a positive integer number of hours: a decimal / zero is
    // flagged P8.
    let desc = "Does a thing.\\nMore detail here.\\nDo the thing";
    let service = format!(
        "[{{\"serviceName\":\"Pricing Service\",\"serviceDescription\":\"{desc}\",\"serviceType\":\"A2A\",\"fee\":\"\",\"subscription\":[{{\"interval\":\"month\",\"fee\":\"10\"}}],\"freeTrial\":\"24.5\"}}]"
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"P8".to_string()), "got {:?}", codes(&r));
    assert!(!r.pass);
}

#[test]
fn a2mcp_free_trial_fails_p7() {
    // A2MCP does not support a free trial → P7.
    let desc = "Does a thing.\\nMore detail here.\\nDo the thing";
    let service = format!(
        "[{{\"serviceName\":\"Pricing Service\",\"serviceDescription\":\"{desc}\",\"serviceType\":\"A2MCP\",\"fee\":\"0.5\",\"endpoint\":\"https://api.example.com/mcp\",\"freeTrial\":\"72\"}}]"
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"P7".to_string()), "got {:?}", codes(&r));
    assert!(!r.pass);
}

#[test]
fn too_short_service_name_fails_s1() {
    let service = svc(
        "Q",
        "Does a thing.\\nMore detail here.\\nDo the thing",
        "A2A",
        "5",
        None,
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"S1".to_string()), "got {:?}", codes(&r));
    assert!(!r.pass);
}

#[test]
fn user_ignores_service() {
    // A bad service should NOT produce findings for a user role.
    let service = svc("Q", "x", "BADTYPE", "", None);
    let r = run_validation("user", Some("Buyer Bot"), None, Some(&service));
    assert!(r.pass, "got {:?}", codes(&r));
}

#[test]
fn bilingual_name_without_middledot_fails_n6() {
    let r = run_validation("asp", Some("健身 Bot"), None, None);
    assert!(codes(&r).contains(&"N6".to_string()), "got {:?}", codes(&r));
}

#[test]
fn bilingual_name_with_middledot_ok_n6() {
    let r = run_validation("asp", Some("健身 \u{00B7} Bot"), None, None);
    assert!(
        !codes(&r).contains(&"N6".to_string()),
        "got {:?}",
        codes(&r)
    );
}

#[test]
fn long_bilingual_name_not_blocked_by_cjk_length_cap() {
    // 22 chars: a mixed CJK + Latin name (N6-compliant separator) must use
    // the 3..=25 bound, NOT the dense pure-CJK 12-char cap, so no N1.
    let r = run_validation(
        "asp",
        Some("健身 \u{00B7} Fitness Coach Pro"),
        None,
        None,
    );
    assert!(
        !codes(&r).contains(&"N1".to_string()),
        "got {:?}",
        codes(&r)
    );
    assert!(
        !codes(&r).contains(&"N6".to_string()),
        "got {:?}",
        codes(&r)
    );
}

#[test]
fn pure_cjk_over_twelve_chars_fails_n1() {
    // 13 pure-CJK chars: still bounded by the 2..=12 cap.
    let r = run_validation("asp", Some("一二三四五六七八九十一二三"), None, None);
    assert!(codes(&r).contains(&"N1".to_string()), "got {:?}", codes(&r));
}

#[test]
fn hex_in_service_description_emits_d7_not_duplicate_u2() {
    // A 0x address in `servicedescription` must surface once as D7, never
    // also as U2 for the same field (no duplicate diagnostic).
    let desc = "Summarizes text 0xdeadbeefdeadbeef.\\nHandles long docs.\\nSummarize this";
    let service = svc("Document Summarizer", desc, "A2A", "0", None);
    let r = run_validation("asp", Some("Summary Bot"), None, Some(&service));
    let desc_field = "service[0].servicedescription";
    let desc_codes: Vec<&str> = r
        .findings
        .iter()
        .filter(|f| f.field == desc_field)
        .map(|f| f.code.as_str())
        .collect();
    assert!(
        desc_codes.contains(&"D7"),
        "expected D7, got {desc_codes:?}"
    );
    assert!(
        !desc_codes.contains(&"U2"),
        "U2 must not duplicate D7, got {desc_codes:?}"
    );
}

#[test]
fn hex_address_in_name_fails_u2() {
    let r = run_validation("user", Some("Agent 0xdeadbeef"), None, None);
    assert!(codes(&r).contains(&"U2".to_string()));
}

#[test]
fn embedded_id_fails_n2() {
    let r = run_validation("asp", Some("Helper Bot 3"), None, None);
    assert!(codes(&r).contains(&"N2".to_string()), "got {:?}", codes(&r));
}

#[test]
fn bare_numeric_fee_ok() {
    let service = svc(
        "Numeric Fee Service",
        "Does a thing.\\nMore detail here.\\nDo the thing",
        "A2MCP",
        "10",
        Some("https://example.com/mcp"),
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    let c = codes(&r);
    assert!(!c.contains(&"P1".to_string()), "got {:?}", c);
}

#[test]
fn fee_with_currency_suffix_fails_p1() {
    // A currency token attached to the number (with or without a space) is no
    // longer accepted — the fee must be a plain number, USDT implicit.
    for fee in &["1USDT", "1.5USDG", "10USDT", "0USDG"] {
        let service = svc(
            "Some Service",
            "Does a thing.\nMore detail here.\nDo the thing",
            "A2MCP",
            fee,
            Some("https://example.com/mcp"),
        );
        let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
        assert!(codes(&r).contains(&"P1".to_string()), "fee={fee} got {:?}", codes(&r));
    }
}

// ─── N1: Latin/mixed name length boundary values ──────────────────────────

#[test]
fn latin_name_two_chars_fails_n1() {
    // 2 chars — below the 3-char Latin minimum.
    let r = run_validation("asp", Some("AB"), None, None);
    assert!(codes(&r).contains(&"N1".to_string()), "got {:?}", codes(&r));
}

#[test]
fn latin_name_three_chars_passes_n1() {
    let r = run_validation("asp", Some("Bot"), None, None);
    assert!(!codes(&r).contains(&"N1".to_string()), "got {:?}", codes(&r));
}

#[test]
fn latin_name_twenty_five_chars_passes_n1() {
    // Exactly 25 chars — upper bound is inclusive.
    let r = run_validation("asp", Some("ABCDEFGHIJKLMNOPQRSTUVWXY"), None, None);
    assert!(!codes(&r).contains(&"N1".to_string()), "got {:?}", codes(&r));
}

#[test]
fn latin_name_twenty_six_chars_fails_n1() {
    // 26 chars — one over the limit.
    let r = run_validation("asp", Some("ABCDEFGHIJKLMNOPQRSTUVWXYZ"), None, None);
    assert!(codes(&r).contains(&"N1".to_string()), "got {:?}", codes(&r));
}

// ─── N3: ordinal suffix ────────────────────────────────────────────────────

#[test]
fn ordinal_suffix_v2_fails_n3() {
    assert!(has_ordinal_suffix("Agent_v2"));
    let r = run_validation("asp", Some("Agent_v2"), None, None);
    assert!(codes(&r).contains(&"N3".to_string()), "got {:?}", codes(&r));
}

#[test]
fn ordinal_suffix_paren_digit_fails_n3() {
    assert!(has_ordinal_suffix("Agent Bot (2)"));
    let r = run_validation("asp", Some("Agent Bot (2)"), None, None);
    assert!(codes(&r).contains(&"N3".to_string()), "got {:?}", codes(&r));
}

#[test]
fn hash_suffix_triggers_n3() {
    // "Bot#3": has_ordinal_suffix detects the trailing #3, has_embedded_agent_id detects #3,
    // and has_decorative_symbols detects '#' in DECOR — so N2 + N3 + N8 all fire together.
    assert!(has_ordinal_suffix("Bot#3"));
    let r = run_validation("asp", Some("Bot#3"), None, None);
    let c = codes(&r);
    assert!(c.contains(&"N3".to_string()), "expected N3, got {:?}", c);
    assert!(c.contains(&"N2".to_string()), "expected N2 (hash marker_digit_run), got {:?}", c);
    assert!(c.contains(&"N8".to_string()), "expected N8 ('#' in DECOR), got {:?}", c);
}

#[test]
fn plain_name_does_not_fail_n3() {
    // "Bot Proto" contains no ordinal suffix, no trailing number, no decorative symbols.
    assert!(!has_ordinal_suffix("Bot Proto"));
    let r = run_validation("asp", Some("Bot Proto"), None, None);
    assert!(!codes(&r).contains(&"N3".to_string()), "got {:?}", codes(&r));
}

#[test]
fn ordinal_suffix_no3_fails_n3() {
    assert!(has_ordinal_suffix("BotNo3"));
    assert!(has_ordinal_suffix("Bot No.3"));
}

#[test]
fn name_without_ordinal_passes_n3() {
    assert!(!has_ordinal_suffix("AgentBot"));
    assert!(!has_ordinal_suffix("Predict v2 future")); // v2 not at end alone
}

// ─── N8: decorative symbols ────────────────────────────────────────────────

#[test]
fn exclamation_fails_n8() {
    assert!(has_decorative_symbols("Bot!"));
    let r = run_validation("asp", Some("My Bot!"), None, None);
    assert!(codes(&r).contains(&"N8".to_string()), "got {:?}", codes(&r));
}

#[test]
fn slash_fails_n8() {
    assert!(has_decorative_symbols("Buy/Sell Bot"));
}

#[test]
fn leading_hyphen_fails_n8() {
    assert!(has_decorative_symbols("-BotName"));
}

#[test]
fn trailing_hyphen_fails_n8() {
    assert!(has_decorative_symbols("BotName-"));
}

#[test]
fn internal_hyphen_allowed_n8() {
    // A single internal hyphen joining two words is explicitly allowed.
    assert!(!has_decorative_symbols("Trade-Bot"));
}

#[test]
fn standalone_hyphen_fails_n8() {
    assert!(has_decorative_symbols("A - B"));
}

// ─── U3: negative-capability phrase ───────────────────────────────────────

#[test]
fn name_with_negative_capability_fails_u3() {
    let r = run_validation("asp", Some("Currently not supported"), None, None);
    assert!(codes(&r).contains(&"U3".to_string()), "got {:?}", codes(&r));
}

#[test]
fn description_with_negative_capability_fails_u3() {
    let r = run_validation(
        "asp",
        Some("GoodBot"),
        Some("currently not supported for this chain"),
        None,
    );
    assert!(codes(&r).contains(&"U3".to_string()), "got {:?}", codes(&r));
}

#[test]
fn cjk_negative_capability_fails_u3() {
    assert!(contains_negative_capability("暂不支持"));
    assert!(contains_negative_capability("目前不支持"));
}

#[test]
fn copytrading_delivery_note_is_not_negative_capability_u3() {
    // The field-5 part-3 delivery descriptor "(不)支持跟单" is a delivery
    // attribute, NOT a capability gap — it must never trip U3. This is the
    // spec's own correct normal-service example wording.
    assert!(!contains_negative_capability("交付物形式为文件，不支持跟单。"));
    assert!(!contains_negative_capability("交付物形式为结构化信号，支持跟单。"));
    // A bare "不支持" without the "目前/暂" gap framing no longer fires either.
    assert!(!contains_negative_capability("不支持"));
}

#[test]
fn normal_description_passes_u3() {
    assert!(!contains_negative_capability("Handles trading on multiple chains."));
}

// ─── U4 + P1: A2MCP empty fee ─────────────────────────────────────────────

#[test]
fn a2mcp_empty_fee_fails_u4_and_p1() {
    let service = svc(
        "My MCP Service",
        "Summarizes text.\nHandles long docs.\nSummarize this",
        "A2MCP",
        "",
        Some("https://example.com/mcp"),
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    let c = codes(&r);
    assert!(c.contains(&"U4".to_string()), "expected U4, got {:?}", c);
    assert!(c.contains(&"P1".to_string()), "expected P1, got {:?}", c);
}

// ─── P1: invalid fee format ────────────────────────────────────────────────

#[test]
fn non_numeric_fee_fails_p1() {
    let service = svc(
        "Some Service",
        "Does a thing.\nMore detail here.\nDo the thing",
        "A2A",
        "much_money",
        None,
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"P1".to_string()), "got {:?}", codes(&r));
}

#[test]
fn fee_with_extra_token_fails_p1() {
    // Three tokens: number + currency + extra → malformed.
    let service = svc(
        "Some Service",
        "Does a thing.\nMore detail here.\nDo the thing",
        "A2A",
        "10 USDT extra",
        None,
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"P1".to_string()), "got {:?}", codes(&r));
}

// ─── S3: service name duplicates agent name ────────────────────────────────

#[test]
fn service_name_same_as_agent_name_fails_s3() {
    let service = svc(
        "Agent Name",
        "Does a thing.\nMore detail here.\nDo the thing",
        "A2A",
        "5",
        None,
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"S3".to_string()), "got {:?}", codes(&r));
}

#[test]
fn service_name_case_insensitive_duplicate_fails_s3() {
    let service = svc(
        "agent name",
        "Does a thing.\nMore detail here.\nDo the thing",
        "A2A",
        "5",
        None,
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"S3".to_string()), "got {:?}", codes(&r));
}

#[test]
fn service_name_different_from_agent_passes_s3() {
    let service = svc(
        "Trade Executor",
        "Does a thing.\nMore detail here.\nDo the thing",
        "A2A",
        "5",
        None,
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(!codes(&r).contains(&"S3".to_string()), "got {:?}", codes(&r));
}

// ─── S4: service name contains price info ─────────────────────────────────

#[test]
fn service_name_with_usdt_fails_s4() {
    let service = svc(
        "Pay 5 USDT Service",
        "Does a thing.\nMore detail here.\nDo the thing",
        "A2A",
        "5",
        None,
    );
    let r = run_validation("asp", Some("Other Agent"), None, Some(&service));
    assert!(codes(&r).contains(&"S4".to_string()), "got {:?}", codes(&r));
}

#[test]
fn service_name_with_free_fails_s4() {
    let service = svc(
        "Get Access Free",
        "Does a thing.\nMore detail here.\nDo the thing",
        "A2A",
        "0",
        None,
    );
    let r = run_validation("asp", Some("Other Agent"), None, Some(&service));
    assert!(codes(&r).contains(&"S4".to_string()), "got {:?}", codes(&r));
}

// ─── S6: service name with test marker ────────────────────────────────────

#[test]
fn service_name_with_test_marker_fails_s6() {
    let service = svc(
        "Trade Bot (test)",
        "Does a thing.\nMore detail here.\nDo the thing",
        "A2A",
        "5",
        None,
    );
    let r = run_validation("asp", Some("Other Agent"), None, Some(&service));
    assert!(codes(&r).contains(&"S6".to_string()), "got {:?}", codes(&r));
}

// ─── U5: contradicting type token ─────────────────────────────────────────

#[test]
fn a2a_service_name_mentioning_a2mcp_fails_u5() {
    let service = svc(
        "My A2MCP Service",
        "Does a thing.\nMore detail here.\nDo the thing",
        "A2A",
        "5",
        None,
    );
    let r = run_validation("asp", Some("Other Agent"), None, Some(&service));
    assert!(codes(&r).contains(&"U5".to_string()), "got {:?}", codes(&r));
}

#[test]
fn a2mcp_service_name_mentioning_a2a_fails_u5() {
    let service = svc(
        "Use a2a protocol",
        "Does a thing.\nMore detail here.\nDo the thing",
        "A2MCP",
        "5",
        Some("https://example.com/mcp"),
    );
    let r = run_validation("asp", Some("Other Agent"), None, Some(&service));
    assert!(codes(&r).contains(&"U5".to_string()), "got {:?}", codes(&r));
}

#[test]
fn a2a_not_contradicted_by_a2mcp_substring() {
    // "a2mcp" contains "a2a" as a prefix — ensure standalone_word prevents false positive.
    assert_eq!(contradicting_type_token("a2mcp helper", "A2MCP"), None);
    assert_eq!(contradicting_type_token("use a2a calls", "A2A"), None);
}

// ─── T1: invalid servicetype ───────────────────────────────────────────────

#[test]
fn invalid_servicetype_fails_t1() {
    let service = svc(
        "Some Service",
        "Does a thing.\nMore detail here.\nDo the thing",
        "REST",
        "5",
        None,
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"T1".to_string()), "got {:?}", codes(&r));
}

#[test]
fn empty_servicetype_fails_t1() {
    let service = svc(
        "Some Service",
        "Does a thing.\nMore detail here.\nDo the thing",
        "",
        "5",
        None,
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"T1".to_string()), "got {:?}", codes(&r));
}

// ─── PARSE: invalid JSON array ─────────────────────────────────────────────

#[test]
fn invalid_service_json_fails_parse() {
    let r = run_validation("asp", Some("Agent Name"), None, Some("not json at all"));
    assert!(codes(&r).contains(&"PARSE".to_string()), "got {:?}", codes(&r));
    assert!(!r.pass);
}

#[test]
fn service_json_object_not_array_fails_parse() {
    let r = run_validation(
        "asp",
        Some("Agent Name"),
        None,
        Some("{\"name\":\"foo\"}"),
    );
    assert!(codes(&r).contains(&"PARSE".to_string()), "got {:?}", codes(&r));
}

#[test]
fn user_ignores_invalid_service_json() {
    // User silently ignores --service regardless of content.
    let r = run_validation("user", Some("Buyer Bot"), None, Some("not json"));
    assert!(r.pass, "got {:?}", codes(&r));
}

// ─── D1–D9: service description structure (three-part, display-width) ──────

#[test]
fn description_single_line_fails_d1() {
    // Only one non-empty line → part 2 absent → D1.
    let service = svc(
        "Doc Summarizer",
        "Does one thing only",
        "A2A",
        "5",
        None,
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"D1".to_string()), "got {:?}", codes(&r));
}

#[test]
fn description_empty_fails_d1() {
    let service = svc("Doc Summarizer", "", "A2A", "5", None);
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"D1".to_string()), "got {:?}", codes(&r));
}

#[test]
fn description_two_parts_non_subscription_suggests_d1() {
    // FE-21 structural downgrade (spec §2.1): a NON-subscription (single-fee) A2A
    // service should have EXACTLY 3 paragraphs. A 2-paragraph description is the
    // wrong count → D1 STILL fires, but now as an ADVISORY ("suggest") finding, so
    // the listing PASSES (pass: true) while surfacing the suggestion.
    let service = svc(
        "Doc Summarizer",
        "Summary line.\nProvide: a document and a target language.",
        "A2A",
        "5",
        None,
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"D1".to_string()), "got {:?}", codes(&r));
    assert_eq!(severity_of(&r, "D1"), Some("suggest"), "D1 must be advisory, got {:?}", codes(&r));
    assert!(r.pass, "structural D1 must not block A2A; got {:?}", codes(&r));
}

#[test]
fn description_two_parts_subscription_passes_d1() {
    // FE-21: a SUBSCRIPTION-priced A2A service uses EXACTLY 2 paragraphs
    // (1. core capabilities; 2. what will be delivered) → no D1.
    let service = "[{\"serviceName\":\"Pricing Service\",\"serviceDescription\":\"Daily signals across DEX.\\nDelivered as structured signals; copy-trading supported.\",\"serviceType\":\"A2A\",\"fee\":\"\",\"subscription\":[{\"interval\":\"month\",\"fee\":\"10\"}]}]";
    let r = run_validation("asp", Some("Agent Name"), Some("A helpful agent."), Some(service));
    assert!(!codes(&r).contains(&"D1".to_string()), "got {:?}", codes(&r));
    assert!(r.pass, "got {:?}", codes(&r));
}

#[test]
fn description_three_parts_subscription_suggests_d1() {
    // FE-21 structural downgrade (spec §2.1): a subscription A2A service should
    // have EXACTLY 2 paragraphs. 3 paragraphs is the wrong count → D1 STILL fires,
    // now as an ADVISORY ("suggest") finding → the listing PASSES. Advisory applies
    // uniformly to all A2A regardless of billing model (no billing-model branch).
    let service = "[{\"serviceName\":\"Pricing Service\",\"serviceDescription\":\"Daily signals across DEX.\\nNothing to provide.\\nDelivered as structured signals.\",\"serviceType\":\"A2A\",\"fee\":\"\",\"subscription\":[{\"interval\":\"month\",\"fee\":\"10\"}]}]";
    let r = run_validation("asp", Some("Agent Name"), None, Some(service));
    assert!(codes(&r).contains(&"D1".to_string()), "got {:?}", codes(&r));
    assert_eq!(severity_of(&r, "D1"), Some("suggest"), "D1 must be advisory, got {:?}", codes(&r));
    assert!(r.pass, "structural D1 must not block A2A; got {:?}", codes(&r));
}

#[test]
fn a2a_empty_description_still_blocks_d1() {
    // Empty/blank A2A serviceDescription is a MISSING-REQUIRED-FIELD error and
    // stays BLOCKING (spec §2.1 / Change 2a) — this guards the empty-vs-structural
    // split. `parse_services_lenient` trims, so a whitespace-only description
    // collapses to "" and hits the empty-D1 block branch.
    let service = svc("Doc Summarizer", "   ", "A2A", "5", None);
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"D1".to_string()), "got {:?}", codes(&r));
    assert_eq!(severity_of(&r, "D1"), Some("block"), "empty D1 must block, got {:?}", codes(&r));
    assert!(!r.pass, "empty A2A description must block; got {:?}", codes(&r));
}

#[test]
fn description_over_width_single_paragraph_suggests_not_blocks() {
    // FE-21 length downgrade (spec §2.1): a single-paragraph A2A description whose
    // only line exceeds the 400-width per-paragraph limit fires D3 — as an ADVISORY
    // ("suggest") finding — and the wrong (1) paragraph count fires D1 (also
    // advisory), so the listing still PASSES (non-empty, non-prohibited).
    let p1 = "A".repeat(401);
    let service = svc("Doc Summarizer", &p1, "A2A", "5", None);
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    let c = codes(&r);
    assert!(c.contains(&"D3".to_string()), "expected D3, got {:?}", c);
    assert_eq!(severity_of(&r, "D3"), Some("suggest"), "D3 must be advisory, got {:?}", c);
    assert!(r.pass, "over-width structural findings must not block A2A; got {:?}", c);
}

#[test]
fn a2mcp_description_skips_structure_no_d1() {
    // FE-21 is A2A-only. An A2MCP `serviceDescription` is the request description
    // (FE-16, skill rule) — its buyer-facing paragraph structure is NOT checked
    // here, so a 1-paragraph A2MCP description must NOT trip D1.
    let service = svc(
        "Doc Summarizer",
        "1.[Service Description] summarizes text",
        "A2MCP",
        "10",
        Some("https://example.com/mcp"),
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(!codes(&r).contains(&"D1".to_string()), "got {:?}", codes(&r));
}

#[test]
fn a2mcp_description_still_blocks_prohibited_content_fe22() {
    // FE-22 (prohibited content) applies to EVERY service, including A2MCP:
    // a URL in an A2MCP description must still surface D6 and block.
    let service = svc(
        "Doc Summarizer",
        "Summarizes text, see https://example.com",
        "A2MCP",
        "10",
        Some("https://example.com/mcp"),
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"D6".to_string()), "got {:?}", codes(&r));
    assert!(!r.pass);
}

#[test]
fn description_three_parts_passes() {
    // Line 1 = summary, line 2 = what to provide, line 3 = delivery note → clean.
    let service = svc(
        "Doc Summarizer",
        "Summarizes docs for busy analysts.\nProvide: 1. a document 2. a target language.\nDelivery: a markdown file; no copy-trading.",
        "A2A",
        "5",
        None,
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(r.pass, "got {:?}", codes(&r));
}

#[test]
fn spec_normal_service_correct_example_passes() {
    // The spec's own "正确示范（普通服务）" — a 3-part description whose part 3 is
    // "交付物形式为文件，不支持跟单". This MUST pass QA (regression: the bare-"不支持"
    // U3 rule used to block the spec's own example).
    let service = svc(
        "Meme Token 一键发币",
        "提供 Meme token 一键发行能力，只需要图片和名称，就可以帮你发布 memetoken。\n需要提供：Meme token 的图片，和名称。\n提供的交付物形式为文件，不支持跟单。",
        "A2A",
        "5",
        None,
    );
    let r = run_validation("asp", Some("MemeGo"), None, Some(&service));
    assert!(r.pass, "spec normal-service example must pass, got {:?}", codes(&r));
}

#[test]
fn spec_trading_signal_correct_example_passes() {
    // The spec's own "正确示范（交易信号类）" — a 3-part description whose part 3
    // carries the canonical structured signal example. This MUST pass QA. It is a
    // regression for several description edge cases that co-occur in a real
    // signal example and are easy to break by tightening a rule:
    //   • truncated token address "0x12…ab" — < 6 trailing hex digits, so it must
    //     NOT trip D7 (a full 0x address would); the spec writes addresses this way.
    //   • pipe / "$TOKEN" / "≤" / "%" symbols in the delivery note — description
    //     has no decorative-symbol rule (that is name-only N8), so they must pass.
    //   • "支持跟单" delivery attribute — must NOT trip U3 (bare-"不支持" narrowing).
    let service = svc(
        "跟单信号订阅服务",
        "面向链上交易者的跟单信号服务，覆盖 DEX、Polymarket 预测市场、Hyperliquid 合约三个市场，每日推送 3-5 条可执行信号，统一含方向、入场、止盈止损、建议仓位与有效期。\n无需提供额外材料，订阅后自动接收所支持市场的全部信号。\n交付物形式为结构化信号，支持跟单。信号示例：DEX 信号：X Layer | $TOKEN (0x12…ab) | BUY | 0.042-0.045 | 滑点 ≤1% | 仓位 5% | 24h 内有效。",
        "A2A",
        "5",
        None,
    );
    let r = run_validation("asp", Some("SignalRaven"), None, Some(&service));
    assert!(
        r.pass,
        "spec trading-signal example must pass (truncated 0x12…ab must not trip D7), got {:?}",
        codes(&r)
    );
}

#[test]
fn description_part3_over_400_width_fails_d5() {
    // Part 3 = the 3rd line; 401 half-width chars > 400 width → D5 (not D3/D4).
    let p3 = "C".repeat(401);
    let desc = format!("Short summary.\nProvide a document.\n{p3}");
    let service = svc("Doc Summarizer", &desc, "A2A", "5", None);
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"D5".to_string()), "got {:?}", codes(&r));
    // Parts 1 and 2 are within limits → no D3/D4.
    assert!(!codes(&r).contains(&"D3".to_string()), "got {:?}", codes(&r));
    assert!(!codes(&r).contains(&"D4".to_string()), "got {:?}", codes(&r));
}

#[test]
fn description_middle_line_length_gated_as_part2() {
    // With 3 lines, part 2 is the MIDDLE line (not the last) — a too-long middle
    // line must surface as D4, proving parts are positional not first/last.
    let p2 = "B".repeat(401);
    let desc = format!("Short summary.\n{p2}\nDelivery: a file.");
    let service = svc("Doc Summarizer", &desc, "A2A", "5", None);
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"D4".to_string()), "got {:?}", codes(&r));
}

#[test]
fn description_profit_guarantee_cjk_fails_d9() {
    let service = svc(
        "Signal Service",
        "每日推送交易信号，稳赚不赔。\n无需提供额外材料。",
        "A2A",
        "5",
        None,
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"D9".to_string()), "got {:?}", codes(&r));
}

#[test]
fn description_profit_guarantee_en_fails_d9() {
    let service = svc(
        "Signal Service",
        "Daily trade signals with guaranteed returns.\nNothing to provide.",
        "A2A",
        "5",
        None,
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"D9".to_string()), "got {:?}", codes(&r));
}

#[test]
fn description_test_marker_fails_u1() {
    // Spec: "所有字段不允许包含 (pre)、(test)" — the service description too.
    let service = svc(
        "Doc Summarizer",
        "Summarizes docs (test).\nProvide a document.\nDelivery: a file.",
        "A2A",
        "5",
        None,
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"U1".to_string()), "got {:?}", codes(&r));
}

#[test]
fn clean_description_has_no_profit_guarantee_d9() {
    // A legitimate description must not trip D9.
    let service = svc(
        "Signal Service",
        "面向链上交易者的跟单信号服务，覆盖 DEX 现货。\n无需提供额外材料。\n交付物形式为结构化信号，支持跟单。",
        "A2A",
        "5",
        None,
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(!codes(&r).contains(&"D9".to_string()), "got {:?}", codes(&r));
}

#[test]
fn description_over_1200_width_fails_d2() {
    // 1201 half-width chars across two lines → total display width 1201 > 1200.
    let part1 = "x".repeat(600);
    let part2 = "y".repeat(601);
    let desc = format!("{part1}\n{part2}");
    let service = svc("Doc Summarizer", &desc, "A2A", "5", None);
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"D2".to_string()), "got {:?}", codes(&r));
    // Each part is also >400 width → D3/D4 fire too; this test only pins D2.
}

#[test]
fn description_cjk_width_counts_double_d2() {
    // 601 CJK chars on one line + a short second line. Width = 601*2 = 1202 > 1200.
    let part1 = "测".repeat(601);
    let desc = format!("{part1}\n需要提供钱包地址");
    let service = svc("Doc Summarizer", &desc, "A2A", "5", None);
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"D2".to_string()), "got {:?}", codes(&r));
}

#[test]
fn description_part1_over_400_width_fails_d3() {
    // Part 1 = first line; 401 half-width chars > 400 width.
    let p1 = "A".repeat(401);
    let desc = format!("{p1}\nProvide a document.");
    let service = svc("Doc Summarizer", &desc, "A2A", "5", None);
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"D3".to_string()), "got {:?}", codes(&r));
}

#[test]
fn description_part2_over_400_width_fails_d4() {
    // Part 2 = LAST line; 401 half-width chars > 400 width.
    let p2 = "B".repeat(401);
    let desc = format!("Short summary.\n{p2}");
    let service = svc("Doc Summarizer", &desc, "A2A", "5", None);
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"D4".to_string()), "got {:?}", codes(&r));
}

#[test]
fn description_with_url_fails_d6() {
    let desc = "Short summary.\nhttps://example.com for more";
    let service = svc("Doc Summarizer", desc, "A2A", "5", None);
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"D6".to_string()), "got {:?}", codes(&r));
}

// ─── S1 boundary values ────────────────────────────────────────────────────

#[test]
fn service_name_four_chars_fails_s1() {
    // 4 chars — below the 5-char minimum.
    let service = svc(
        "Abcd",
        "Does a thing.\nMore detail here.\nDo the thing",
        "A2A",
        "5",
        None,
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"S1".to_string()), "got {:?}", codes(&r));
}

#[test]
fn service_name_five_chars_passes_s1() {
    let service = svc(
        "Abcde",
        "Does a thing.\nMore detail here.\nDo the thing",
        "A2A",
        "5",
        None,
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(!codes(&r).contains(&"S1".to_string()), "got {:?}", codes(&r));
}

#[test]
fn service_name_thirty_chars_passes_s1() {
    let service = svc(
        "ABCDEFGHIJKLMNOPQRSTUVWXYZ1234",
        "Does a thing.\nMore detail here.\nDo the thing",
        "A2A",
        "5",
        None,
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(!codes(&r).contains(&"S1".to_string()), "got {:?}", codes(&r));
}

#[test]
fn service_name_thirty_one_chars_fails_s1() {
    let service = svc(
        "ABCDEFGHIJKLMNOPQRSTUVWXYZ12345",
        "Does a thing.\nMore detail here.\nDo the thing",
        "A2A",
        "5",
        None,
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"S1".to_string()), "got {:?}", codes(&r));
}

// ─── contains_hex_address boundary values ─────────────────────────────────

#[test]
fn hex_address_five_digits_is_not_detected() {
    // Exactly 5 hex chars after "0x" — below the 6-char threshold.
    assert!(!contains_hex_address("0x12345"));
    assert!(!contains_hex_address("prefix 0xabcde suffix"));
}

#[test]
fn hex_address_six_digits_is_detected() {
    // Exactly 6 hex chars — meets the threshold.
    assert!(contains_hex_address("0x123456"));
    assert!(contains_hex_address("0xABCDEF"));
}

#[test]
fn hex_address_uppercase_x_is_detected() {
    assert!(contains_hex_address("0XDEADBEEF12"));
}

// ─── has_test_marker full branch coverage ──────────────────────────────────

#[test]
fn hyphen_test_suffix_fails() {
    assert!(has_test_marker("bot-test"));
    assert!(has_test_marker("service-pre"));
    assert!(has_test_marker("Agent-dev"));
    assert!(has_test_marker("bot-beta"));
    assert!(has_test_marker("svc-staging"));
}

#[test]
fn underscore_test_suffix_fails() {
    assert!(has_test_marker("bot_test"));
    assert!(has_test_marker("service_pre"));
    assert!(has_test_marker("agent_dev"));
    assert!(has_test_marker("bot_beta"));
    assert!(has_test_marker("svc_staging"));
}

#[test]
fn dot_test_suffix_fails() {
    assert!(has_test_marker("bot.test"));
    assert!(has_test_marker("service.pre"));
}

#[test]
fn trailing_space_test_fails() {
    assert!(has_test_marker("Agent test"));
    assert!(has_test_marker("Service pre"));
    assert!(has_test_marker("Bot dev"));
    assert!(has_test_marker("Agent beta"));
    assert!(has_test_marker("Bot staging"));
}

#[test]
fn mid_word_test_does_not_trigger() {
    // "protest" / "Predict" contain "pre"/"test" but not as a delimited marker.
    assert!(!has_test_marker("protest"));
    assert!(!has_test_marker("Predict"));
    // "testing" — "test" followed by 'i' (alphanumeric) → boundary check fails → no match.
    assert!(!has_test_marker("testing"));
    // "contextual" — no delimited marker form.
    assert!(!has_test_marker("contextual"));
}

#[test]
fn underscore_test_in_middle_triggers() {
    // "pre_test_bot" DOES trigger: delimited_marker_present finds "_test" at
    // index 3, and the next char is '_' (a non-alphanumeric boundary) → true.
    // The "pre" prefix before "_test" is irrelevant to the algorithm.
    // (Contrast `mid_word_test_does_not_trigger`: "pretest" with no delimiter
    // before "test" must NOT match — that is correct, by design.)
    assert!(has_test_marker("pre_test_bot"));
}

#[test]
fn hyphen_test_mid_word_does_not_trigger() {
    // "-testing" → after "-test" the next char is 'i' (alphanumeric) → no match.
    assert!(!has_test_marker("bot-testing"));
}

// ─── asp description U3 ──────────────────────────────────────────────

#[test]
fn asp_description_with_negative_capability_fails_u3() {
    let r = run_validation(
        "asp",
        Some("GoodBot"),
        Some("Trading is currently not supported"),
        None,
    );
    assert!(codes(&r).contains(&"U3".to_string()), "got {:?}", codes(&r));
}

#[test]
fn user_description_with_negative_capability_also_fails_u3() {
    // Universal text rules apply to all roles.
    let r = run_validation(
        "user",
        Some("Buyer"),
        Some("currently not supported"),
        None,
    );
    assert!(codes(&r).contains(&"U3".to_string()), "got {:?}", codes(&r));
}

// ─── Additional boundary edge cases ───────────────────────────────────────

// S3: empty agent name skips duplicate check
#[test]
fn s3_does_not_trigger_when_agent_name_empty() {
    // Source: `if !agent_name.is_empty()` guard before S3 check.
    let service = svc(
        "Trade Executor",
        "Does a thing.\nMore detail here.\nDo the thing",
        "A2A",
        "5",
        None,
    );
    // No name provided → agent_name = "" → S3 guard skips.
    let r = run_validation("asp", None, None, Some(&service));
    assert!(!codes(&r).contains(&"S3".to_string()), "got {:?}", codes(&r));
}

// S4: CJK free word triggers S4
#[test]
fn service_name_with_cjk_free_fails_s4() {
    let service = svc(
        "免费翻译服务Pro",
        "Does a thing.\nMore detail here.\nDo the thing",
        "A2A",
        "0",
        None,
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"S4".to_string()), "got {:?}", codes(&r));
}

// S1: empty service name is skipped (0 chars does not report S1)
#[test]
fn empty_service_name_does_not_trigger_s1() {
    // Source: `if !svc.service_name.is_empty()` guard before S1 check.
    let service = svc(
        "",
        "Does a thing.\nMore detail here.\nDo the thing",
        "A2A",
        "5",
        None,
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(!codes(&r).contains(&"S1".to_string()), "got {:?}", codes(&r));
}

// contains_hex_address: non-hex char terminates the run before 6 digits
#[test]
fn hex_address_non_hex_char_terminates_run() {
    // "0x12345g" — 'g' is not a hex digit; run length = 5 < 6 → false.
    assert!(!contains_hex_address("0x12345g"));
    // "0x123456g" — 6 hex digits before 'g' → true.
    assert!(contains_hex_address("0x123456g"));
}

// D1 + D2 both fire on a single over-long line
#[test]
fn description_over_1200_single_line_fails_d1_and_d2() {
    // A single line of 1201 chars → D2 (total width > 1200) and D1 (only 1 part).
    let long = "x".repeat(1201);
    let service = svc("Doc Summarizer", &long, "A2A", "5", None);
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    let c = codes(&r);
    assert!(c.contains(&"D2".to_string()), "expected D2, got {:?}", c);
    assert!(c.contains(&"D1".to_string()), "expected D1 (single line → no part 2), got {:?}", c);
}

// N3 integration: No. suffix through run_validation
#[test]
fn no_ordinal_suffix_integration_fails_n3() {
    assert!(has_ordinal_suffix("BotNo3"));
    let r = run_validation("asp", Some("BotNo3"), None, None);
    assert!(codes(&r).contains(&"N3".to_string()), "got {:?}", codes(&r));
}

// ─── Message-unification invariant (same rule group → same message) ───────────
// After removing the `fe` field, message unification is the only grouping
// mechanism. Each test below fires multiple sub-checks under one rule group and
// asserts every finding for that field shares a single message.

#[test]
fn service_name_findings_unify_to_same_message() {
    // S1 (length) + S3 (duplicates agent) + S4 (price) + S6 (test marker) all
    // map to the service-name message. Craft a name that hits S1 + S6 + S4.
    // "ab(test)USDT" < 5 chars is not easy to combine, so test one multi-hit
    // combo: an 4-char name with "free" and a test marker.
    let service = svc(
        "free(test)",
        "Does a thing.\nMore detail here.\nDo the thing",
        "A2A",
        "5",
        None,
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    let field_findings: Vec<&Finding> = r
        .findings
        .iter()
        .filter(|f| f.field == "service[0].name")
        .collect();
    assert!(field_findings.len() > 1, "expected multiple service-name findings, got {:?}", codes(&r));
    let first_msg = &field_findings[0].message;
    for f in &field_findings {
        assert_eq!(&f.message, first_msg, "service-name finding {:?} has diverging message", f.code);
    }
    assert_eq!(first_msg, super::fe::FE06);
}

#[test]
fn description_prohibited_findings_unify_to_same_message() {
    // D6 (URL) + U1 (test marker) + D9 (profit guarantee) all map to the
    // prohibited-content message. A description can't have D7 (hex) + D6
    // simultaneously in a short string without other noise, so use U1 + D6.
    let desc_with_url_and_marker =
        "稳赚不赔 — see https://example.com (test).\nProvide something.\nDelivery: file.";
    let service = svc("Doc Summarizer", desc_with_url_and_marker, "A2A", "5", None);
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    let fd = "service[0].servicedescription";
    let field_findings: Vec<&Finding> =
        r.findings.iter().filter(|f| f.field == fd).collect();
    assert!(field_findings.len() > 1, "expected multiple description findings, got {:?}", codes(&r));
    let first_msg = &field_findings[0].message;
    for f in &field_findings {
        assert_eq!(&f.message, first_msg, "description finding {:?} has diverging message", f.code);
    }
    assert_eq!(first_msg, super::fe::FE22);
}

#[test]
fn json_output_does_not_contain_fe_key() {
    // The serialized JSON for a blocking finding must never include an "fe" key —
    // it was internal only and has been removed from the struct entirely.
    let r = run_validation("asp", Some("Trump_v2(test)"), None, None);
    assert!(!r.pass);
    let json = serde_json::to_string(&r).unwrap();
    assert!(
        !json.contains("\"fe\""),
        "serialized output must not contain 'fe' key, got: {json}"
    );
}

#[test]
fn json_output_contains_expected_fields_only() {
    // Each finding must serialize exactly: field, code, severity, message.
    let r = run_validation("asp", Some("X"), None, None);
    let json: serde_json::Value = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
    let finding = &json["findings"][0];
    assert!(finding["field"].is_string(), "missing 'field'");
    assert!(finding["code"].is_string(), "missing 'code'");
    assert!(finding["severity"].is_string(), "missing 'severity'");
    assert!(finding["message"].is_string(), "missing 'message'");
    assert!(finding["fe"].is_null(), "'fe' must not be present in JSON");
}

#[test]
fn endpoint_findings_unify_to_same_message() {
    // T2 (A2MCP missing endpoint) fires alone; pair it with U2 (hex address as
    // endpoint) to get two findings on the endpoint field with the same message.
    // Easiest: supply an A2MCP service with a 0x hex address as the endpoint
    // (fails T4 — not https:// — and U2 — hex in endpoint field).
    let service = format!(
        "[{{\"serviceName\":\"Some MCP\",\"serviceDescription\":\"Does a thing.\\nMore detail.\\nDo the thing\",\
         \"serviceType\":\"A2MCP\",\"fee\":\"5\",\"endpoint\":\"0xdeadbeefdeadbeef\"}}]"
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    let ep_findings: Vec<&Finding> = r
        .findings
        .iter()
        .filter(|f| f.field == "service[0].endpoint")
        .collect();
    assert!(ep_findings.len() > 1, "expected multiple endpoint findings, got {:?}", codes(&r));
    let first_msg = &ep_findings[0].message;
    for f in &ep_findings {
        assert_eq!(&f.message, first_msg, "endpoint finding {:?} has diverging message", f.code);
    }
    assert_eq!(first_msg, super::fe::FE11);
}

