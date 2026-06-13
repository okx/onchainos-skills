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
            "[{{\"name\":\"{name}\",\"servicedescription\":\"{desc_json}\",\"servicetype\":\"{stype}\",\"fee\":\"{fee}\"{ep}}}]"
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

// ─── N1: Latin/mixed name length boundary values ──────────────────────────

#[test]
fn latin_name_two_chars_fails_n1() {
    // 2 chars — below the 3-char Latin minimum.
    let r = run_validation("provider", Some("AB"), None, None);
    assert!(codes(&r).contains(&"N1".to_string()), "got {:?}", codes(&r));
}

#[test]
fn latin_name_three_chars_passes_n1() {
    let r = run_validation("provider", Some("Bot"), None, None);
    assert!(!codes(&r).contains(&"N1".to_string()), "got {:?}", codes(&r));
}

#[test]
fn latin_name_twenty_five_chars_passes_n1() {
    // Exactly 25 chars — upper bound is inclusive.
    let r = run_validation("provider", Some("ABCDEFGHIJKLMNOPQRSTUVWXY"), None, None);
    assert!(!codes(&r).contains(&"N1".to_string()), "got {:?}", codes(&r));
}

#[test]
fn latin_name_twenty_six_chars_fails_n1() {
    // 26 chars — one over the limit.
    let r = run_validation("provider", Some("ABCDEFGHIJKLMNOPQRSTUVWXYZ"), None, None);
    assert!(codes(&r).contains(&"N1".to_string()), "got {:?}", codes(&r));
}

// ─── N3: ordinal suffix ────────────────────────────────────────────────────

#[test]
fn ordinal_suffix_v2_fails_n3() {
    assert!(has_ordinal_suffix("Agent_v2"));
    let r = run_validation("provider", Some("Agent_v2"), None, None);
    assert!(codes(&r).contains(&"N3".to_string()), "got {:?}", codes(&r));
}

#[test]
fn ordinal_suffix_paren_digit_fails_n3() {
    assert!(has_ordinal_suffix("Agent Bot (2)"));
    let r = run_validation("provider", Some("Agent Bot (2)"), None, None);
    assert!(codes(&r).contains(&"N3".to_string()), "got {:?}", codes(&r));
}

#[test]
fn hash_suffix_triggers_n3() {
    // "Bot#3": has_ordinal_suffix detects the trailing #3, has_embedded_agent_id detects #3,
    // and has_decorative_symbols detects '#' in DECOR — so N2 + N3 + N8 all fire together.
    assert!(has_ordinal_suffix("Bot#3"));
    let r = run_validation("provider", Some("Bot#3"), None, None);
    let c = codes(&r);
    assert!(c.contains(&"N3".to_string()), "expected N3, got {:?}", c);
    assert!(c.contains(&"N2".to_string()), "expected N2 (hash marker_digit_run), got {:?}", c);
    assert!(c.contains(&"N8".to_string()), "expected N8 ('#' in DECOR), got {:?}", c);
}

#[test]
fn plain_name_does_not_fail_n3() {
    // "Bot Proto" contains no ordinal suffix, no trailing number, no decorative symbols.
    assert!(!has_ordinal_suffix("Bot Proto"));
    let r = run_validation("provider", Some("Bot Proto"), None, None);
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
    let r = run_validation("provider", Some("My Bot!"), None, None);
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
    let r = run_validation("provider", Some("Does not support"), None, None);
    assert!(codes(&r).contains(&"U3".to_string()), "got {:?}", codes(&r));
}

#[test]
fn description_with_negative_capability_fails_u3() {
    let r = run_validation(
        "provider",
        Some("GoodBot"),
        Some("currently not supported for this chain"),
        None,
    );
    assert!(codes(&r).contains(&"U3".to_string()), "got {:?}", codes(&r));
}

#[test]
fn cjk_negative_capability_fails_u3() {
    assert!(contains_negative_capability("暂不支持"));
    assert!(contains_negative_capability("不支持"));
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
    let r = run_validation("provider", Some("Agent Name"), None, Some(&service));
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
    let r = run_validation("provider", Some("Agent Name"), None, Some(&service));
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
    let r = run_validation("provider", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"P1".to_string()), "got {:?}", codes(&r));
}

// ─── S3: service name duplicates agent name ────────────────────────────────

#[test]
fn service_name_same_as_agent_name_fails_s3() {
    let service = svc(
        "Agent Name",
        "Does a thing.\nMore detail here.\nDo the thing",
        "A2A",
        "5 USDT",
        None,
    );
    let r = run_validation("provider", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"S3".to_string()), "got {:?}", codes(&r));
}

#[test]
fn service_name_case_insensitive_duplicate_fails_s3() {
    let service = svc(
        "agent name",
        "Does a thing.\nMore detail here.\nDo the thing",
        "A2A",
        "5 USDT",
        None,
    );
    let r = run_validation("provider", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"S3".to_string()), "got {:?}", codes(&r));
}

#[test]
fn service_name_different_from_agent_passes_s3() {
    let service = svc(
        "Trade Executor",
        "Does a thing.\nMore detail here.\nDo the thing",
        "A2A",
        "5 USDT",
        None,
    );
    let r = run_validation("provider", Some("Agent Name"), None, Some(&service));
    assert!(!codes(&r).contains(&"S3".to_string()), "got {:?}", codes(&r));
}

// ─── S4: service name contains price info ─────────────────────────────────

#[test]
fn service_name_with_usdt_fails_s4() {
    let service = svc(
        "Pay 5 USDT Service",
        "Does a thing.\nMore detail here.\nDo the thing",
        "A2A",
        "5 USDT",
        None,
    );
    let r = run_validation("provider", Some("Other Agent"), None, Some(&service));
    assert!(codes(&r).contains(&"S4".to_string()), "got {:?}", codes(&r));
}

#[test]
fn service_name_with_free_fails_s4() {
    let service = svc(
        "Get Access Free",
        "Does a thing.\nMore detail here.\nDo the thing",
        "A2A",
        "0 USDT",
        None,
    );
    let r = run_validation("provider", Some("Other Agent"), None, Some(&service));
    assert!(codes(&r).contains(&"S4".to_string()), "got {:?}", codes(&r));
}

// ─── S6: service name with test marker ────────────────────────────────────

#[test]
fn service_name_with_test_marker_fails_s6() {
    let service = svc(
        "Trade Bot (test)",
        "Does a thing.\nMore detail here.\nDo the thing",
        "A2A",
        "5 USDT",
        None,
    );
    let r = run_validation("provider", Some("Other Agent"), None, Some(&service));
    assert!(codes(&r).contains(&"S6".to_string()), "got {:?}", codes(&r));
}

// ─── U5: contradicting type token ─────────────────────────────────────────

#[test]
fn a2a_service_name_mentioning_a2mcp_fails_u5() {
    let service = svc(
        "My A2MCP Service",
        "Does a thing.\nMore detail here.\nDo the thing",
        "A2A",
        "5 USDT",
        None,
    );
    let r = run_validation("provider", Some("Other Agent"), None, Some(&service));
    assert!(codes(&r).contains(&"U5".to_string()), "got {:?}", codes(&r));
}

#[test]
fn a2mcp_service_name_mentioning_a2a_fails_u5() {
    let service = svc(
        "Use a2a protocol",
        "Does a thing.\nMore detail here.\nDo the thing",
        "A2MCP",
        "5 USDT",
        Some("https://example.com/mcp"),
    );
    let r = run_validation("provider", Some("Other Agent"), None, Some(&service));
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
        "5 USDT",
        None,
    );
    let r = run_validation("provider", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"T1".to_string()), "got {:?}", codes(&r));
}

#[test]
fn empty_servicetype_fails_t1() {
    let service = svc(
        "Some Service",
        "Does a thing.\nMore detail here.\nDo the thing",
        "",
        "5 USDT",
        None,
    );
    let r = run_validation("provider", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"T1".to_string()), "got {:?}", codes(&r));
}

// ─── PARSE: invalid JSON array ─────────────────────────────────────────────

#[test]
fn invalid_service_json_fails_parse() {
    let r = run_validation("provider", Some("Agent Name"), None, Some("not json at all"));
    assert!(codes(&r).contains(&"PARSE".to_string()), "got {:?}", codes(&r));
    assert!(!r.pass);
}

#[test]
fn service_json_object_not_array_fails_parse() {
    let r = run_validation(
        "provider",
        Some("Agent Name"),
        None,
        Some("{\"name\":\"foo\"}"),
    );
    assert!(codes(&r).contains(&"PARSE".to_string()), "got {:?}", codes(&r));
}

#[test]
fn requester_ignores_invalid_service_json() {
    // Requester silently ignores --service regardless of content.
    let r = run_validation("requester", Some("Buyer Bot"), None, Some("not json"));
    assert!(r.pass, "got {:?}", codes(&r));
}

// ─── D1–D6: service description structure ─────────────────────────────────

#[test]
fn description_missing_parts_fails_d1() {
    // Only one line → D1.
    let service = svc(
        "Doc Summarizer",
        "Does one thing only",
        "A2A",
        "5 USDT",
        None,
    );
    let r = run_validation("provider", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"D1".to_string()), "got {:?}", codes(&r));
}

#[test]
fn description_two_parts_fails_d1() {
    let service = svc(
        "Doc Summarizer",
        "Summary line.\nCapabilities line.",
        "A2A",
        "5 USDT",
        None,
    );
    let r = run_validation("provider", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"D1".to_string()), "got {:?}", codes(&r));
}

#[test]
fn description_over_400_chars_fails_d2() {
    // 401-char description.
    let long = "x".repeat(401);
    let service = svc("Doc Summarizer", &long, "A2A", "5 USDT", None);
    let r = run_validation("provider", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"D2".to_string()), "got {:?}", codes(&r));
}

#[test]
fn description_part1_over_50_chars_fails_d3() {
    // Part 1 (summary) exceeds 50 chars.
    let p1 = "A".repeat(51);
    let desc = format!("{p1}\nCapabilities paragraph here.\nExample prompt one");
    let service = svc("Doc Summarizer", &desc, "A2A", "5 USDT", None);
    let r = run_validation("provider", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"D3".to_string()), "got {:?}", codes(&r));
}

#[test]
fn description_part2_over_150_chars_fails_d4() {
    let p2 = "B".repeat(151);
    let desc = format!("Short summary.\n{p2}\nExample prompt one");
    let service = svc("Doc Summarizer", &desc, "A2A", "5 USDT", None);
    let r = run_validation("provider", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"D4".to_string()), "got {:?}", codes(&r));
}

#[test]
fn description_over_3_prompts_fails_d5() {
    let desc = "Short summary.\nCapabilities line.\nPrompt one\nPrompt two\nPrompt three\nPrompt four";
    let service = svc("Doc Summarizer", desc, "A2A", "5 USDT", None);
    let r = run_validation("provider", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"D5".to_string()), "got {:?}", codes(&r));
}

#[test]
fn description_prompt_over_80_chars_fails_d5() {
    let long_prompt = "C".repeat(81);
    let desc = format!("Short summary.\nCapabilities line.\n{long_prompt}");
    let service = svc("Doc Summarizer", &desc, "A2A", "5 USDT", None);
    let r = run_validation("provider", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"D5".to_string()), "got {:?}", codes(&r));
}

#[test]
fn description_with_url_fails_d6() {
    let desc = "Short summary.\nCapabilities.\nhttps://example.com for more";
    let service = svc("Doc Summarizer", desc, "A2A", "5 USDT", None);
    let r = run_validation("provider", Some("Agent Name"), None, Some(&service));
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
        "5 USDT",
        None,
    );
    let r = run_validation("provider", Some("Agent Name"), None, Some(&service));
    assert!(codes(&r).contains(&"S1".to_string()), "got {:?}", codes(&r));
}

#[test]
fn service_name_five_chars_passes_s1() {
    let service = svc(
        "Abcde",
        "Does a thing.\nMore detail here.\nDo the thing",
        "A2A",
        "5 USDT",
        None,
    );
    let r = run_validation("provider", Some("Agent Name"), None, Some(&service));
    assert!(!codes(&r).contains(&"S1".to_string()), "got {:?}", codes(&r));
}

#[test]
fn service_name_thirty_chars_passes_s1() {
    let service = svc(
        "ABCDEFGHIJKLMNOPQRSTUVWXYZ1234",
        "Does a thing.\nMore detail here.\nDo the thing",
        "A2A",
        "5 USDT",
        None,
    );
    let r = run_validation("provider", Some("Agent Name"), None, Some(&service));
    assert!(!codes(&r).contains(&"S1".to_string()), "got {:?}", codes(&r));
}

#[test]
fn service_name_thirty_one_chars_fails_s1() {
    let service = svc(
        "ABCDEFGHIJKLMNOPQRSTUVWXYZ12345",
        "Does a thing.\nMore detail here.\nDo the thing",
        "A2A",
        "5 USDT",
        None,
    );
    let r = run_validation("provider", Some("Agent Name"), None, Some(&service));
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
    // "pretest_bot" DOES trigger: delimited_marker_present finds "_test" at position 7,
    // next char is '_' (non-alphanumeric boundary) → returns true.
    // The prefix "pre" before "_test" is irrelevant to the algorithm.
    assert!(has_test_marker("pretest_bot"));
}

#[test]
fn hyphen_test_mid_word_does_not_trigger() {
    // "-testing" → after "-test" the next char is 'i' (alphanumeric) → no match.
    assert!(!has_test_marker("bot-testing"));
}

// ─── provider description U3 ──────────────────────────────────────────────

#[test]
fn provider_description_with_negative_capability_fails_u3() {
    let r = run_validation(
        "provider",
        Some("GoodBot"),
        Some("Does not support trading"),
        None,
    );
    assert!(codes(&r).contains(&"U3".to_string()), "got {:?}", codes(&r));
}

#[test]
fn requester_description_with_negative_capability_also_fails_u3() {
    // Universal text rules apply to all roles.
    let r = run_validation(
        "requester",
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
        "5 USDT",
        None,
    );
    // No name provided → agent_name = "" → S3 guard skips.
    let r = run_validation("provider", None, None, Some(&service));
    assert!(!codes(&r).contains(&"S3".to_string()), "got {:?}", codes(&r));
}

// S4: CJK free word triggers S4
#[test]
fn service_name_with_cjk_free_fails_s4() {
    let service = svc(
        "免费翻译服务Pro",
        "Does a thing.\nMore detail here.\nDo the thing",
        "A2A",
        "0 USDT",
        None,
    );
    let r = run_validation("provider", Some("Agent Name"), None, Some(&service));
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
        "5 USDT",
        None,
    );
    let r = run_validation("provider", Some("Agent Name"), None, Some(&service));
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

// D1 + D2 both fire on single long line
#[test]
fn description_over_400_single_line_fails_d1_and_d2() {
    // A single line of 401 chars → D2 (too long) and D1 (only 1 part).
    let long = "x".repeat(401);
    let service = svc("Doc Summarizer", &long, "A2A", "5 USDT", None);
    let r = run_validation("provider", Some("Agent Name"), None, Some(&service));
    let c = codes(&r);
    assert!(c.contains(&"D2".to_string()), "expected D2, got {:?}", c);
    assert!(c.contains(&"D1".to_string()), "expected D1 (single line → <3 parts), got {:?}", c);
}

// N3 integration: No. suffix through run_validation
#[test]
fn no_ordinal_suffix_integration_fails_n3() {
    assert!(has_ordinal_suffix("BotNo3"));
    let r = run_validation("provider", Some("BotNo3"), None, None);
    assert!(codes(&r).contains(&"N3".to_string()), "got {:?}", codes(&r));
}

