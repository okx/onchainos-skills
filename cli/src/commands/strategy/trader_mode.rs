//! Trader Mode (SA / SD-A) primitives: intent build/sign, SD-A activation,
//! and the 60018 retry-once spec.

use anyhow::{anyhow, bail, Result};
use std::future::Future;
use std::pin::Pin;
use zeroize::Zeroizing;

use crate::client::ApiClient;

use super::api;
use super::status::is_upgrade_required;
use super::types::RegisterTeeInfoReq;

// ── signMsg build + sign ──

/// Pre-resolved inputs. `build_intent` is pure (no clock/IO) → byte-stable
/// output, which BE verifies the signature against.
pub struct BuildIntentArgs<'a> {
    pub chain_id: i64,
    /// SA wallet address (EVM 0x… or SOL base58).
    pub recipient: &'a str,
    pub from_token: &'a str,
    pub to_token: &'a str,
    /// Raw integer string (`amount * 10^decimals`).
    pub from_amount_raw: &'a str,
    /// ISO 8601 ms with trailing `Z` (e.g. `"2026-05-06T06:41:47.340Z"`).
    pub created_at: &'a str,
    pub expired_at: &'a str,
    pub timestamp_ms: i64,
}

/// Phase 1 BE accepts this single name for all 4 strategy types.
pub const STRATEGY_TYPE_NAME_PHASE_1: &str = "LimitOrderUbased";

const INTENT_HEADER: &str =
    "You will place an order which will be verified and auto-signed by the trusted execution environment.";

/// Byte-stable `signMsg` plaintext (LF-separated, no trailing newline):
///
/// ```text
/// <header>
///
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

/// Human decimal → raw integer string (`"0.01"` + 6 → `"10000"`). Bails on
/// non-numeric, multiple dots, or fractional digits beyond `decimals`.
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

/// personal_sign semantics — Solana: ed25519 over hex bytes; EVM: EIP-191 +
/// keccak256 + ed25519. Returns base64 for `verifySignInfo.signature`.
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

// ── SD-A activation ──

pub struct ActivateCtx {
    pub account_id: String,
    pub session_cert: String,
    /// Base64 ed25519 seed. `Zeroizing` wipes cloned copies on drop.
    pub session_seed_b64: Zeroizing<String>,
    /// Activation TTL in ms (caller picks).
    pub expire_ms_from_now: i64,
}

/// SD-A: getAttestDocHex → ed25519-sign → registerTeeInfo. Fatal on failure.
pub async fn activate(client: &mut ApiClient, ctx: &ActivateCtx) -> Result<()> {
    let attest_doc_hex = api::request_attest_doc_hex_from_sa(client).await?;
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

// ── 60018 retry wrapper ──

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// `op`; on `UpgradeRequired` run `activate_fn` then `op` once more.
/// Spec for the inline retry pattern in `handlers.rs` (which can't use
/// this helper directly because `client` is `&mut`).
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

}
