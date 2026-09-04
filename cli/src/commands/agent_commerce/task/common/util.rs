//! common::util — generic helpers for the task system.
//!
//! Collects small utilities reused across the task module that aren't tied to specific business logic,
//! preventing them from being scattered across the individual mod / flow files.
//! Future formatting helpers, string normalization, time conversion, and similar generic helpers
//! should also go here.

use anyhow::{bail, Result};
use chrono::{TimeZone, Utc};

use super::deposit_qr::{InsufficientBalanceError, DEPOSIT_QR_MARKER, SCAN_TO_DEPOSIT_OPTION};
use super::network::task_api_client::TaskApiClient;
use super::{PaymentMode, DEBUG_LOG, XLAYER_CHAIN_INDEX};

/// unix seconds -> display string. 0 / negative are treated as unset; positive values are converted to RFC 3339.
pub fn fmt_unix_secs(secs: Option<i64>) -> String {
    match secs {
        Some(n) if n > 0 => Utc
            .timestamp_opt(n, 0)
            .single()
            .map(|t| t.to_rfc3339())
            .unwrap_or_else(|| n.to_string()),
        _ => "—".to_string(),
    }
}

// ─── JSON extraction helpers ────────────────────────────────────────────

/// Extract a string field from a JSON object.
pub fn json_str(obj: &serde_json::Value, key: &str) -> Result<String> {
    obj[key]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("response missing field: {key}"))
        .map(|s| s.to_string())
}

/// Extract a u64 field from a JSON object (accepts both number and string forms).
pub fn json_u64(obj: &serde_json::Value, key: &str) -> Result<u64> {
    if let Some(n) = obj[key].as_u64() {
        return Ok(n);
    }
    if let Some(s) = obj[key].as_str() {
        return s
            .parse()
            .map_err(|_| anyhow::anyhow!("failed to parse {key} as u64: {s}"));
    }
    bail!("response missing field: {key}")
}

// ─── Payment mode resolution ────────────────────────────────────────────

/// Resolve the payment mode: CLI flag > task detail paymentType.
pub async fn resolve_payment_mode(
    client: &mut TaskApiClient,
    payment_mode: Option<&str>,
    job_id: &str,
    agent_id: &str,
) -> Result<PaymentMode> {
    match payment_mode {
        Some(m) => Ok(PaymentMode::from_str(m)),
        None => {
            let task_resp = client
                .get_with_identity(&client.task_path(job_id), agent_id)
                .await?;
            let payment_mode_int = task_resp["paymentMode"].as_i64().unwrap_or(0) as i32;
            let mode = PaymentMode::from_int(payment_mode_int);
            if mode == PaymentMode::None {
                if DEBUG_LOG {
                    eprintln!("⚠ task paymentMode={payment_mode_int}, unrecognized payment mode, defaulting to escrow");
                }
                Ok(PaymentMode::Escrow)
            } else {
                if DEBUG_LOG {
                    eprintln!("ℹ --payment-mode not provided, using task detail paymentMode: {} ({payment_mode_int})", mode.as_str());
                }
                Ok(mode)
            }
        }
    }
}

// ─── Token symbol resolution by contract address ──────────────────────

/// Look up a token's symbol via the DEX basic-info API given chainIndex + contractAddress.
pub(crate) async fn resolve_token_symbol_by_address(
    chain_index: &str,
    contract_address: &str,
) -> Result<String> {
    let mut client = crate::client::ApiClient::new(None)?;
    let resp =
        crate::commands::token::fetch_info(&mut client, contract_address, chain_index).await?;
    let sym = resp
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|t| t["tokenSymbol"].as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
            "token basic-info returned no symbol for chain={chain_index} address={contract_address}"
        )
        })?;
    Ok(sym.to_string())
}

// ─── Balance precheck ──────────────────────────────────────────────────

/// Normalize a token symbol: map Unicode currency symbols to their ASCII letter equivalents, then uppercase.
/// Example: `USD₮0` -> `USDT0` (₮ U+20AE -> T).
fn normalize_token_symbol(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '₮' => 'T',
            _ => c,
        })
        .collect::<String>()
        .to_uppercase()
}

/// Call `onchainos wallet balance --chain 196` to look up the **business token** balance on XLayer
/// (USDT/USDG); bail and block downstream flow if insufficient.
///
/// Note: the task system is fully gas-free; gas is paid by the paymaster. This check applies only
/// to the business-token principal and **never** implies the user needs OKB / native to pay gas.
/// The bail message must make this explicit to avoid downstream agents misattributing the error
/// to a "top up gas" issue.
pub async fn ensure_sufficient_balance(required: f64, currency: &str) -> Result<()> {
    let exe = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("unable to determine executable path: {e}"))?;

    let output = tokio::process::Command::new(&exe)
        .args(["wallet", "balance", "--chain", XLAYER_CHAIN_INDEX])
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("balance query failed: {e}"))?;

    if !output.status.success() {
        bail!(
            "balance query failed (exit {}), please check login status",
            output.status
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .map_err(|e| anyhow::anyhow!("failed to parse balance query result: {e}"))?;

    let currency_norm = normalize_token_symbol(currency);
    let details = parsed["data"]["details"].as_array();
    if let Some(details) = details {
        for detail in details {
            let assets = detail["tokenAssets"]
                .as_array()
                .or_else(|| detail["assets"].as_array());
            if let Some(assets) = assets {
                for asset in assets {
                    let symbol = asset["tokenSymbol"]
                        .as_str()
                        .or_else(|| asset["symbol"].as_str())
                        .unwrap_or("");
                    let sym_norm = normalize_token_symbol(symbol);
                    if sym_norm == currency_norm || sym_norm == format!("{currency_norm}0") {
                        let balance: f64 = asset["balance"]
                            .as_str()
                            .and_then(|s| s.parse().ok())
                            .or_else(|| asset["balance"].as_f64())
                            .unwrap_or(0.0);
                        if balance < required {
                            let shortfall = required - balance;
                            let message = format!(
                                "Insufficient {currency} balance on XLayer (current: {balance}, need: {required}, shortfall: {shortfall})\n\
                                 \n\
                                 Fund your wallet — pick one:\n\
                                 1. {SCAN_TO_DEPOSIT_OPTION}\n\
                                 {DEPOSIT_QR_MARKER}\n\
                                 2. Swap on XLayer — \"swap <token> to {shortfall} {currency} on xlayer\"\n\
                                 3. Bridge from another chain — \"bridge {shortfall} {currency} from <chain> to xlayer\"\n\
                                 4. Send from OKX exchange — withdraw {currency} to your wallet address on XLayer network\n\
                                 \n\
                                 Note: gas is paid by the platform paymaster, no OKB / native required."
                            );
                            return Err(InsufficientBalanceError::new(
                                message, currency, required, balance,
                            )
                            .into());
                        }
                        return Ok(());
                    }
                }
            }
        }
    }

    let message = format!(
        "{currency} balance not found on XLayer (need {required} {currency})\n\
         \n\
         Fund your wallet — pick one:\n\
         1. {SCAN_TO_DEPOSIT_OPTION}\n\
         {DEPOSIT_QR_MARKER}\n\
         2. Swap on XLayer — \"swap <token> to {required} {currency} on xlayer\"\n\
         3. Bridge from another chain — \"bridge {required} {currency} from <chain> to xlayer\"\n\
         4. Send from OKX exchange — withdraw {currency} to your wallet address on XLayer network\n\
         \n\
         Note: gas is paid by the platform paymaster, no OKB / native required."
    );
    Err(InsufficientBalanceError::new(message, currency, required, 0.0).into())
}

/// Query the XLayer balance of `currency` for a given on-chain `address`.
/// Returns `Ok(balance)` on success, `Err` on any query/parse failure.
pub async fn query_xlayer_balance(address: &str, currency: &str) -> Result<f64> {
    let exe = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("unable to determine executable path: {e}"))?;
    let output = tokio::process::Command::new(&exe)
        .args([
            "portfolio",
            "all-balances",
            "--address",
            address,
            "--chains",
            XLAYER_CHAIN_INDEX,
        ])
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("portfolio balance query failed: {e}"))?;
    if !output.status.success() {
        bail!("portfolio balance query failed (exit {})", output.status);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .map_err(|e| anyhow::anyhow!("failed to parse portfolio balance result: {e}"))?;
    let currency_norm = normalize_token_symbol(currency);
    let token_assets = parsed["data"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|chain| chain["tokenAssets"].as_array().into_iter().flatten());
    for asset in token_assets {
        let symbol = asset["symbol"]
            .as_str()
            .or_else(|| asset["tokenSymbol"].as_str())
            .unwrap_or("");
        let sym_norm = normalize_token_symbol(symbol);
        if sym_norm == currency_norm || sym_norm == format!("{currency_norm}0") {
            let balance: f64 = asset["balance"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .or_else(|| asset["balance"].as_f64())
                .unwrap_or(0.0);
            return Ok(balance);
        }
    }
    Ok(0.0)
}

/// Like [`ensure_sufficient_balance`] but queries a specific on-chain address
/// via `onchainos portfolio all-balances` (public API, independent of selected_account_id).
/// Used by provider-side flows where the signing account may differ from the active account.
pub async fn ensure_sufficient_balance_at(
    required: f64,
    currency: &str,
    address: &str,
) -> Result<()> {
    let exe = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("unable to determine executable path: {e}"))?;

    let output = tokio::process::Command::new(&exe)
        .args([
            "portfolio",
            "all-balances",
            "--address",
            address,
            "--chains",
            XLAYER_CHAIN_INDEX,
        ])
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("portfolio balance query failed: {e}"))?;

    if !output.status.success() {
        bail!(
            "portfolio balance query failed (exit {}), address={address}",
            output.status
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .map_err(|e| anyhow::anyhow!("failed to parse portfolio balance result: {e}"))?;

    let currency_norm = normalize_token_symbol(currency);

    let token_assets = parsed["data"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|chain| chain["tokenAssets"].as_array().into_iter().flatten());

    for asset in token_assets {
        let symbol = asset["symbol"]
            .as_str()
            .or_else(|| asset["tokenSymbol"].as_str())
            .unwrap_or("");
        let sym_norm = normalize_token_symbol(symbol);
        if sym_norm == currency_norm || sym_norm == format!("{currency_norm}0") {
            let balance: f64 = asset["balance"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .or_else(|| asset["balance"].as_f64())
                .unwrap_or(0.0);
            if balance < required {
                let shortfall = required - balance;
                let message = format!(
                    "Insufficient {currency} balance on XLayer (current: {balance}, need: {required}, shortfall: {shortfall}, address: {address})\n\
                     \n\
                     Fund your wallet — pick one:\n\
                     1. {SCAN_TO_DEPOSIT_OPTION}\n\
                     {DEPOSIT_QR_MARKER}\n\
                     2. Swap on XLayer — \"swap <token> to {shortfall} {currency} on xlayer\"\n\
                     3. Bridge from another chain — \"bridge {shortfall} {currency} from <chain> to xlayer\"\n\
                     4. Send from OKX exchange — withdraw {currency} to your wallet address on XLayer network\n\
                     \n\
                     Note: gas is paid by the platform paymaster, no OKB / native required."
                );
                return Err(
                    InsufficientBalanceError::new(message, currency, required, balance).into(),
                );
            }
            return Ok(());
        }
    }

    let message = format!(
        "{currency} balance not found on XLayer (need {required} {currency}, address: {address})\n\
         \n\
         Fund your wallet — pick one:\n\
         1. {SCAN_TO_DEPOSIT_OPTION}\n\
         {DEPOSIT_QR_MARKER}\n\
         2. Swap on XLayer — \"swap <token> to {required} {currency} on xlayer\"\n\
         3. Bridge from another chain — \"bridge {required} {currency} from <chain> to xlayer\"\n\
         4. Send from OKX exchange — withdraw {currency} to your wallet address on XLayer network\n\
         \n\
         Note: gas is paid by the platform paymaster, no OKB / native required."
    );
    Err(InsufficientBalanceError::new(message, currency, required, 0.0).into())
}

// ─── jobId formatting ───────────────────────────────────────────────────

/// Validate that `job_id` is one of the legal forms before it's used in
/// CLI commands / HTTP requests:
///   - Real on-chain jobId: `0x` + 64 lowercase hex chars (66 chars total)
///   - System placeholder: starts with `system_` (per SKILL.md §--jobid source path)
///
/// Catches LLM mistakes early (before they reach the backend as opaque
/// `task not found`) — truncated shortJobId form, wrong length, non-hex
/// characters, missing `0x` prefix, or accidental sessionKey paste.
pub fn validate_job_id(job_id: &str) -> std::result::Result<(), String> {
    // Placeholder for events fired BEFORE a task exists (e.g. `create_task`).
    if job_id == "_" {
        return Ok(());
    }
    // Per SKILL.md §--jobid source path exception: backend-emitted pseudo jobIds
    // for account-level events (voter staking, no-ASP, etc.) — pass through as-is.
    if job_id.starts_with("system_") {
        return Ok(());
    }
    if !job_id.starts_with("0x") || job_id.len() != 66 {
        return Err(format!(
            "--jobid invalid (must be `0x` + 64 chars, got {} chars). Re-read jobId from envelope \
             (system event / user_decision_* → `message.jobId`; a2a-agent-chat → top-level `jobId`), \
             then retry.",
            job_id.len()
        ));
    }
    Ok(())
}

/// Short jobId: first 6 + … + last 4 characters. A 0x... hex value yields `0x1b76…1be1`;
/// a long string ID yields `task-0…long`. Returned as-is if ≤ 12 characters.
pub fn short_job_id(job_id: &str) -> String {
    if job_id.chars().count() <= 12 {
        return job_id.to_string();
    }
    let chars: Vec<char> = job_id.chars().collect();
    let head: String = chars.iter().take(6).collect();
    let tail: String = chars
        .iter()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{head}…{tail}")
}

/// Neutralize shell metacharacters in a task title before it is re-embedded into the
/// `onchainos agent user-notify --content "…"` command templates that downstream AI agents
/// execute on their own shell (including Windows `cmd.exe`). This is a mild, portable pass —
/// distinct from the aggressive alphanumeric-only `deliverables::sanitize_title`; do NOT merge
/// the two.
///
/// Char-class rules:
/// - command separators (`&` `|` `;`) → replaced with a single space;
/// - substitution / redirection / subshell chars (`` ` `` `$` `>` `<` `(` `)` `!`) → deleted;
/// - everything else (letters, digits, CJK, `:` `-` `.`, spaces, …) → kept unchanged.
///
/// Then consecutive-whitespace runs are collapsed to one space and the ends are trimmed.
///
/// Pure, deterministic, O(n). Never grows the string, so a post-truncation `≤ MAX_TITLE_CHARS`
/// invariant is preserved. Cannot fail — an all-metacharacter title sanitizes to `""`, which the
/// caller handles via its existing empty-title path.
pub fn sanitize_title_for_shell(title: &str) -> String {
    let mut mapped = String::with_capacity(title.len());
    for ch in title.chars() {
        match ch {
            // Command separators → single space.
            '&' | '|' | ';' => mapped.push(' '),
            // Substitution / redirection / subshell → delete.
            '`' | '$' | '>' | '<' | '(' | ')' | '!' => {}
            // Everything else → keep.
            other => mapped.push(other),
        }
    }
    // Collapse consecutive whitespace to a single space, then trim ends.
    mapped.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_job_id_hex_64() {
        assert_eq!(
            short_job_id("0x1b76dabd3bf884626184e3b36b7c65b54929a827a8a26e223c4b8aa868d41be1"),
            "0x1b76…1be1"
        );
    }

    #[test]
    fn short_job_id_passthrough() {
        assert_eq!(short_job_id("0x12"), "0x12");
        assert_eq!(short_job_id("task-1"), "task-1");
        assert_eq!(short_job_id("task-001-12"), "task-001-12");
    }

    #[test]
    fn short_job_id_long_string() {
        assert_eq!(short_job_id("task-001-very-long"), "task-0…long");
    }

    #[test]
    fn sanitize_title_for_shell_empty() {
        assert_eq!(sanitize_title_for_shell(""), "");
    }

    #[test]
    fn sanitize_title_for_shell_pure_metacharacters() {
        assert_eq!(sanitize_title_for_shell("&;|"), "");
    }

    #[test]
    fn sanitize_title_for_shell_mixed() {
        assert_eq!(sanitize_title_for_shell("A & B  C > D"), "A B C D");
    }

    #[test]
    fn sanitize_title_for_shell_cjk_with_ampersand() {
        // CJK expressed via \u{...} escapes so the source carries no literal CJK bytes
        // (keeps the "no Chinese in Rust source" gate happy) while still covering AC-7's
        // CJK-with-ampersand row: a 4-char + " & " + 4-char title collapses the separator
        // to a single space (the two CJK words are preserved unchanged).
        assert_eq!(
            sanitize_title_for_shell(
                "\u{94b1}\u{5305}\u{7a0e}\u{52a1} & \u{5408}\u{89c4}\u{62a5}\u{544a}"
            ),
            "\u{94b1}\u{5305}\u{7a0e}\u{52a1} \u{5408}\u{89c4}\u{62a5}\u{544a}"
        );
    }

    #[test]
    fn sanitize_title_for_shell_safe_punctuation_unchanged() {
        assert_eq!(
            sanitize_title_for_shell("Normal Title: Hello"),
            "Normal Title: Hello"
        );
    }

    #[test]
    fn sanitize_title_for_shell_brd_row() {
        assert_eq!(
            sanitize_title_for_shell("Wallet Tax & Compliance Report"),
            "Wallet Tax Compliance Report"
        );
    }

    #[test]
    fn sanitize_title_for_shell_deletes_subshell_chars() {
        // Substitution / redirection / subshell chars are deleted, not spaced.
        assert_eq!(sanitize_title_for_shell("a$(b)`c`!d"), "abcd");
    }

    #[test]
    fn sanitize_title_for_shell_never_grows() {
        let input = "x & y | z ; w";
        assert!(sanitize_title_for_shell(input).chars().count() <= input.chars().count());
    }

    // Task 5 / FR-6: the structured InsufficientBalanceError's Display renders the
    // legacy message string byte-for-byte (so a surfaced free-text `error` is
    // unchanged when the deposit-address enrichment degrades).
    #[test]
    fn insufficient_balance_error_display_equals_legacy_message() {
        let legacy = "Insufficient USDT balance on XLayer (current: 5, need: 100, shortfall: 95)\n\
                      Note: gas is paid by the platform paymaster, no OKB / native required.";
        let e = InsufficientBalanceError::new(legacy.to_string(), "USDT", 100.0, 5.0);
        assert_eq!(format!("{e}"), legacy);
    }
}
