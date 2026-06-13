use super::*;
use serde_json::json;
use super::super::models::{AgentCard, AgentService};
use crate::client::DEFAULT_BASE_URL;
use crate::commands::Context;
use crate::config::AppConfig;

fn ctx_no_override() -> Context {
    Context {
        config: AppConfig::default(),
        base_url_override: None,
        chain_override: None,
    }
}

fn ctx_with_base(url: &str) -> Context {
    Context {
        config: AppConfig::default(),
        base_url_override: Some(url.to_string()),
        chain_override: None,
    }
}

// ─── parse_stars_arg: happy path ─────────────────────────────────────

#[test]
fn parse_stars_arg_accepts_integers() {
    assert_eq!(parse_stars_arg("0", "--score").unwrap(), 0);
    assert_eq!(parse_stars_arg("1", "--score").unwrap(), 20);
    assert_eq!(parse_stars_arg("5", "--score").unwrap(), 100);
}

#[test]
fn parse_stars_arg_accepts_one_and_two_decimals() {
    assert_eq!(parse_stars_arg("4.5", "--score").unwrap(), 90);
    assert_eq!(parse_stars_arg("5.00", "--score").unwrap(), 100);
    assert_eq!(parse_stars_arg("0.01", "--score").unwrap(), 0); // 0.2 → round to 0
    assert_eq!(parse_stars_arg("0.03", "--score").unwrap(), 1); // 0.6 → round to 1
}

#[test]
fn parse_stars_arg_round_half_up_at_wire_boundary() {
    // 3.30 / 3.31 / 3.32 all collapse to wire 66 (0.05-star grain).
    assert_eq!(parse_stars_arg("3.30", "--score").unwrap(), 66);
    assert_eq!(parse_stars_arg("3.31", "--score").unwrap(), 66);
    assert_eq!(parse_stars_arg("3.32", "--score").unwrap(), 66);
    // 3.33 rounds up to wire 67 (= 66.6 round-half-up).
    assert_eq!(parse_stars_arg("3.33", "--score").unwrap(), 67);
    // 3.35 is exact (no rounding needed).
    assert_eq!(parse_stars_arg("3.35", "--score").unwrap(), 67);
    // Upper-edge: 4.97 → 99.4 → 99; 4.98 / 4.99 → 100.
    assert_eq!(parse_stars_arg("4.97", "--score").unwrap(), 99);
    assert_eq!(parse_stars_arg("4.98", "--score").unwrap(), 100);
    assert_eq!(parse_stars_arg("4.99", "--score").unwrap(), 100);
}

#[test]
fn parse_stars_arg_trims_whitespace() {
    assert_eq!(parse_stars_arg("  4.5  ", "--score").unwrap(), 90);
}

// ─── parse_stars_arg: rejected inputs ────────────────────────────────

#[test]
fn parse_stars_arg_rejects_more_than_two_decimals() {
    assert!(parse_stars_arg("3.333", "--score").is_err());
    assert!(parse_stars_arg("0.001", "--score").is_err());
}

#[test]
fn parse_stars_arg_rejects_trailing_dot() {
    assert!(parse_stars_arg("3.", "--score").is_err());
}

#[test]
fn parse_stars_arg_rejects_signs_and_exponent() {
    assert!(parse_stars_arg("-1", "--score").is_err());
    assert!(parse_stars_arg("+5", "--score").is_err());
    assert!(parse_stars_arg("5e0", "--score").is_err());
}

#[test]
fn parse_stars_arg_rejects_out_of_range() {
    assert!(parse_stars_arg("6", "--score").is_err());
    assert!(parse_stars_arg("5.01", "--score").is_err());
}

#[test]
fn parse_stars_arg_rejects_non_numeric() {
    assert!(parse_stars_arg("abc", "--score").is_err());
    assert!(parse_stars_arg("3.3.3", "--score").is_err());
    assert!(parse_stars_arg("", "--score").is_err());
    assert!(parse_stars_arg("   ", "--score").is_err());
}

// ─── score_to_stars: wire (0..=100) → stars (0.0..=5.0) ──────────────

#[test]
fn score_to_stars_is_exact_at_two_decimals() {
    assert_eq!(score_to_stars(0), 0.0);
    assert_eq!(score_to_stars(66), 3.3);
    assert_eq!(score_to_stars(67), 3.35);
    assert_eq!(score_to_stars(70), 3.5);
    assert_eq!(score_to_stars(89), 4.45);
    assert_eq!(score_to_stars(90), 4.5);
    assert_eq!(score_to_stars(100), 5.0);
}

#[test]
fn score_to_stars_clamps_above_100() {
    assert_eq!(score_to_stars(101), 5.0);
    assert_eq!(score_to_stars(u64::MAX), 5.0);
}

// ─── convert_feedback_list_scores: average + items + list ────────────

fn assert_score_eq(v: &Value, expected: f64) {
    let got = v.as_f64().expect("expected numeric");
    assert!(
        (got - expected).abs() < 1e-9,
        "expected {expected}, got {got}"
    );
}

#[test]
fn convert_feedback_list_scores_rewrites_average_and_items() {
    let mut v = json!({
        "average": 89,
        "items": [
            { "score": 90 },
            { "score": 70 },
            { "score": 67 },
        ],
    });
    convert_feedback_list_scores(&mut v);
    assert_score_eq(&v["average"], 4.45);
    assert_score_eq(&v["items"][0]["score"], 4.5);
    assert_score_eq(&v["items"][1]["score"], 3.5);
    assert_score_eq(&v["items"][2]["score"], 3.35);
}

#[test]
fn convert_feedback_list_scores_rewrites_list_field() {
    let mut v = json!({ "list": [ { "score": 100 } ] });
    convert_feedback_list_scores(&mut v);
    assert_score_eq(&v["list"][0]["score"], 5.0);
}

#[test]
fn convert_feedback_list_scores_leaves_non_numeric_fields_alone() {
    let mut v = json!({
        "average": "n/a",
        "items": [
            { "score": "n/a" },
            { "other_field": 5 },
        ],
    });
    let before = v.clone();
    convert_feedback_list_scores(&mut v);
    assert_eq!(v, before);
}

// ─── normalize_bcp47 ─────────────────────────────────────────────────

#[test]
fn normalize_bcp47_canonicalizes_casing_and_separator() {
    assert_eq!(normalize_bcp47(Some("zh-CN")).as_deref(), Some("zh-CN"));
    assert_eq!(normalize_bcp47(Some("zh_CN")).as_deref(), Some("zh-CN"));
    assert_eq!(normalize_bcp47(Some("ZH-cn")).as_deref(), Some("zh-CN"));
    assert_eq!(normalize_bcp47(Some("en_us")).as_deref(), Some("en-US"));
    assert_eq!(
        normalize_bcp47(Some("zh-hant-tw")).as_deref(),
        Some("zh-Hant-TW")
    );
    assert_eq!(normalize_bcp47(Some("  en-US  ")).as_deref(), Some("en-US"));
}

#[test]
fn normalize_bcp47_default_region_completes_bare_language() {
    // Bare supported languages get the product's canonical region.
    assert_eq!(normalize_bcp47(Some("zh")).as_deref(), Some("zh-CN"));
    assert_eq!(normalize_bcp47(Some("ZH")).as_deref(), Some("zh-CN"));
    assert_eq!(normalize_bcp47(Some("en")).as_deref(), Some("en-US"));
    assert_eq!(normalize_bcp47(Some("ja")).as_deref(), Some("ja-JP"));
    // Unmapped bare languages pass through unchanged.
    assert_eq!(normalize_bcp47(Some("fr")).as_deref(), Some("fr"));
    // Tags that already carry a region / script are NOT overridden.
    assert_eq!(normalize_bcp47(Some("zh-TW")).as_deref(), Some("zh-TW"));
    assert_eq!(normalize_bcp47(Some("zh-Hant")).as_deref(), Some("zh-Hant"));
    assert_eq!(normalize_bcp47(Some("en-GB")).as_deref(), Some("en-GB"));
}

#[test]
fn normalize_bcp47_rejects_blank_and_malformed_language() {
    assert_eq!(normalize_bcp47(None), None);
    assert_eq!(normalize_bcp47(Some("")), None);
    assert_eq!(normalize_bcp47(Some("   ")), None);
    assert_eq!(normalize_bcp47(Some("1-CN")), None); // language subtag not alpha
    assert_eq!(normalize_bcp47(Some("z")), None); // too short
}

// ─── agent get row enrichment: label mappings ────────────────────────

#[test]
fn role_label_maps_known_and_omits_unknown() {
    assert_eq!(role_label("requester"), Some("User Agent"));
    assert_eq!(role_label("provider"), Some("Agent Service Provider (ASP)"));
    assert_eq!(role_label("evaluator"), Some("Evaluator Agent"));
    assert_eq!(
        role_label(" provider "),
        Some("Agent Service Provider (ASP)")
    );
    assert_eq!(role_label("buyer"), None); // raw alias not a canonical enum
    assert_eq!(role_label(""), None);
}

#[test]
fn status_label_maps_int_and_string() {
    assert_eq!(status_label(&json!(1)), Some("active"));
    assert_eq!(status_label(&json!("active")), Some("active"));
    assert_eq!(status_label(&json!(2)), Some("not listed"));
    assert_eq!(status_label(&json!("2")), Some("not listed"));
    assert_eq!(status_label(&json!(3)), Some("unavailable"));
    assert_eq!(status_label(&json!(4)), Some("unavailable"));
    assert_eq!(status_label(&json!(5)), Some("unavailable"));
    assert_eq!(status_label(&json!(99)), None);
    assert_eq!(status_label(&json!(null)), None);
}

#[test]
fn approval_label_maps_known_codes() {
    assert_eq!(approval_label(1), Some("Not listed"));
    assert_eq!(approval_label(2), Some("Listing under review"));
    assert_eq!(
        approval_label(4),
        Some("Listed — eligible for task recommendations")
    );
    assert_eq!(approval_label(5), Some("Listing rejected"));
    assert_eq!(
        approval_label(7),
        Some("This agent is currently unavailable")
    );
    assert_eq!(approval_label(3), None);
    assert_eq!(approval_label(0), None);
}

// ─── agent get row enrichment: ratingStars ───────────────────────────

#[test]
fn format_rating_stars_representative_values() {
    assert_eq!(format_rating_stars(92), "4.6"); // 4.60 → trailing zero trimmed
    assert_eq!(format_rating_stars(89), "4.45");
    assert_eq!(format_rating_stars(100), "5"); // whole
    assert_eq!(format_rating_stars(0), "0");
    assert_eq!(format_rating_stars(90), "4.5");
    assert_eq!(format_rating_stars(85), "4.25");
    assert_eq!(format_rating_stars(70), "3.5");
    assert_eq!(format_rating_stars(66), "3.3");
    assert_eq!(format_rating_stars(101), "5"); // clamped
}

#[test]
fn rating_stars_omitted_when_no_reputation() {
    // count == 0 → omit
    assert_eq!(rating_stars(&json!({ "score": 0, "count": 0 })), None);
    // score absent → omit
    assert_eq!(rating_stars(&json!({ "count": 3 })), None);
    // present score + nonzero count → Some
    assert_eq!(
        rating_stars(&json!({ "score": 92, "count": 18 })).as_deref(),
        Some("4.6")
    );
    // score 0 with positive count is a real "0 stars" rating → keep
    assert_eq!(
        rating_stars(&json!({ "score": 0, "count": 2 })).as_deref(),
        Some("0")
    );
}

// ─── agent get row enrichment: full row + envelope walk ──────────────

#[test]
fn enrich_agent_row_adds_all_four_fields() {
    let mut row = json!({
        "agentId": 42,
        "role": "provider",
        "status": 1,
        "approvalDisplayStatus": 4,
        "reputation": { "score": 92, "count": 18 },
    });
    enrich_agent_row(&mut row);
    assert_eq!(row["roleLabel"], json!("Agent Service Provider (ASP)"));
    assert_eq!(row["statusLabel"], json!("active"));
    assert_eq!(
        row["approvalLabel"],
        json!("Listed — eligible for task recommendations")
    );
    assert_eq!(row["ratingStars"], json!("4.6"));
    // Raw fields untouched.
    assert_eq!(row["role"], json!("provider"));
    assert_eq!(row["status"], json!(1));
    assert_eq!(row["approvalDisplayStatus"], json!(4));
    assert_eq!(row["reputation"], json!({ "score": 92, "count": 18 }));
}

#[test]
fn enrich_agent_row_omits_unknown_and_absent() {
    let mut row = json!({
        "agentId": 7,
        "role": "buyer",        // alias, not canonical → omit roleLabel
        "status": 99,            // unknown → omit statusLabel
        // no approvalDisplayStatus → omit approvalLabel
        "reputation": { "count": 0 }, // count 0 → omit ratingStars
    });
    enrich_agent_row(&mut row);
    assert!(row.get("roleLabel").is_none());
    assert!(row.get("statusLabel").is_none());
    assert!(row.get("approvalLabel").is_none());
    assert!(row.get("ratingStars").is_none());
}

// ─── agent get row enrichment: `card` array ─────────────────────────

#[test]
fn build_agent_card_provider_full_ordered_with_services_and_rating() {
    let mut row = json!({
        "agentId": 42,
        "name": "DeFi Analyzer",
        "role": "provider",
        "status": 1,
        "approvalDisplayStatus": 4,
        "address": "0xabcdef0123456789abcdef0123456789abcd1234",
        "description": "On-chain data analysis.",
        "picture": "https://cdn.example.com/a.png",
        "services": [
            { "serviceName": "TVL Query", "serviceType": "A2MCP", "fee": "10",
              "endpoint": "https://api.example.com/mcp" },
            { "serviceName": "Yield Check", "serviceType": "A2A" },
            { "serviceName": "Whale Alert", "serviceType": "A2A", "fee": "5" },
        ],
        "reputation": { "score": 92, "count": 18 },
    });
    enrich_agent_row(&mut row);
    let card = row["card"].as_array().expect("card present");
    let pairs: Vec<(&str, &str)> = card
        .iter()
        .map(|r| (r["label"].as_str().unwrap(), r["value"].as_str().unwrap()))
        .collect();
    assert_eq!(
        pairs,
        vec![
            ("Agent ID", "#42"),
            ("Name", "DeFi Analyzer"),
            ("Role", "Agent Service Provider (ASP)"),
            ("Status", "active"),
            (
                "Approval status",
                "Listed — eligible for task recommendations"
            ),
            ("Address", "0xabcd…1234"),
            ("Description", "On-chain data analysis."),
            ("Profile photo", "https://cdn.example.com/a.png"),
            (
                "Service 1",
                "TVL Query — API service, 10 USDT, https://api.example.com/mcp"
            ),
            ("Service 2", "Yield Check — agent-to-agent, free"),
            ("Service 3", "Whale Alert — agent-to-agent, 5 USDT"),
            ("Rating", "★ 4.6 (18 reviews)"),
        ]
    );
    // Raw fields untouched.
    assert_eq!(row["role"], json!("provider"));
    assert!(row["services"].is_array());
}

#[test]
fn build_agent_card_requester_has_no_service_rows_and_description_not_set() {
    let mut row = json!({
        "agentId": 58,
        "name": "MyBuyer",
        "role": "requester",
        "status": 1,
        // empty description → "(not set)"; no picture → "default".
        "description": "",
        // Anomaly: backend returned services for a non-provider — must be dropped.
        "services": [
            { "serviceName": "Should Not Appear", "serviceType": "A2MCP", "fee": "1",
              "endpoint": "https://x" },
        ],
        "reputation": { "score": 0, "count": 0 },
    });
    enrich_agent_row(&mut row);
    let card = row["card"].as_array().expect("card present");
    let labels: Vec<&str> = card.iter().map(|r| r["label"].as_str().unwrap()).collect();
    // No Service rows at all, even though services[] is non-empty.
    assert!(labels.iter().all(|l| !l.starts_with("Service")));
    // Description always emitted with "(not set)" when empty.
    let desc = card
        .iter()
        .find(|r| r["label"] == json!("Description"))
        .expect("description row");
    assert_eq!(desc["value"], json!("(not set)"));
    // Profile photo defaults to "default".
    let photo = card
        .iter()
        .find(|r| r["label"] == json!("Profile photo"))
        .expect("photo row");
    assert_eq!(photo["value"], json!("default"));
    // ratingStars omitted when count 0 → no Rating row.
    assert!(labels.iter().all(|l| *l != "Rating"));
}

#[test]
fn build_agent_card_omits_rating_when_count_zero() {
    let mut row = json!({
        "agentId": 7,
        "role": "evaluator",
        "reputation": { "score": 80, "count": 0 },
    });
    enrich_agent_row(&mut row);
    let card = row["card"].as_array().expect("card present");
    assert!(card.iter().all(|r| r["label"] != json!("Rating")));
    // Evaluator is not a provider → no Service rows.
    assert!(card
        .iter()
        .all(|r| !r["label"].as_str().unwrap().starts_with("Service")));
}

#[test]
fn build_agent_card_includes_txhash_when_present() {
    let mut row = json!({
        "agentId": 1,
        "role": "requester",
        "txHash": "0xabcdef0f12",
    });
    enrich_agent_row(&mut row);
    let card = row["card"].as_array().unwrap();
    let tx = card
        .iter()
        .find(|r| r["label"] == json!("txHash"))
        .expect("txHash row");
    assert_eq!(tx["value"], json!("0xabcdef0f12"));
}

// ─── `cells` helpers: truncate_name ─────────────────────────────────

#[test]
fn truncate_name_appends_ellipsis_only_when_longer() {
    assert_eq!(truncate_name("short", 20), "short");
    // 21-char name → truncated to 20 + ellipsis.
    let n21 = "abcdefghijklmnopqrstu"; // 21 chars
    assert_eq!(n21.chars().count(), 21);
    assert_eq!(truncate_name(n21, 20), "abcdefghijklmnopqrst…");
    // exactly 20 → unchanged.
    let n20 = "abcdefghijklmnopqrst"; // 20 chars
    assert_eq!(truncate_name(n20, 20), n20);
}

// ─── §1 agent-list cells ─────────────────────────────────────────────

fn cell_pairs(cells: &Value) -> Vec<(String, String)> {
    cells
        .as_array()
        .expect("cells is an array")
        .iter()
        .map(|c| {
            (
                c["label"].as_str().unwrap().to_string(),
                c["value"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

#[test]
fn build_agent_list_cells_full_provider_row() {
    let row = json!({
        "agentId": 42,
        "name": "DeFi Analyzer",
        "role": "provider",
        "status": 1,
        "approvalDisplayStatus": 4,
        "reputation": { "score": 92, "count": 18 },
    });
    let cells = build_agent_list_cells(row.as_object().unwrap());
    assert_eq!(
        cell_pairs(&Value::Array(cells)),
        vec![
            ("Agent ID".to_string(), "#42".to_string()),
            ("Name".to_string(), "DeFi Analyzer".to_string()),
            (
                "Role".to_string(),
                "Agent Service Provider (ASP)".to_string()
            ),
            ("Status".to_string(), "active".to_string()),
            (
                "Approval status".to_string(),
                "Listed — eligible for task recommendations".to_string()
            ),
            ("Rating".to_string(), "★ 4.6 (18)".to_string()),
        ]
    );
}

#[test]
fn build_agent_list_cells_count_zero_no_rating_and_truncates_name() {
    let row = json!({
        "agentId": "58",
        "name": "A really long agent name that exceeds twenty",
        "role": "requester",
        "status": 1,
        "reputation": { "score": 0, "count": 0 },
    });
    let cells = build_agent_list_cells(row.as_object().unwrap());
    let pairs = cell_pairs(&Value::Array(cells));
    // 6 columns always present.
    assert_eq!(pairs.len(), 6);
    // First 20 chars of the name, then `…` (char 20 happens to be a space).
    assert_eq!(
        pairs[1],
        ("Name".to_string(), "A really long agent …".to_string())
    );
    assert_eq!(pairs[2].1, "User Agent");
    // count 0 → No rating yet (never `—` in list view).
    assert_eq!(
        pairs[5],
        ("Rating".to_string(), "No rating yet".to_string())
    );
    // no approvalDisplayStatus → `—`.
    assert_eq!(pairs[4], ("Approval status".to_string(), "—".to_string()));
}

#[test]
fn build_agent_list_cells_review_failed_with_reason() {
    let row = json!({
        "agentId": 7,
        "name": "RejectedAgent",
        "role": "provider",
        "status": 2,
        "approvalDisplayStatus": 5,
        "approvalRemark": "Name violates policy",
        "reputation": { "score": 80, "count": 3 },
    });
    let cells = build_agent_list_cells(row.as_object().unwrap());
    let pairs = cell_pairs(&Value::Array(cells));
    assert_eq!(
        pairs[4],
        (
            "Approval status".to_string(),
            "Review failed (reason: Name violates policy)".to_string()
        )
    );
    // status 2 → not listed.
    assert_eq!(pairs[3], ("Status".to_string(), "not listed".to_string()));
}

#[test]
fn build_agent_list_cells_review_failed_empty_remark() {
    let row = json!({
        "agentId": 8,
        "name": "X",
        "role": "provider",
        "approvalDisplayStatus": 5,
        "approvalRemark": "   ",
    });
    let cells = build_agent_list_cells(row.as_object().unwrap());
    let pairs = cell_pairs(&Value::Array(cells));
    assert_eq!(
        pairs[4],
        ("Approval status".to_string(), "Review failed".to_string())
    );
    // unknown status (absent) → `—`.
    assert_eq!(pairs[3], ("Status".to_string(), "—".to_string()));
}

#[test]
fn add_agent_list_cells_walks_envelope_and_skips_detail_unaffected() {
    let mut env = json!({
        "total": 1,
        "list": [
            {
                "agentList": [
                    { "agentId": 1, "name": "A", "role": "requester", "status": 1 },
                ],
            },
        ],
    });
    add_agent_list_cells(&mut env);
    let cells = &env["list"][0]["agentList"][0]["cells"];
    assert!(cells.is_array());
    assert_eq!(cells.as_array().unwrap().len(), 6);
}

// ─── §6 search cells ─────────────────────────────────────────────────

#[test]
fn build_search_cells_feedbackrate_not_divided() {
    // feedbackRate is ALREADY 0–5: 4.6 must render as ★ 4.6, NOT 4.6/20.
    let row = json!({
        "agentId": "1128",
        "name": "DeFi Analyzer",
        "profileDescription": "On-chain data analysis",
        "feedbackRate": 4.6,
        "serviceMinPrice": 10.0,
        "services": [
            { "serviceName": "TVL Query", "serviceType": "A2MCP",
              "feeAmount": 10.0, "feeToken": "USDT", "endpoint": "https://x" }
        ],
    });
    let cells = build_search_cells(row.as_object().unwrap());
    assert_eq!(
        cell_pairs(&Value::Array(cells)),
        vec![
            ("Agent ID".to_string(), "#1128".to_string()),
            ("Name".to_string(), "DeFi Analyzer".to_string()),
            ("Rating".to_string(), "★ 4.6".to_string()),
            ("Min price".to_string(), "10.0".to_string()),
            (
                "Top service".to_string(),
                "TVL Query (API service, 10.0 USDT)".to_string()
            ),
        ]
    );
}

#[test]
fn build_search_cells_null_rate_null_price_absent_services() {
    // feedbackRate null → `—`; serviceMinPrice null → `—`; services key
    // absent (NON_NULL) → `—` Top service.
    let row = json!({
        "agentId": "1129",
        "name": "On-chain Insights",
        "profileDescription": "Analytics",
        "feedbackRate": null,
        "serviceMinPrice": null,
    });
    let cells = build_search_cells(row.as_object().unwrap());
    let pairs = cell_pairs(&Value::Array(cells));
    assert_eq!(pairs[2], ("Rating".to_string(), "—".to_string()));
    assert_eq!(pairs[3], ("Min price".to_string(), "—".to_string()));
    assert_eq!(pairs[4], ("Top service".to_string(), "—".to_string()));
}

#[test]
fn build_search_cells_feedbackrate_zero_is_no_rating_yet() {
    // 0 means no feedback yet — never `★ 0`.
    let row = json!({
        "agentId": "1130",
        "name": "NewAgent",
        "feedbackRate": 0,
        "serviceMinPrice": 1.0,
        "services": [
            { "serviceName": "Free Tier", "serviceType": "A2A" }
        ],
    });
    let cells = build_search_cells(row.as_object().unwrap());
    let pairs = cell_pairs(&Value::Array(cells));
    assert_eq!(
        pairs[2],
        ("Rating".to_string(), "No rating yet".to_string())
    );
    // A2A with no fee → "free"; no token appended.
    assert_eq!(
        pairs[4],
        (
            "Top service".to_string(),
            "Free Tier (agent-to-agent, free)".to_string()
        )
    );
}

#[test]
fn add_search_cells_walks_flat_list() {
    let mut env = json!({
        "total": 1,
        "list": [ { "agentId": "1", "name": "A", "feedbackRate": null } ],
    });
    add_search_cells(&mut env);
    assert_eq!(env["list"][0]["cells"].as_array().unwrap().len(), 5);
}

// ─── §4 service-list cells ───────────────────────────────────────────

#[test]
fn build_service_cells_a2mcp_pascalcase() {
    // service-list returns PascalCase keys per references/discover.md §service-list.
    let svc = json!({
        "ServiceName": "TVL Query",
        "ServiceType": "A2MCP",
        "Fee": "10",
        "Endpoint": "https://api.example.com/mcp",
        "ServiceDescription": "Query protocol TVL by chain.",
    });
    let cells = build_service_cells(1, &svc).expect("cells");
    assert_eq!(
        cell_pairs(&Value::Array(cells)),
        vec![
            ("#".to_string(), "1".to_string()),
            ("Name".to_string(), "TVL Query".to_string()),
            ("Type".to_string(), "API service".to_string()),
            ("Fee".to_string(), "10 USDT".to_string()),
            (
                "Endpoint".to_string(),
                "https://api.example.com/mcp".to_string()
            ),
            (
                "Description".to_string(),
                "Query protocol TVL by chain.".to_string()
            ),
        ]
    );
}

#[test]
fn build_service_cells_a2a_no_fee_no_endpoint() {
    let svc = json!({ "ServiceName": "Yield Check", "ServiceType": "A2A" });
    let cells = build_service_cells(2, &svc).expect("cells");
    let pairs = cell_pairs(&Value::Array(cells));
    assert_eq!(pairs[2], ("Type".to_string(), "agent-to-agent".to_string()));
    // A2A no fee → free.
    assert_eq!(pairs[3], ("Fee".to_string(), "free".to_string()));
    // A2A endpoint always `—`.
    assert_eq!(pairs[4], ("Endpoint".to_string(), "—".to_string()));
    // missing description → `—`.
    assert_eq!(pairs[5], ("Description".to_string(), "—".to_string()));
}

#[test]
fn build_service_cells_a2a_with_fee() {
    let svc = json!({ "ServiceName": "Whale Alert", "ServiceType": "A2A", "Fee": "5" });
    let cells = build_service_cells(3, &svc).expect("cells");
    let pairs = cell_pairs(&Value::Array(cells));
    assert_eq!(pairs[3], ("Fee".to_string(), "5 USDT".to_string()));
    assert_eq!(pairs[4], ("Endpoint".to_string(), "—".to_string()));
}

#[test]
fn build_service_cells_returns_none_without_name() {
    let svc = json!({ "ServiceType": "A2MCP", "Fee": "1" });
    assert!(build_service_cells(1, &svc).is_none());
}

#[test]
fn add_service_list_cells_indexes_one_based_and_skips_nameless() {
    let mut resp = json!({
        "agentId": 42,
        "services": [
            { "ServiceName": "First", "ServiceType": "A2A" },
            { "ServiceType": "A2MCP" }, // nameless → no cells, no index burn
            { "ServiceName": "Third", "ServiceType": "A2MCP", "Fee": "2", "Endpoint": "https://z" },
        ],
    });
    add_service_list_cells(&mut resp);
    let svcs = resp["services"].as_array().unwrap();
    assert_eq!(svcs[0]["cells"][0]["value"], json!("1"));
    assert!(svcs[1].get("cells").is_none());
    // Third keeps index 2 (nameless one did not consume an index).
    assert_eq!(svcs[2]["cells"][0]["value"], json!("2"));
}

// ─── §5 feedback-list cells ──────────────────────────────────────────

#[test]
fn build_feedback_cells_full_entry() {
    // score is already a 0.00–5.00 float (convert_feedback_list_scores ran).
    let item = json!({
        "creatorId": 88,
        "score": 5.0,
        "description": "Timely delivery, accurate data",
        "taskId": "0xabc03e8",
        "createdAt": "2026-04-20",
    });
    let cells = build_feedback_cells(item.as_object().unwrap());
    assert_eq!(
        cell_pairs(&Value::Array(cells)),
        vec![
            ("Score".to_string(), "★ 5".to_string()),
            ("Reviewer".to_string(), "#88".to_string()),
            ("Task".to_string(), "0xabc03e8".to_string()),
            ("Date".to_string(), "2026-04-20".to_string()),
            (
                "Comment".to_string(),
                "Timely delivery, accurate data".to_string()
            ),
        ]
    );
}

#[test]
fn build_feedback_cells_no_comment_and_missing_task() {
    let item = json!({
        "creatorId": 77,
        "score": 4.45,
        "createdAt": "2026-04-15",
    });
    let cells = build_feedback_cells(item.as_object().unwrap());
    let pairs = cell_pairs(&Value::Array(cells));
    assert_eq!(pairs[0], ("Score".to_string(), "★ 4.45".to_string()));
    // missing taskId → `—`.
    assert_eq!(pairs[2], ("Task".to_string(), "—".to_string()));
    // empty/missing description → `(no comment)`.
    assert_eq!(
        pairs[4],
        ("Comment".to_string(), "(no comment)".to_string())
    );
}

#[test]
fn add_feedback_list_cells_walks_items() {
    let mut resp = json!({
        "agentId": 42,
        "average": 4.45,
        "items": [
            { "creatorId": 88, "score": 4.5, "description": "ok", "createdAt": "2026-04-20" },
        ],
    });
    add_feedback_list_cells(&mut resp);
    assert_eq!(resp["items"][0]["cells"].as_array().unwrap().len(), 5);
    assert_eq!(resp["items"][0]["cells"][0]["value"], json!("★ 4.5"));
}

#[test]
fn enrich_agent_get_rows_walks_double_layer_envelope() {
    let mut env = json!({
        "total": 1,
        "list": [
            {
                "ownerAddress": "0xabc",
                "agentList": [
                    { "agentId": 1, "role": "requester", "status": 2,
                      "approvalDisplayStatus": 5 },
                    { "agentId": 2, "role": "evaluator", "status": 3,
                      "reputation": { "score": 100, "count": 4 } },
                ],
            },
        ],
    });
    enrich_agent_get_rows(&mut env);
    let rows = &env["list"][0]["agentList"];
    assert_eq!(rows[0]["roleLabel"], json!("User Agent"));
    assert_eq!(rows[0]["statusLabel"], json!("not listed"));
    assert_eq!(rows[0]["approvalLabel"], json!("Listing rejected"));
    assert!(rows[0].get("ratingStars").is_none());
    assert_eq!(rows[1]["roleLabel"], json!("Evaluator Agent"));
    assert_eq!(rows[1]["statusLabel"], json!("unavailable"));
    assert_eq!(rows[1]["ratingStars"], json!("5"));
}

// ─── REAL backend shapes (live /agent-list & /service-list verified) ──
//
// The live `/agent-list` endpoint returns a SINGLE-layer `list[*]` of flat
// agent rows with INTEGER role and `profileDescription` / `profilePicture`
// / `agentWalletAddress` field names (NOT the double-layer / string-role /
// `description` schema the older doc + synthetic tests above assume). These
// tests pin the tolerant handling against the real shapes.

#[test]
fn enrich_agent_row_accepts_integer_role() {
    // role=2 (provider) as an integer, the live backend form.
    let mut row = json!({ "agentId": 392, "role": 2, "status": 1, "approvalDisplayStatus": 1 });
    enrich_agent_row(&mut row);
    assert_eq!(row["roleLabel"], json!("Agent Service Provider (ASP)"));
    assert_eq!(row["statusLabel"], json!("active"));
    assert_eq!(row["approvalLabel"], json!("Not listed"));
    // role untouched (still the integer).
    assert_eq!(row["role"], json!(2));
}

#[test]
fn enrich_agent_get_rows_walks_single_layer_envelope() {
    // Live `/agent-list` shape: data.list[*] are flat agent rows, NO
    // `agentList` sub-layer.
    let mut env = json!({
        "total": 1,
        "list": [
            { "agentId": 392, "name": "Agent 392", "role": 2, "status": 1,
              "approvalDisplayStatus": 1 },
        ],
    });
    enrich_agent_get_rows(&mut env);
    let row = &env["list"][0];
    assert_eq!(row["roleLabel"], json!("Agent Service Provider (ASP)"));
    assert_eq!(row["statusLabel"], json!("active"));
    assert_eq!(row["approvalLabel"], json!("Not listed"));
    // `card` was assembled too.
    assert!(row["card"].is_array());
}

#[test]
fn build_agent_card_reads_live_backend_field_names() {
    // profileDescription / profilePicture / agentWalletAddress + int role.
    let mut row = json!({
        "agentId": 392,
        "name": "Agent 392",
        "role": 2,
        "status": 1,
        "approvalDisplayStatus": 1,
        "agentWalletAddress": "0x30c140554508a515a8da0fe1e2377c4d8eff59d7",
        "profileDescription": "On-chain data analysis.",
        "profilePicture": "https://cdn.example.com/x.png",
    });
    enrich_agent_row(&mut row);
    let card = row["card"].as_array().expect("card present");
    let pairs: Vec<(&str, &str)> = card
        .iter()
        .map(|r| (r["label"].as_str().unwrap(), r["value"].as_str().unwrap()))
        .collect();
    assert!(pairs.contains(&("Role", "Agent Service Provider (ASP)")));
    assert!(pairs.contains(&("Status", "active")));
    assert!(pairs.contains(&("Address", "0x30c1…59d7")));
    assert!(pairs.contains(&("Description", "On-chain data analysis.")));
    assert!(pairs.contains(&("Profile photo", "https://cdn.example.com/x.png")));
}

#[test]
fn add_service_list_cells_walks_array_of_wrappers_with_list_key() {
    // Live `/service-list` shape: data is an ARRAY of
    // `{ agentInfo, list:[service…] }`; services under `list`.
    let mut data = json!([
        {
            "agentInfo": { "agentId": "392", "name": "Agent 392" },
            "list": [
                { "serviceName": "Mock Service 1", "serviceType": "A2MCP",
                  "fee": "0.3", "endpoint": "https://x", "serviceDescription": "desc" },
                { "serviceName": "Mock Service 2", "serviceType": "A2A" },
            ],
            "page": 1, "pageSize": 20, "total": 2,
        }
    ]);
    add_service_list_cells(&mut data);
    let svcs = &data[0]["list"];
    assert_eq!(svcs[0]["cells"][0], json!({ "label": "#", "value": "1" }));
    assert_eq!(
        svcs[0]["cells"][1],
        json!({ "label": "Name", "value": "Mock Service 1" })
    );
    assert_eq!(
        svcs[0]["cells"][2],
        json!({ "label": "Type", "value": "API service" })
    );
    assert_eq!(svcs[1]["cells"][0], json!({ "label": "#", "value": "2" }));
    assert_eq!(
        svcs[1]["cells"][2],
        json!({ "label": "Type", "value": "agent-to-agent" })
    );
}

#[test]
fn add_feedback_list_cells_walks_list_key() {
    // Live `/feedback-list` shape: entries under `list` (not `items`).
    let mut data = json!({
        "list": [
            { "score": 5.0, "creatorId": 88, "taskId": "0xabc", "createdAt": "2026-04-20",
              "description": "Great" },
        ],
        "total": 1,
    });
    add_feedback_list_cells(&mut data);
    let cells = &data["list"][0]["cells"];
    assert_eq!(cells[0], json!({ "label": "Score", "value": "★ 5" }));
    assert_eq!(cells[1], json!({ "label": "Reviewer", "value": "#88" }));
    assert_eq!(cells[4], json!({ "label": "Comment", "value": "Great" }));
}

// ─── build_precheck (registration §2 uniqueness) ─────────────────────

#[test]
fn precheck_requester_exists_blocks_create() {
    let data = json!({
        "list": [
            { "agentId": 10, "name": "My Buyer", "role": 1, "ownerAddress": "0xSIGNER" },
            { "agentId": 11, "name": "My ASP",   "role": 2, "ownerAddress": "0xSIGNER" },
        ],
    });
    let r = build_precheck(&data, "0xsigner", "requester");
    assert_eq!(r["canCreate"], json!(false));
    assert_eq!(r["uniqueness"], json!("single"));
    assert_eq!(r["existingSameRole"][0]["agentId"], json!("10"));
    assert_eq!(r["existingSameRole"][0]["roleLabel"], json!("User Agent"));
    assert!(r.get("knownAgentIds").is_none());
}

#[test]
fn precheck_requester_absent_allows_create() {
    let data = json!({ "list": [
        { "agentId": 11, "name": "My ASP", "role": 2, "ownerAddress": "0xSIGNER" },
    ]});
    let r = build_precheck(&data, "0xSIGNER", "requester");
    assert_eq!(r["canCreate"], json!(true));
    assert_eq!(r["existingSameRole"].as_array().unwrap().len(), 0);
    assert!(r.get("knownAgentIds").is_none());
}

#[test]
fn precheck_provider_always_creatable_and_counts() {
    let data = json!({ "list": [
        { "agentId": 11, "name": "ASP One", "role": 2, "ownerAddress": "0xSIGNER" },
        { "agentId": 12, "name": "ASP Two", "role": 2, "ownerAddress": "0xSIGNER" },
        { "agentId": 10, "name": "Buyer",   "role": 1, "ownerAddress": "0xSIGNER" },
    ]});
    let r = build_precheck(&data, "0xSIGNER", "provider");
    assert_eq!(r["canCreate"], json!(true));
    assert_eq!(r["uniqueness"], json!("multiple"));
    assert_eq!(r["providerCount"], json!(2));
    assert_eq!(r["existingSameRole"].as_array().unwrap().len(), 2);
}

#[test]
fn precheck_scopes_to_signing_wallet_only() {
    let data = json!({ "list": [
        { "agentId": 50, "name": "Other Eval", "role": 3, "ownerAddress": "0xOTHER" },
    ]});
    let r = build_precheck(&data, "0xSIGNER", "evaluator");
    assert_eq!(r["canCreate"], json!(true));
    assert!(r.get("knownAgentIds").is_none());
}

#[test]
fn precheck_double_layer_and_missing_owner() {
    let data = json!({ "list": [
        { "ownerAddress": "0xSIGNER", "accountName": "main", "agentList": [
            { "agentId": 7, "name": "Eval", "role": 3 },
        ] },
    ]});
    let r = build_precheck(&data, "0xSIGNER", "evaluator");
    assert_eq!(r["canCreate"], json!(false));
    assert_eq!(r["existingSameRole"][0]["agentId"], json!("7"));
}

#[test]
fn collect_owned_counts_for_has_agents_gate() {
    let data = json!({ "list": [
        { "agentId": 1, "name": "A", "role": 1, "ownerAddress": "0xSIGNER" },
    ]});
    assert_eq!(collect_owned_agents(&data, "0xSIGNER").len(), 1);
    assert_eq!(collect_owned_agents(&data, "0xOTHER").len(), 0);
}

// ─── parse_agent_unsigned ─────────────────────────────────────────────

#[test]
fn parse_agent_unsigned_empty_array_is_err() {
    assert!(parse_agent_unsigned(json!([])).is_err());
}

#[test]
fn parse_agent_unsigned_non_array_is_err() {
    // Backend wraps data in an array; a bare object is unexpected.
    assert!(parse_agent_unsigned(json!({ "unsignedTxHash": "0xabc" })).is_err());
    assert!(parse_agent_unsigned(json!(null)).is_err());
}

#[test]
fn parse_agent_unsigned_empty_object_returns_ok_with_defaults() {
    // All fields have `#[serde(default)]` → an empty object deserializes fine.
    let result = parse_agent_unsigned(json!([{}])).expect("empty element should deserialize");
    assert_eq!(result.unsigned_tx_hash, "");
    assert_eq!(result.sign_type, "");
}

#[test]
fn parse_agent_unsigned_extracts_first_element() {
    let result = parse_agent_unsigned(json!([
        { "unsignedTxHash": "0xfirst" },
        { "unsignedTxHash": "0xsecond" },
    ]))
    .expect("first element should parse");
    assert_eq!(result.unsigned_tx_hash, "0xfirst");
}

#[test]
fn parse_agent_unsigned_reads_sign_type_and_extra_data() {
    let result = parse_agent_unsigned(json!([{
        "unsignedTxHash": "0xabc",
        "signType": "ed25519",
        "extraData": { "communicationAddress": "0xaddr" },
    }]))
    .expect("should parse");
    assert_eq!(result.unsigned_tx_hash, "0xabc");
    assert_eq!(result.sign_type, "ed25519");
    assert_eq!(result.extra_data["communicationAddress"], json!("0xaddr"));
}

// ─── reconstruct_get_url_for_log ──────────────────────────────────────

#[test]
fn reconstruct_get_url_no_query_omits_question_mark() {
    let ctx = ctx_no_override();
    let url = reconstruct_get_url_for_log(&ctx, "/api/v1/agents", &[]);
    assert_eq!(url, format!("{DEFAULT_BASE_URL}/api/v1/agents"));
    assert!(!url.contains('?'));
}

#[test]
fn reconstruct_get_url_non_empty_query_appends_pairs() {
    let ctx = ctx_no_override();
    let url = reconstruct_get_url_for_log(
        &ctx,
        "/api/v1/agents",
        &[("chainIndex", "196"), ("page", "1")],
    );
    assert!(url.starts_with(&format!("{DEFAULT_BASE_URL}/api/v1/agents?")));
    assert!(url.contains("chainIndex=196"));
    assert!(url.contains("page=1"));
}

#[test]
fn reconstruct_get_url_filters_empty_values() {
    let ctx = ctx_no_override();
    // Empty-string values are skipped; only non-empty pairs appear.
    let url = reconstruct_get_url_for_log(
        &ctx,
        "/api/v1/agents",
        &[("chainIndex", "196"), ("agentIdList", ""), ("page", "2")],
    );
    assert!(url.contains("chainIndex=196"));
    assert!(url.contains("page=2"));
    assert!(!url.contains("agentIdList"));
}

#[test]
fn reconstruct_get_url_all_empty_values_omits_question_mark() {
    let ctx = ctx_no_override();
    let url = reconstruct_get_url_for_log(
        &ctx,
        "/api/v1/agents",
        &[("chainIndex", ""), ("page", "")],
    );
    assert!(!url.contains('?'));
}

#[test]
fn reconstruct_get_url_respects_base_url_override() {
    let ctx = ctx_with_base("https://pre.example.com");
    let url = reconstruct_get_url_for_log(&ctx, "/api/v1/test", &[("k", "v")]);
    assert!(url.starts_with("https://pre.example.com/api/v1/test?"));
    assert!(url.contains("k=v"));
}

// ─── normalize_role ───────────────────────────────────────────────────

#[test]
fn normalize_role_canonical_strings() {
    assert_eq!(normalize_role("requester").unwrap(), "requester");
    assert_eq!(normalize_role("provider").unwrap(),  "provider");
    assert_eq!(normalize_role("evaluator").unwrap(), "evaluator");
}

#[test]
fn normalize_role_numeric_aliases() {
    assert_eq!(normalize_role("1").unwrap(), "requester");
    assert_eq!(normalize_role("2").unwrap(), "provider");
    assert_eq!(normalize_role("3").unwrap(), "evaluator");
}

#[test]
fn normalize_role_string_aliases() {
    assert_eq!(normalize_role("buyer").unwrap(),     "requester");
    assert_eq!(normalize_role("requestor").unwrap(), "requester");
}

#[test]
fn normalize_role_is_case_insensitive_and_trims_whitespace() {
    assert_eq!(normalize_role("PROVIDER").unwrap(),        "provider");
    assert_eq!(normalize_role("Requester").unwrap(),       "requester");
    assert_eq!(normalize_role("  evaluator  ").unwrap(),   "evaluator");
}

#[test]
fn normalize_role_unknown_input_is_err() {
    assert!(normalize_role("seller").is_err());
    assert!(normalize_role("admin").is_err());
    assert!(normalize_role("4").is_err());
    assert!(normalize_role("").is_err());
}

// ─── require_non_empty ────────────────────────────────────────────────

#[test]
fn require_non_empty_returns_trimmed_value() {
    assert_eq!(require_non_empty(Some("hello"),   "--x").unwrap(), "hello");
    assert_eq!(require_non_empty(Some("  hi  "),  "--x").unwrap(), "hi");
}

#[test]
fn require_non_empty_rejects_blank_and_none() {
    assert!(require_non_empty(Some(""),    "--x").is_err());
    assert!(require_non_empty(Some("   "), "--x").is_err());
    assert!(require_non_empty(None,        "--x").is_err());
}

// ─── trim_or_empty ────────────────────────────────────────────────────

#[test]
fn trim_or_empty_trims_and_handles_none() {
    assert_eq!(trim_or_empty(Some("hello")),   "hello");
    assert_eq!(trim_or_empty(Some("  hi  ")),  "hi");
    assert_eq!(trim_or_empty(Some("")),         "");
    assert_eq!(trim_or_empty(None),             "");
}

// ─── normalize_singleton_object ───────────────────────────────────────

#[test]
fn normalize_singleton_object_unwraps_one_element_array() {
    let arr = json!([{ "key": "val" }]);
    assert_eq!(normalize_singleton_object(arr), json!({ "key": "val" }));
}

#[test]
fn normalize_singleton_object_keeps_multi_element_array() {
    let arr = json!([{ "a": 1 }, { "b": 2 }]);
    let orig = arr.clone();
    assert_eq!(normalize_singleton_object(arr), orig);
}

#[test]
fn normalize_singleton_object_keeps_bare_object() {
    let obj = json!({ "key": "val" });
    let orig = obj.clone();
    assert_eq!(normalize_singleton_object(obj), orig);
}

#[test]
fn normalize_singleton_object_keeps_empty_array() {
    let arr = json!([]);
    let orig = arr.clone();
    assert_eq!(normalize_singleton_object(arr), orig);
}

#[test]
fn normalize_singleton_object_does_not_unwrap_single_non_object() {
    // A one-element array of a non-object (string, number) must NOT be unwrapped.
    let arr = json!(["just a string"]);
    let orig = arr.clone();
    assert_eq!(normalize_singleton_object(arr), orig);

    let arr2 = json!([42]);
    let orig2 = arr2.clone();
    assert_eq!(normalize_singleton_object(arr2), orig2);
}

// ─── parse_services / normalize_service ──────────────────────────────

#[test]
fn parse_services_none_returns_empty_vec() {
    let v = parse_services(None).unwrap();
    assert!(v.is_empty());
}

#[test]
fn parse_services_valid_a2mcp() {
    let raw = r#"[{"name":"TVL Query","servicedescription":"desc","servicetype":"A2MCP","fee":"10","endpoint":"https://x"}]"#;
    let svcs = parse_services(Some(raw)).unwrap();
    assert_eq!(svcs.len(), 1);
    assert_eq!(svcs[0].service_name,              "TVL Query");
    assert_eq!(svcs[0].service_type,              "A2MCP");
    assert_eq!(svcs[0].fee,                       "10");
    assert_eq!(svcs[0].endpoint.as_deref(),       Some("https://x"));
}

#[test]
fn parse_services_valid_a2a_endpoint_cleared() {
    // A2A services must not carry an endpoint (normalize_service clears it).
    let raw = r#"[{"name":"Yield","servicedescription":"yields","servicetype":"A2A","fee":"5","endpoint":"https://should-be-cleared"}]"#;
    let svcs = parse_services(Some(raw)).unwrap();
    assert_eq!(svcs[0].service_type, "A2A");
    assert!(svcs[0].endpoint.is_none(), "A2A endpoint must be cleared");
}

#[test]
fn parse_services_uppercases_servicetype() {
    let raw = r#"[{"name":"S","servicedescription":"d","servicetype":"a2a","fee":"1"}]"#;
    let svcs = parse_services(Some(raw)).unwrap();
    assert_eq!(svcs[0].service_type, "A2A");
}

#[test]
fn parse_services_a2mcp_missing_endpoint_is_err() {
    let raw = r#"[{"name":"S","servicedescription":"desc","servicetype":"A2MCP","fee":"5"}]"#;
    assert!(parse_services(Some(raw)).is_err());
}

#[test]
fn parse_services_unknown_servicetype_is_err() {
    let raw = r#"[{"name":"S","servicedescription":"desc","servicetype":"REST","fee":"5"}]"#;
    assert!(parse_services(Some(raw)).is_err());
}

#[test]
fn parse_services_missing_name_is_err() {
    let raw = r#"[{"servicedescription":"desc","servicetype":"A2A","fee":"1"}]"#;
    assert!(parse_services(Some(raw)).is_err());
}

#[test]
fn parse_services_missing_description_is_err() {
    let raw = r#"[{"name":"S","servicetype":"A2A","fee":"1"}]"#;
    // serde requires `servicedescription` field (no default) → deserialization error.
    assert!(parse_services(Some(raw)).is_err());
}

#[test]
fn parse_services_invalid_json_is_err() {
    assert!(parse_services(Some("{not json}")).is_err());
}

// ─── ensure_provider_has_service ──────────────────────────────────────

fn make_a2a_service() -> AgentService {
    AgentService {
        id: None,
        service_name: "Svc".to_string(),
        service_description: "d".to_string(),
        fee: "1".to_string(),
        service_type: "A2A".to_string(),
        endpoint: None,
    }
}

fn make_card(role: &str, services: Vec<AgentService>) -> AgentCard {
    AgentCard {
        role: role.to_string(),
        name: "X".to_string(),
        profile_picture: "".to_string(),
        profile_description: "".to_string(),
        communication_address: None,
        services,
    }
}

#[test]
fn ensure_provider_has_service_ok_when_services_present() {
    let card = make_card("provider", vec![make_a2a_service()]);
    assert!(ensure_provider_has_service(&card).is_ok());
}

#[test]
fn ensure_provider_has_service_err_when_provider_has_no_services() {
    let card = make_card("provider", vec![]);
    assert!(ensure_provider_has_service(&card).is_err());
}

#[test]
fn ensure_provider_has_service_ok_for_requester_without_services() {
    let card = make_card("requester", vec![]);
    assert!(ensure_provider_has_service(&card).is_ok());
}

#[test]
fn ensure_provider_has_service_ok_for_evaluator_without_services() {
    let card = make_card("evaluator", vec![]);
    assert!(ensure_provider_has_service(&card).is_ok());
}

// ─── parse_u32_arg ────────────────────────────────────────────────────

#[test]
fn parse_u32_arg_none_returns_default() {
    assert_eq!(parse_u32_arg(None, "--x", 5, None, None, false).unwrap(), 5);
}

#[test]
fn parse_u32_arg_parses_valid_integer() {
    assert_eq!(parse_u32_arg(Some("42"), "--x", 0, None, None, false).unwrap(), 42);
    assert_eq!(parse_u32_arg(Some("0"),  "--x", 1, None, None, false).unwrap(), 0);
}

#[test]
fn parse_u32_arg_non_integer_is_err() {
    assert!(parse_u32_arg(Some("abc"),  "--x", 0, None, None, false).is_err());
    assert!(parse_u32_arg(Some("3.14"), "--x", 0, None, None, false).is_err());
    assert!(parse_u32_arg(Some("-1"),   "--x", 0, None, None, false).is_err());
}

#[test]
fn parse_u32_arg_below_min_is_err() {
    assert!(parse_u32_arg(Some("1"), "--x", 0, Some(5), None, false).is_err());
}

#[test]
fn parse_u32_arg_at_boundaries_ok() {
    assert_eq!(parse_u32_arg(Some("5"),  "--x", 0, Some(5), Some(10), false).unwrap(), 5);
    assert_eq!(parse_u32_arg(Some("10"), "--x", 0, Some(5), Some(10), false).unwrap(), 10);
}

#[test]
fn parse_u32_arg_above_max_clamps_when_flag_set() {
    assert_eq!(parse_u32_arg(Some("100"), "--x", 0, None, Some(20), true).unwrap(), 20);
}

#[test]
fn parse_u32_arg_above_max_is_err_without_clamp() {
    assert!(parse_u32_arg(Some("100"), "--x", 0, None, Some(20), false).is_err());
}

// ─── push_optional_query ──────────────────────────────────────────────

#[test]
fn push_optional_query_adds_trimmed_value() {
    let mut q = Vec::new();
    push_optional_query(&mut q, "key", Some("  val  "));
    assert_eq!(q, vec![("key".to_string(), "val".to_string())]);
}

#[test]
fn push_optional_query_skips_none_and_blank() {
    let mut q = Vec::new();
    push_optional_query(&mut q, "k", None);
    push_optional_query(&mut q, "k", Some(""));
    push_optional_query(&mut q, "k", Some("   "));
    assert!(q.is_empty());
}

// ─── push_multi_query ─────────────────────────────────────────────────

#[test]
fn push_multi_query_adds_all_non_blank_trimmed() {
    let mut q = Vec::new();
    push_multi_query(&mut q, "k", &["a".to_string(), "  b  ".to_string(), "c".to_string()]);
    assert_eq!(q, vec![
        ("k".to_string(), "a".to_string()),
        ("k".to_string(), "b".to_string()),
        ("k".to_string(), "c".to_string()),
    ]);
}

#[test]
fn push_multi_query_skips_blank_values() {
    let mut q = Vec::new();
    push_multi_query(&mut q, "k", &["".to_string(), "   ".to_string()]);
    assert!(q.is_empty());
}

// ─── redact_token_for_debug ───────────────────────────────────────────

#[test]
fn redact_token_short_appends_stars() {
    // len <= 16: token + "***"
    assert_eq!(redact_token_for_debug("abc"),              "abc***");
    assert_eq!(redact_token_for_debug("1234567890123456"), "1234567890123456***");
}

#[test]
fn redact_token_long_shows_first8_and_last6() {
    // len > 16: first 8 chars + "***" + last 6 chars
    let token = "abcdefghijklmnopqrstuvwxyz"; // 26 chars
    assert_eq!(redact_token_for_debug(token), "abcdefgh***uvwxyz");
}

#[test]
fn redact_token_exactly_17_chars() {
    // 17 chars: first 8 + *** + last 6 = "abcdefgh***klmnopq"[last6="lmnopq"]
    let token = "abcdefghijklmnopq"; // 17 chars
    assert_eq!(redact_token_for_debug(token), "abcdefgh***lmnopq");
}

// ─── short_address ────────────────────────────────────────────────────

#[test]
fn short_address_standard_40_hex_chars() {
    let addr = "0x30c140554508a515a8da0fe1e2377c4d8eff59d7";
    assert_eq!(short_address(addr).unwrap(), "0x30c1…59d7");
}

#[test]
fn short_address_minimum_8_hex_chars_ok() {
    assert_eq!(short_address("0x12345678").unwrap(), "0x1234…5678");
}

#[test]
fn short_address_7_hex_chars_is_none() {
    assert!(short_address("0x1234567").is_none());
}

#[test]
fn short_address_accepts_uppercase_0x_prefix() {
    let result = short_address("0X30c140554508a515a8da0fe1e2377c4d8eff59d7");
    assert!(result.is_some());
    assert_eq!(result.unwrap(), "0x30c1…59d7");
}

#[test]
fn short_address_non_hex_chars_is_none() {
    assert!(short_address("0xGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG").is_none());
}

#[test]
fn short_address_missing_prefix_is_none() {
    assert!(short_address("30c140554508a515a8da0fe1e2377c4d8eff59d7").is_none());
}

// ─── format_search_rate ───────────────────────────────────────────────

#[test]
fn format_search_rate_trims_trailing_zeros() {
    assert_eq!(format_search_rate(4.6_f64),  "4.6");
    assert_eq!(format_search_rate(5.0_f64),  "5");
    assert_eq!(format_search_rate(4.45_f64), "4.45");
    assert_eq!(format_search_rate(0.0_f64),  "0");
    assert_eq!(format_search_rate(3.5_f64),  "3.5");
    assert_eq!(format_search_rate(1.0_f64),  "1");
}

// ─── format_top_service ───────────────────────────────────────────────

#[test]
fn format_top_service_a2mcp_with_fee_and_token() {
    let svc = json!({
        "serviceName": "TVL Query",
        "serviceType": "A2MCP",
        "feeAmount": 10.0,
        "feeToken": "USDT",
        "endpoint": "https://x",
    });
    let result = format_top_service(&svc).unwrap();
    assert_eq!(result, "TVL Query (API service, 10.0 USDT)");
}

#[test]
fn format_top_service_a2a_no_fee_renders_free() {
    let svc = json!({ "serviceName": "Yield Check", "serviceType": "A2A" });
    let result = format_top_service(&svc).unwrap();
    assert_eq!(result, "Yield Check (agent-to-agent, free)");
}

#[test]
fn format_top_service_no_name_returns_none() {
    let svc = json!({ "serviceType": "A2MCP", "feeAmount": 5.0 });
    assert!(format_top_service(&svc).is_none());
}

#[test]
fn format_top_service_truncates_at_40_chars() {
    // Construct a name long enough that the formatted string exceeds 40 chars.
    let svc = json!({
        "serviceName": "A Very Long Service Name Indeed For Testing",
        "serviceType": "A2A",
    });
    let result = format_top_service(&svc).unwrap();
    let chars: Vec<char> = result.chars().collect();
    // truncate_name pads with '…' when exceeding the limit.
    assert!(chars.len() <= 41, "truncated string should be ≤40 chars + ellipsis");
    // Last char is the ellipsis when truncated.
    assert_eq!(chars.last(), Some(&'…'));
}

// ─── reconstruct_post_url_for_log ─────────────────────────────────────────

#[test]
fn reconstruct_post_url_for_log_uses_default_base_url_when_no_override() {
    let url = reconstruct_post_url_for_log(&ctx_no_override(), "/agent/create");
    assert_eq!(url, format!("{}{}", DEFAULT_BASE_URL, "/agent/create"));
}

#[test]
fn reconstruct_post_url_for_log_uses_override_base_url() {
    let url = reconstruct_post_url_for_log(&ctx_with_base("https://pre.okx.com"), "/agent/create");
    assert_eq!(url, "https://pre.okx.com/agent/create");
}

#[test]
fn reconstruct_post_url_for_log_appends_path_verbatim() {
    let url = reconstruct_post_url_for_log(&ctx_with_base("https://pre.okx.com"), "/agent/sign?foo=bar");
    assert_eq!(url, "https://pre.okx.com/agent/sign?foo=bar");
}

// ─── identity_ws_url ─────────────────────────────────────────────────────

const WS_URL_PROD: &str = "wss://wsdex.okx.com:8443/ws/v5/private";

// Serialize env-var tests to prevent data races under `cargo test`'s default
// multi-threaded runner. Both tests mutate the process-global OKX_AGENTIC_WS_URL
// var, so they must not execute concurrently.
static WS_ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn identity_ws_url_returns_prod_default_when_env_unset() {
    let _lock = WS_ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("OKX_AGENTIC_WS_URL");
    assert_eq!(identity_ws_url(), WS_URL_PROD);
}

#[test]
fn identity_ws_url_returns_override_when_env_set() {
    let _lock = WS_ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("OKX_AGENTIC_WS_URL", "wss://pre-ws.okx.com/ws/v5/private");
    let url = identity_ws_url();
    std::env::remove_var("OKX_AGENTIC_WS_URL");
    assert_eq!(url, "wss://pre-ws.okx.com/ws/v5/private");
}

// ─── build_precheck: reason field ────────────────────────────────────────

#[test]
fn precheck_can_create_false_includes_reason_message() {
    // A requester already exists under the same wallet → canCreate=false.
    let data = json!({ "list": [
        { "agentId": 10, "name": "Existing Buyer", "role": 1, "ownerAddress": "0xADDR" },
    ]});
    let r = build_precheck(&data, "0xADDR", "requester");
    assert_eq!(r["canCreate"], json!(false));
    let reason = r["reason"].as_str().expect("reason must be a string when canCreate=false");
    assert!(
        reason.contains("User Agent"),
        "reason should mention the role label; got: {reason}"
    );
    assert!(
        reason.contains("already registered") || reason.contains("only one"),
        "reason should explain the uniqueness constraint; got: {reason}"
    );
}

#[test]
fn precheck_can_create_true_has_no_reason_field() {
    let data = json!({ "list": [] });
    let r = build_precheck(&data, "0xADDR", "requester");
    assert_eq!(r["canCreate"], json!(true));
    assert!(r.get("reason").is_none(), "reason must be absent when canCreate=true");
}

// ─── normalize_service: A2MCP empty fee ──────────────────────────────────

#[test]
fn normalize_service_a2mcp_empty_fee_is_err() {
    let svc = AgentService {
        id: None,
        service_name: "My Service".to_string(),
        service_description: "desc".to_string(),
        fee: "".to_string(),
        service_type: "A2MCP".to_string(),
        endpoint: Some("https://example.com/mcp".to_string()),
    };
    let result = normalize_service(svc);
    assert!(result.is_err(), "A2MCP with empty fee must be an error");
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("fee"), "error message should mention 'fee'; got: {msg}");
}

#[test]
fn normalize_service_a2mcp_whitespace_only_fee_is_err() {
    let svc = AgentService {
        id: None,
        service_name: "My Service".to_string(),
        service_description: "desc".to_string(),
        fee: "   ".to_string(),
        service_type: "A2MCP".to_string(),
        endpoint: Some("https://example.com/mcp".to_string()),
    };
    let result = normalize_service(svc);
    assert!(result.is_err(), "A2MCP with whitespace-only fee must be an error after trim");
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("fee"), "error message should mention 'fee'; got: {msg}");
}
