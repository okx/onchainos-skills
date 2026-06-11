use super::*;

fn svc(name: &str, desc: &str, stype: &str, fee: &str, endpoint: Option<&str>) -> String {
    let ep = match endpoint {
        Some(e) => format!(",\"endpoint\":\"{e}\""),
        None => String::new(),
    };
    format!(
            "[{{\"name\":\"{name}\",\"servicedescription\":\"{desc}\",\"servicetype\":\"{stype}\",\"fee\":\"{fee}\"{ep}}}]"
        )
}

fn codes(r: &ValidationResult) -> Vec<String> {
    r.findings.iter().map(|f| f.code.clone()).collect()
}

#[test]
fn clean_provider_passes() {
    // 3-part description, good name, valid A2MCP service.
    let desc = "Summarizes text.\\nHandles long docs and articles.\\nSummarize this article";
    let service = svc(
        "Document Summarizer",
        desc,
        "A2MCP",
        "10 USDT",
        Some("https://example.com/mcp"),
    );
    let r = run_validation(
        "provider",
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
    let r = run_validation("provider", Some("FitnessBot(test)"), None, None);
    assert!(codes(&r).contains(&"U1".to_string()));
    assert!(!r.pass);
}

#[test]
fn name_predict_does_not_fail_u1() {
    // "Predict" contains "pre" but is not a delimited marker.
    let r = run_validation("requester", Some("Predict"), None, None);
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
        "5 USDT",
        Some(""),
    );
    let r = run_validation("provider", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"T2".to_string()), "got {:?}", codes(&r));
    assert!(!r.pass);
}

#[test]
fn a2a_with_endpoint_fails_t3() {
    let service = svc(
        "Some A2A Service",
        "Does a thing.\\nMore detail here.\\nDo the thing",
        "A2A",
        "5 USDT",
        Some("https://example.com"),
    );
    let r = run_validation("provider", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"T3".to_string()), "got {:?}", codes(&r));
    assert!(!r.pass);
}

#[test]
fn fee_with_negotiable_paren_fails_p3_and_p4() {
    let service = svc(
        "Pricing Service",
        "Does a thing.\\nMore detail here.\\nDo the thing",
        "A2A",
        "0.2 USDT (negotiable)",
        None,
    );
    let r = run_validation("provider", Some("Agent Name"), None, Some(&service));
    let c = codes(&r);
    assert!(c.contains(&"P3".to_string()), "got {:?}", c);
    assert!(c.contains(&"P4".to_string()), "got {:?}", c);
    assert!(!r.pass);
}

#[test]
fn bad_currency_fails_p2() {
    let service = svc(
        "ETH Pricing Service",
        "Does a thing.\\nMore detail here.\\nDo the thing",
        "A2A",
        "5 ETH",
        None,
    );
    let r = run_validation("provider", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"P2".to_string()), "got {:?}", codes(&r));
    assert!(!r.pass);
}

#[test]
fn too_short_service_name_fails_s1() {
    let service = svc(
        "Q",
        "Does a thing.\\nMore detail here.\\nDo the thing",
        "A2A",
        "5 USDT",
        None,
    );
    let r = run_validation("provider", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"S1".to_string()), "got {:?}", codes(&r));
    assert!(!r.pass);
}

#[test]
fn requester_ignores_service() {
    // A bad service should NOT produce findings for a requester role.
    let service = svc("Q", "x", "BADTYPE", "", None);
    let r = run_validation("requester", Some("Buyer Bot"), None, Some(&service));
    assert!(r.pass, "got {:?}", codes(&r));
}

#[test]
fn bilingual_name_without_middledot_fails_n6() {
    let r = run_validation("provider", Some("健身 Bot"), None, None);
    assert!(codes(&r).contains(&"N6".to_string()), "got {:?}", codes(&r));
}

#[test]
fn bilingual_name_with_middledot_ok_n6() {
    let r = run_validation("provider", Some("健身 \u{00B7} Bot"), None, None);
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
        "provider",
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
    let r = run_validation("provider", Some("一二三四五六七八九十一二三"), None, None);
    assert!(codes(&r).contains(&"N1".to_string()), "got {:?}", codes(&r));
}

#[test]
fn hex_in_service_description_emits_d7_not_duplicate_u2() {
    // A 0x address in `servicedescription` must surface once as D7, never
    // also as U2 for the same field (no duplicate diagnostic).
    let desc = "Summarizes text 0xdeadbeefdeadbeef.\\nHandles long docs.\\nSummarize this";
    let service = svc("Document Summarizer", desc, "A2A", "0 USDT", None);
    let r = run_validation("provider", Some("Summary Bot"), None, Some(&service));
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
    let r = run_validation("requester", Some("Agent 0xdeadbeef"), None, None);
    assert!(codes(&r).contains(&"U2".to_string()));
}

#[test]
fn embedded_id_fails_n2() {
    let r = run_validation("provider", Some("Helper Bot 3"), None, None);
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
    let r = run_validation("provider", Some("Agent Name"), None, Some(&service));
    let c = codes(&r);
    assert!(!c.contains(&"P1".to_string()), "got {:?}", c);
    assert!(!c.contains(&"P2".to_string()), "got {:?}", c);
}
