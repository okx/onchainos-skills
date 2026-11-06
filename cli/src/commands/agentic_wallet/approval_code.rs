//! Approval email double confirmation.
//!
//! When the account has `doubleConfirmation` on, the backend rejects a token
//! approval (approve / increaseAllowance broadcast, Permit / Permit2 sign-msg)
//! with code `40030` until the user confirms it with an email code. The CLI
//! process exits between agent turns, so the paused operation is kept as a
//! pending record in the encrypted keyring:
//!
//! 1. The user-facing entry point gets `40030` and calls [`start`]. It saves the
//!    pending record, requests a code (`sendEmailCode`), and returns a
//!    `confirming` result with scene [`SCENE_CODE_SENT`].
//! 2. `wallet approval-code verify --verification-code` checks the code
//!    (`verifyEmailCode`). Only `verified=true` replays the saved original
//!    request once, unchanged. `wallet approval-code resend` and `cancel`
//!    resend the code for the same data or drop the record.
//!
//! The record holds the original request (it carries session signatures), so
//! it lives only in the keyring and is never printed or logged. The code itself
//! is never stored.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::commands::sink::CodedError;
use crate::wallet_api::{ApiCodeError, WalletApiClient};
use crate::{keyring_store, output, wallet_store};

/// Backend business code: the approval needs email double confirmation.
pub(crate) const APPROVAL_VERIFICATION_REQUIRED_CODE: &str = "40030";
/// `confirming.scene` emitted after a code was sent.
pub(crate) const SCENE_CODE_SENT: &str = "approval_code_sent";

const PENDING_KEY: &str = "pending_approval_verification";
/// Local upper bound for a paused approval. The backend enforces the code
/// lifetime (5 minutes); this only stops a stale record from being replayed.
const PENDING_TTL_SECS: i64 = 30 * 60;

// Backend business codes of sendEmailCode / verifyEmailCode.
const CODE_INVALID: &str = "40032";
const CODE_EXPIRED: &str = "40033";
const CODE_ATTEMPTS_EXCEEDED: &str = "40034";
const CODE_TX_NOT_PENDING: &str = "40035";
const CODE_SEND_TOO_FREQUENT: &str = "40036";

const PENDING_LOCK_FILE: &str = "approval-pending.lock";

/// Backend risk warning (for example `approve_eoa`). After email verification it
/// is not expected; if it still arrives, the CLI stops.
const RISK_WARNING_CODE: &str = "81362";

const SEND_CODE_PATH: &str = "/priapi/v5/wallet/agentic/policy/sendEmailCode";
const VERIFY_CODE_PATH: &str = "/priapi/v5/wallet/agentic/policy/verifyEmailCode";
const SIGN_MSG_PATH: &str = "/priapi/v5/wallet/agentic/pre-transaction/sign-msg";

const NEXT_AFTER_SEND: &str = "Ask the user for the 6-digit code from the bound email, then run `onchainos wallet approval-code verify --verification-code <code>`. To get a new code, run `onchainos wallet approval-code resend`. If the user cancels, run `onchainos wallet approval-code cancel`.";

/// True when `e` carries the backend `40030` approval-verification code.
pub(crate) fn is_verification_required(e: &anyhow::Error) -> bool {
    e.downcast_ref::<ApiCodeError>()
        .is_some_and(|api| api.code == APPROVAL_VERIFICATION_REQUIRED_CODE)
}

/// Map `40030` on a path without an email-verification flow (internal x402 /
/// agent signing) to an explicit stop. Other errors pass through.
pub(crate) fn reject_unsupported(e: anyhow::Error) -> anyhow::Error {
    if !is_verification_required(&e) {
        return e;
    }
    CodedError::new(
        "approval_verification_unsupported",
        None,
        "This operation includes a token approval that requires email verification, but this internal signing path does not support email verification. The operation was stopped and nothing was executed.",
    )
    .into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PendingKind {
    /// broadcast-transaction; `verification_data` is unsignedInfo `data[0]`.
    Broadcast,
    /// sign-msg (EIP-712); `verification_data` is the full typedData.
    SignMessage,
}

/// A paused approval and its exact original request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PendingApproval {
    pub(crate) kind: PendingKind,
    pub(crate) account_id: String,
    pub(crate) address: String,
    /// Backend chainIndex of the original request.
    pub(crate) chain_index: String,
    pub(crate) verification_data: Value,
    /// Broadcast: the `extraData` string. Sign message: the full sign-msg body.
    pub(crate) request: Value,
    /// Swap trace id sent as a header on the original contract-call broadcast.
    #[serde(default)]
    pub(crate) trace_tid: Option<String>,
    pub(crate) created_at: i64,
    /// Fingerprint of the login session that paused the approval. A record
    /// from an older session is never used, even if deleting it failed.
    #[serde(default)]
    pub(crate) session_fingerprint: String,
}

impl PendingApproval {
    pub(crate) fn broadcast(
        account_id: &str,
        address: &str,
        chain_index: &str,
        unsigned_info_item: Value,
        extra_data: &str,
        trace_tid: Option<String>,
    ) -> Self {
        Self {
            kind: PendingKind::Broadcast,
            account_id: account_id.to_string(),
            address: address.to_string(),
            chain_index: chain_index.to_string(),
            verification_data: unsigned_info_item,
            request: Value::String(extra_data.to_string()),
            trace_tid,
            created_at: chrono::Utc::now().timestamp(),
            session_fingerprint: String::new(),
        }
    }

    pub(crate) fn sign_message(
        account_id: &str,
        address: &str,
        chain_index: &str,
        typed_data: Value,
        sign_body: Value,
    ) -> Self {
        Self {
            kind: PendingKind::SignMessage,
            account_id: account_id.to_string(),
            address: address.to_string(),
            chain_index: chain_index.to_string(),
            verification_data: typed_data,
            request: sign_body,
            trace_tid: None,
            created_at: chrono::Utc::now().timestamp(),
            session_fingerprint: String::new(),
        }
    }

    /// Body for `sendEmailCode` (no code) or `verifyEmailCode` (with code).
    fn verification_body(&self, code: Option<&str>) -> Value {
        let mut body = json!({ "chainIndex": self.chain_index });
        let field = match self.kind {
            PendingKind::Broadcast => "data",
            PendingKind::SignMessage => "message",
        };
        body[field] = self.verification_data.clone();
        if let Some(code) = code {
            body["code"] = json!(code);
        }
        body
    }

    fn is_expired_at(&self, now: i64) -> bool {
        now - self.created_at > PENDING_TTL_SECS
    }

    fn operation(&self) -> &'static str {
        match self.kind {
            PendingKind::Broadcast => "broadcast",
            PendingKind::SignMessage => "signMessage",
        }
    }
}

// ── pending store ────────────────────────────────────────────────────

/// Storage for the single paused approval. The keyring implementation is used
/// at runtime; tests use an in-memory one, so they never touch real records.
trait PendingStore {
    fn read(&self) -> Result<Option<String>>;
    fn write(&self, json: &str) -> Result<()>;
    fn remove(&self) -> Result<()>;
}

struct KeyringPendingStore;

impl PendingStore for KeyringPendingStore {
    fn read(&self) -> Result<Option<String>> {
        Ok(keyring_store::read_blob()?.get(PENDING_KEY).cloned())
    }

    fn write(&self, json: &str) -> Result<()> {
        keyring_store::set(PENDING_KEY, json)
    }

    fn remove(&self) -> Result<()> {
        if self.read()?.is_none() {
            return Ok(());
        }
        keyring_store::delete(PENDING_KEY)?;
        if self.read()?.is_some() {
            anyhow::bail!("the paused approval record could not be removed");
        }
        Ok(())
    }
}

/// Run `f` while holding the cross-process lock for the paused approval, so
/// load, compare, delete, and save steps from different CLI processes never
/// interleave.
fn with_pending_lock<T>(f: impl FnOnce(&dyn PendingStore) -> Result<T>) -> Result<T> {
    let path = crate::home::ensure_onchainos_home()?.join(PENDING_LOCK_FILE);
    let _lock = lock_file(&path)?;
    f(&KeyringPendingStore)
}

/// Open and exclusively lock `path`. The lock is released when the file drops.
fn lock_file(path: &std::path::Path) -> Result<std::fs::File> {
    use fs2::FileExt;
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    file.lock_exclusive()
        .context("failed to lock the paused approval")?;
    Ok(file)
}

fn save_in(store: &dyn PendingStore, pending: &PendingApproval) -> Result<()> {
    let json = serde_json::to_string(pending).context("failed to serialize pending approval")?;
    store.write(&json)
}

/// Load the record. A record that cannot be parsed is removed and treated as absent.
fn load_in(store: &dyn PendingStore) -> Result<Option<PendingApproval>> {
    match store.read()? {
        None => Ok(None),
        Some(json) => match serde_json::from_str(&json) {
            Ok(pending) => Ok(Some(pending)),
            Err(_) => {
                store.remove()?;
                Ok(None)
            }
        },
    }
}

/// Atomically consume the record: remove it only if it is still `expected`.
/// Returns `true` only to the caller that removed it; only that caller may replay.
fn take_if_same(store: &dyn PendingStore, expected: &PendingApproval) -> Result<bool> {
    let still_pending = load_in(store)?.is_some_and(|current| is_same_pending(&current, expected));
    if !still_pending {
        return Ok(false);
    }
    store.remove()?;
    Ok(true)
}

fn save_pending(pending: &PendingApproval) -> Result<()> {
    with_pending_lock(|store| save_in(store, pending))
}

fn load_pending() -> Result<Option<PendingApproval>> {
    with_pending_lock(load_in)
}

/// Remove the paused approval. Errors are returned: a caller that reports
/// "cancelled" or relies on the removal must not ignore them.
fn remove_pending() -> Result<()> {
    with_pending_lock(|store| store.remove())
}

/// Best-effort removal on login. Failure is safe to ignore here: a record from
/// the previous session no longer matches the session fingerprint, so it can
/// never be verified or replayed.
pub(crate) fn clear_pending() {
    let _ = remove_pending();
}

/// Short, non-reversible fingerprint of the current login session.
fn current_session_fingerprint() -> Result<String> {
    use sha2::{Digest, Sha256};
    let session = wallet_store::load_session()?
        .ok_or_else(|| anyhow::anyhow!(super::common::ERR_NOT_LOGGED_IN))?;
    let digest = Sha256::digest(session.session_cert.as_bytes());
    Ok(hex::encode(&digest[..16]))
}

fn not_found_error() -> anyhow::Error {
    CodedError::new(
        "approval_verification_not_found",
        None,
        "No approval is waiting for email verification. Nothing was executed. Start again from the original request with a new preview and confirmation.",
    )
    .into()
}

/// Load the pending record and check that its account still owns its address
/// in the local wallet and that it is fresh. The account is the one that owns
/// the `--from` address, which need not be the selected account. A stale or
/// foreign record is dropped, never reused.
fn load_pending_for_current_account() -> Result<PendingApproval> {
    let pending = load_pending()?.ok_or_else(not_found_error)?;
    let wallets = wallet_store::load_wallets()?;
    let fingerprint = current_session_fingerprint()?;
    if let Err(e) = check_pending_context(
        &pending,
        wallets.as_ref(),
        &fingerprint,
        chrono::Utc::now().timestamp(),
    ) {
        // Remove only this record: another process may have saved a newer one.
        with_pending_lock(|store| take_if_same(store, &pending))?;
        return Err(e);
    }
    Ok(pending)
}

/// True when `account_id` still lists `address` in the local wallet.
fn account_owns_address(
    wallets: &wallet_store::WalletsJson,
    account_id: &str,
    address: &str,
) -> bool {
    wallets.accounts_map.get(account_id).is_some_and(|entry| {
        entry
            .address_list
            .iter()
            .any(|a| a.address.eq_ignore_ascii_case(address))
    })
}

fn check_pending_context(
    pending: &PendingApproval,
    wallets: Option<&wallet_store::WalletsJson>,
    session_fingerprint: &str,
    now: i64,
) -> Result<()> {
    let owned =
        wallets.is_some_and(|w| account_owns_address(w, &pending.account_id, &pending.address));
    let same_session = !pending.session_fingerprint.is_empty()
        && pending.session_fingerprint == session_fingerprint;
    if !owned || !same_session {
        return Err(CodedError::new(
            "approval_verification_context_changed",
            None,
            "The wallet account or login session changed after the approval was paused. The old verification cannot be reused and nothing was executed. Start again from the original request with a new preview and confirmation.",
        )
        .into());
    }
    if pending.is_expired_at(now) {
        return Err(CodedError::new(
            "approval_verification_expired",
            None,
            "The paused approval has expired. Nothing was executed. Start again from the original request with a new preview and confirmation.",
        )
        .into());
    }
    Ok(())
}

// ── send code ────────────────────────────────────────────────────────

fn send_result_error(result: Result<Value>) -> anyhow::Error {
    match result {
        Ok(data) if need_double_confirmation(&data) => crate::output::CliConfirming {
            message: "This approval requires email verification (approval double confirmation is on). A verification code was sent to the bound email. The approval has not been executed yet.".to_string(),
            next: NEXT_AFTER_SEND.to_string(),
            scene: Some(SCENE_CODE_SENT.to_string()),
        }
        .into(),
        // needDoubleConfirmation=false: no code was sent and nothing is
        // pending verification. This is not a pass; hand the decision back to
        // the original command instead of retrying here.
        Ok(_) => not_pending_error(None),
        Err(e) if backend_code(&e) == Some(CODE_TX_NOT_PENDING) => {
            not_pending_error(backend_error_data(&e))
        }
        Err(e) => {
            let (code, message) = if backend_code(&e) == Some(CODE_SEND_TOO_FREQUENT) {
                (
                    "approval_code_send_too_frequent",
                    "This approval requires email verification, but codes were requested too often and no new code was sent. The approval has not been executed and is kept. Ask the user to retry later or cancel.",
                )
            } else {
                (
                    "approval_code_send_failed",
                    "This approval requires email verification, but the verification code was not sent, or the send result is unknown. The approval has not been executed and is kept. Ask the user whether to resend the code or cancel.",
                )
            };
            let data = backend_error_data(&e).unwrap_or_else(|| transport_error_data(&e));
            let reason = data["backendMessage"]
                .as_str()
                .or_else(|| data["reason"].as_str())
                .unwrap_or_default()
                .to_string();
            let message = if reason.is_empty() {
                message.to_string()
            } else {
                format!("{message} Reason: {reason}")
            };
            let mut err = CodedError::new(code, None, message).with_next_steps(json!([
                { "action": "resend", "command": "onchainos wallet approval-code resend" },
                { "action": "cancel", "command": "onchainos wallet approval-code cancel" }
            ]));
            err = err.with_data(data);
            err.into()
        }
    }
}

/// Reason for a failure that carries no backend business code (HTTP 5xx,
/// a non-JSON body such as a 404 page, or a network error). The error chain
/// holds only the response status and a body preview, never the request.
fn transport_error_data(e: &anyhow::Error) -> Value {
    const MAX_REASON_CHARS: usize = 300;
    let reason: String = format!("{e:#}").chars().take(MAX_REASON_CHARS).collect();
    json!({ "reason": reason })
}

/// The operation is not waiting for verification (needDoubleConfirmation=false
/// or backend 40035). Pure: callers drop the record with [`drop_if_not_pending`].
fn not_pending_error(data: Option<Value>) -> anyhow::Error {
    let mut err = CodedError::new(
        "approval_verification_not_pending",
        None,
        "The backend reports that this operation is not waiting for email verification, so no code can be used for it. The approval was not executed. Do not retry automatically; ask the user before running the original command again.",
    );
    if let Some(data) = data {
        err = err.with_data(data);
    }
    err.into()
}

/// Remove the paused record when `err` says the operation is no longer
/// pending, so an old verification is never reused. A failed removal is
/// reported instead of the original error.
fn drop_if_not_pending(err: anyhow::Error, pending: &PendingApproval) -> anyhow::Error {
    let not_pending = err
        .downcast_ref::<CodedError>()
        .is_some_and(|c| c.code == "approval_verification_not_pending");
    if not_pending {
        // Remove only this record: another process may have saved a newer one.
        if let Err(e) = with_pending_lock(|store| take_if_same(store, pending)) {
            return CodedError::new(
                "approval_verification_unavailable",
                None,
                format!("The backend reports that this operation is not waiting for email verification, but the paused record could not be removed ({e:#}). Nothing was executed. Run `onchainos wallet approval-code cancel`."),
            )
            .into();
        }
    }
    err
}

fn backend_code(e: &anyhow::Error) -> Option<&str> {
    e.downcast_ref::<ApiCodeError>()
        .map(|api| api.code.as_str())
}

fn need_double_confirmation(data: &Value) -> bool {
    first_item(data)["needDoubleConfirmation"].as_bool() == Some(true)
}

fn verified(data: &Value) -> bool {
    first_item(data)["verified"].as_bool() == Some(true)
}

/// The envelope `data` may be an object or a one-element array.
fn first_item(data: &Value) -> &Value {
    match data {
        Value::Array(items) => items.first().unwrap_or(&Value::Null),
        other => other,
    }
}

fn backend_error_data(e: &anyhow::Error) -> Option<Value> {
    e.downcast_ref::<ApiCodeError>()
        .map(|api| json!({ "backendCode": api.code, "backendMessage": api.msg }))
}

async fn send_code(
    client: &mut WalletApiClient,
    access_token: &str,
    pending: &PendingApproval,
) -> Result<Value> {
    client
        .post_authed_mutation_no_retry(
            SEND_CODE_PATH,
            access_token,
            &pending.verification_body(None),
        )
        .await
}

/// Entry point for a `40030` on a user-facing path. Saves the paused
/// operation, requests a code, and returns the error to propagate: a
/// `confirming` result on success, or a coded error otherwise. Never `Ok`,
/// because the original operation has not run.
pub(crate) async fn start(
    client: &mut WalletApiClient,
    access_token: &str,
    mut pending: PendingApproval,
) -> anyhow::Error {
    let saved = current_session_fingerprint().and_then(|fingerprint| {
        pending.session_fingerprint = fingerprint;
        save_pending(&pending)
    });
    if let Err(e) = saved {
        return CodedError::new(
            "approval_verification_unavailable",
            None,
            format!("This approval requires email verification, but the CLI could not keep the paused operation ({e:#}). Nothing was executed."),
        )
        .into();
    }
    drop_if_not_pending(
        send_result_error(send_code(client, access_token, &pending).await),
        &pending,
    )
}

// ── commands ─────────────────────────────────────────────────────────

/// onchainos wallet approval-code resend
pub(super) async fn cmd_resend() -> Result<()> {
    let pending = load_pending_for_current_account()?;
    let access_token = super::auth::ensure_tokens_refreshed().await?;
    let mut client = WalletApiClient::new()?;
    Err(drop_if_not_pending(
        send_result_error(send_code(&mut client, &access_token, &pending).await),
        &pending,
    ))
}

/// onchainos wallet approval-code cancel
pub(super) async fn cmd_cancel() -> Result<()> {
    let existed = with_pending_lock(|store| {
        let existed = store.read()?.is_some();
        store.remove()?;
        Ok(existed)
    })?;
    output::success(json!({ "cancelled": existed }));
    Ok(())
}

/// onchainos wallet approval-code verify --verification-code <code>
pub(super) async fn cmd_verify(code: &str) -> Result<()> {
    crate::validators::validate_verification_code(code, "verification-code")?;
    let code = code.trim();
    let pending = load_pending_for_current_account()?;
    let access_token = super::auth::ensure_tokens_refreshed().await?;
    let mut client = WalletApiClient::new()?;

    let data = client
        .post_authed_mutation_no_retry(
            VERIFY_CODE_PATH,
            &access_token,
            &pending.verification_body(Some(code)),
        )
        .await
        .map_err(|e| drop_if_not_pending(verify_failed_error(e), &pending))?;
    if !verified(&data) {
        return Err(CodedError::new(
            "approval_code_invalid",
            None,
            "The verification code is incorrect. The approval was not completed. Ask the user to enter it again or get a new code.",
        )
        .with_next_steps(verify_retry_steps())
        .into());
    }

    // Consume the record under the lock before the replay. Only the process
    // that removed it replays, so one verification replays at most once, even
    // if another process verifies or a new approval replaces the record.
    if !with_pending_lock(|store| take_if_same(store, &pending))? {
        return Err(not_found_error());
    }
    let result = replay(&mut client, &access_token, &pending).await?;
    output::success(result);
    Ok(())
}

fn is_same_pending(a: &PendingApproval, b: &PendingApproval) -> bool {
    a.kind == b.kind
        && a.account_id == b.account_id
        && a.address == b.address
        && a.chain_index == b.chain_index
        && a.created_at == b.created_at
        && a.session_fingerprint == b.session_fingerprint
        && a.verification_data == b.verification_data
        && a.request == b.request
}

fn verify_retry_steps() -> Value {
    json!([
        { "action": "verify", "command": "onchainos wallet approval-code verify --verification-code <code>" },
        { "action": "resend", "command": "onchainos wallet approval-code resend" },
        { "action": "cancel", "command": "onchainos wallet approval-code cancel" }
    ])
}

fn verify_failed_error(e: anyhow::Error) -> anyhow::Error {
    let Some(data) = backend_error_data(&e) else {
        // Transport failure: the verification state is unknown. Keep the
        // record so the user can verify again or resend.
        return CodedError::new(
            "approval_code_verify_failed",
            None,
            format!("Verification could not be confirmed ({e:#}). The approval was not executed. Ask the user whether to verify again or get a new code."),
        )
        .with_next_steps(verify_retry_steps())
        .into();
    };
    let (code, message, next_steps) = match backend_code(&e) {
        Some(CODE_INVALID) => (
            "approval_code_invalid",
            "The verification code is incorrect. The approval was not completed. Ask the user to enter it again or get a new code.",
            verify_retry_steps(),
        ),
        Some(CODE_EXPIRED) => (
            "approval_code_expired",
            "The verification code has expired. The approval was not completed. Ask the user whether to get a new code or cancel.",
            resend_or_cancel_steps(),
        ),
        Some(CODE_ATTEMPTS_EXCEEDED) => (
            "approval_code_attempts_exceeded",
            "Too many failed attempts. The approval was not completed. Stop; the user can try again in 10 minutes. Do not verify or resend on your own.",
            resend_or_cancel_steps(),
        ),
        Some(CODE_TX_NOT_PENDING) => return not_pending_error(Some(data)),
        _ => (
            "approval_code_verify_failed",
            "The backend rejected the verification code. The approval was not executed.",
            verify_retry_steps(),
        ),
    };
    let message = if code == "approval_code_verify_failed" {
        let reason = data["backendMessage"].as_str().unwrap_or_default();
        format!(
            "{message} Reason: {reason}. Ask the user whether to verify again or get a new code."
        )
    } else {
        message.to_string()
    };
    CodedError::new(code, None, message)
        .with_data(data)
        .with_next_steps(next_steps)
        .into()
}

fn resend_or_cancel_steps() -> Value {
    json!([
        { "action": "resend", "command": "onchainos wallet approval-code resend" },
        { "action": "cancel", "command": "onchainos wallet approval-code cancel" }
    ])
}

/// Replay the saved original request once, unchanged.
async fn replay(
    client: &mut WalletApiClient,
    access_token: &str,
    pending: &PendingApproval,
) -> Result<Value> {
    let result = match pending.kind {
        PendingKind::Broadcast => replay_broadcast(client, access_token, pending).await,
        PendingKind::SignMessage => replay_sign_message(client, access_token, pending).await,
    };
    result.map_err(map_replay_error)
}

/// Map a replay failure. A risk warning or a second 40030 after a verified
/// code means the backend did not accept the verification: stop explicitly.
fn map_replay_error(e: anyhow::Error) -> anyhow::Error {
    if backend_code(&e) == Some(RISK_WARNING_CODE) {
        let warning = e
            .downcast_ref::<ApiCodeError>()
            .map(|api| api.msg.clone())
            .unwrap_or_default();
        return CodedError::new(
            "approval_risk_warning_after_verification",
            None,
            format!("The email code was accepted, but the backend returned a risk warning ({warning}). Nothing was executed. Stop and report it; do not verify again, rerun the original command, or add --force."),
        )
        .with_data(json!({ "backendCode": RISK_WARNING_CODE, "backendMessage": warning }))
        .into();
    }
    if is_verification_required(&e) {
        CodedError::new(
            "approval_verification_unavailable",
            None,
            "The code was verified, but the backend still requires email verification for this approval. The CLI stopped and did not retry. Nothing was executed.",
        )
        .into()
    } else {
        e
    }
}

async fn replay_broadcast(
    client: &mut WalletApiClient,
    access_token: &str,
    pending: &PendingApproval,
) -> Result<Value> {
    let extra_data = pending
        .request
        .as_str()
        .context("paused approval has no broadcast request")?;
    let ts = chrono::Utc::now().timestamp_millis().to_string();
    let headers: Vec<(&str, &str)> = match pending.trace_tid.as_deref() {
        Some(tid) => vec![("ok-client-tid", tid), ("ok-client-timestamp", ts.as_str())],
        None => vec![],
    };
    let resp = client
        .broadcast_transaction(
            access_token,
            &pending.account_id,
            &pending.address,
            &pending.chain_index,
            extra_data,
            if headers.is_empty() {
                None
            } else {
                Some(headers.as_slice())
            },
            // Default user broadcast endpoint, the same one the paused call used.
            None,
        )
        .await?;
    if pending.trace_tid.is_some() {
        let _ = wallet_store::clear_swap_trace_id();
    }
    Ok(json!({
        "operation": pending.operation(),
        "txHash": resp.tx_hash,
        "orderId": resp.order_id,
    }))
}

async fn replay_sign_message(
    client: &mut WalletApiClient,
    access_token: &str,
    pending: &PendingApproval,
) -> Result<Value> {
    // No-retry, like the broadcast replay: the request is sent at most once.
    let data = client
        .post_authed_mutation_no_retry(SIGN_MSG_PATH, access_token, &pending.request)
        .await?;
    let signature = super::sign::extract_signature(&data)?;
    Ok(json!({
        "operation": pending.operation(),
        "signature": signature,
    }))
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn api_error(code: &str) -> anyhow::Error {
        ApiCodeError {
            code: code.to_string(),
            msg: "email double confirmation required".to_string(),
            http_status: 200,
        }
        .into()
    }

    fn broadcast_pending() -> PendingApproval {
        let mut pending = PendingApproval::broadcast(
            "acct-1",
            "0xabc",
            "196",
            json!({ "unsignedTxHash": "0x01", "extraData": { "nonce": 1 } }),
            "{\"msgForSign\":{}}",
            None,
        );
        pending.session_fingerprint = "fp-1".to_string();
        pending
    }

    /// In-memory store so tests never read or delete a real paused approval.
    #[derive(Default)]
    struct MemStore {
        value: std::cell::RefCell<Option<String>>,
        fail_remove: bool,
    }

    impl PendingStore for MemStore {
        fn read(&self) -> Result<Option<String>> {
            Ok(self.value.borrow().clone())
        }
        fn write(&self, json: &str) -> Result<()> {
            *self.value.borrow_mut() = Some(json.to_string());
            Ok(())
        }
        fn remove(&self) -> Result<()> {
            if self.fail_remove {
                anyhow::bail!("keyring delete failed");
            }
            *self.value.borrow_mut() = None;
            Ok(())
        }
    }

    fn typed_data() -> Value {
        json!({
            "types": { "EIP712Domain": [] },
            "primaryType": "PermitTransferFrom",
            "domain": { "name": "Permit2", "chainId": 1 },
            "message": { "spender": "0xbb", "nonce": "1" }
        })
    }

    #[test]
    fn detects_40030_only_from_backend_code() {
        assert!(is_verification_required(&api_error("40030")));
        assert!(is_verification_required(
            &api_error("40030").context("sign-msg failed")
        ));
        assert!(!is_verification_required(&api_error("81362")));
        // Message text alone never triggers verification.
        assert!(!is_verification_required(&anyhow::anyhow!(
            "code=40030 email double confirmation required"
        )));
    }

    #[test]
    fn reject_unsupported_stops_40030_and_passes_other_errors() {
        let err = reject_unsupported(api_error("40030"));
        let coded = err.downcast_ref::<CodedError>().expect("coded error");
        assert_eq!(coded.code, "approval_verification_unsupported");

        let err = reject_unsupported(api_error("81363"));
        assert_eq!(err.downcast_ref::<ApiCodeError>().unwrap().code, "81363");
    }

    #[test]
    fn broadcast_body_sends_unsigned_info_item_as_data() {
        let pending = broadcast_pending();
        let body = pending.verification_body(None);
        assert_eq!(body["chainIndex"], "196");
        assert_eq!(body["data"], pending.verification_data);
        assert!(body.get("message").is_none());
        assert!(body.get("code").is_none());

        let body = pending.verification_body(Some("012345"));
        assert_eq!(body["code"], "012345");
        assert_eq!(body["data"], pending.verification_data);
    }

    #[test]
    fn sign_body_sends_full_typed_data_as_message() {
        let sign_body = json!({ "chainIndex": "1", "payload": [{ "message": typed_data() }] });
        let pending =
            PendingApproval::sign_message("acct-1", "0xabc", "1", typed_data(), sign_body.clone());
        let body = pending.verification_body(Some("654321"));
        assert_eq!(body["chainIndex"], "1");
        assert_eq!(body["message"], typed_data());
        assert!(body.get("data").is_none());
        assert_eq!(pending.request, sign_body);
    }

    #[test]
    fn pending_round_trips_without_changing_the_original_request() {
        let pending = broadcast_pending();
        let json = serde_json::to_string(&pending).unwrap();
        let back: PendingApproval = serde_json::from_str(&json).unwrap();
        assert_eq!(back.kind, PendingKind::Broadcast);
        assert_eq!(back.request, pending.request);
        assert_eq!(back.verification_data, pending.verification_data);
        assert!(!json.contains("\"code\""));
    }

    fn wallets_with(entries: &[(&str, &str)], selected: &str) -> wallet_store::WalletsJson {
        let mut wallets = wallet_store::WalletsJson {
            selected_account_id: selected.to_string(),
            ..Default::default()
        };
        for (account_id, address) in entries {
            wallets.accounts_map.insert(
                account_id.to_string(),
                serde_json::from_value(json!({
                    "addressList": [{ "address": address, "chainIndex": "196", "chainName": "xlayer" }]
                }))
                .unwrap(),
            );
        }
        wallets
    }

    #[test]
    fn pending_context_accepts_from_address_of_a_non_selected_account() {
        // `--from` resolved to acct-1 while acct-2 is selected: still verifiable.
        let pending = broadcast_pending();
        let wallets = wallets_with(&[("acct-1", "0xABC"), ("acct-2", "0xdef")], "acct-2");
        assert!(
            check_pending_context(&pending, Some(&wallets), "fp-1", pending.created_at).is_ok()
        );
    }

    #[test]
    fn pending_context_rejects_changed_wallet_and_stale_record() {
        let pending = broadcast_pending();
        let now = pending.created_at;
        let wallets = wallets_with(&[("acct-1", "0xabc")], "acct-1");
        assert!(check_pending_context(&pending, Some(&wallets), "fp-1", now).is_ok());

        for other in [
            wallets_with(&[("acct-2", "0xabc")], "acct-2"),
            wallets_with(&[("acct-1", "0x999")], "acct-1"),
        ] {
            let err = check_pending_context(&pending, Some(&other), "fp-1", now).unwrap_err();
            assert_eq!(
                err.downcast_ref::<CodedError>().unwrap().code,
                "approval_verification_context_changed"
            );
        }
        assert!(check_pending_context(&pending, None, "fp-1", now).is_err());

        let err =
            check_pending_context(&pending, Some(&wallets), "fp-1", now + PENDING_TTL_SECS + 1)
                .unwrap_err();
        assert_eq!(
            err.downcast_ref::<CodedError>().unwrap().code,
            "approval_verification_expired"
        );
    }

    #[test]
    fn pending_from_another_login_session_is_never_used() {
        let pending = broadcast_pending();
        let wallets = wallets_with(&[("acct-1", "0xabc")], "acct-1");
        let now = pending.created_at;
        for fingerprint in ["fp-2", ""] {
            let err =
                check_pending_context(&pending, Some(&wallets), fingerprint, now).unwrap_err();
            assert_eq!(coded(&err), "approval_verification_context_changed");
        }
        // A record saved without a fingerprint is rejected too.
        let mut legacy = pending.clone();
        legacy.session_fingerprint.clear();
        assert!(check_pending_context(&legacy, Some(&wallets), "", now).is_err());
    }

    #[test]
    fn take_if_same_consumes_the_record_exactly_once() {
        let store = MemStore::default();
        let pending = broadcast_pending();
        save_in(&store, &pending).unwrap();
        assert!(take_if_same(&store, &pending).unwrap());
        assert!(store.read().unwrap().is_none());
        // A second verifier (or a concurrent process) finds nothing to replay.
        assert!(!take_if_same(&store, &pending).unwrap());
    }

    #[test]
    fn take_if_same_keeps_a_replaced_record() {
        let store = MemStore::default();
        let old = broadcast_pending();
        let mut newer = old.clone();
        newer.created_at += 5;
        newer.request = Value::String("{\"new\":1}".to_string());
        save_in(&store, &newer).unwrap();
        assert!(!take_if_same(&store, &old).unwrap());
        assert_eq!(load_in(&store).unwrap().unwrap().request, newer.request);
    }

    #[test]
    fn take_if_same_fails_when_the_record_cannot_be_removed() {
        let store = MemStore {
            fail_remove: true,
            ..Default::default()
        };
        let pending = broadcast_pending();
        save_in(&store, &pending).unwrap();
        // Not consumed ⇒ the caller must not replay.
        assert!(take_if_same(&store, &pending).is_err());
        assert!(store.read().unwrap().is_some());
    }

    #[test]
    fn corrupt_record_is_removed_and_treated_as_absent() {
        let store = MemStore::default();
        store.write("not json").unwrap();
        assert!(load_in(&store).unwrap().is_none());
        assert!(store.read().unwrap().is_none());

        let failing = MemStore {
            fail_remove: true,
            ..Default::default()
        };
        failing.write("not json").unwrap();
        assert!(load_in(&failing).is_err());
    }

    #[test]
    fn pending_lock_is_exclusive_across_handles() {
        use fs2::FileExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PENDING_LOCK_FILE);
        let held = lock_file(&path).unwrap();
        let other = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        assert!(other.try_lock_exclusive().is_err());
        drop(held);
        assert!(other.try_lock_exclusive().is_ok());
    }

    #[test]
    fn replaced_pending_record_is_not_the_same_operation() {
        let a = broadcast_pending();
        assert!(is_same_pending(&a, &a.clone()));
        let mut other = a.clone();
        other.request = Value::String("{\"msgForSign\":{\"other\":1}}".to_string());
        assert!(!is_same_pending(&a, &other));
        let mut later = a.clone();
        later.created_at += 1;
        assert!(!is_same_pending(&a, &later));
    }

    #[test]
    fn code_sent_only_when_backend_says_need_double_confirmation() {
        let err = send_result_error(Ok(json!({ "needDoubleConfirmation": true })));
        let confirming = err.downcast_ref::<crate::output::CliConfirming>().unwrap();
        assert_eq!(confirming.scene.as_deref(), Some(SCENE_CODE_SENT));
        assert!(confirming.next.contains("approval-code verify"));

        // Array-shaped envelope data is accepted too.
        let err = send_result_error(Ok(json!([{ "needDoubleConfirmation": true }])));
        assert!(err.downcast_ref::<crate::output::CliConfirming>().is_some());
    }

    #[test]
    fn need_double_confirmation_false_is_not_a_pass() {
        let err = send_result_error(Ok(json!({ "needDoubleConfirmation": false })));
        assert!(err.downcast_ref::<crate::output::CliConfirming>().is_none());
        let coded = err.downcast_ref::<CodedError>().unwrap();
        assert_eq!(coded.code, "approval_verification_not_pending");
        assert!(coded.message.contains("not waiting for email verification"));
        assert!(!coded.message.contains("was sent"));
    }

    #[test]
    fn send_failure_does_not_claim_the_code_was_sent() {
        let err = send_result_error(Err(api_error("50011")));
        let coded = err.downcast_ref::<CodedError>().unwrap();
        assert_eq!(coded.code, "approval_code_send_failed");
        assert_eq!(coded.data.as_ref().unwrap()["backendCode"], "50011");

        let err = send_result_error(Err(anyhow::anyhow!(
            "failed to parse wallet API response as JSON (HTTP 404): <html>not found</html>"
        )));
        let coded = err.downcast_ref::<CodedError>().unwrap();
        assert_eq!(coded.code, "approval_code_send_failed");
        let data = coded.data.as_ref().unwrap();
        assert!(data.get("backendCode").is_none());
        assert!(data["reason"].as_str().unwrap().contains("HTTP 404"));

        let long = "x".repeat(1000);
        let err = send_result_error(Err(anyhow::anyhow!(long)));
        let reason = err
            .downcast_ref::<CodedError>()
            .unwrap()
            .data
            .as_ref()
            .unwrap()["reason"]
            .as_str()
            .unwrap()
            .chars()
            .count();
        assert_eq!(reason, 300);
    }

    fn coded(err: &anyhow::Error) -> &str {
        err.downcast_ref::<CodedError>().unwrap().code.as_str()
    }

    #[test]
    fn verify_errors_map_by_backend_code() {
        assert_eq!(
            coded(&verify_failed_error(api_error("40032"))),
            "approval_code_invalid"
        );
        assert_eq!(
            coded(&verify_failed_error(api_error("40033"))),
            "approval_code_expired"
        );
        assert_eq!(
            coded(&verify_failed_error(api_error("40034"))),
            "approval_code_attempts_exceeded"
        );
        assert_eq!(
            coded(&verify_failed_error(api_error("40035"))),
            "approval_verification_not_pending"
        );
        assert_eq!(
            coded(&verify_failed_error(api_error("50000"))),
            "approval_code_verify_failed"
        );
    }

    #[test]
    fn send_errors_map_by_backend_code() {
        assert_eq!(
            coded(&send_result_error(Err(api_error("40036")))),
            "approval_code_send_too_frequent"
        );
        assert_eq!(
            coded(&send_result_error(Err(api_error("40037")))),
            "approval_code_send_failed"
        );
        assert_eq!(
            coded(&send_result_error(Err(api_error("40035")))),
            "approval_verification_not_pending"
        );
    }

    #[test]
    fn replay_errors_after_verification_stop_explicitly() {
        let err = map_replay_error(api_error("81362"));
        assert_eq!(coded(&err), "approval_risk_warning_after_verification");
        let message = &err.downcast_ref::<CodedError>().unwrap().message;
        assert!(message.contains("Nothing was executed"));
        assert_eq!(
            coded(&map_replay_error(api_error("40030"))),
            "approval_verification_unavailable"
        );
        let other = map_replay_error(api_error("81363"));
        assert_eq!(other.downcast_ref::<ApiCodeError>().unwrap().code, "81363");
    }

    #[test]
    fn send_failure_message_carries_the_reason() {
        let err = send_result_error(Err(anyhow::anyhow!("HTTP 404 not found")));
        let c = err.downcast_ref::<CodedError>().unwrap();
        assert!(c.message.contains("Reason: HTTP 404 not found"));
    }

    #[test]
    fn only_verified_true_counts_as_verified() {
        assert!(verified(&json!({ "verified": true })));
        assert!(verified(&json!([{ "verified": true }])));
        assert!(!verified(&json!({ "verified": false })));
        assert!(!verified(&json!({ "verified": "true" })));
        assert!(!verified(&json!({})));
        assert!(!verified(&Value::Null));
    }

    #[test]
    fn verify_backend_rejection_keeps_backend_code_without_echoing_input() {
        let err = verify_failed_error(api_error("50120"));
        let coded = err.downcast_ref::<CodedError>().unwrap();
        assert_eq!(coded.code, "approval_code_verify_failed");
        assert_eq!(coded.data.as_ref().unwrap()["backendCode"], "50120");
    }
}
