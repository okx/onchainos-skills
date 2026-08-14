//! Deposit-address QR enrichment for the Agent-Commerce insufficient-balance path.
//!
//! On an insufficient XLayer business-token balance the four balance-check call
//! sites (`create-task` advisory; `set-payment-mode` / `confirm-accept` /
//! `dispute raise` blocking) enrich their output with the caller's XLayer
//! receiving address: a machine-readable `depositAddress` in the JSON envelope
//! plus a terminal-only Unicode QR rendered to STDERR when stderr is a TTY.
//!
//! Every path silent-degrades (FR-6): any address-resolution or QR failure falls
//! back to today's exact behavior — never a new error, never a panic.

use std::io::{IsTerminal, Write};

use crate::commands::agent_commerce::task::signing;

/// Fixed display label for the (only) supported deposit chain.
const DEPOSIT_CHAIN_LABEL: &str = "XLayer";
/// Minimum stderr width (columns) to render the QR block without garbling.
/// Below this we keep the address text and skip the QR (FR-3 / FR-6).
const MIN_QR_COLUMNS: usize = 33;

/// Option-1 label of the "Fund your wallet — pick one:" recharge list
/// (feedback !6c6489d8): the scan-to-deposit choice the QR belongs to. Shared
/// with the `util.rs` balance-message templates so the option text and the
/// stderr QR header never drift apart.
pub const SCAN_TO_DEPOSIT_OPTION: &str = "Scan code and recharge directly for your wallet";

/// In-text QR placeholder embedded in the `util.rs` balance-message templates.
/// [`fill_qr_marker`] always strips it from the final message — the playbook
/// uses the `depositAddress` JSON field (direct CLI) or extracts the `0x`
/// address from the option-1 text (relay via ASP notify) to render the QR.
/// Embedding executable commands or custom markers in the error text is
/// intentionally avoided: LLM sub-sessions may flag them as prompt injection.
pub const DEPOSIT_QR_MARKER: &str = "{{DEPOSIT_QR}}";

/// Strip the [`DEPOSIT_QR_MARKER`] from a balance message.
///
/// The marker is always removed regardless of whether an address is available.
/// QR rendering is driven by the `depositAddress` JSON field (direct CLI) or
/// address extraction from the option-1 text (ASP relay) — not by in-text
/// markers, which are unreliable across LLM translation and may trigger
/// prompt-injection detection.
pub fn fill_qr_marker(message: &str, _address: Option<&str>) -> String {
    message
        .replace(&format!("{DEPOSIT_QR_MARKER}\n"), "")
        .replace(DEPOSIT_QR_MARKER, "")
}

/// Structured insufficient-balance error carried through `anyhow` and recovered
/// by a downcast in `main.rs`. `Display` is byte-for-byte the existing balance
/// message, so the surfaced free-text `error` is unchanged when it degrades.
///
/// Manual `Display` + `Error` impls (not `thiserror`) to match the codebase
/// convention (`CliConfirming`, `sink::CodedError`) — this crate has no
/// `thiserror` dependency.
#[derive(Debug, Clone)]
pub struct InsufficientBalanceError {
    /// Existing human message, byte-for-byte (Display renders exactly this).
    pub message: String,
    /// Business token in play ("USDT" | "USDG").
    pub currency: String,
    /// Required amount, decimal string.
    pub required: String,
    /// Available amount, decimal string.
    pub available: String,
    /// Shortfall (required - available), decimal string.
    pub shortfall: String,
    /// XLayer chainIndex — `chains::resolve_chain("xlayer")` -> "196".
    pub chain_index: String,
    /// Fixed deposit chain label, "XLayer".
    pub deposit_chain: String,
    /// Resolved deposit address; `None` until a call site resolves it (silent-degrade).
    pub deposit_address: Option<String>,
}

impl std::fmt::Display for InsufficientBalanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for InsufficientBalanceError {}

impl InsufficientBalanceError {
    /// Construct from the balance-check context. `available` / `required` /
    /// `shortfall` are formatted with the same `f64` Display the legacy `bail!`
    /// used, so any surfaced number is unchanged.
    pub fn new(message: String, currency: &str, required: f64, available: f64) -> Self {
        let shortfall = (required - available).max(0.0);
        Self {
            message,
            currency: currency.to_string(),
            required: format!("{required}"),
            available: format!("{available}"),
            shortfall: format!("{shortfall}"),
            chain_index: crate::chains::resolve_chain("xlayer"),
            deposit_chain: DEPOSIT_CHAIN_LABEL.to_string(),
            deposit_address: None,
        }
    }
}

/// A resolved XLayer deposit target.
#[derive(Debug, Clone)]
pub struct DepositInfo {
    /// Bare XLayer address (no scheme/amount — FR-4).
    pub address: String,
    /// Fixed "XLayer".
    pub deposit_chain: String,
    /// "196" via `chains::resolve_chain`.
    pub chain_index: String,
}

/// FR-2: build `DepositInfo` from an explicit address (ASP signing account) — no
/// agentId resolution.
pub fn deposit_info_for_address(address: &str) -> DepositInfo {
    DepositInfo {
        address: address.to_string(),
        deposit_chain: DEPOSIT_CHAIN_LABEL.to_string(),
        chain_index: crate::chains::resolve_chain("xlayer"),
    }
}

/// FR-1: resolve the current caller's XLayer deposit address via the existing
/// agentId -> wallet resolver. Silent-degrades to `None` on any failure.
pub async fn resolve_current_deposit_info(agent_id: &str) -> Option<DepositInfo> {
    match signing::resolve_wallet_by_agent_id(agent_id).await {
        Ok((_account_id, address)) if !address.is_empty() => {
            Some(deposit_info_for_address(&address))
        }
        _ => None,
    }
}

/// Best-effort terminal width from `$COLUMNS`. `None` when unknown (render).
fn detected_columns() -> Option<usize> {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

/// One-line "deposit here" address hint (address text is always safe to show).
fn address_hint(info: &DepositInfo, currency: &str, shortfall: &str) -> String {
    format!(
        "Deposit {currency} to this {} address (short {shortfall}):\n{}",
        info.deposit_chain, info.address
    )
}

/// Render the QR + full address text + "XLayer" label to STDERR, but only when
/// stderr is a TTY and the terminal is wide enough. Never panics; QR encode
/// errors are swallowed (FR-6). No-op on non-TTY / piped / MCP output.
pub fn maybe_render_qr_stderr(info: &DepositInfo, currency: &str, shortfall: &str) {
    if !std::io::stderr().is_terminal() {
        return;
    }
    // Bind the stderr handle (writeln! on a bound writer keeps stderr output off
    // stdout without tripping clippy::explicit_write).
    let mut err = std::io::stderr();
    // Best-effort width gate: render when width is unknown; below the QR module
    // count + quiet zone keep only the address text (never a garbled QR).
    if detected_columns().is_some_and(|cols| cols < MIN_QR_COLUMNS) {
        let _ = writeln!(err, "{}", address_hint(info, currency, shortfall));
        return;
    }
    // Print the option-1 label first so the QR reads as the first "Fund your
    // wallet — pick one:" choice (feedback !6c6489d8) rather than a detached
    // block above the list; the QR block now follows the option-1 text instead
    // of sitting at the very top.
    let _ = writeln!(err, "1. {SCAN_TO_DEPOSIT_OPTION}");
    if let Ok(block) = crate::qr::render_address_qr_unicode(&info.address) {
        let _ = writeln!(err, "{block}");
    }
    // Address text always accompanies the QR (and stands in for it on encode failure).
    let _ = writeln!(err, "{}", address_hint(info, currency, shortfall));
}

/// Blocking (FR-1): if `err` is an `InsufficientBalanceError`, resolve the
/// current caller's deposit address, render the QR to stderr, attach the address,
/// and re-wrap. Any other error is returned unchanged (FR-6 — never a new failure).
pub async fn enrich_blocking(err: anyhow::Error, agent_id: &str) -> anyhow::Error {
    if err.downcast_ref::<InsufficientBalanceError>().is_none() {
        return err;
    }
    // Safe: checked above that the chain contains the type.
    let mut enriched = err
        .downcast_ref::<InsufficientBalanceError>()
        .unwrap()
        .clone();
    // Preserve the full displayed message (incl. any anyhow context layer),
    // matching main.rs's `{e:#}` rendering of the generic error path.
    enriched.message = format!("{err:#}");
    match resolve_current_deposit_info(agent_id).await {
        Some(info) => {
            enriched.deposit_address = Some(info.address.clone());
            // Keep the surfaced error free of in-text QR markers; the resolved
            // address is carried by the structured depositAddress sibling.
            enriched.message = fill_qr_marker(&enriched.message, Some(&info.address));
            maybe_render_qr_stderr(&info, &enriched.currency, &enriched.shortfall);
        }
        // Address unresolved -> deposit_address stays None (JSON degrades to the
        // verbatim {ok:false,error} envelope); strip the marker so no raw
        // {{DEPOSIT_QR}} placeholder leaks into the surfaced error text (FR-6).
        None => {
            enriched.message = fill_qr_marker(&enriched.message, None);
        }
    }
    anyhow::Error::from(enriched)
}

/// Blocking (FR-2): same as [`enrich_blocking`] but the address is explicit (the
/// ASP signing account) — no agentId resolution. `depositAddress` == `address`
/// verbatim.
pub fn enrich_blocking_at(err: anyhow::Error, address: &str) -> anyhow::Error {
    match err.downcast_ref::<InsufficientBalanceError>() {
        Some(ib) => {
            let mut enriched = ib.clone();
            enriched.message = format!("{err:#}");
            enriched.deposit_address = Some(address.to_string());
            // Keep the surfaced error free of in-text QR markers; the explicit
            // address is carried by the structured depositAddress sibling.
            enriched.message = fill_qr_marker(&enriched.message, Some(address));
            let info = deposit_info_for_address(address);
            maybe_render_qr_stderr(&info, &enriched.currency, &enriched.shortfall);
            anyhow::Error::from(enriched)
        }
        None => err,
    }
}

/// Build the base `balanceWarning` object (no address) — pure, per
/// `cli_command_spec.md`. `depositAddress` / `depositChain` are added by
/// [`balance_warning_with_address`] only when an address resolves.
pub(crate) fn balance_warning_base(err: &InsufficientBalanceError) -> serde_json::Value {
    serde_json::json!({
        "sufficient": false,
        "chain": err.deposit_chain,
        "chainIndex": err.chain_index,
        "currency": err.currency,
        "required": err.required,
        "available": err.available,
        "shortfall": err.shortfall,
    })
}

/// Build the `balanceWarning` object with a resolved deposit target — the base
/// object plus the two machine-readable script-contract keys `depositAddress` /
/// `depositChain` (PRD §8 acceptance #1; breaking-change-protected per
/// `cli_command_spec.md`). Pure — no QR side effect (the caller renders).
fn balance_warning_with_address(
    err: &InsufficientBalanceError,
    info: &DepositInfo,
) -> serde_json::Value {
    let mut obj = balance_warning_base(err);
    obj["depositAddress"] = serde_json::Value::String(info.address.clone());
    obj["depositChain"] = serde_json::Value::String(info.deposit_chain.clone());
    obj
}

/// Advisory (`create-task`): build the `balanceWarning` JSON object plus the
/// marker-stripped advisory message. Resolves + renders the QR to stderr as a
/// side effect; `depositAddress` / `depositChain` are present only when the
/// address resolves (silent-degrade otherwise). The returned `String` is the
/// `InsufficientBalanceError` message with [`DEPOSIT_QR_MARKER`] removed; QR
/// rendering is driven by structured JSON instead of in-text markers.
pub async fn balance_warning_json(
    err: &InsufficientBalanceError,
    agent_id: &str,
) -> (serde_json::Value, String) {
    match resolve_current_deposit_info(agent_id).await {
        Some(info) => {
            let obj = balance_warning_with_address(err, &info);
            maybe_render_qr_stderr(&info, &err.currency, &err.shortfall);
            let message = fill_qr_marker(&err.message, Some(&info.address));
            (obj, message)
        }
        None => (
            balance_warning_base(err),
            fill_qr_marker(&err.message, None),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_ib() -> InsufficientBalanceError {
        InsufficientBalanceError::new("Insufficient USDT balance".to_string(), "USDT", 100.0, 5.0)
    }

    // fill_qr_marker(Some) strips the marker line. The resolved address is
    // carried separately by depositAddress, not embedded into the error text.
    #[test]
    fn fill_qr_marker_some_strips_marker_line() {
        let msg = "Fund your wallet — pick one:\n1. Scan code\n{{DEPOSIT_QR}}\n2. Swap";
        let out = fill_qr_marker(msg, Some("0xDEPOSIT"));
        assert_eq!(out, "Fund your wallet — pick one:\n1. Scan code\n2. Swap");
        assert!(!out.contains(DEPOSIT_QR_MARKER));
        assert!(!out.contains("0xDEPOSIT"));
    }

    // fill_qr_marker(None) drops the whole marker line (marker + its newline) so
    // no raw placeholder leaks and the list closes up (option 1 -> option 2).
    #[test]
    fn fill_qr_marker_none_strips_marker_line() {
        let msg = "Fund your wallet — pick one:\n1. Scan code\n{{DEPOSIT_QR}}\n2. Swap";
        let out = fill_qr_marker(msg, None);
        assert_eq!(out, "Fund your wallet — pick one:\n1. Scan code\n2. Swap");
        assert!(!out.contains(DEPOSIT_QR_MARKER));
    }

    // A message with no marker passes through unchanged for both arms — safe to
    // call on any balance message (e.g. a legacy or context-only error text).
    #[test]
    fn fill_qr_marker_no_marker_is_noop() {
        let msg = "Insufficient USDT balance";
        assert_eq!(fill_qr_marker(msg, Some("0xabc")), msg);
        assert_eq!(fill_qr_marker(msg, None), msg);
    }

    // FR-2 / FR-5: explicit-address DepositInfo is XLayer @ chainIndex 196,
    // resolved via chains::resolve_chain (never a hardcoded literal).
    #[test]
    fn deposit_info_for_address_is_xlayer_196() {
        let info = deposit_info_for_address("0xabc");
        assert_eq!(info.chain_index, "196");
        assert_eq!(info.deposit_chain, "XLayer");
        assert_eq!(info.address, "0xabc");
    }

    // Constructor derives chain/shortfall correctly and Display is the message.
    #[test]
    fn insufficient_balance_error_fields_and_display() {
        let ib = sample_ib();
        assert_eq!(ib.chain_index, "196");
        assert_eq!(ib.deposit_chain, "XLayer");
        assert_eq!(ib.currency, "USDT");
        assert_eq!(ib.required, "100");
        assert_eq!(ib.available, "5");
        assert_eq!(ib.shortfall, "95");
        assert!(ib.deposit_address.is_none());
        assert_eq!(format!("{ib}"), "Insufficient USDT balance");
    }

    // FR-6: a non-InsufficientBalanceError passes through enrich_blocking_at
    // untouched — no new failure, message preserved.
    #[test]
    fn enrich_blocking_at_passes_through_other_errors() {
        let other = anyhow::anyhow!("some other failure");
        let out = enrich_blocking_at(other, "0xabc");
        assert!(out.downcast_ref::<InsufficientBalanceError>().is_none());
        assert_eq!(format!("{out}"), "some other failure");
    }

    // FR-2: enrich_blocking_at attaches the explicit address verbatim + XLayer.
    #[test]
    fn enrich_blocking_at_attaches_explicit_address() {
        let err = anyhow::Error::from(sample_ib());
        let out = enrich_blocking_at(err, "0xASP_SIGNER");
        let ib = out
            .downcast_ref::<InsufficientBalanceError>()
            .expect("still an InsufficientBalanceError");
        assert_eq!(ib.deposit_address.as_deref(), Some("0xASP_SIGNER"));
        assert_eq!(ib.deposit_chain, "XLayer");
        assert_eq!(ib.shortfall, "95");
    }

    // enrich_blocking_at preserves an anyhow context wrapper's full chain (`{:#}`)
    // in the message field — byte-for-byte the error text main.rs would emit.
    #[test]
    fn enrich_blocking_at_preserves_context_message() {
        let err = anyhow::Error::from(sample_ib()).context("Raising a dispute requires a deposit");
        let out = enrich_blocking_at(err, "0xASP");
        let ib = out.downcast_ref::<InsufficientBalanceError>().unwrap();
        assert_eq!(
            ib.message,
            "Raising a dispute requires a deposit: Insufficient USDT balance"
        );
        // Display of the re-wrapped error renders that same message.
        assert_eq!(
            format!("{out}"),
            "Raising a dispute requires a deposit: Insufficient USDT balance"
        );
    }

    // balanceWarning shape matches cli_command_spec.md; address-unresolved path
    // omits depositAddress/depositChain (base object).
    #[test]
    fn balance_warning_base_shape_and_omits_address() {
        let v = balance_warning_base(&sample_ib());
        assert_eq!(v["sufficient"], serde_json::json!(false));
        assert_eq!(v["chain"], "XLayer");
        assert_eq!(v["chainIndex"], "196");
        assert_eq!(v["currency"], "USDT");
        assert_eq!(v["required"], "100");
        assert_eq!(v["available"], "5");
        assert_eq!(v["shortfall"], "95");
        assert!(
            v.get("depositAddress").is_none(),
            "address must be absent when unresolved"
        );
        assert!(v.get("depositChain").is_none());
    }

    // Address-resolved path: the base object gains exactly the two script-contract
    // keys depositAddress/depositChain with exact literal spelling (PRD §8 acceptance
    // #1; breaking-change-protected per cli_command_spec.md), and every base field
    // survives the merge. Mirrors output.rs::insufficient_balance_json_some_emits_siblings.
    #[test]
    fn balance_warning_with_address_adds_deposit_keys() {
        let info = deposit_info_for_address("0xDEPOSIT_TARGET");
        let v = balance_warning_with_address(&sample_ib(), &info);
        // The two resolved-only keys, exact literal names + values.
        assert_eq!(v["depositAddress"], serde_json::json!(info.address));
        assert_eq!(v["depositAddress"], "0xDEPOSIT_TARGET");
        assert_eq!(v["depositChain"], "XLayer");
        // Base fields are preserved unchanged under the merge.
        assert_eq!(v["sufficient"], serde_json::json!(false));
        assert_eq!(v["chain"], "XLayer");
        assert_eq!(v["chainIndex"], "196");
        assert_eq!(v["currency"], "USDT");
        assert_eq!(v["required"], "100");
        assert_eq!(v["available"], "5");
        assert_eq!(v["shortfall"], "95");
    }
}
