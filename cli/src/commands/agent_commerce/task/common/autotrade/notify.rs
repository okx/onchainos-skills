//! Auto copy-trade outcome self-report (`swap execute --notify-job-id <jobId>`).
//!
//! Fix for the "DEX auto-execution result never reached the user" bug: the
//! execution card used to *instruct* the calling agent to run
//! `onchainos agent user-notify` after the trade — a separate LLM step that
//! one-shot sub sessions were observed to drop (0/6 DEX failures notified).
//! With `--notify-job-id` the executing CLI pushes the outcome itself, success
//! AND failure, so delivery no longer depends on the agent remembering a
//! second command (Lark doc Hcw8dDoQ… improvement #1: deterministic mechanical
//! steps).
//!
//! Content is single-language, selected by the per-job language marker
//! (`user_lang::resolve` — fed deterministically from the user's verbatim
//! decision replies). No LLM sits between the CLI and the user here, so the
//! copy ships pre-rendered in en/zh variants; an execution always follows an
//! A/B consent reply, so the marker is in place by the time it is needed.
//! Notify failures are non-fatal: the trade result must never be masked by a
//! reporting error.

use serde_json::Value;

use super::super::user_lang::{self, Lang};

/// Identity of the trade being reported — everything the executing command
/// already knows from its own arguments.
pub(crate) struct TradeRef<'a> {
    pub job_id: &'a str,
    /// Chain name as passed on the command line (e.g. `base`).
    pub chain: &'a str,
    /// Human-readable from-side amount (`--readable-amount`, or raw `--amount`).
    pub amount: &'a str,
    /// From/to display names: token symbols when the swap response carries
    /// them, else the (shortened) command-line token arguments.
    pub from: &'a str,
    pub to: &'a str,
}

/// Push the swap outcome to the task user. Called by `swap execute` when
/// `--notify-job-id` is present; infallible by design (a lost notification is
/// logged, never turned into a command failure — and never masks the real
/// swap error already being propagated).
pub(crate) fn notify_swap_outcome(
    job_id: &str,
    chain: &str,
    display_amount: &str,
    from_arg: &str,
    to_arg: &str,
    outcome: Result<&Value, &anyhow::Error>,
) {
    let lang = user_lang::resolve(job_id);
    let from_short = short_id(from_arg);
    let to_short = short_id(to_arg);
    let msg = match outcome {
        Ok(out) => {
            let t = TradeRef {
                job_id,
                chain,
                amount: display_amount,
                from: token_symbol(&out["fromToken"]).unwrap_or(&from_short),
                to: token_symbol(&out["toToken"]).unwrap_or(&to_short),
            };
            let tx = out["swapTxHash"].as_str().unwrap_or("");
            let order = out["swapOrderId"].as_str().unwrap_or("");
            // Consent read at execution time so the wording/pause hint reflect the
            // CURRENT consent, not the card-emission-time snapshot.
            let consent = super::consent::load_consent(job_id).ok().flatten();
            let auto_mode = consent
                .as_ref()
                .map(|c| matches!(c.mode, super::consent::ConsentMode::Auto))
                .unwrap_or(false);
            // The "within your cap" line only for a trade the cap actually gated:
            // Auto mode AND a buy (dex buys spend a quote-stablecoin alias —
            // usdt by default, usdc by preference/fallback; sells are never
            // cap-gated — claiming a 500-dollar sell was "within your 50 limit"
            // would be false). The line carries the ACTUAL paid stablecoin's
            // symbol so it matches the swap line above it (PRD copy: USDT).
            let from_is_quote =
                super::consent::QUOTE_WHITELIST.contains(&from_arg.to_ascii_lowercase().as_str());
            let cap = if auto_mode && from_is_quote {
                consent
                    .and_then(|c| c.cap_u)
                    .map(|c| format!("{c} {}", from_arg.to_ascii_uppercase()))
            } else {
                None
            };
            success_message(&t, tx, order, cap.as_deref(), auto_mode, lang)
        }
        Err(e) => {
            let t = TradeRef {
                job_id,
                chain,
                amount: display_amount,
                from: &from_short,
                to: &to_short,
            };
            failure_message(&t, &flatten_reason(&format!("{e:#}")), lang)
        }
    };
    if let Err(e) = super::super::okx_a2a::user_notify(&msg, None, false) {
        eprintln!("[autotrade] outcome notification failed (non-fatal): {e}");
    }
}

/// Success notice (PRD OX2Hd Step 3① equivalent, CLI-filled), single-language.
/// `auto_mode` picks the wording: an Auto-consent execution says "auto-executed";
/// a manual one-shot (option B) must NOT — the user explicitly chose manual, and
/// "auto-executed" would read as the setting they declined having turned itself on.
/// The cap line + pause hint appear only when an Auto cap actually gated the trade.
fn success_message(
    t: &TradeRef,
    tx_hash: &str,
    order_id: &str,
    cap: Option<&str>,
    auto_mode: bool,
    lang: Lang,
) -> String {
    let job = short_id(t.job_id);
    // Gas Station broadcasts can return an orderId before the on-chain hash exists.
    let result_part = match lang {
        Lang::En => {
            if !tx_hash.is_empty() {
                format!("Tx: {tx_hash}")
            } else if !order_id.is_empty() {
                format!("Order: {order_id}")
            } else {
                "submitted — check wallet history for the tx id".to_string()
            }
        }
        Lang::Zh => {
            if !tx_hash.is_empty() {
                format!("交易哈希: {tx_hash}")
            } else if !order_id.is_empty() {
                format!("订单号: {order_id}")
            } else {
                "已提交,交易 ID 可稍后在交易历史中查询".to_string()
            }
        }
    };
    let executed_part = match (lang, auto_mode) {
        (Lang::En, true) => "dex signal auto-executed",
        (Lang::En, false) => "dex signal executed (per your confirmation)",
        (Lang::Zh, true) => "dex 信号已自动执行",
        (Lang::Zh, false) => "dex 信号已执行(经你确认)",
    };
    let mut s = match lang {
        Lang::En => format!(
            "[Auto Copy-Trade] Job {job}: {executed_part} — swap {amount} {from} → {to} on {chain}. {result_part}",
            amount = t.amount,
            from = t.from,
            to = t.to,
            chain = t.chain,
        ),
        Lang::Zh => format!(
            "[自动跟单] 任务 {job}:{executed_part} — {chain} 链 swap {amount} {from} → {to}。{result_part}",
            amount = t.amount,
            from = t.from,
            to = t.to,
            chain = t.chain,
        ),
    };
    // `cap` arrives pre-formatted with its stablecoin symbol ("50 USDT") so the
    // limit line names the actual paid currency (PRD copy denominates in USDT).
    if let Some(cap) = cap {
        s.push_str(&match lang {
            Lang::En => format!(
                "\nWithin your {cap} per-trade auto limit. Reply \"Pause auto copy-trading\" to turn it off anytime."
            ),
            Lang::Zh => format!("\n本次在你的每笔 {cap} 自动限额内。回复「暂停自动跟单」可随时关闭。"),
        });
    }
    s
}

/// Failure notice, single-language: the reason + "manual redo possible, no auto-retry".
fn failure_message(t: &TradeRef, reason: &str, lang: Lang) -> String {
    let job = short_id(t.job_id);
    match lang {
        Lang::En => format!(
            "[Auto Copy-Trade] Job {job}: auto-execution FAILED — swap {amount} {from} → {to} on {chain} did not complete. Reason: {reason}\n\
             You can run this trade manually; auto copy-trade will not retry it.",
            amount = t.amount,
            from = t.from,
            to = t.to,
            chain = t.chain,
        ),
        Lang::Zh => format!(
            "[自动跟单] 任务 {job}:自动执行失败 — {chain} 链 swap {amount} {from} → {to} 未完成。原因: {reason}\n\
             可手动补做,系统不会自动重试。",
            amount = t.amount,
            from = t.from,
            to = t.to,
            chain = t.chain,
        ),
    }
}

/// Degrade notice (a signal arrived but was not executed): CLI-delivered so the
/// reason always reaches the user — the follow-up `user-notify` step used to be
/// left to the consuming agent and was skippable (and invisible from headless
/// sessions). On success the payload is marked `notification_pushed` and its
/// fill-in template dropped, so the agent doesn't notify a second time; on push
/// failure the payload stays intact as the agent-side fallback.
pub(crate) fn push_degrade_notice(n: &mut super::card::NotifyOnly, job_id: &str) {
    // Latch dedup (a duplicate of an ALREADY-EXECUTED signal) is absorbed
    // silently — pushing "not executed, redo manually" right after the success
    // notice would contradict it and invite a manual double-trade.
    if n.reason == super::DegradeReason::ReplaySkip.as_str() {
        n.notification_pushed = true; // handled: deliberately suppressed
        n.notification_template = String::new();
        n.guidance = "Duplicate of an already-executed signal — deliberately absorbed. \
                      Do NOT notify the user; just end the turn."
            .to_string();
        return;
    }
    let lang = user_lang::resolve(job_id);
    let msg = degrade_message(job_id, &n.reason, lang);
    match super::super::okx_a2a::user_notify(&msg, None, false) {
        Ok(()) => {
            n.notification_pushed = true;
            n.notification_template = String::new();
            // In-band instruction so even a resumed session holding OLD playbook
            // text (which says "notify the user with reason+template") won't double-send.
            n.guidance = "The CLI already delivered this degrade notice to the user — \
                          do NOT run `onchainos agent user-notify` again; just end the turn."
                .to_string();
        }
        Err(e) => {
            eprintln!(
                "[autotrade] degrade notification failed (non-fatal, agent fallback keeps the template): {e}"
            );
            n.guidance = "CLI push failed — deliver this degrade notice yourself: fill \
                          `notificationTemplate` with `reason` (localized) and push it via \
                          `onchainos agent user-notify`."
                .to_string();
        }
    }
}

/// Single-language degrade notice. `reason` is the stable machine code
/// (freshness_expired / subscription_inactive / …) embedded verbatim.
fn degrade_message(job_id: &str, reason: &str, lang: Lang) -> String {
    let job = short_id(job_id);
    if reason == super::DegradeReason::MultipleTakeProfitUnsupported.as_str() {
        return match lang {
            Lang::En => format!(
                "[Auto Copy-Trade] The provider's signal for job {job} contains multiple take-profit levels. The current version supports only one, so this trade was not executed. The deliverable is saved for manual review."
            ),
            Lang::Zh => format!(
                "[自动跟单] 任务 {job}:服务商信号包含多个止盈目标,当前版本仅支持单个止盈目标,因此本次未自动执行。交付物已保存,可手动查看处理。"
            ),
        };
    }
    match lang {
        Lang::En => format!(
            "[Auto Copy-Trade] The provider's signal for job {job} was not executed ({reason}). \
             The deliverable is saved for manual review."
        ),
        Lang::Zh => format!(
            "[自动跟单] 任务 {job}:服务商信号未执行(原因: {reason})。交付物已保存,可手动查看处理。"
        ),
    }
}

/// `tokenSymbol` from a swap-response token object, when non-empty.
fn token_symbol(token: &Value) -> Option<&str> {
    token["tokenSymbol"].as_str().filter(|s| !s.is_empty())
}

/// Shorten long hex-ish identifiers (job ids, token addresses) to `0xabcd…1234`;
/// short values (symbols, aliases like `usdc`) pass through unchanged. Char-based
/// (not byte-sliced) so an unexpected non-ASCII value can never panic a
/// notification helper after a trade already succeeded.
fn short_id(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() > 16 {
        let head: String = chars[..6].iter().collect();
        let tail: String = chars[chars.len() - 4..].iter().collect();
        format!("{head}…{tail}")
    } else {
        s.to_string()
    }
}

/// One-line, bounded failure reason: anyhow chains can be multi-line and long.
fn flatten_reason(raw: &str) -> String {
    let mut one_line = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX: usize = 300;
    if one_line.chars().count() > MAX {
        one_line = one_line.chars().take(MAX).collect::<String>() + "…";
    }
    one_line
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t<'a>() -> TradeRef<'a> {
        TradeRef {
            job_id: "0xb5b8b2b800000000000000000000000000000000000000000000000000009b35",
            chain: "base",
            amount: "25",
            from: "USDC",
            to: "PEPE",
        }
    }

    #[test]
    fn success_en_is_single_language_with_tx_and_cap_pause_line() {
        let msg = success_message(&t(), "0xTxHash", "", Some("50 USDT"), true, Lang::En);
        assert!(
            msg.contains("[Auto Copy-Trade] Job 0xb5b8…9b35"),
            "got: {msg}"
        );
        assert!(msg.contains("swap 25 USDC → PEPE on base"));
        assert!(msg.contains("Tx: 0xTxHash"));
        assert!(msg.contains("50 USDT per-trade auto limit"));
        assert!(msg.contains("Pause auto copy-trading"));
        // Single language: no Chinese block in the English notice.
        assert!(!msg.contains("自动跟单"), "got: {msg}");
    }

    #[test]
    fn success_zh_is_single_language_with_tx_and_cap_pause_line() {
        let msg = success_message(&t(), "0xTxHash", "", Some("50 USDT"), true, Lang::Zh);
        assert!(msg.contains("[自动跟单] 任务 0xb5b8…9b35"), "got: {msg}");
        assert!(msg.contains("交易哈希: 0xTxHash"));
        assert!(msg.contains("每笔 50 USDT 自动限额"));
        assert!(msg.contains("暂停自动跟单"));
        assert!(!msg.contains("[Auto Copy-Trade]"), "got: {msg}");
    }

    #[test]
    fn manual_one_shot_success_does_not_claim_auto_execution() {
        let en = success_message(&t(), "0xTxHash", "", None, false, Lang::En);
        assert!(en.contains("per your confirmation"), "got: {en}");
        assert!(!en.contains("auto-executed"));
        let zh = success_message(&t(), "0xTxHash", "", None, false, Lang::Zh);
        assert!(zh.contains("经你确认"), "got: {zh}");
        assert!(!zh.contains("已自动执行"));
    }

    #[test]
    fn replay_skip_degrade_is_absorbed_silently() {
        // Latch dedup must NOT tell the user "not executed" after a successful
        // execution — it is marked handled without any push.
        let mut n = super::super::card::make_notify_only("/tmp/x", "replay_skip");
        push_degrade_notice(&mut n, "job1");
        assert!(n.notification_pushed);
        assert!(n.notification_template.is_empty());
        assert!(n.guidance.contains("Do NOT notify"), "got: {}", n.guidance);
    }

    #[test]
    fn success_without_cap_has_no_pause_line() {
        let en = success_message(&t(), "0xTxHash", "", None, true, Lang::En);
        assert!(!en.contains("auto limit"));
        let zh = success_message(&t(), "0xTxHash", "", None, true, Lang::Zh);
        assert!(!zh.contains("暂停自动跟单"));
    }

    #[test]
    fn success_falls_back_to_order_id_then_pending() {
        let by_order = success_message(&t(), "", "ord-1", None, true, Lang::En);
        assert!(by_order.contains("Order: ord-1"));
        let pending_zh = success_message(&t(), "", "", None, true, Lang::Zh);
        assert!(pending_zh.contains("交易历史"));
    }

    #[test]
    fn failure_carries_reason_and_manual_hint_per_language() {
        let en = failure_message(&t(), "insufficient balance", Lang::En);
        assert!(en.contains("FAILED"));
        assert!(en.contains("Reason: insufficient balance"));
        assert!(en.contains("will not retry"));
        assert!(!en.contains("自动执行失败"));
        let zh = failure_message(&t(), "insufficient balance", Lang::Zh);
        assert!(zh.contains("自动执行失败"));
        assert!(zh.contains("原因: insufficient balance"));
        assert!(zh.contains("不会自动重试"));
        assert!(!zh.contains("[Auto Copy-Trade]"));
    }

    #[test]
    fn degrade_message_is_single_language_with_reason() {
        let en = degrade_message(
            "0xb5b8b2b800000000000000000000000000000000000000000000000000009b35",
            "freshness_expired",
            Lang::En,
        );
        assert!(
            en.contains("job 0xb5b8…9b35") && en.contains("(freshness_expired)"),
            "got: {en}"
        );
        assert!(!en.contains("自动跟单"));
        let zh = degrade_message(
            "0xb5b8b2b800000000000000000000000000000000000000000000000000009b35",
            "freshness_expired",
            Lang::Zh,
        );
        assert!(zh.contains("[自动跟单]") && zh.contains("freshness_expired"));
        assert!(!zh.contains("[Auto Copy-Trade]"));
    }

    #[test]
    fn multiple_take_profit_notice_explains_the_product_limit() {
        let en = degrade_message(
            "job1",
            super::super::DegradeReason::MultipleTakeProfitUnsupported.as_str(),
            Lang::En,
        );
        assert!(en.contains("multiple take-profit levels"));
        assert!(en.contains("supports only one"));
        let zh = degrade_message(
            "job1",
            super::super::DegradeReason::MultipleTakeProfitUnsupported.as_str(),
            Lang::Zh,
        );
        assert!(zh.contains("多个止盈目标"));
        assert!(zh.contains("仅支持单个止盈目标"));
    }

    #[test]
    fn flatten_reason_single_lines_and_truncates() {
        let flat = flatten_reason("line one\n  line two");
        assert_eq!(flat, "line one line two");
        let long = "x".repeat(400);
        let cut = flatten_reason(&long);
        assert_eq!(cut.chars().count(), 301); // 300 + ellipsis
        assert!(cut.ends_with('…'));
    }

    #[test]
    fn short_id_shortens_only_long_values() {
        assert_eq!(short_id("usdc"), "usdc");
        assert_eq!(
            short_id("0xb5b8b2b800000000000000000000000000000000000000000000000000009b35"),
            "0xb5b8…9b35"
        );
    }
}
