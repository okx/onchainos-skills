//! Trader Mode (SA / SD-A) flow primitives.
//!
//! Everything that participates in placing or resuming a limit order
//! beyond plain HTTP transport lives here:
//! - `build_intent` / `sign_intent` — intent plaintext + personal_sign-style
//!   signature (EVM: EIP-191 + keccak256 + ed25519; Solana: ed25519 over hex
//!   of the bytes). Mirrors `commands::agentic_wallet::sign::personal_sign`.
//! - `ActivateCtx` / `activate` — SD-A orchestration (getAttestDocHex →
//!   ed25519 sign → registerTeeInfo). Failure aborts; no auto-retry of
//!   activation itself (tech-design §5.1).
//! - `retry_on_upgrade` — generic 60018 retry wrapper. Currently the
//!   handlers inline the same pattern; `retry_on_upgrade` is kept as a
//!   tested specification of the SD-A retry rule. New call sites should
//!   prefer it.
//! - `format_create_followup` / `format_cancel_followup` — output helpers
//!   that print the post-submit "wait then re-query" line.

use anyhow::{anyhow, bail, Result};
use std::future::Future;
use std::pin::Pin;
use zeroize::Zeroizing;

use crate::client::ApiClient;

use super::api;
use super::status::is_upgrade_required;
use super::types::RegisterTeeInfoReq;

// ── signMsg construction + signing ─────────────────────────────────────

/// Inputs to build a Phase 1 U-pegged limit-order intent message.
///
/// All values are pre-resolved — `build_intent` itself is pure (no clock,
/// no I/O) so its output is deterministic for tests. Field order in the
/// output is **byte-stable** because BE verifies the signature against
/// the exact text.
pub struct BuildIntentArgs<'a> {
    /// `Chain Index` line. BE accepts integer chain id (e.g. 501 for SOL).
    pub chain_id: i64,
    /// `Recipient` line — the SA wallet address (EVM 0x… or SOL base58).
    pub recipient: &'a str,
    /// `From Token` / `To Token` — token contract addresses.
    pub from_token: &'a str,
    pub to_token: &'a str,
    /// `From Amount(precision adjusted)` — raw integer string already
    /// shifted by token decimals (e.g. "10000" for 0.01 USDC at 6 decimals).
    pub from_amount_raw: &'a str,
    /// ISO 8601 with millisecond precision and trailing `Z`
    /// (e.g. "2026-05-06T06:41:47.340Z"). Caller passes both so the
    /// function stays clock-free.
    pub created_at: &'a str,
    pub expired_at: &'a str,
    /// `Timestamp` line — milliseconds since epoch.
    pub timestamp_ms: i64,
}

/// Strategy Type string baked into the signed intent. BE accepts this
/// single value for all 4 P0 strategy types (buy_dip / take_profit /
/// stop_loss / chase_high) — the per-type semantic lives in `strategyType`
/// at the request level, not in the intent text.
pub const STRATEGY_TYPE_NAME_PHASE_1: &str = "LimitOrderUbased";

/// Header that BE expects at the very start of `signMsg`. A blank line
/// follows it before the first key/value field.
const INTENT_HEADER: &str =
    "You will place an order which will be verified and auto-signed by the trusted execution environment.";

/// Build the Phase 1 U-pegged `signMsg` plaintext. Output is byte-stable
/// (no clock, no rand) so callers control exact reproducibility.
///
/// Format (LF-separated, no trailing newline):
/// ```text
/// <header>
///                                  ← blank line (single \n separator)
/// Chain Index: <int>
/// Strategy Type: LimitOrderUbased
/// Recipient: <address>
/// Created At: <ISO 8601 ms Z>
/// Expired At: <ISO 8601 ms Z>
/// From Token: <address>
/// To Token: <address>
/// From Amount(precision adjusted): <raw int>
/// Timestamp: <ms epoch>
/// ```
pub fn build_intent(args: BuildIntentArgs<'_>) -> String {
    format!(
        "{header}\n\nChain Index: {chain_id}\n\
         Strategy Type: {strategy_type}\n\
         Recipient: {recipient}\n\
         Created At: {created_at}\n\
         Expired At: {expired_at}\n\
         From Token: {from_token}\n\
         To Token: {to_token}\n\
         From Amount(precision adjusted): {from_amount}\n\
         Timestamp: {timestamp}",
        header = INTENT_HEADER,
        chain_id = args.chain_id,
        strategy_type = STRATEGY_TYPE_NAME_PHASE_1,
        recipient = args.recipient,
        created_at = args.created_at,
        expired_at = args.expired_at,
        from_token = args.from_token,
        to_token = args.to_token,
        from_amount = args.from_amount_raw,
        timestamp = args.timestamp_ms,
    )
}

/// Shift a human-readable decimal amount (e.g. "0.01") by `decimals`
/// places to produce the raw integer string the BE expects (e.g. "10000"
/// for 6-decimal USDC).
///
/// Pure string manipulation — no `BigDecimal` dependency, supports any
/// precision. Bails on:
/// - non-numeric chars
/// - more than one `.`
/// - fractional digits exceeding `decimals` (would silently lose precision)
pub fn shift_value(amount: &str, decimals: u32) -> Result<String> {
    let trimmed = amount.trim();
    if trimmed.is_empty() {
        bail!("amount is empty");
    }
    if !trimmed.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        bail!("amount must be a positive decimal number, got `{trimmed}`");
    }
    if trimmed.matches('.').count() > 1 {
        bail!("amount has multiple decimal points, got `{trimmed}`");
    }
    let (integer_part, fractional_part) = trimmed.split_once('.').unwrap_or((trimmed, ""));
    if (fractional_part.len() as u32) > decimals {
        bail!(
            "amount `{trimmed}` has {} fractional digit(s), more than the token's {} decimals",
            fractional_part.len(),
            decimals
        );
    }
    let mut buf = String::with_capacity(integer_part.len() + decimals as usize);
    buf.push_str(integer_part);
    buf.push_str(fractional_part);
    for _ in fractional_part.len()..(decimals as usize) {
        buf.push('0');
    }
    let trimmed_zeros = buf.trim_start_matches('0');
    if trimmed_zeros.is_empty() {
        Ok("0".to_string())
    } else {
        Ok(trimmed_zeros.to_string())
    }
}

/// Sign `intent` with the session ed25519 seed using `personal_sign` semantics
/// (mirrors `commands::agentic_wallet::sign::personal_sign`):
///   - Solana (chain == "501"): hex-encode the UTF-8 bytes, then `ed25519_sign_hex`
///     — equivalent to raw ed25519 over the bytes.
///   - EVM (everything else): EIP-191 personal_sign — `\x19Ethereum Signed Message:\n<len>`
///     prefix + keccak256 + ed25519, fed via `ed25519_sign_eip191(_, _, "utf8")`.
///
/// Returns a base64-encoded signature suitable for `verifySignInfo.signature`.
/// `signMsg` is always sent as UTF-8 plaintext; BE picks the verification path
/// from `verifySignInfo.chainId`. The legacy `encoding` field was removed
/// 2026-05-07.
pub fn sign_intent(intent: &str, chain: &str, session_seed_b64: &str) -> Result<String> {
    if super::supported_chains::is_solana(chain) {
        let hex_msg = hex::encode(intent.as_bytes());
        crate::crypto::ed25519_sign_hex(&hex_msg, session_seed_b64)
            .map_err(|e| anyhow!("ed25519 sign failed: {e:#}"))
    } else {
        use base64::Engine;
        let seed_bytes = base64::engine::general_purpose::STANDARD
            .decode(session_seed_b64)
            .map_err(|e| anyhow!("session seed is not valid base64: {e:#}"))?;
        crate::crypto::ed25519_sign_eip191(intent, &seed_bytes, "utf8")
            .map_err(|e| anyhow!("eip191 sign failed: {e:#}"))
    }
}

// ── SD-A activation orchestration ─────────────────────────────────────

/// Inputs `activate` needs from the wallet session. Subcommand collects
/// these once at the top of its handler and passes the struct in.
pub struct ActivateCtx {
    pub account_id: String,
    pub session_cert: String,
    /// Base64-encoded ed25519 seed (the session private key). Wrapped in
    /// `Zeroizing` so cloning a session into `ActivateCtx` does not leave a
    /// stray cleartext copy in memory after the activation finishes.
    pub session_seed_b64: Zeroizing<String>,
    /// How long the activation should remain valid, milliseconds. Caller
    /// chooses; tech-design doesn't pin a default.
    pub expire_ms_from_now: i64,
}

/// Run the SD-A flow once. Prints `Trader Mode activated.` on success.
///
/// Two-step flow per tech-design §5.1:
/// 1. GET getAttestDocHex → returns `attestDocHex` (hex string from SA TEE)
/// 2. ed25519-sign the hex with the session seed → base64 sessionSig
/// 3. POST registerTeeInfo with {accountId, timestamp, expireTimestamp,
///    attestDocHex, sessionCert, sessionSig}
///
/// Failure aborts the calling business operation immediately — no retry
/// of activation itself.
pub async fn activate(client: &mut ApiClient, ctx: &ActivateCtx) -> Result<()> {
    let attest_doc_hex = api::get_attest_doc_hex(client).await?;
    let sig = crate::crypto::ed25519_sign_hex(&attest_doc_hex, &ctx.session_seed_b64)?;

    let now_ms = chrono::Utc::now().timestamp_millis();
    let req = RegisterTeeInfoReq {
        account_id: ctx.account_id.clone(),
        timestamp: now_ms,
        expire_timestamp: now_ms.saturating_add(ctx.expire_ms_from_now),
        attest_doc_hex,
        session_cert: ctx.session_cert.clone(),
        session_sig: sig,
    };
    api::register_tee_info(client, &req).await?;
    println!("Trader Mode activated.");
    Ok(())
}

// ── Generic 60018 retry wrapper ───────────────────────────────────────

/// Boxed-future closure type. Callers wrap their async expression in
/// `Box::pin(async move { ... })`. The closure must be re-callable (`Fn`)
/// because we may invoke it twice on UpgradeRequired.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Run `op`. If it returns `UpgradeRequired`, run `activate_fn`, then run
/// `op` once more.
///
/// Why pinned-Box closures: async-fn-in-trait is unstable for our toolchain
/// version, and writing a generic over `Fut` requires `Send`/lifetime
/// juggling that adds noise at every call site. `Box::pin(async ...)` keeps
/// call sites readable; the allocation cost is negligible for an HTTP-bound op.
pub async fn retry_on_upgrade<T, OpF, ActF>(op: OpF, activate_fn: ActF) -> Result<T>
where
    OpF: Fn() -> BoxFuture<'static, Result<T>>,
    ActF: FnOnce() -> BoxFuture<'static, Result<()>>,
{
    match op().await {
        Err(e) if is_upgrade_required(&e) => {
            activate_fn().await?;
            op().await
        }
        other => other,
    }
}

// ── Output formatters (post-submit "wait then re-query") ──────────────

/// Format a "wait then re-query" hint following the create-limit response.
///
/// Returns a multi-line string ready for direct print. The first line is
/// always emitted; the second (`After ~Ns ...`) is suppressed when
/// `wait_secs == 0` (Solana case).
pub fn format_create_followup(order_id: &str, status: &str, wait_secs: i64) -> String {
    let head = format!(
        "Order created (id={order_id}). status={status}. estimatedWaitTime={wait_secs}s."
    );
    if wait_secs <= 0 {
        head
    } else {
        format!(
            "{head}\nAfter ~{wait_secs}s, run: onchainos strategy list --order-id {order_id}"
        )
    }
}

/// Same shape, but for `cancel` — only emits a wait hint when the BE
/// returns one. tech-design §4.2 says cancel may not always include
/// `estimatedWaitTime`; pass `None` in that case. `updated` is the
/// BE-reported count (`updateNum`), kept as `i64` to match the wire type;
/// negative values would never appear in practice but are surfaced
/// faithfully rather than wrapping silently.
pub fn format_cancel_followup(updated: i64, wait_secs: Option<i64>) -> String {
    let head = format!("Cancelled {updated} order(s).");
    match wait_secs {
        Some(n) if n > 0 => format!(
            "{head} estimatedWaitTime={n}s. Re-query with `strategy list` after the wait."
        ),
        _ => format!(
            "{head} Re-query with `strategy list` after a few seconds; trading orders may finalise as `completed`."
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::status::{check_response, StrategyApiError};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn sample_intent_args<'a>() -> BuildIntentArgs<'a> {
        BuildIntentArgs {
            chain_id: 501,
            recipient: "5HVKBVReErFGgAUgcHMKc2vCRr8Q2WMZXcvKWJwsLern",
            from_token: "11111111111111111111111111111111",
            to_token: "4NBTf8PfLH4oLFnwf3knv46FY9i5oXjDxffCetXRpump",
            from_amount_raw: "12000000",
            created_at: "2026-05-06T06:41:47.340Z",
            expired_at: "2026-05-13T06:41:47.340Z",
            timestamp_ms: 1778654507340,
        }
    }

    // ── build_intent ─────────────────────────────────────────────

    /// Output must be byte-stable: any drift breaks BE signature verification.
    #[test]
    fn build_intent_matches_phase1_template() {
        let s = build_intent(sample_intent_args());
        let expected = "You will place an order which will be verified and auto-signed by the trusted execution environment.\n\
            \n\
            Chain Index: 501\n\
            Strategy Type: LimitOrderUbased\n\
            Recipient: 5HVKBVReErFGgAUgcHMKc2vCRr8Q2WMZXcvKWJwsLern\n\
            Created At: 2026-05-06T06:41:47.340Z\n\
            Expired At: 2026-05-13T06:41:47.340Z\n\
            From Token: 11111111111111111111111111111111\n\
            To Token: 4NBTf8PfLH4oLFnwf3knv46FY9i5oXjDxffCetXRpump\n\
            From Amount(precision adjusted): 12000000\n\
            Timestamp: 1778654507340";
        assert_eq!(s, expected);
    }

    /// Same input → byte-identical output (no clock dep, no nondet).
    #[test]
    fn build_intent_is_deterministic() {
        let a = build_intent(sample_intent_args());
        let b = build_intent(sample_intent_args());
        assert_eq!(a, b);
    }

    /// Header line is fixed English prose followed by a blank line.
    #[test]
    fn build_intent_header_is_fixed() {
        let s = build_intent(sample_intent_args());
        let lines: Vec<&str> = s.split('\n').collect();
        assert_eq!(
            lines[0],
            "You will place an order which will be verified and auto-signed by the trusted execution environment."
        );
        assert_eq!(lines[1], "", "second line must be empty (blank line after header)");
        assert!(lines[2].starts_with("Chain Index:"));
    }

    // ── shift_value ──────────────────────────────────────────────

    #[test]
    fn shift_value_typical_cases() {
        assert_eq!(shift_value("0.01", 6).unwrap(), "10000");        // 0.01 USDC → 10000
        assert_eq!(shift_value("1", 18).unwrap(), "1000000000000000000"); // 1 ETH
        assert_eq!(shift_value("1.5", 18).unwrap(), "1500000000000000000");
        assert_eq!(shift_value("100", 6).unwrap(), "100000000");
        assert_eq!(shift_value("12", 6).unwrap(), "12000000");        // 12 USDC → 12000000
        assert_eq!(shift_value("0.000001", 6).unwrap(), "1");        // smallest USDC unit
        assert_eq!(shift_value("0", 6).unwrap(), "0");
        assert_eq!(shift_value(".5", 6).unwrap(), "500000");          // ".5" = "0.5"
    }

    #[test]
    fn shift_value_rejects_excess_precision() {
        let err = shift_value("0.0000001", 6).unwrap_err();
        let s = format!("{err:#}");
        assert!(s.contains("more than"), "want precision-overflow err, got: {s}");
    }

    #[test]
    fn shift_value_rejects_invalid_input() {
        assert!(shift_value("", 6).is_err());
        assert!(shift_value("abc", 6).is_err());
        assert!(shift_value("1.2.3", 6).is_err());
        assert!(shift_value("-1", 6).is_err()); // negative not supported
        assert!(shift_value("1e6", 6).is_err()); // scientific not supported
    }

    // ── sign_intent ──────────────────────────────────────────────

    /// Solana path (chain "501") returns valid base64.
    #[test]
    fn sign_intent_solana_returns_base64() {
        use base64::Engine;
        let seed_b64 = base64::engine::general_purpose::STANDARD.encode(vec![7u8; 32]);
        let intent = build_intent(sample_intent_args());
        let sig = sign_intent(&intent, "501", &seed_b64).unwrap();
        assert!(!sig.is_empty());
        assert!(
            base64::engine::general_purpose::STANDARD.decode(&sig).is_ok(),
            "signature must be valid base64: {sig}"
        );
    }

    /// EVM path (chain != "501") goes through EIP-191 + keccak256 + ed25519.
    /// Result is also base64.
    #[test]
    fn sign_intent_evm_returns_base64() {
        use base64::Engine;
        let seed_b64 = base64::engine::general_purpose::STANDARD.encode(vec![7u8; 32]);
        let intent = build_intent(sample_intent_args());
        let sig = sign_intent(&intent, "1", &seed_b64).unwrap();
        assert!(!sig.is_empty());
        assert!(
            base64::engine::general_purpose::STANDARD.decode(&sig).is_ok(),
            "signature must be valid base64: {sig}"
        );
    }

    /// EVM and Solana paths produce different signatures for the same intent —
    /// EVM applies EIP-191 prefix + keccak256, Solana signs raw bytes.
    #[test]
    fn sign_intent_evm_and_solana_diverge() {
        use base64::Engine;
        let seed_b64 = base64::engine::general_purpose::STANDARD.encode(vec![5u8; 32]);
        let intent = build_intent(sample_intent_args());
        let evm = sign_intent(&intent, "1", &seed_b64).unwrap();
        let sol = sign_intent(&intent, "501", &seed_b64).unwrap();
        assert_ne!(evm, sol, "EIP-191 path must produce a different signature than raw-bytes path");
    }

    /// Same intent + seed + chain → byte-identical signature (ed25519 is deterministic).
    #[test]
    fn sign_intent_is_deterministic() {
        use base64::Engine;
        let seed_b64 = base64::engine::general_purpose::STANDARD.encode(vec![3u8; 32]);
        let intent = build_intent(sample_intent_args());
        let a = sign_intent(&intent, "501", &seed_b64).unwrap();
        let b = sign_intent(&intent, "501", &seed_b64).unwrap();
        assert_eq!(a, b);
    }

    // ── retry_on_upgrade ─────────────────────────────────────────

    fn upgrade_required_error() -> anyhow::Error {
        let v = serde_json::json!({"code": 60018, "msg": "upgrade required"});
        check_response(&v).unwrap_err()
    }

    fn other_be_error() -> anyhow::Error {
        let v = serde_json::json!({"code": 60002, "msg": "no order"});
        check_response(&v).unwrap_err()
    }

    /// Op succeeds first try → activate is NOT called, op called exactly once.
    #[tokio::test]
    async fn happy_path_calls_op_once_only() {
        let op_calls = Arc::new(AtomicUsize::new(0));
        let activate_calls = Arc::new(AtomicUsize::new(0));
        let op_calls_c = op_calls.clone();
        let activate_calls_c = activate_calls.clone();

        let result: Result<i32> = retry_on_upgrade(
            move || {
                let c = op_calls_c.clone();
                Box::pin(async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Ok(42)
                })
            },
            move || {
                let c = activate_calls_c.clone();
                Box::pin(async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
            },
        )
        .await;

        assert_eq!(result.unwrap(), 42);
        assert_eq!(op_calls.load(Ordering::SeqCst), 1);
        assert_eq!(activate_calls.load(Ordering::SeqCst), 0);
    }

    /// First call returns UpgradeRequired → activate called → second op succeeds.
    #[tokio::test]
    async fn upgrade_path_activates_and_retries_once() {
        let op_calls = Arc::new(AtomicUsize::new(0));
        let activate_calls = Arc::new(AtomicUsize::new(0));
        let op_calls_c = op_calls.clone();
        let activate_calls_c = activate_calls.clone();

        let result: Result<i32> = retry_on_upgrade(
            move || {
                let c = op_calls_c.clone();
                Box::pin(async move {
                    let n = c.fetch_add(1, Ordering::SeqCst);
                    if n == 0 {
                        Err(upgrade_required_error())
                    } else {
                        Ok(99)
                    }
                })
            },
            move || {
                let c = activate_calls_c.clone();
                Box::pin(async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
            },
        )
        .await;

        assert_eq!(result.unwrap(), 99);
        assert_eq!(op_calls.load(Ordering::SeqCst), 2, "op must run twice");
        assert_eq!(activate_calls.load(Ordering::SeqCst), 1);
    }

    /// Non-UpgradeRequired errors bubble up immediately — no activation,
    /// no retry. tech-design §5.3 hard rule.
    #[tokio::test]
    async fn other_errors_do_not_trigger_activation() {
        let op_calls = Arc::new(AtomicUsize::new(0));
        let activate_calls = Arc::new(AtomicUsize::new(0));
        let op_calls_c = op_calls.clone();
        let activate_calls_c = activate_calls.clone();

        let result: Result<i32> = retry_on_upgrade(
            move || {
                let c = op_calls_c.clone();
                Box::pin(async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Err(other_be_error())
                })
            },
            move || {
                let c = activate_calls_c.clone();
                Box::pin(async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
            },
        )
        .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        let s = err.downcast_ref::<StrategyApiError>().expect("typed err");
        assert_eq!(s.code, 60002);
        assert_eq!(op_calls.load(Ordering::SeqCst), 1, "must not retry");
        assert_eq!(activate_calls.load(Ordering::SeqCst), 0);
    }

    /// Activation failure during retry path → original op did run once, but
    /// the wrapper bails on the activation error before the second op call.
    #[tokio::test]
    async fn activation_failure_bails_before_retry() {
        let op_calls = Arc::new(AtomicUsize::new(0));
        let op_calls_c = op_calls.clone();

        let result: Result<i32> = retry_on_upgrade(
            move || {
                let c = op_calls_c.clone();
                Box::pin(async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Err(upgrade_required_error())
                })
            },
            || Box::pin(async { Err(anyhow::anyhow!("attest server unreachable")) }),
        )
        .await;

        assert!(result.is_err());
        assert_eq!(op_calls.load(Ordering::SeqCst), 1, "second op must NOT run");
    }

    // ── wait_time formatters ─────────────────────────────────────

    #[test]
    fn solana_zero_wait_omits_followup_line() {
        let out = format_create_followup("ord-1", "creating", 0);
        assert!(out.contains("Order created"));
        assert!(out.contains("estimatedWaitTime=0s"));
        assert!(
            !out.contains("After ~"),
            "SOL=0 must NOT print wait line: {out}"
        );
    }

    #[test]
    fn nonzero_wait_emits_followup_line() {
        let out = format_create_followup("ord-2", "creating", 12);
        assert!(out.contains("estimatedWaitTime=12s"));
        assert!(out.contains("After ~12s"));
        assert!(out.contains("strategy list --order-id ord-2"));
    }

    #[test]
    fn negative_wait_treated_as_zero() {
        let out = format_create_followup("ord-3", "creating", -3);
        assert!(!out.contains("After ~"));
    }

    #[test]
    fn cancel_with_wait() {
        let out = format_cancel_followup(2_i64, Some(12));
        assert!(out.contains("Cancelled 2 order(s)"));
        assert!(out.contains("12s"));
    }

    #[test]
    fn cancel_without_wait_falls_back_to_generic_hint() {
        let out = format_cancel_followup(1_i64, None);
        assert!(out.contains("Cancelled 1 order(s)"));
        assert!(
            out.to_lowercase().contains("re-query"),
            "expected fallback hint, got: {out}"
        );
    }
}
