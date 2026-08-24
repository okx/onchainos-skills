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
        // A malformed SINGLE fee is P1 only. P3/P4 exist but mean something else
        // entirely now — they were reassigned to the subscription rules (P3 =
        // A2MCP carrying a subscription, P4 = interval != month), so neither may
        // fire for a single-fee problem.
        assert!(!c.contains(&"P3".to_string()), "P3 is subscription-only, fee={fee} got {:?}", c);
        assert!(!c.contains(&"P4".to_string()), "P4 is interval-only, fee={fee} got {:?}", c);
        assert!(!c.contains(&"P5".to_string()), "P5 is tier-fee-only, fee={fee} got {:?}", c);
        assert!(!r.pass);
    }
}

// ─── Subscription pricing (P2, P3, P4, P5) ────────────────────────────────
// The single-fee side is P1 (above); these four are the subscription/billing-model
// codes. Each asserts the FIELD as well as the code, because the four share two
// FE messages (P2/P3 → FE17, P4 → FE18, P5 → FE19) and the skill layer renders by
// (field, message) — a finding parked on the wrong field renders under the wrong
// input.

#[test]
fn a2a_with_neither_fee_nor_subscription_fails_p2() {
    // A2A must carry EXACTLY ONE billing model. Neither → P2 on the fee field
    // (the counterpart of P6, which fires when both are present).
    let service = svc(
        "Pricing Service",
        "Does a thing.\\nMore detail here.\\nDo the thing",
        "A2A",
        "",
        None,
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"P2".to_string()), "got {:?}", codes(&r));
    let f = r.findings.iter().find(|f| f.code == "P2").unwrap();
    assert_eq!(f.field, "service[0].fee", "P2 belongs on the fee field");
    assert_eq!(f.message, super::fe::FE17);
    assert!(!r.pass);
}

#[test]
fn a2mcp_with_subscription_fails_p3() {
    // Subscription pricing is A2A-only: an A2MCP service carrying `subscription[]`
    // is flagged P3 — even when the tier itself is perfectly well-formed.
    let service = "[{\"serviceName\":\"Pricing Service\",\"serviceDescription\":\"Does a thing.\",\"serviceType\":\"A2MCP\",\"fee\":\"10\",\"endpoint\":\"https://api.example.com/mcp\",\"subscription\":[{\"interval\":\"month\",\"fee\":\"10\"}]}]";
    let r = run_validation("asp", Some("Agent Name"), None, Some(service));
    let c = codes(&r);
    assert!(c.contains(&"P3".to_string()), "got {:?}", c);
    let f = r.findings.iter().find(|f| f.code == "P3").unwrap();
    assert_eq!(f.field, "service[0].subscription", "P3 belongs on the subscription field");
    assert_eq!(f.message, super::fe::FE17);
    // The A2A-only tier checks must NOT also run for A2MCP (early return).
    assert!(!c.contains(&"P4".to_string()), "tier checks are A2A-only, got {:?}", c);
    assert!(!c.contains(&"P5".to_string()), "tier checks are A2A-only, got {:?}", c);
    assert!(!r.pass);
}

#[test]
fn subscription_interval_other_than_month_fails_p4() {
    // `month` is the only billing period the product supports today.
    for interval in &["year", "week", "day"] {
        let service = format!(
            "[{{\"serviceName\":\"Pricing Service\",\"serviceDescription\":\"Does a thing.\",\"serviceType\":\"A2A\",\"fee\":\"\",\"subscription\":[{{\"interval\":\"{interval}\",\"fee\":\"10\"}}]}}]"
        );
        let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
        let c = codes(&r);
        assert!(c.contains(&"P4".to_string()), "interval={interval} got {:?}", c);
        let f = r.findings.iter().find(|f| f.code == "P4").unwrap();
        assert_eq!(f.field, "service[0].subscription");
        assert_eq!(f.message, super::fe::FE18);
        // A well-formed tier fee must not drag P5 along.
        assert!(!c.contains(&"P5".to_string()), "interval={interval} got {:?}", c);
        assert!(!r.pass);
    }
}

#[test]
fn subscription_interval_month_is_case_insensitive_no_p4() {
    // `eq_ignore_ascii_case` + trim: "Month" / " MONTH " are accepted.
    for interval in &["Month", " MONTH "] {
        let service = format!(
            "[{{\"serviceName\":\"Pricing Service\",\"serviceDescription\":\"Does a thing.\",\"serviceType\":\"A2A\",\"fee\":\"\",\"subscription\":[{{\"interval\":\"{interval}\",\"fee\":\"10\"}}]}}]"
        );
        let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
        assert!(!codes(&r).contains(&"P4".to_string()), "interval={interval} got {:?}", codes(&r));
        assert!(r.pass, "interval={interval} got {:?}", codes(&r));
    }
}

#[test]
fn subscription_tier_fee_not_plain_number_fails_p5() {
    // A tier fee follows the SAME contract as the single-purchase fee (plain
    // number, USDT implied, ≤6 decimals) — but a violation is P5 on the
    // subscription field, never P1 on the fee field (`fee` is legitimately empty
    // on a subscription-priced service, which is exactly why the codes differ).
    // Covers: currency token, negotiation wording, empty tier fee, 7 decimals.
    for tier_fee in &["10 USDT", "面议", "", "1.1234567"] {
        let service = format!(
            "[{{\"serviceName\":\"Pricing Service\",\"serviceDescription\":\"Does a thing.\",\"serviceType\":\"A2A\",\"fee\":\"\",\"subscription\":[{{\"interval\":\"month\",\"fee\":\"{tier_fee}\"}}]}}]"
        );
        let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
        let c = codes(&r);
        assert!(c.contains(&"P5".to_string()), "tier fee={tier_fee:?} got {:?}", c);
        let f = r.findings.iter().find(|f| f.code == "P5").unwrap();
        assert_eq!(f.field, "service[0].subscription", "P5 belongs on the subscription field");
        assert_eq!(f.message, super::fe::FE19);
        assert!(!c.contains(&"P1".to_string()), "a bad TIER fee is not P1, tier fee={tier_fee:?} got {:?}", c);
        assert!(!c.contains(&"P2".to_string()), "the subscription IS the billing model, got {:?}", c);
        assert!(!r.pass);
    }
}

#[test]
fn every_bad_subscription_tier_reports_p5() {
    // The tier loop checks each entry: two malformed tiers → two P5 findings (the
    // ASP sees every broken tier at once instead of fixing them one round-trip at
    // a time). The good middle tier contributes nothing.
    let service = "[{\"serviceName\":\"Pricing Service\",\"serviceDescription\":\"Does a thing.\",\"serviceType\":\"A2A\",\"fee\":\"\",\"subscription\":[{\"interval\":\"month\",\"fee\":\"5 USDT\"},{\"interval\":\"month\",\"fee\":\"10\"},{\"interval\":\"month\",\"fee\":\"面议\"}]}]";
    let r = run_validation("asp", Some("Agent Name"), None, Some(service));
    let p5s = r.findings.iter().filter(|f| f.code == "P5").count();
    assert_eq!(p5s, 2, "expected one P5 per malformed tier, got {:?}", codes(&r));
}

#[test]
fn subscription_tier_fee_edge_values_pass_p5() {
    // Boundary of the shared fee contract: an integer, "0", and exactly 6
    // decimals are all plain numbers → no P5.
    for tier_fee in &["10", "0", "0.123456"] {
        let service = format!(
            "[{{\"serviceName\":\"Pricing Service\",\"serviceDescription\":\"Does a thing.\",\"serviceType\":\"A2A\",\"fee\":\"\",\"subscription\":[{{\"interval\":\"month\",\"fee\":\"{tier_fee}\"}}]}}]"
        );
        let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
        assert!(!codes(&r).contains(&"P5".to_string()), "tier fee={tier_fee} got {:?}", codes(&r));
        assert!(r.pass, "tier fee={tier_fee} got {:?}", codes(&r));
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
fn hex_in_service_description_passes() {
    // A 0x address in `servicedescription` is not checked.
    let desc = "Summarizes text 0xdeadbeefdeadbeef.\nHandles long docs.\nSummarize this";
    let service = svc("Document Summarizer", desc, "A2A", "0", None);
    let r = run_validation("asp", Some("Summary Bot"), None, Some(&service));
    assert!(r.pass, "got {:?}", codes(&r));
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

// ─── Negative-capability wording (former U3) — REMOVED everywhere ─────────
// Guards against re-adding the rule on ANY field: agent name, agent
// description, service name, service description. Declaring a capability
// boundary is honest disclosure, and no FE message ever named the rule, so it
// was unreachable-by-repair.

#[test]
fn negative_capability_no_longer_flagged_on_name_or_description() {
    // Name that is itself a "not supported" phrase: only the ordinary name rules
    // may fire (length / symbols) — never a U3.
    let r = run_validation("asp", Some("Currently not supported"), None, None);
    assert!(!codes(&r).contains(&"U3".to_string()), "got {:?}", codes(&r));

    // Agent description (asp) — clean name, capability-boundary wording.
    let r = run_validation(
        "asp",
        Some("GoodBot"),
        Some("currently not supported for this chain"),
        None,
    );
    assert!(r.pass, "got {:?}", codes(&r));

    // Same for a non-asp role (the universal text rules run for every role).
    let r = run_validation("user", Some("Buyer"), Some("currently not supported"), None);
    assert!(r.pass, "got {:?}", codes(&r));

    // CJK gap wording on the agent description.
    let r = run_validation("asp", Some("GoodBot"), Some("暂不支持 Solana，目前不支持跨链。"), None);
    assert!(r.pass, "got {:?}", codes(&r));
}

#[test]
fn negative_capability_no_longer_flagged_on_service_name_or_description() {
    // Service NAME carrying the gap wording (5–30 chars so S1 doesn't fire) and a
    // description carrying it too → the whole listing passes.
    let service = svc(
        "暂不支持跨链的摘要服务",
        "提供多链文档摘要能力，面向研究员。\n需要提供：文档原文。\n交付物为文件，暂不支持 Solana，不支持跟单。",
        "A2A",
        "5",
        None,
    );
    let r = run_validation("asp", Some("GoodBot"), None, Some(&service));
    assert!(!codes(&r).contains(&"U3".to_string()), "got {:?}", codes(&r));
    assert!(r.pass, "a capability-boundary note must not block, got {:?}", codes(&r));
}

// ─── FE05 must name every check that reports it ────────────────────────────

#[test]
fn agent_description_test_marker_message_mentions_test_markers() {
    // U1 on the agent description reports FE05. The message therefore has to name
    // the test marker — otherwise the ASP is told to remove links / trim length /
    // fill in the field, none of which is the actual cause, and the skill layer
    // (which drafts the correction FROM this message) edits the wrong thing while
    // the marker survives. Fires on the trailing " beta" form, the realistic case
    // ("… currently in beta").
    let r = run_validation("asp", Some("GoodBot"), Some("A summarizer, currently in beta"), None);
    let f = r.findings.iter().find(|f| f.code == "U1").expect("U1 expected");
    assert_eq!(f.field, "description");
    assert_eq!(f.message, super::fe::FE05);
    assert!(
        f.message.contains("test marker"),
        "FE05 must name the test-marker check: {}",
        f.message
    );
    assert!(!r.pass);
}

// ─── D8: agent-level description length ───────────────────────────────────
// D8 is the ONLY length rule on the agent-level `description` and is distinct
// from the service-description cap (D2) in three ways worth pinning: it counts
// CHARACTERS (not east-asian display width), it BLOCKS (D2 only suggests), and
// it is ASP-ONLY.

#[test]
fn agent_description_over_500_chars_fails_d8() {
    let desc = "A".repeat(501);
    let r = run_validation("asp", Some("GoodBot"), Some(&desc), None);
    assert!(codes(&r).contains(&"D8".to_string()), "got {:?}", codes(&r));
    let f = r.findings.iter().find(|f| f.code == "D8").unwrap();
    assert_eq!(f.field, "description", "D8 belongs on the agent-level description");
    assert_eq!(f.message, super::fe::FE05);
    assert_eq!(f.severity, "block", "D8 blocks, unlike the advisory service-description D2");
    assert!(!r.pass);
}

#[test]
fn agent_description_exactly_500_chars_passes_d8() {
    let desc = "A".repeat(500);
    let r = run_validation("asp", Some("GoodBot"), Some(&desc), None);
    assert!(!codes(&r).contains(&"D8".to_string()), "500 is inclusive, got {:?}", codes(&r));
    assert!(r.pass, "got {:?}", codes(&r));
}

#[test]
fn agent_description_d8_counts_chars_not_display_width() {
    // 500 CJK chars = 1000 display width. D8 uses `chars().count()`, so this
    // passes — guards against someone "unifying" D8 with D2's `display_width`.
    let cjk = "测".repeat(500);
    let r = run_validation("asp", Some("GoodBot"), Some(&cjk), None);
    assert!(!codes(&r).contains(&"D8".to_string()), "D8 counts chars, got {:?}", codes(&r));
    assert!(r.pass, "got {:?}", codes(&r));

    // One char more and it blocks.
    let cjk = "测".repeat(501);
    let r = run_validation("asp", Some("GoodBot"), Some(&cjk), None);
    assert!(codes(&r).contains(&"D8".to_string()), "got {:?}", codes(&r));
}

#[test]
fn agent_description_d8_is_asp_only() {
    // Non-ASP roles have no agent-description length cap (nor D6) — only the
    // universal U1 marker check runs for them.
    let desc = "A".repeat(501);
    for role in &["user", "evaluator"] {
        let r = run_validation(role, Some("Buyer Bot"), Some(&desc), None);
        assert!(!codes(&r).contains(&"D8".to_string()), "role={role} got {:?}", codes(&r));
        assert!(r.pass, "role={role} got {:?}", codes(&r));
    }
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

// ─── S2: service names must be unique within one ASP listing ──────────────

#[test]
fn duplicate_service_names_fail_s2() {
    let services = r#"[
        {"serviceName":"Trade Executor","serviceDescription":"Executes trades.","serviceType":"A2A","fee":"5"},
        {"serviceName":"Trade Executor","serviceDescription":"Executes other trades.","serviceType":"A2A","fee":"6"}
    ]"#;
    let r = run_validation("asp", Some("Agent Name"), None, Some(services));

    assert!(codes(&r).contains(&"S2".to_string()), "got {:?}", codes(&r));
    assert!(!r.pass, "duplicate service names must block registration");
    assert!(
        r.findings
            .iter()
            .any(|f| f.code == "S2" && f.field == "service[1].name"),
        "the repeated service entry should be identified"
    );
}

#[test]
fn duplicate_service_names_ignore_ascii_case_and_whitespace() {
    let services = r#"[
        {"serviceName":"Trade Executor","serviceDescription":"Executes trades.","serviceType":"A2A","fee":"5"},
        {"serviceName":"  trade executor  ","serviceDescription":"Executes other trades.","serviceType":"A2A","fee":"6"}
    ]"#;
    let r = run_validation("asp", Some("Agent Name"), None, Some(services));

    assert!(codes(&r).contains(&"S2".to_string()), "got {:?}", codes(&r));
    assert!(!r.pass);
}

#[test]
fn distinct_service_names_pass_s2() {
    let services = r#"[
        {"serviceName":"Trade Executor","serviceDescription":"Executes trades.","serviceType":"A2A","fee":"5"},
        {"serviceName":"Portfolio Analyst","serviceDescription":"Analyzes portfolios.","serviceType":"A2A","fee":"6"}
    ]"#;
    let r = run_validation("asp", Some("Agent Name"), None, Some(services));

    assert!(!codes(&r).contains(&"S2".to_string()), "got {:?}", codes(&r));
    assert!(r.pass, "distinct valid services should pass: {:?}", codes(&r));
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
fn u5_message_points_at_the_wording_not_the_service_type() {
    // U5 must NOT reuse the T1 message (FE10): the serviceType value is valid, so
    // "re-pick A2A or A2MCP from the menu" sends the ASP to change a correct field
    // and, for A2A → A2MCP, cascades into T2 / P3. It must name the offending
    // field (name / description) instead.
    let service = svc(
        "My A2MCP Service",
        "Does a thing.\nMore detail here.\nDo the thing",
        "A2A",
        "5",
        None,
    );
    let r = run_validation("asp", Some("Other Agent"), None, Some(&service));
    let u5 = r.findings.iter().find(|f| f.code == "U5").expect("U5 expected");
    assert_eq!(u5.field, "service[0].name");
    assert_ne!(u5.message, super::fe::FE10, "U5 must not reuse the T1 message");
    assert_eq!(u5.message, super::fe::FE10_U5);

    // T1 (a genuinely invalid type value) still carries FE10.
    let bad = svc("Some Service", "Does a thing.", "REST", "5", None);
    let r = run_validation("asp", Some("Other Agent"), None, Some(&bad));
    let t1 = r.findings.iter().find(|f| f.code == "T1").expect("T1 expected");
    assert_eq!(t1.message, super::fe::FE10);
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

// ─── D1–D6: service description required-ness, length, prohibited content ──
//
// There is NO paragraph-count rule: the A2A three-part layout is collection-time
// guidance in the skill (register.md §3 Step 2c), so the validator accepts a
// non-empty A2A description of ANY shape — the tests below pin that (single-line,
// 2-part, 3-part all pass identically for both billing models).

#[test]
fn description_single_line_passes_no_d1() {
    // A single non-empty line is a valid A2A description: paragraph count is not
    // validated, so no D1 and the listing passes.
    let service = svc(
        "Doc Summarizer",
        "Does one thing only",
        "A2A",
        "5",
        None,
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(!codes(&r).contains(&"D1".to_string()), "got {:?}", codes(&r));
    assert!(r.pass, "a non-empty single-line description must pass, got {:?}", codes(&r));
}

#[test]
fn description_empty_fails_d1() {
    let service = svc("Doc Summarizer", "", "A2A", "5", None);
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"D1".to_string()), "got {:?}", codes(&r));
}

#[test]
fn description_two_parts_non_subscription_passes_no_d1() {
    // A NON-subscription (single-fee) A2A service with 2 paragraphs: the paragraph
    // count is not validated, so no D1 and the listing passes.
    let service = svc(
        "Doc Summarizer",
        "Summary line.\nProvide: a document and a target language.",
        "A2A",
        "5",
        None,
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(!codes(&r).contains(&"D1".to_string()), "got {:?}", codes(&r));
    assert!(r.pass, "got {:?}", codes(&r));
}

#[test]
fn description_two_parts_subscription_passes_no_d1() {
    // Same 2-paragraph shape on a SUBSCRIPTION-priced A2A service → also no D1.
    let service = "[{\"serviceName\":\"Pricing Service\",\"serviceDescription\":\"Daily signals across DEX.\\nDelivered as structured signals; copy-trading supported.\",\"serviceType\":\"A2A\",\"fee\":\"\",\"subscription\":[{\"interval\":\"month\",\"fee\":\"10\"}]}]";
    let r = run_validation("asp", Some("Agent Name"), Some("A helpful agent."), Some(service));
    assert!(!codes(&r).contains(&"D1".to_string()), "got {:?}", codes(&r));
    assert!(r.pass, "got {:?}", codes(&r));
}

#[test]
fn description_three_parts_subscription_passes_no_d1() {
    // Billing model no longer affects the description rules: a 3-paragraph body on
    // a subscription service is accepted exactly like the 2-paragraph one above —
    // there is no paragraph-count rule to branch on.
    let service = "[{\"serviceName\":\"Pricing Service\",\"serviceDescription\":\"Daily signals across DEX.\\nNothing to provide.\\nDelivered as structured signals.\",\"serviceType\":\"A2A\",\"fee\":\"\",\"subscription\":[{\"interval\":\"month\",\"fee\":\"10\"}]}]";
    let r = run_validation("asp", Some("Agent Name"), None, Some(service));
    assert!(!codes(&r).contains(&"D1".to_string()), "got {:?}", codes(&r));
    assert!(r.pass, "got {:?}", codes(&r));
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
fn d1_and_d2_carry_separate_messages() {
    // FE-21 is split per sub-check. D1 (blocking, empty) and D2 (advisory,
    // over-length) must NOT share one sentence: the old shared text told an ASP
    // who had written 1001 characters that the description was "empty" and ordered
    // a resubmit while `pass` was true.
    let empty = svc("Doc Summarizer", "   ", "A2A", "5", None);
    let r = run_validation("asp", Some("Agent Name"), None, Some(&empty));
    let d1 = r.findings.iter().find(|f| f.code == "D1").expect("D1 expected");
    assert_eq!(d1.message, super::fe::FE21_D1);
    assert!(d1.message.contains("empty"), "D1 names the empty field: {}", d1.message);
    assert!(
        !d1.message.contains("1000"),
        "D1 must not talk about the length cap: {}",
        d1.message
    );

    // 1001 CJK chars = 2002 display width → D2 only.
    let long = "测".repeat(1001);
    let over = svc("Doc Summarizer", &long, "A2A", "5", None);
    let r = run_validation("asp", Some("Agent Name"), None, Some(&over));
    let d2 = r.findings.iter().find(|f| f.code == "D2").expect("D2 expected");
    assert_eq!(d2.message, super::fe::FE21_D2);
    assert_eq!(d2.severity, "suggest");
    assert!(r.pass, "over-length is advisory only, got {:?}", codes(&r));
    assert!(
        !d2.message.contains("empty"),
        "D2 must not claim the description is empty: {}",
        d2.message
    );
    assert!(
        !d2.message.contains("resubmit"),
        "an advisory finding must not order a resubmit: {}",
        d2.message
    );
    // Neither sentence re-states the retired per-part line layout.
    for m in [super::fe::FE21_D1, super::fe::FE21_D2] {
        assert!(!m.contains("own line"), "paragraph layout is not a rule: {m}");
    }
    // The two can never co-occur (D1 returns early), so one field carries one message.
    assert!(!codes(&r).contains(&"D1".to_string()), "got {:?}", codes(&r));
}

#[test]
fn description_long_single_paragraph_has_no_findings() {
    // No per-paragraph length limit and no paragraph-count rule: a single-paragraph
    // A2A description whose only line is long (401 chars, well under the 2000-width
    // total cap) raises NOTHING — never D3 (removed) and never D1.
    let p1 = "A".repeat(401);
    let service = svc("Doc Summarizer", &p1, "A2A", "5", None);
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    let c = codes(&r);
    assert!(!c.contains(&"D1".to_string()), "paragraph count is not validated, got {:?}", c);
    assert!(!c.contains(&"D3".to_string()), "D3 no longer exists, got {:?}", c);
    assert!(r.pass, "got {:?}", c);
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
fn a2mcp_description_allows_url_no_d6() {
    // D6 (URL) is A2A-only: the A2MCP request description REQUIRES a `curl`
    // example carrying the real https endpoint (FE-16, skill rule), so a URL
    // in an A2MCP description must NOT trip D6.
    let service = svc(
        "Doc Summarizer",
        "1.[Service Description] summarizes text\\n2.[Parameter Spec] text (string, required): source text\\n3.[Request Method] POST\\n4.[Request Example] curl -X POST https://example.com/mcp -d '{\\\"text\\\":\\\"hi\\\"}'",
        "A2MCP",
        "10",
        Some("https://example.com/mcp"),
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(!codes(&r).contains(&"D6".to_string()), "got {:?}", codes(&r));
    assert!(r.pass, "expected pass, got {:?}", codes(&r));
}

#[test]
fn a2mcp_description_still_blocks_test_marker_fe22() {
    // U1 (test marker) still applies to A2MCP.
    let service = svc(
        "Doc Summarizer",
        "保证收益的服务(test)",
        "A2MCP",
        "10",
        Some("https://example.com/mcp"),
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"U1".to_string()), "got {:?}", codes(&r));
    assert!(!r.pass);
}

#[test]
fn a2a_description_with_url_still_fails_d6() {
    // The URL ban stays fully in force for A2A descriptions.
    let service = svc(
        "Doc Summarizer",
        "Summarizes text, see https://example.com\nProvide a document.\nDelivers a markdown file.",
        "A2A",
        "5",
        None,
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
fn description_decorative_symbols_pass() {
    // A description has NO decorative-symbol rule — that is name-only N8. Pipes,
    // "$TOKEN", "≤", "%" and an ellipsis in a delivery note / example line must
    // raise nothing. Paragraph count is not validated, so nothing about a signal
    // service is special here — see validate.rs `check_service_description`.
    let service = svc(
        "Doc Summarizer",
        "面向链上交易者的服务，每日推送 3-5 条可执行信号。\n无需提供额外材料。\n交付物形式为结构化信号：X Layer | $TOKEN (0x12…ab) | BUY | 0.042-0.045 | 滑点 ≤1% | 仓位 5%。",
        "A2A",
        "5",
        None,
    );
    let r = run_validation("asp", Some("SignalRaven"), None, Some(&service));
    assert!(r.pass, "symbols in a description must not block, got {:?}", codes(&r));
}

#[test]
fn description_long_part_within_total_passes() {
    // No per-paragraph limit: a single 900-half-width part is fine as long as
    // the total display width stays ≤ 2000.
    let p3 = "C".repeat(900);
    let desc = format!("Short summary.\nProvide a document.\n{p3}");
    let service = svc("Doc Summarizer", &desc, "A2A", "5", None);
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(r.pass, "long part within total must pass, got {:?}", codes(&r));
}

#[test]
fn description_profit_guarantee_cjk_has_no_description_finding() {
    let service = svc(
        "Signal Service",
        "每日推送交易信号，稳赚不赔。\n无需提供额外材料。",
        "A2A",
        "5",
        None,
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(r.pass, "profit wording is no longer a service-description rule; got {:?}", codes(&r));
    assert!(r.findings.is_empty(), "got {:?}", codes(&r));
}

#[test]
fn description_profit_guarantee_en_has_no_description_finding() {
    let service = svc(
        "Signal Service",
        "Daily trade signals with guaranteed returns.\nNothing to provide.",
        "A2A",
        "5",
        None,
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(r.pass, "profit wording is no longer a service-description rule; got {:?}", codes(&r));
    assert!(r.findings.is_empty(), "got {:?}", codes(&r));
}

#[test]
fn a2mcp_profit_guarantee_has_no_description_finding() {
    let service = svc(
        "Price Feed MCP",
        "Returns price quotes, guaranteed profit for every call",
        "A2MCP",
        "10",
        Some("https://api.example.com/mcp"),
    );
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(r.pass, "profit wording is no longer a service-description rule; got {:?}", codes(&r));
    assert!(r.findings.is_empty(), "got {:?}", codes(&r));
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
fn description_over_2000_width_fails_d2() {
    // 2001 half-width chars across two lines → total display width 2001 > 2000.
    let part1 = "x".repeat(1000);
    let part2 = "y".repeat(1001);
    let desc = format!("{part1}\n{part2}");
    let service = svc("Doc Summarizer", &desc, "A2A", "5", None);
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"D2".to_string()), "got {:?}", codes(&r));
}

#[test]
fn description_cjk_width_counts_double_d2() {
    // 1001 CJK chars on one line + a short second line. Width = 1001*2 = 2002 > 2000.
    let part1 = "测".repeat(1001);
    let desc = format!("{part1}\n需要提供钱包地址");
    let service = svc("Doc Summarizer", &desc, "A2A", "5", None);
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"D2".to_string()), "got {:?}", codes(&r));
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

// D1 + D2 both fire on a single over-long line
#[test]
fn description_over_2000_single_line_suggests_d2_only() {
    // A single line of 2001 chars → D2 (total width > 2000) as the ONLY finding:
    // being a single paragraph is not itself a problem, so no D1 accompanies it,
    // and the advisory D2 does not fail the listing.
    let long = "x".repeat(2001);
    let service = svc("Doc Summarizer", &long, "A2A", "5", None);
    let r = run_validation("asp", Some("Agent Name"), None, Some(&service));
    let c = codes(&r);
    assert!(c.contains(&"D2".to_string()), "expected D2, got {:?}", c);
    assert_eq!(severity_of(&r, "D2"), Some("suggest"), "D2 must be advisory, got {:?}", c);
    assert!(!c.contains(&"D1".to_string()), "paragraph count is not validated, got {:?}", c);
    assert!(r.pass, "an over-length description must not block, got {:?}", c);
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
    // S1 (length) + S2/S3 (duplicates) + S4 (price) + S6 (test marker) all
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
    // Requirement ① still holds after FE-22 became a composed sentence: D6 (URL) +
    // U1 (test marker) on the SAME field both carry the SAME message, so the skill
    // layer's de-dup by (field, message) renders one line.
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
    assert!(!codes(&r).contains(&"D9".to_string()), "got {:?}", codes(&r));
    assert_eq!(first_msg, &super::fe::fe22(true, true), "got: {first_msg}");
}

// ─── FE-22 composed message (only the checks that fired are named) ──────────

#[test]
fn fe22_names_only_the_checks_that_fired() {
    // A test-marker-only hit (the A2MCP case) must NOT mention links — the old
    // fixed sentence told those ASPs to delete the endpoint URL their request
    // example is required to carry.
    let marker_only = super::fe::fe22(false, true);
    assert!(marker_only.contains("remove the test marker"), "got: {marker_only}");
    assert!(!marker_only.contains("link"), "must not mention links: {marker_only}");

    let url_only = super::fe::fe22(true, false);
    assert!(url_only.contains("remove the link"), "got: {url_only}");
    assert!(!url_only.contains("test marker"), "got: {url_only}");

    // Both removals collapse into one clause — never "remove … and remove …".
    let both = super::fe::fe22(true, true);
    assert!(both.contains("remove the link and the test marker"), "got: {both}");
    assert_eq!(both.matches("remove").count(), 1, "verb repeated: {both}");
}

#[test]
fn fe22_blocking_hits_command_a_resubmit() {
    for blocking in [super::fe::fe22(true, false), super::fe::fe22(false, true)] {
        assert!(blocking.contains("Then resubmit"), "got: {blocking}");
    }
}

#[test]
fn fe22_empty_input_stays_descriptive() {
    // Defensive branch — never reached from check_service_description, but it must
    // not produce a dangling "needs a change: ." sentence.
    let none = super::fe::fe22(false, false);
    assert!(!none.contains("needs a change"), "got: {none}");
    assert!(!none.is_empty());
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
fn non_https_endpoint_fails_t4() {
    // A non-https endpoint (here a 0x address) on A2MCP fails T4 with FE11.
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
    assert_eq!(ep_findings.len(), 1, "expected one endpoint finding, got {:?}", codes(&r));
    assert_eq!(ep_findings[0].code, "T4");
    assert_eq!(ep_findings[0].message, super::fe::FE11);
}
