//! OKX.AI trading hackathon — Trading ASP registration.
//!
//! Public API surface: `POST /priapi/v5/wallet/agentic/activity/registration`
//! (authenticated; requires wallet login).

use crate::client::ApiClient;
use crate::commands::sink::CodedError;
use crate::commands::Context;
use crate::output;
use crate::token_alias;
use crate::wallet_store;
use anyhow::{bail, Result};
use clap::Subcommand;
use serde_json::Value;

const PROJECT_HEADER: &str = "4d156bf0c61130f2692d097ecb68dbe4";

/// Fixed activity ID for the current OKX.AI trading hackathon — not user-configurable.
const ACTIVITY_ID: &str = "okx-marathon-0730";
/// Fixed chain index (X Layer) this hackathon registers on — not user-configurable.
const CHAIN_INDEX: &str = "196";
/// Solana's chain index. Its addresses are not EVM, so they can never stand in
/// for the X Layer address the registration submits.
const SOLANA_CHAIN_INDEX: &str = "501";

/// Accepted `--account-type` values, in the exact spelling both surfaces take.
const ACCOUNT_TYPES: [&str; 2] = ["web3", "cefi"];

/// Upper bound on `--agent-id`. Real ids are short; this only stops an
/// obviously-wrong value (a pasted paragraph, a truncated JSON blob) from being
/// submitted as a registration that cannot afterwards be listed or undone.
const MAX_AGENT_ID_LEN: usize = 128;

/// `errorCode` for a rejection the registration backend itself returned — the
/// ASP failed an eligibility check, and the accompanying message is the reason.
const CODE_REJECTED: &str = "hackathon_registration_rejected";
/// `errorCode` for a failure that never reached the registration logic
/// (connection error, timeout, 5xx, an HTML error page). NOT an eligibility
/// verdict, and must never be reported to the user as one.
const CODE_UNAVAILABLE: &str = "hackathon_service_unavailable";

#[derive(Subcommand)]
pub enum HackathonCommand {
    /// Enter one of the user's existing Trading ASPs in the OKX.AI trading hackathon
    /// (requires wallet login). Always registers on X Layer — the top-level --chain flag is ignored.
    Register {
        /// Agent ID of the existing Trading ASP to enter.
        #[arg(long)]
        agent_id: String,
        /// Account type: "web3" or "cefi".
        #[arg(long, value_parser = clap::builder::PossibleValuesParser::new(ACCOUNT_TYPES))]
        account_type: String,
        /// Wallet address; auto-resolved from the wallet's X Layer address when omitted (both web3 & cefi).
        #[arg(long)]
        address: Option<String>,
        /// CeFi user ID (required when --account-type=cefi).
        #[arg(long)]
        uid: Option<String>,
    },
}

/// Which account the ASP competes with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccountType {
    Web3,
    CeFi,
}

impl AccountType {
    /// Parse a caller-supplied account type.
    ///
    /// Case-sensitive on purpose: the CLI's clap `value_parser` is
    /// case-sensitive, so matching that here keeps the CLI and the MCP tool
    /// accepting exactly the same spellings.
    fn parse(raw: &str) -> Result<Self> {
        match raw {
            "web3" => Ok(Self::Web3),
            "cefi" => Ok(Self::CeFi),
            other => bail!(
                "invalid account type {other:?} — expected exactly \"web3\" or \"cefi\" (lowercase)"
            ),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Web3 => "web3",
            Self::CeFi => "cefi",
        }
    }
}

/// A validated registration, ready to submit.
///
/// Built only by `prepare_registration`, so anything holding one has already
/// passed account-type parsing, the uid rules, and address validation.
pub struct RegistrationRequest {
    agent_id: String,
    account_type: AccountType,
    address: String,
    uid: Option<String>,
}

/// Hand-written so the CeFi uid never reaches a log line or a panic message —
/// it is a user identifier, and `#[derive(Debug)]` would print it verbatim.
impl std::fmt::Debug for RegistrationRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegistrationRequest")
            .field("agent_id", &self.agent_id)
            .field("account_type", &self.account_type)
            .field("address", &self.address)
            .field("uid", &self.uid.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

pub async fn execute(_ctx: &Context, command: HackathonCommand) -> Result<()> {
    let result: Value = match command {
        HackathonCommand::Register {
            agent_id,
            account_type,
            address,
            uid,
        } => {
            let request = prepare_registration(agent_id, &account_type, address, uid)?;
            register(&request).await?
        }
    };
    output::success(result);
    Ok(())
}

/// Validate the caller's raw registration params (spec §1).
///
/// **MUST** be the only way a `RegistrationRequest` is built: the CLI arm and
/// the MCP tool both call this, so neither surface can drift into accepting
/// something the other rejects.
pub fn prepare_registration(
    agent_id: String,
    account_type: &str,
    address: Option<String>,
    uid: Option<String>,
) -> Result<RegistrationRequest> {
    // ① Reject an unknown account type first. The CLI is also guarded by clap's
    //    value_parser, but the MCP tool takes a free-form string — so the check
    //    has to live where both surfaces share it.
    let account_type = AccountType::parse(account_type)?;
    // ② The agent id is what actually gets registered, and there is no
    //    list/update/status surface to correct it afterwards.
    let agent_id = validate_agent_id(agent_id)?;
    // ③ uid must match the account type in both directions. The request body
    //    carries no account-type field, so uid presence is the ONLY signal the
    //    backend gets: a uid that silently goes missing (or arrives on a web3
    //    registration) would register the wrong account without any error.
    let uid = normalize_uid(uid);
    match (account_type, uid.is_some()) {
        (AccountType::CeFi, false) => bail!("--uid is required for CeFi account registration"),
        (AccountType::Web3, true) => bail!(
            "--uid is only valid with --account-type cefi — omit it, or register as cefi instead"
        ),
        _ => {}
    }
    // ④ Auto-resolve the wallet's X Layer (EVM) address when omitted
    //    (both web3 & cefi register on X Layer).
    let address = match address {
        Some(a) => a,
        None => resolve_registration_evm_address()?,
    };
    // ⑤ Validate the (resolved) address for the target chain.
    token_alias::validate_address_for_chain(CHAIN_INDEX, &address, "address")?;
    Ok(RegistrationRequest {
        agent_id,
        account_type,
        address,
        uid,
    })
}

/// Trim the caller's uid and treat a blank one as absent.
///
/// `Some("")` / `Some("   ")` must NOT count as "a uid was supplied": the
/// request body carries no account-type field, so a blank uid would submit a
/// CeFi registration with nothing identifying the CeFi account — and the caller
/// would still be told it succeeded.
fn normalize_uid(uid: Option<String>) -> Option<String> {
    uid.map(|u| u.trim().to_string()).filter(|u| !u.is_empty())
}

/// Reject an agent id that is blank, over-long, or carries control characters.
///
/// This is not an injection guard — the value is only ever JSON-encoded, never
/// interpolated into a URL or a shell. It stops a placeholder, an empty reply,
/// or a mangled paste from being submitted as a real, unrecoverable
/// registration.
fn validate_agent_id(agent_id: String) -> Result<String> {
    let trimmed = agent_id.trim();
    if trimmed.is_empty() {
        bail!(
            "--agent-id is required — pass the Trading ASP's agent id from `onchainos agent get-my-agents`"
        );
    }
    if trimmed.chars().any(char::is_control) {
        bail!(
            "--agent-id contains control characters — pass the plain agent id from `onchainos agent get-my-agents`"
        );
    }
    let len = trimmed.chars().count();
    if len > MAX_AGENT_ID_LEN {
        bail!(
            "--agent-id is too long ({len} chars, max {MAX_AGENT_ID_LEN}) — pass just the agent id"
        );
    }
    Ok(trimmed.to_string())
}

pub async fn register(request: &RegistrationRequest) -> Result<Value> {
    let body = build_registration_body(request);
    let path = "/priapi/v5/wallet/agentic/activity/registration";
    let headers = [("OK-ACCESS-PROJECT", PROJECT_HEADER)];
    let mut auth_client = ensure_logged_in_client().await?;
    auth_client
        .post_with_headers(path, &body, Some(&headers))
        .await
        .map_err(classify_submit_error)?;
    Ok(build_registration_confirmation(request))
}

/// MCP entry point: validate → submit → record an audit line.
///
/// The CLI gets its audit entry from `main.rs`, which returns before that call
/// for `Commands::Mcp`. Without this, a registration made through the tool —
/// an action with no list, status, or undo surface — would leave no local trace
/// at all.
pub async fn register_via_mcp(
    agent_id: String,
    account_type: &str,
    address: Option<String>,
    uid: Option<String>,
) -> Result<Value> {
    // Deliberately omits `--uid` (a user identifier the CLI path redacts out of
    // the log) and `--address` (derivable from the wallet, and noise here).
    let args = vec![
        "hackathon".to_string(),
        "register".to_string(),
        "--agent-id".to_string(),
        agent_id.clone(),
        "--account-type".to_string(),
        account_type.to_string(),
    ];
    let started = std::time::Instant::now();
    let result = match prepare_registration(agent_id, account_type, address, uid) {
        Ok(request) => register(&request).await,
        Err(e) => Err(e),
    };
    crate::audit::log(
        "mcp",
        "hackathon register",
        result.is_ok(),
        started.elapsed(),
        Some(args),
        result.as_ref().err().map(|e| format!("{e:#}")).as_deref(),
    );
    result
}

/// Split a backend business rejection from a transport / HTTP-layer failure.
///
/// Both reach the caller as a bare `{ok:false, error:"…"}`, so the skill was
/// left sorting them by reading the prose — which misfires on a timeout, a 5xx,
/// or an HTML error page and tells the user their ASP failed the
/// trading-type / subscription / trial checks when nothing was ever evaluated.
/// Tagging the two with distinct `errorCode`s makes that branch mechanical on
/// both surfaces (`output::error_coded` on the CLI, `err()` in the MCP server).
///
/// Anything not recognisably the backend's own envelope is classified as
/// unavailable: over-reporting a rejection as an outage is recoverable, the
/// reverse sends the user off to "fix" a perfectly valid ASP.
fn classify_submit_error(e: anyhow::Error) -> anyhow::Error {
    const API_ERROR: &str = "API error (code=";
    let rendered = format!("{e:#}");
    match rendered.find(API_ERROR) {
        Some(idx) => {
            let detail = rendered[idx..]
                .split_once("): ")
                .map(|(_, msg)| msg.trim())
                .filter(|msg| !msg.is_empty())
                .unwrap_or(rendered.as_str())
                .to_string();
            CodedError::new(CODE_REJECTED, None, detail).into()
        }
        None => CodedError::new(CODE_UNAVAILABLE, None, rendered).into(),
    }
}

fn build_registration_body(request: &RegistrationRequest) -> Value {
    let mut body = serde_json::json!({
        "activityId": ACTIVITY_ID,
        "agentId": request.agent_id,
        "chainIndex": CHAIN_INDEX,
        "address": request.address,
    });
    if let Some(uid) = uid_for(request) {
        body["uid"] = Value::String(uid.to_string());
    }
    body
}

/// Builds the CLI/MCP success payload.
///
/// Two fields are deliberately NOT echoed:
/// * `activityId` — an internal identifier the skill is forbidden to show the
///   user, so keeping it out of the output removes the leak vector entirely.
/// * `uid` — the CeFi user identifier. It is redacted from the audit log, so
///   printing it verbatim on stdout (and, per the repo's "show the executed
///   command" rule, into the chat transcript) would undo that. The caller
///   supplied it, so echoing it back conveys nothing.
///
/// Both omissions also trim the caller's context.
fn build_registration_confirmation(request: &RegistrationRequest) -> Value {
    serde_json::json!({
        "registered": true,
        "agentId": request.agent_id,
        "accountType": request.account_type.as_str(),
        "chainIndex": CHAIN_INDEX,
        "address": request.address,
    })
}

/// The uid to submit — `Some` only for a CeFi registration.
///
/// `prepare_registration` already rejects the mismatched combinations and blanks
/// out an empty uid, so this is a belt-and-braces guard on the one place the uid
/// is allowed to leave the process: the request body.
fn uid_for(request: &RegistrationRequest) -> Option<&str> {
    match request.account_type {
        AccountType::CeFi => request.uid.as_deref(),
        AccountType::Web3 => None,
    }
}

pub fn resolve_registration_evm_address() -> Result<String> {
    let account = selected_account_entry()?;
    pick_registration_address(&account.address_list)
        .ok_or_else(|| anyhow::anyhow!("could not find an EVM address in the selected account"))
}

/// Pick the address to register with: the account's X Layer (`CHAIN_INDEX`)
/// entry when it has one, otherwise any other EVM address it carries.
///
/// An EVM address is shared across EVM chains, so the fallback resolves to the
/// same string in practice — it only covers an account whose address list does
/// not enumerate X Layer. Preferring the X Layer entry keeps the submitted value
/// matching what the skill tells the user it submitted (the current wallet's
/// X Layer address), instead of "whichever EVM row happens to come first".
fn pick_registration_address(addresses: &[wallet_store::AddressInfo]) -> Option<String> {
    let is_evm = |a: &&wallet_store::AddressInfo| a.address.starts_with("0x");
    addresses
        .iter()
        .find(|a| a.chain_index == CHAIN_INDEX && is_evm(a))
        .or_else(|| {
            addresses
                .iter()
                .find(|a| a.chain_index != SOLANA_CHAIN_INDEX && is_evm(a))
        })
        .map(|a| a.address.clone())
}

/// Shared login-check + selected-account lookup for the address resolver.
fn selected_account_entry() -> Result<wallet_store::AccountMapEntry> {
    let wallets = wallet_store::load_wallets()?
        .ok_or_else(|| anyhow::anyhow!("not logged in — please run: onchainos wallet login"))?;
    if wallets.selected_account_id.is_empty() {
        bail!("not logged in — please run: onchainos wallet login");
    }
    wallets
        .accounts_map
        .get(&wallets.selected_account_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("selected account has no address list — please re-login"))
}

/// Pre-flight login check for the authenticated registration endpoint.
///
/// Long-lived MCP server clients are constructed once via `ApiClient::new()`
/// (sync) and cache the JWT they had at startup — that token may have expired
/// by the time `register` runs. To avoid sharing a stale token, we always
/// build a fresh `ApiClient::new_async()` here: it has the full JWT lifecycle
/// (expiry check + refresh + AK fallback) baked in.
async fn ensure_logged_in_client() -> Result<ApiClient> {
    match wallet_store::load_wallets() {
        Ok(Some(w)) if !w.selected_account_id.is_empty() => {}
        _ => bail!("not logged in — please run: onchainos wallet login"),
    }
    ApiClient::new_async(None).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const EVM_ADDR: &str = "0x1111111111111111111111111111111111111111";
    const SOL_ADDR: &str = "So11111111111111111111111111111111111111112";

    /// Build a request the way both surfaces do, with an explicit address so the
    /// helper never touches the wallet store.
    fn prepared(account_type: &str, uid: Option<&str>) -> Result<RegistrationRequest> {
        prepare_registration(
            "agent-1".to_string(),
            account_type,
            Some(EVM_ADDR.to_string()),
            uid.map(str::to_string),
        )
    }

    #[test]
    fn register_body_web3_omits_uid() {
        let body = build_registration_body(&prepared("web3", None).expect("valid web3 params"));
        assert_eq!(body["activityId"], "okx-marathon-0730");
        assert_eq!(body["agentId"], "agent-1");
        assert_eq!(body["chainIndex"], "196");
        assert_eq!(body["address"], EVM_ADDR);
        assert!(body.get("uid").is_none());
    }

    #[test]
    fn register_body_cefi_includes_uid() {
        let body =
            build_registration_body(&prepared("cefi", Some("uid-1")).expect("valid cefi params"));
        assert_eq!(body["uid"], "uid-1");
    }

    #[test]
    fn register_confirmation_web3_shape() {
        let confirmation =
            build_registration_confirmation(&prepared("web3", None).expect("valid web3 params"));
        assert_eq!(confirmation["registered"], true);
        assert_eq!(confirmation["agentId"], "agent-1");
        assert_eq!(confirmation["accountType"], "web3");
        assert_eq!(confirmation["chainIndex"], "196");
        assert_eq!(confirmation["address"], EVM_ADDR);
        assert!(confirmation.get("uid").is_none());
    }

    #[test]
    fn register_confirmation_never_echoes_activity_id() {
        // The internal activity id must stay out of the output on both account
        // types — the skill is forbidden to show it to the user.
        for (account_type, uid) in [("web3", None), ("cefi", Some("uid-1"))] {
            let request = prepared(account_type, uid).expect("valid params");
            let confirmation = build_registration_confirmation(&request);
            assert!(
                confirmation.get("activityId").is_none(),
                "{account_type} confirmation leaked activityId: {confirmation}"
            );
        }
        // ...while the request body still carries it.
        let body = build_registration_body(&prepared("web3", None).expect("valid web3 params"));
        assert_eq!(body["activityId"], ACTIVITY_ID);
    }

    #[test]
    fn register_confirmation_never_echoes_uid() {
        // The uid is redacted from the audit log; echoing it on stdout would put
        // it straight back into the terminal and the chat transcript. It is
        // caller-supplied, so returning it carries no information either.
        let request = prepared("cefi", Some("uid-1")).expect("valid cefi params");
        let confirmation = build_registration_confirmation(&request);
        assert!(
            confirmation.get("uid").is_none(),
            "confirmation leaked the CeFi uid: {confirmation}"
        );
        assert!(
            !confirmation.to_string().contains("uid-1"),
            "confirmation leaked the CeFi uid value: {confirmation}"
        );
        // ...while the request body still carries it — that is the whole point
        // of collecting it.
        assert_eq!(build_registration_body(&request)["uid"], "uid-1");
    }

    #[test]
    fn blank_uid_does_not_satisfy_the_cefi_requirement() {
        // `Some("")` used to pass the `uid.is_some()` check, submitting a CeFi
        // registration whose body carried `"uid": ""` — a success message for an
        // account that was never identified.
        for blank in ["", "   ", "\t", "\n"] {
            let err = prepared("cefi", Some(blank))
                .expect_err("a blank uid must not count as a supplied uid");
            assert!(
                err.to_string().contains("--uid is required"),
                "blank uid {blank:?} was accepted: {err}"
            );
        }
    }

    #[test]
    fn blank_uid_is_ignored_on_a_web3_registration() {
        // The mirror case: a blank uid is "not supplied", so it must not trip
        // the web3-rejects-uid rule either.
        let request = prepared("web3", Some("   ")).expect("a blank uid is simply absent");
        assert!(build_registration_body(&request).get("uid").is_none());
    }

    #[test]
    fn uid_is_trimmed_before_submission() {
        let body = build_registration_body(
            &prepared("cefi", Some("  1234567890  ")).expect("valid cefi params"),
        );
        assert_eq!(body["uid"], "1234567890");
    }

    #[test]
    fn prepare_rejects_unusable_agent_ids() {
        for (raw, needle) in [
            ("", "--agent-id is required"),
            ("   ", "--agent-id is required"),
            ("agent\n-1", "control characters"),
            (&"a".repeat(MAX_AGENT_ID_LEN + 1), "too long"),
        ] {
            let err =
                prepare_registration(raw.to_string(), "web3", Some(EVM_ADDR.to_string()), None)
                    .expect_err("an unusable agent id must be rejected");
            assert!(
                err.to_string().contains(needle),
                "agent id {raw:?} produced an unexpected error: {err}"
            );
        }
    }

    #[test]
    fn prepare_trims_the_agent_id() {
        let body = build_registration_body(
            &prepare_registration(
                "  agent-1  ".to_string(),
                "web3",
                Some(EVM_ADDR.to_string()),
                None,
            )
            .expect("valid params"),
        );
        assert_eq!(body["agentId"], "agent-1");
    }

    #[test]
    fn backend_envelope_errors_are_tagged_as_rejections() {
        let coded = classify_submit_error(anyhow::anyhow!(
            "API error (code=51000): ASP is not a trading-type service"
        ));
        let coded = coded
            .downcast_ref::<CodedError>()
            .expect("classified as a CodedError");
        assert_eq!(coded.code, CODE_REJECTED);
        // The backend's own wording reaches the caller unaltered — the CLI only
        // strips its `API error (code=…): ` envelope. The skill translates this
        // message for the user, so it must arrive complete and unparaphrased.
        assert_eq!(coded.message, "ASP is not a trading-type service");
    }

    #[test]
    fn transport_errors_are_never_tagged_as_rejections() {
        // A 5xx, a timeout, or an HTML error page means the eligibility rules
        // were never evaluated — reporting these as rejections is what sends a
        // user off to "fix" a valid ASP.
        for raw in [
            "Network unavailable — check your connection",
            "Server error (HTTP 5xx)",
            "HTTP 404 Not Found: <html>…</html>",
            "Rate limited — retry with backoff",
        ] {
            let coded = classify_submit_error(anyhow::anyhow!("{raw}"));
            let coded = coded
                .downcast_ref::<CodedError>()
                .expect("classified as a CodedError");
            assert_eq!(coded.code, CODE_UNAVAILABLE, "misclassified: {raw}");
        }
    }

    #[test]
    fn prepare_cefi_without_uid_errors() {
        let err = prepared("cefi", None).expect_err("cefi without uid must error");
        assert!(err.to_string().contains("--uid is required"));
    }

    #[test]
    fn prepare_web3_with_uid_errors() {
        // A uid on a web3 registration would be dropped from the body, silently
        // registering the wrong account — reject it instead.
        let err = prepared("web3", Some("uid-1")).expect_err("web3 with uid must error");
        assert!(err
            .to_string()
            .contains("only valid with --account-type cefi"));
    }

    #[test]
    fn prepare_web3_ok_without_uid() {
        assert!(prepared("web3", None).is_ok());
    }

    #[test]
    fn prepare_cefi_ok_with_uid() {
        assert!(prepared("cefi", Some("uid-1")).is_ok());
    }

    #[test]
    fn prepare_rejects_unknown_and_miscased_account_types() {
        // The MCP tool has no clap value_parser in front of it, so a wrongly
        // cased "CeFi" must be rejected here rather than falling through to a
        // web3 registration with the uid dropped.
        for raw in ["CeFi", "CEFI", "Web3", "binance", ""] {
            let err = prepare_registration(
                "agent-1".to_string(),
                raw,
                Some(EVM_ADDR.to_string()),
                Some("uid-1".to_string()),
            )
            .expect_err("only exact lowercase web3/cefi are accepted");
            assert!(
                err.to_string().contains("invalid account type"),
                "unexpected error for {raw:?}: {err}"
            );
        }
    }

    /// Minimal address row — only the two fields the picker reads are set.
    fn addr_row(chain_index: &str, address: &str) -> wallet_store::AddressInfo {
        wallet_store::AddressInfo {
            account_id: String::new(),
            address: address.to_string(),
            chain_index: chain_index.to_string(),
            chain_name: String::new(),
            address_type: String::new(),
            chain_path: String::new(),
        }
    }

    #[test]
    fn address_picker_prefers_the_x_layer_row() {
        // The skill tells the user it submitted their X Layer address, so the
        // X Layer row wins even when another EVM row is listed first.
        let rows = [
            addr_row(SOLANA_CHAIN_INDEX, SOL_ADDR),
            addr_row("1", "0x2222222222222222222222222222222222222222"),
            addr_row(CHAIN_INDEX, EVM_ADDR),
        ];
        assert_eq!(pick_registration_address(&rows).as_deref(), Some(EVM_ADDR));
    }

    #[test]
    fn address_picker_falls_back_to_another_evm_row() {
        // An account whose list does not enumerate X Layer still registers: EVM
        // addresses are shared across EVM chains.
        let rows = [
            addr_row(SOLANA_CHAIN_INDEX, SOL_ADDR),
            addr_row("1", EVM_ADDR),
        ];
        assert_eq!(pick_registration_address(&rows).as_deref(), Some(EVM_ADDR));
    }

    #[test]
    fn address_picker_never_returns_a_non_evm_address() {
        let solana_only = [addr_row(SOLANA_CHAIN_INDEX, SOL_ADDR)];
        assert!(pick_registration_address(&solana_only).is_none());
        assert!(pick_registration_address(&[]).is_none());
    }

    #[test]
    fn prepare_rejects_address_that_fails_chain_validation() {
        let err = prepare_registration(
            "agent-1".to_string(),
            "web3",
            Some("not-an-address".to_string()),
            None,
        )
        .expect_err("a non-EVM address must fail chain validation");
        assert!(!err.to_string().is_empty());
    }
}
