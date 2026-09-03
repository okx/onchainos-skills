//! OKX.AI A2MCP prepared-payment state.
//!
//! This is deliberately separate from [`super::state::PaymentState`]. Generic
//! quote/pay JSON remains byte-compatible; the only dispatch signal is the
//! trusted `source` field read from the local state file.

use std::collections::BTreeMap;
use std::fs;

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use url::Url;
use uuid::Uuid;

use super::state::{self, ParamCarrier, ParamSpec};

pub const A2MCP_INTENT_VERSION: u32 = 1;
pub const A2MCP_SOURCE: &str = "okx_ai_a2mcp";
pub const ERR_CONFIRMATION_REQUIRED: &str = "a2mcp_payment_confirmation_required";
pub const ERR_INSUFFICIENT_BALANCE: &str = "a2mcp_insufficient_balance";
pub const ERR_ALREADY_CREATED: &str = "a2mcp_payment_intent_already_created";
pub const ERR_ALREADY_EXECUTED: &str = "a2mcp_payment_already_executed";
pub const ERR_EXPIRED: &str = "a2mcp_payment_intent_expired";
pub const ERR_INVALID_INTENT: &str = "a2mcp_invalid_payment_intent";
pub const ERR_INVALID_PARAMS: &str = "a2mcp_invalid_typed_params";
pub const ERR_OVERRIDES_FORBIDDEN: &str = "a2mcp_payment_overrides_forbidden";
pub const ERR_PREPARED_EXPIRED_OR_MISSING: &str = "a2mcp_prepared_expired_or_missing";

const A2MCP_PREPARED_SOURCE: &str = "okx_ai_a2mcp_prepared";
const A2MCP_PREPARED_ID_PREFIX: &str = "a2prep_";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum A2mcpPaymentSource {
    GenericQuote,
    OkxAiA2mcp,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct A2mcpFrozenRequestV1 {
    endpoint: String,
    method: String,
    typed_params: Map<String, Value>,
    param_plan: Vec<ParamSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resource: Option<Value>,
}

impl A2mcpFrozenRequestV1 {
    pub fn new(
        endpoint: String,
        method: String,
        typed_params: Map<String, Value>,
        param_plan: Vec<ParamSpec>,
        resource: Option<Value>,
    ) -> Result<Self> {
        if endpoint.trim().is_empty() || method.trim().is_empty() {
            bail!("{ERR_INVALID_PARAMS}: endpoint and method are required");
        }
        let method = method.trim().to_ascii_uppercase();
        if !matches!(method.as_str(), "GET" | "POST") {
            bail!("{ERR_INVALID_PARAMS}: A2MCP request method must be GET or POST");
        }
        let parsed_endpoint = Url::parse(&endpoint)
            .map_err(|error| anyhow!("{ERR_INVALID_PARAMS}: invalid Endpoint URL: {error}"))?;
        if parsed_endpoint.scheme() != "https" {
            bail!("{ERR_INVALID_PARAMS}: Endpoint must use HTTPS");
        }
        for spec in &param_plan {
            if matches!(spec.carrier, ParamCarrier::Header)
                && matches!(
                    spec.name.to_ascii_lowercase().as_str(),
                    "payment-signature"
                        | "payment-required"
                        | "www-authenticate"
                        | "authorization"
                        | "proxy-authorization"
                        | "host"
                        | "content-length"
                        | "transfer-encoding"
                        | "connection"
                )
            {
                bail!(
                    "{ERR_INVALID_PARAMS}: reserved header parameter '{}'",
                    spec.name
                );
            }
            if let Some(value) = typed_params.get(&spec.name) {
                if !matches!(spec.carrier, ParamCarrier::Body) && !is_scalar(value) {
                    bail!(
                        "{ERR_INVALID_PARAMS}: non-body parameter '{}' must be scalar",
                        spec.name
                    );
                }
            }
        }
        Ok(Self {
            endpoint,
            method,
            typed_params,
            param_plan,
            resource,
        })
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
    pub fn method(&self) -> &str {
        &self.method
    }
    pub fn typed_params(&self) -> &Map<String, Value> {
        &self.typed_params
    }
    pub fn param_plan(&self) -> &[ParamSpec] {
        &self.param_plan
    }
    pub fn resource(&self) -> Option<&Value> {
        self.resource.as_ref()
    }
}

fn is_scalar(value: &Value) -> bool {
    matches!(
        value,
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)
    )
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct A2mcpPreparedCandidate {
    candidate_id: String,
    raw_accept: Value,
    symbol: String,
    network: String,
    chain_id: String,
    chain_name: String,
    is_mainnet: bool,
    scheme: String,
    amount_atomic: String,
    amount_display: String,
    decimals: u32,
    authorization_type: String,
    balance_status: String,
    available_amount: String,
    required_amount: String,
    shortfall: String,
    deposit_address: String,
}

impl A2mcpPreparedCandidate {
    pub fn candidate_id(&self) -> &str {
        &self.candidate_id
    }
    pub fn raw_accept(&self) -> &Value {
        &self.raw_accept
    }
    pub fn symbol(&self) -> &str {
        &self.symbol
    }
    pub fn network(&self) -> &str {
        &self.network
    }
    pub fn chain_id(&self) -> &str {
        &self.chain_id
    }
    pub fn chain_name(&self) -> &str {
        &self.chain_name
    }
    pub fn is_mainnet(&self) -> bool {
        self.is_mainnet
    }
    pub fn scheme(&self) -> &str {
        &self.scheme
    }
    pub fn amount_atomic(&self) -> &str {
        &self.amount_atomic
    }
    pub fn amount_display(&self) -> &str {
        &self.amount_display
    }
    pub fn decimals(&self) -> u32 {
        self.decimals
    }
    pub fn authorization_type(&self) -> &str {
        &self.authorization_type
    }
    pub fn balance_status(&self) -> &str {
        &self.balance_status
    }
    pub fn available_amount(&self) -> &str {
        &self.available_amount
    }
    pub fn required_amount(&self) -> &str {
        &self.required_amount
    }
    pub fn shortfall(&self) -> &str {
        &self.shortfall
    }
    pub fn deposit_address(&self) -> &str {
        &self.deposit_address
    }

    #[cfg(test)]
    pub(super) fn new_for_test(
        candidate_id: String,
        raw_accept: Value,
        symbol: String,
        decimals: u32,
        authorization_type: String,
        balance_status: String,
    ) -> Self {
        let network = raw_accept
            .get("network")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let scheme = raw_accept
            .get("scheme")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let amount_atomic = super::payment_flow::extract_amount(&raw_accept).unwrap_or_default();
        Self {
            candidate_id,
            raw_accept,
            symbol,
            network,
            chain_id: "196".into(),
            chain_name: "X Layer".into(),
            is_mainnet: true,
            scheme,
            amount_atomic: amount_atomic.clone(),
            amount_display: amount_atomic.clone(),
            decimals,
            authorization_type,
            balance_status,
            available_amount: amount_atomic.clone(),
            required_amount: amount_atomic,
            shortfall: "0".into(),
            deposit_address: String::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct A2mcpSelectedAcceptV1 {
    raw: Value,
    network: String,
    asset: String,
    symbol: String,
    decimals: u32,
    scheme: String,
    authorization_type: String,
    amount: String,
    pay_to: String,
    balance_status: String,
}

impl A2mcpSelectedAcceptV1 {
    pub fn try_from_prepared_candidate(candidate: A2mcpPreparedCandidate) -> Result<Self> {
        let symbol = candidate.symbol.to_ascii_uppercase();
        if !matches!(symbol.as_str(), "USDT" | "USDC" | "USDG") {
            bail!("{ERR_INVALID_INTENT}: unsupported payment asset");
        }
        let raw = candidate.raw_accept;
        let scheme = required_str(&raw, "scheme")?.to_ascii_lowercase();
        let authorization_type = candidate.authorization_type.to_ascii_lowercase();
        let classified_authorization = classify_authorization(&raw)
            .ok_or_else(|| anyhow!("{ERR_INVALID_INTENT}: unsupported scheme/authorization"))?;
        if authorization_type != classified_authorization {
            bail!("{ERR_INVALID_INTENT}: authorization disagrees with raw entry");
        }
        let supported = matches!(
            (scheme.as_str(), authorization_type.as_str()),
            ("exact", "eip3009")
                | ("exact", "permit2")
                | ("upto", "permit2")
                | ("aggr_deferred", "session")
        );
        if !supported {
            bail!("{ERR_INVALID_INTENT}: unsupported scheme/authorization");
        }
        let network = required_str(&raw, "network")?.to_string();
        let asset = required_str(&raw, "asset")?.to_string();
        let amount = super::payment_flow::extract_amount(&raw)?;
        if amount.is_empty() {
            bail!("{ERR_INVALID_INTENT}: missing amount");
        }
        let pay_to = required_str(&raw, "payTo")?.to_string();
        Ok(Self {
            raw,
            network,
            asset,
            symbol,
            decimals: candidate.decimals,
            scheme,
            authorization_type,
            amount,
            pay_to,
            balance_status: candidate.balance_status,
        })
    }

    #[cfg(test)]
    pub(super) fn from_prepared_candidate(candidate: A2mcpPreparedCandidate) -> Self {
        Self::try_from_prepared_candidate(candidate).unwrap()
    }

    pub fn raw(&self) -> &Value {
        &self.raw
    }
    pub fn scheme(&self) -> &str {
        &self.scheme
    }
    pub fn symbol(&self) -> &str {
        &self.symbol
    }
    pub fn balance_status(&self) -> &str {
        &self.balance_status
    }
}

fn required_str<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("{ERR_INVALID_INTENT}: missing {key}"))
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum A2mcpExecutionState {
    Prepared,
    Signing,
    ProofGenerated,
    Replaying,
    Success,
    PendingTerminal,
    FailedTerminal,
    Expired,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct A2mcpExecutionV1 {
    state: A2mcpExecutionState,
    signature_attempts: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct A2mcpPaymentIntentV1 {
    version: u32,
    source: String,
    payment_id: String,
    probe_id: String,
    owner_account_id: String,
    payer_address: String,
    frozen_request: A2mcpFrozenRequestV1,
    selected_accept: A2mcpSelectedAcceptV1,
    execution: A2mcpExecutionV1,
    created_at: u64,
    expires_at: u64,
}

impl A2mcpPaymentIntentV1 {
    pub fn source(&self) -> A2mcpPaymentSource {
        A2mcpPaymentSource::OkxAiA2mcp
    }
    pub fn payment_id(&self) -> &str {
        &self.payment_id
    }
    pub fn owner_account_id(&self) -> &str {
        &self.owner_account_id
    }
    pub fn payer_address(&self) -> &str {
        &self.payer_address
    }
    pub fn frozen_request(&self) -> &A2mcpFrozenRequestV1 {
        &self.frozen_request
    }
    pub fn selected_accept(&self) -> &A2mcpSelectedAcceptV1 {
        &self.selected_accept
    }
    pub fn execution_state(&self) -> A2mcpExecutionState {
        self.execution.state
    }
    pub fn signature_attempts(&self) -> u8 {
        self.execution.signature_attempts
    }

    pub fn begin_signing(&mut self, now: u64) -> Result<()> {
        if now >= self.expires_at {
            self.execution.state = A2mcpExecutionState::Expired;
            self.write()?;
            bail!("{ERR_EXPIRED}: {}", self.payment_id);
        }
        if self.execution.state != A2mcpExecutionState::Prepared {
            bail!("{ERR_ALREADY_EXECUTED}: {}", self.payment_id);
        }
        self.execution.state = A2mcpExecutionState::Signing;
        self.write()
    }

    pub fn record_signature_attempt(&mut self) -> Result<()> {
        if self.execution.state != A2mcpExecutionState::Signing
            || self.execution.signature_attempts >= 3
        {
            bail!("{ERR_ALREADY_EXECUTED}: invalid signature attempt state");
        }
        self.execution.signature_attempts += 1;
        self.write()
    }

    pub fn mark_proof_generated(&mut self) -> Result<()> {
        if self.execution.state != A2mcpExecutionState::Signing {
            bail!("{ERR_ALREADY_EXECUTED}: proof generated outside signing");
        }
        self.execution.state = A2mcpExecutionState::ProofGenerated;
        self.write()
    }

    pub fn mark_replaying(&mut self) -> Result<()> {
        if self.execution.state != A2mcpExecutionState::ProofGenerated {
            bail!("{ERR_ALREADY_EXECUTED}: replay outside proof_generated");
        }
        self.execution.state = A2mcpExecutionState::Replaying;
        self.write()
    }

    pub fn mark_success(&mut self) -> Result<()> {
        self.mark_terminal(A2mcpExecutionState::Success)
    }
    pub fn mark_pending_terminal(&mut self) -> Result<()> {
        self.mark_terminal(A2mcpExecutionState::PendingTerminal)
    }
    pub fn mark_failed_terminal(&mut self) -> Result<()> {
        self.mark_terminal(A2mcpExecutionState::FailedTerminal)
    }

    fn mark_terminal(&mut self, state: A2mcpExecutionState) -> Result<()> {
        if !matches!(
            self.execution.state,
            A2mcpExecutionState::Signing
                | A2mcpExecutionState::ProofGenerated
                | A2mcpExecutionState::Replaying
        ) {
            bail!("{ERR_ALREADY_EXECUTED}: terminal transition from invalid state");
        }
        self.execution.state = state;
        self.write()
    }

    fn validate(&self) -> Result<()> {
        if self.version != A2MCP_INTENT_VERSION || self.source != A2MCP_SOURCE {
            bail!("{ERR_INVALID_INTENT}: unsupported source or version");
        }
        if self.payment_id.is_empty()
            || self.probe_id.is_empty()
            || self.owner_account_id.is_empty()
            || self.payer_address.is_empty()
        {
            bail!("{ERR_INVALID_INTENT}: missing identity field");
        }
        if self.created_at >= self.expires_at || self.execution.signature_attempts > 3 {
            bail!("{ERR_INVALID_INTENT}: invalid lifetime or signature attempts");
        }
        let rebuilt_request = A2mcpFrozenRequestV1::new(
            self.frozen_request.endpoint.clone(),
            self.frozen_request.method.clone(),
            self.frozen_request.typed_params.clone(),
            self.frozen_request.param_plan.clone(),
            self.frozen_request.resource.clone(),
        )?;
        if rebuilt_request != self.frozen_request {
            bail!("{ERR_INVALID_INTENT}: frozen request is inconsistent");
        }
        // Re-run allowlist validation over the frozen wire entry on every load.
        let candidate = A2mcpPreparedCandidate {
            candidate_id: "persisted".into(),
            raw_accept: self.selected_accept.raw.clone(),
            symbol: self.selected_accept.symbol.clone(),
            network: self.selected_accept.network.clone(),
            chain_id: String::new(),
            chain_name: String::new(),
            is_mainnet: true,
            scheme: self.selected_accept.scheme.clone(),
            amount_atomic: self.selected_accept.amount.clone(),
            amount_display: String::new(),
            decimals: self.selected_accept.decimals,
            authorization_type: self.selected_accept.authorization_type.clone(),
            balance_status: self.selected_accept.balance_status.clone(),
            available_amount: String::new(),
            required_amount: String::new(),
            shortfall: String::new(),
            deposit_address: String::new(),
        };
        let checked = A2mcpSelectedAcceptV1::try_from_prepared_candidate(candidate)?;
        if checked != self.selected_accept {
            bail!("{ERR_INVALID_INTENT}: selected accept fields disagree with raw entry");
        }
        Ok(())
    }

    fn write(&self) -> Result<()> {
        self.validate()?;
        let body = serde_json::to_vec_pretty(self).context("serialize A2MCP payment intent")?;
        crate::home::atomic_write(&state::state_path(&self.payment_id)?, &body, true)
            .context("write A2MCP payment intent")
    }
}

#[derive(Clone)]
pub struct A2mcpIntentCreateInput {
    pub probe_id: String,
    pub owner_account_id: String,
    pub payer_address: String,
    pub frozen_request: A2mcpFrozenRequestV1,
    pub selected_accept: A2mcpSelectedAcceptV1,
    pub created_at: u64,
    /// Unix expiry from the challenge, or `0` when the challenge omitted it.
    pub expires_at: u64,
    pub user_confirmed: bool,
}

pub fn create_a2mcp_payment_intent(input: A2mcpIntentCreateInput) -> Result<A2mcpPaymentIntentV1> {
    if !input.user_confirmed {
        bail!("{ERR_CONFIRMATION_REQUIRED}: explicit confirmation is required");
    }
    if input.selected_accept.balance_status != "sufficient" {
        bail!("{ERR_INSUFFICIENT_BALANCE}: selected token balance is not sufficient");
    }
    let expires_at = compute_expires_at(input.expires_at, input.created_at)?;
    let payment_id = payment_id_for_probe(&input.probe_id, &input.owner_account_id);
    let path = state::state_path(&payment_id)?;
    if path.exists() {
        bail!("{ERR_ALREADY_CREATED}: {}", input.probe_id);
    }
    let intent = A2mcpPaymentIntentV1 {
        version: A2MCP_INTENT_VERSION,
        source: A2MCP_SOURCE.to_string(),
        payment_id,
        probe_id: input.probe_id,
        owner_account_id: input.owner_account_id,
        payer_address: input.payer_address,
        frozen_request: input.frozen_request,
        selected_accept: input.selected_accept,
        execution: A2mcpExecutionV1 {
            state: A2mcpExecutionState::Prepared,
            signature_attempts: 0,
        },
        created_at: input.created_at,
        expires_at,
    };
    intent.write()?;
    Ok(intent)
}

fn compute_expires_at(challenge_expires_at: u64, created_at: u64) -> Result<u64> {
    if challenge_expires_at != 0 && challenge_expires_at <= created_at {
        bail!("{ERR_EXPIRED}: challenge expired");
    }
    let local_expiry = created_at.saturating_add(state::MAX_QUOTE_TTL_SECS);
    Ok(if challenge_expires_at == 0 {
        local_expiry
    } else {
        challenge_expires_at.min(local_expiry)
    })
}

fn payment_id_for_probe(probe_id: &str, owner_account_id: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(A2MCP_SOURCE.as_bytes());
    hash.update([0]);
    hash.update(probe_id.as_bytes());
    hash.update([0]);
    hash.update(owner_account_id.as_bytes());
    let hex = hex::encode(hash.finalize());
    format!("pay_{}", &hex[..24])
}

pub fn inspect_payment_source(payment_id: &str) -> Result<A2mcpPaymentSource> {
    validate_payment_id(payment_id)?;
    let path = state::state_path(payment_id)?;
    let bytes = fs::read(&path)
        .with_context(|| format!("{}: {payment_id}", state::TOKEN_QUOTE_EXPIRED_OR_MISSING))?;
    let value: Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("{}: {payment_id}", state::TOKEN_QUOTE_EXPIRED_OR_MISSING))?;
    match value.get("source").and_then(Value::as_str) {
        None => Ok(A2mcpPaymentSource::GenericQuote),
        Some(A2MCP_SOURCE) => Ok(A2mcpPaymentSource::OkxAiA2mcp),
        Some(_) => bail!("{ERR_INVALID_INTENT}: unknown payment source"),
    }
}

pub fn read_a2mcp_payment_intent(
    payment_id: &str,
    current_owner_account_id: &str,
    now: u64,
) -> Result<A2mcpPaymentIntentV1> {
    if inspect_payment_source(payment_id)? != A2mcpPaymentSource::OkxAiA2mcp {
        bail!("{ERR_INVALID_INTENT}: payment state is not an A2MCP intent");
    }
    let bytes = fs::read(state::state_path(payment_id)?)?;
    let mut intent: A2mcpPaymentIntentV1 = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow!("{ERR_INVALID_INTENT}: malformed intent"))?;
    intent.validate()?;
    if intent.owner_account_id != current_owner_account_id {
        bail!("{}: {payment_id}", state::TOKEN_CROSS_USER);
    }
    if now >= intent.expires_at {
        intent.execution.state = A2mcpExecutionState::Expired;
        intent.write()?;
        bail!("{ERR_EXPIRED}: {payment_id}");
    }
    Ok(intent)
}

fn validate_payment_id(payment_id: &str) -> Result<()> {
    if payment_id.is_empty()
        || payment_id.len() > 128
        || !payment_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        bail!("{ERR_INVALID_INTENT}: invalid payment id");
    }
    Ok(())
}

/// A payment-side preparation result. Candidates are branded values created
/// only by decoding the supplied challenge; callers cannot construct one in
/// production code because all fields are private.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct A2mcpPreparedPayment {
    version: u32,
    source: String,
    frozen_request: A2mcpFrozenRequestV1,
    candidates: Vec<A2mcpPreparedCandidate>,
    challenge_expires_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    wallet_error: Option<String>,
}

impl A2mcpPreparedPayment {
    pub fn frozen_request(&self) -> &A2mcpFrozenRequestV1 {
        &self.frozen_request
    }
    pub fn candidates(&self) -> &[A2mcpPreparedCandidate] {
        &self.candidates
    }
    pub fn challenge_expires_at(&self) -> u64 {
        self.challenge_expires_at
    }
    pub fn wallet_error(&self) -> Option<&str> {
        self.wallet_error.as_deref()
    }
    pub fn select(&self, candidate_id: &str) -> Result<A2mcpSelectedAcceptV1> {
        self.validate()?;
        let candidate = self
            .candidates
            .iter()
            .find(|candidate| candidate.candidate_id == candidate_id)
            .cloned()
            .ok_or_else(|| anyhow!("{ERR_INVALID_INTENT}: unknown candidate"))?;
        A2mcpSelectedAcceptV1::try_from_prepared_candidate(candidate)
    }

    fn validate(&self) -> Result<()> {
        if self.version != A2MCP_INTENT_VERSION || self.source != A2MCP_SOURCE {
            bail!("{ERR_INVALID_INTENT}: invalid prepared payload source or version");
        }
        let rebuilt = A2mcpFrozenRequestV1::new(
            self.frozen_request.endpoint.clone(),
            self.frozen_request.method.clone(),
            self.frozen_request.typed_params.clone(),
            self.frozen_request.param_plan.clone(),
            self.frozen_request.resource.clone(),
        )?;
        if rebuilt != self.frozen_request {
            bail!("{ERR_INVALID_INTENT}: frozen request is inconsistent");
        }
        if self.candidates.is_empty() {
            bail!("{ERR_INVALID_INTENT}: prepared payload has no candidates");
        }
        for candidate in &self.candidates {
            if candidate.candidate_id.is_empty()
                || !matches!(
                    candidate.balance_status.as_str(),
                    "sufficient" | "insufficient" | "unavailable"
                )
            {
                bail!("{ERR_INVALID_INTENT}: invalid prepared candidate");
            }
            let selected = A2mcpSelectedAcceptV1::try_from_prepared_candidate(candidate.clone())?;
            if selected.network != candidate.network
                || selected.scheme != candidate.scheme
                || selected.amount != candidate.amount_atomic
            {
                bail!("{ERR_INVALID_INTENT}: candidate display fields disagree with raw entry");
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct A2mcpPreparedStateV1 {
    version: u32,
    source: String,
    prepared_id: String,
    owner_account_id: String,
    created_at: u64,
    expires_at: u64,
    prepared: A2mcpPreparedPayment,
}

impl A2mcpPreparedStateV1 {
    fn validate(&self, prepared_id: &str, owner_account_id: &str, now: u64) -> Result<()> {
        if self.version != A2MCP_INTENT_VERSION
            || self.source != A2MCP_PREPARED_SOURCE
            || self.prepared_id != prepared_id
        {
            bail!("{ERR_PREPARED_EXPIRED_OR_MISSING}: {prepared_id}");
        }
        if self.owner_account_id != owner_account_id {
            bail!("{}: {prepared_id}", state::TOKEN_CROSS_USER);
        }
        if now >= self.expires_at {
            bail!("{ERR_PREPARED_EXPIRED_OR_MISSING}: {prepared_id}");
        }
        self.prepared.validate()
    }
}

fn validate_prepared_id(prepared_id: &str) -> Result<()> {
    let suffix = prepared_id
        .strip_prefix(A2MCP_PREPARED_ID_PREFIX)
        .ok_or_else(|| anyhow!("{ERR_PREPARED_EXPIRED_OR_MISSING}: {prepared_id}"))?;
    if suffix.len() != 32 || !suffix.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("{ERR_PREPARED_EXPIRED_OR_MISSING}: {prepared_id}");
    }
    Ok(())
}

fn read_prepared_state(
    path: &std::path::Path,
    prepared_id: &str,
    owner_account_id: &str,
    now: u64,
) -> Result<A2mcpPreparedStateV1> {
    let bytes =
        fs::read(path).map_err(|_| anyhow!("{ERR_PREPARED_EXPIRED_OR_MISSING}: {prepared_id}"))?;
    let state: A2mcpPreparedStateV1 = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow!("{ERR_PREPARED_EXPIRED_OR_MISSING}: {prepared_id}"))?;
    state.validate(prepared_id, owner_account_id, now)?;
    Ok(state)
}

pub fn store_a2mcp_prepared_payment(
    prepared: A2mcpPreparedPayment,
    owner_account_id: &str,
    created_at: u64,
) -> Result<String> {
    if owner_account_id.is_empty() {
        bail!("wallet_login_required: no selected wallet");
    }
    prepared.validate()?;
    let expires_at = compute_expires_at(prepared.challenge_expires_at, created_at)?;
    write_a2mcp_prepared_state(prepared, owner_account_id, created_at, expires_at)
}

fn write_a2mcp_prepared_state(
    prepared: A2mcpPreparedPayment,
    owner_account_id: &str,
    created_at: u64,
    expires_at: u64,
) -> Result<String> {
    let prepared_id = format!("{A2MCP_PREPARED_ID_PREFIX}{}", Uuid::new_v4().simple());
    let state = A2mcpPreparedStateV1 {
        version: A2MCP_INTENT_VERSION,
        source: A2MCP_PREPARED_SOURCE.to_string(),
        prepared_id: prepared_id.clone(),
        owner_account_id: owner_account_id.to_string(),
        created_at,
        expires_at,
        prepared,
    };
    let body = serde_json::to_vec_pretty(&state).context("serialize A2MCP prepared state")?;
    crate::home::atomic_write(&state::state_path(&prepared_id)?, &body, true)
        .context("write A2MCP prepared state")?;
    Ok(prepared_id)
}

pub fn load_a2mcp_prepared_payment(
    prepared_id: &str,
    owner_account_id: &str,
    now: u64,
) -> Result<A2mcpPreparedPayment> {
    validate_prepared_id(prepared_id)?;
    let path = state::state_path(prepared_id)?;
    match read_prepared_state(&path, prepared_id, owner_account_id, now) {
        Ok(state) => Ok(state.prepared),
        Err(error) => {
            if error
                .to_string()
                .starts_with(ERR_PREPARED_EXPIRED_OR_MISSING)
            {
                let _ = fs::remove_file(path);
            }
            Err(error)
        }
    }
}

pub fn replace_a2mcp_prepared_payment(
    prepared_id: &str,
    prepared: A2mcpPreparedPayment,
    owner_account_id: &str,
    now: u64,
) -> Result<String> {
    prepared.validate()?;
    let claim = claim_a2mcp_prepared_payment(prepared_id, owner_account_id, now)?;
    let replacement = write_a2mcp_prepared_state(
        prepared,
        owner_account_id,
        claim.state.created_at,
        claim.state.expires_at,
    )?;
    claim.commit();
    Ok(replacement)
}

pub struct A2mcpPreparedClaim {
    canonical_path: std::path::PathBuf,
    claim_path: std::path::PathBuf,
    state: A2mcpPreparedStateV1,
    finalized: bool,
}

impl A2mcpPreparedClaim {
    pub fn prepared(&self) -> &A2mcpPreparedPayment {
        &self.state.prepared
    }

    /// Commit one-time consumption. Failure to remove an inaccessible claim
    /// file must never restore a state after its Payment Intent was created.
    pub fn commit(mut self) {
        let _ = fs::remove_file(&self.claim_path);
        self.finalized = true;
    }
}

impl Drop for A2mcpPreparedClaim {
    fn drop(&mut self) {
        if !self.finalized {
            let _ = fs::rename(&self.claim_path, &self.canonical_path);
        }
    }
}

pub fn claim_a2mcp_prepared_payment(
    prepared_id: &str,
    owner_account_id: &str,
    now: u64,
) -> Result<A2mcpPreparedClaim> {
    validate_prepared_id(prepared_id)?;
    let canonical_path = state::state_path(prepared_id)?;
    // Validate ownership before claiming so another account cannot destroy it.
    read_prepared_state(&canonical_path, prepared_id, owner_account_id, now).inspect_err(
        |error| {
            if error
                .to_string()
                .starts_with(ERR_PREPARED_EXPIRED_OR_MISSING)
            {
                let _ = fs::remove_file(&canonical_path);
            }
        },
    )?;
    let claim_path =
        canonical_path.with_file_name(format!(".{prepared_id}.claim-{}", Uuid::new_v4().simple()));
    fs::rename(&canonical_path, &claim_path)
        .map_err(|_| anyhow!("{ERR_PREPARED_EXPIRED_OR_MISSING}: {prepared_id}"))?;
    let state = match read_prepared_state(&claim_path, prepared_id, owner_account_id, now) {
        Ok(state) => state,
        Err(error) => {
            let _ = fs::rename(&claim_path, &canonical_path);
            return Err(error);
        }
    };
    Ok(A2mcpPreparedClaim {
        canonical_path,
        claim_path,
        state,
        finalized: false,
    })
}

pub fn consume_a2mcp_prepared_payment(
    prepared_id: &str,
    owner_account_id: &str,
    now: u64,
) -> Result<A2mcpPreparedPayment> {
    let claim = claim_a2mcp_prepared_payment(prepared_id, owner_account_id, now)?;
    let prepared = claim.prepared().clone();
    claim.commit();
    Ok(prepared)
}

#[derive(Clone)]
pub struct A2mcpPreparedChallengeInput {
    pub challenge: String,
    pub frozen_request: A2mcpFrozenRequestV1,
}

/// Decode a captured challenge, apply the OKX.AI A2MCP asset/scheme policy and
/// query balances without writing generic `PaymentState`.
pub async fn prepare_a2mcp_payment_from_challenge(
    input: A2mcpPreparedChallengeInput,
) -> Result<A2mcpPreparedPayment> {
    let decoded = super::dispatcher::decode_payment_blob(&input.challenge)?;
    let raw_accepts = decoded
        .get("accepts")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| anyhow!("a2mcp_unsupported_payment_asset: challenge has no accepts"))?;
    let (prepared, wallet_error) = super::quote::prepare_a2mcp_candidates(&raw_accepts).await?;
    let by_token = brand_candidates(prepared, &raw_accepts)?;
    if by_token.is_empty() {
        bail!("a2mcp_unsupported_payment_asset: no supported token/scheme candidate");
    }
    let mut frozen_request = input.frozen_request;
    // The challenge is authoritative for payment header binding; never retain
    // a caller-supplied resource after the Endpoint response is available.
    frozen_request.resource = decoded.get("resource").cloned();
    let candidate_expiry = by_token
        .values()
        .filter_map(|candidate| {
            candidate
                .raw_accept
                .get("expires")
                .and_then(parse_unix_value)
                .or_else(|| {
                    candidate
                        .raw_accept
                        .get("validBefore")
                        .and_then(parse_unix_value)
                })
        })
        .min()
        .unwrap_or(0);
    let challenge_expiry = match decoded.get("expires") {
        None | Some(Value::Null) => 0,
        Some(value) => parse_challenge_expiry(value)?,
    };
    let challenge_expires_at = match (challenge_expiry, candidate_expiry) {
        (0, expiry) | (expiry, 0) => expiry,
        (left, right) => left.min(right),
    };
    Ok(A2mcpPreparedPayment {
        version: A2MCP_INTENT_VERSION,
        source: A2MCP_SOURCE.to_string(),
        frozen_request,
        candidates: by_token.into_values().collect(),
        challenge_expires_at,
        wallet_error,
    })
}

/// Refresh wallet balances from prepared state without re-querying
/// token metadata, calling the merchant Endpoint, or obtaining a new challenge.
pub async fn refresh_a2mcp_prepared_payment(
    prepared: A2mcpPreparedPayment,
) -> Result<A2mcpPreparedPayment> {
    let raw_accepts: Vec<Value> = prepared
        .candidates
        .iter()
        .map(|candidate| candidate.raw_accept.clone())
        .collect();
    let mut balance_candidates = prepared
        .candidates
        .iter()
        .enumerate()
        .map(|(accepts_index, candidate)| super::state::Candidate {
            scheme: candidate.scheme.clone(),
            accepts_index,
            chain_id: candidate.chain_id.clone(),
            chain_name: candidate.chain_name.clone(),
            is_mainnet: candidate.is_mainnet,
            token_symbol: candidate.symbol.clone(),
            amount: candidate.amount_atomic.clone(),
            amount_human: candidate.amount_display.clone(),
            decimals: candidate.decimals,
            has_balance: false,
            balance_status: "unavailable".into(),
            available_amount: String::new(),
            required_amount: candidate.required_amount.clone(),
            shortfall: String::new(),
            deposit_address: candidate.deposit_address.clone(),
            recommended: None,
        })
        .collect::<Vec<_>>();
    let wallet_error =
        super::quote::refresh_a2mcp_candidate_balances(&mut balance_candidates, &raw_accepts)
            .await?;
    let mut candidates = prepared.candidates.clone();
    apply_balance_refresh(&mut candidates, balance_candidates)?;
    let refreshed = A2mcpPreparedPayment {
        version: prepared.version,
        source: prepared.source,
        frozen_request: prepared.frozen_request,
        candidates,
        challenge_expires_at: prepared.challenge_expires_at,
        wallet_error,
    };
    refreshed.validate()?;
    Ok(refreshed)
}

fn apply_balance_refresh(
    candidates: &mut [A2mcpPreparedCandidate],
    balance_candidates: Vec<super::state::Candidate>,
) -> Result<()> {
    if candidates.len() != balance_candidates.len() {
        bail!("{ERR_INVALID_INTENT}: candidate set changed during balance refresh");
    }
    for (candidate, balance) in candidates.iter_mut().zip(balance_candidates) {
        candidate.balance_status = balance.balance_status;
        candidate.available_amount = balance.available_amount;
        candidate.required_amount = balance.required_amount;
        candidate.shortfall = balance.shortfall;
        candidate.deposit_address = balance.deposit_address;
    }
    Ok(())
}

fn parse_unix_value(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

fn parse_challenge_expiry(value: &Value) -> Result<u64> {
    if let Some(unix) = parse_unix_value(value) {
        return Ok(unix);
    }
    let text = value
        .as_str()
        .ok_or_else(|| anyhow!("{ERR_INVALID_INTENT}: invalid challenge expiry"))?;
    let timestamp = chrono::DateTime::parse_from_rfc3339(text)
        .map_err(|_| anyhow!("{ERR_INVALID_INTENT}: invalid challenge expiry"))?
        .timestamp();
    u64::try_from(timestamp).map_err(|_| anyhow!("{ERR_INVALID_INTENT}: invalid challenge expiry"))
}

fn brand_candidates(
    prepared: Vec<super::state::Candidate>,
    raw_accepts: &[Value],
) -> Result<BTreeMap<(String, String), A2mcpPreparedCandidate>> {
    let mut by_token = BTreeMap::new();
    for candidate in prepared {
        // `build_candidates` may filter entries. Its accepts_index is the only
        // trustworthy binding back to the exact challenge entry.
        let raw = raw_accepts
            .get(candidate.accepts_index)
            .cloned()
            .ok_or_else(|| anyhow!("{ERR_INVALID_INTENT}: candidate index out of range"))?;
        let Some(authorization_type) = classify_authorization(&raw) else {
            continue;
        };
        let network = required_str(&raw, "network")?.to_string();
        let branded = A2mcpPreparedCandidate {
            candidate_id: format!("candidate_{}", candidate.accepts_index),
            raw_accept: raw,
            symbol: candidate.token_symbol.to_ascii_uppercase(),
            network,
            chain_id: candidate.chain_id,
            chain_name: candidate.chain_name,
            is_mainnet: candidate.is_mainnet,
            scheme: candidate.scheme.to_ascii_lowercase(),
            amount_atomic: candidate.amount,
            amount_display: candidate.amount_human,
            decimals: candidate.decimals,
            authorization_type: authorization_type.to_string(),
            balance_status: candidate.balance_status,
            available_amount: candidate.available_amount,
            required_amount: candidate.required_amount,
            shortfall: candidate.shortfall,
            deposit_address: candidate.deposit_address,
        };
        if !matches!(branded.symbol.as_str(), "USDT" | "USDC" | "USDG") {
            continue;
        }
        let key = (
            required_str(&branded.raw_accept, "network")?.to_string(),
            required_str(&branded.raw_accept, "asset")?.to_ascii_lowercase(),
        );
        match by_token.get(&key) {
            Some(current) if scheme_priority(current) <= scheme_priority(&branded) => {}
            _ => {
                by_token.insert(key, branded);
            }
        }
    }
    Ok(by_token)
}

fn classify_authorization(raw: &Value) -> Option<&'static str> {
    let scheme = raw.get("scheme")?.as_str()?.to_ascii_lowercase();
    let transfer = raw
        .get("extra")
        .and_then(|extra| extra.get("assetTransferMethod"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    match (scheme.as_str(), transfer.as_str()) {
        ("exact", "" | "eip3009" | "eip-3009") => Some("eip3009"),
        ("exact", "permit2") | ("upto", "permit2") => Some("permit2"),
        ("aggr_deferred", "" | "session") => Some("session"),
        _ => None,
    }
}

fn scheme_priority(candidate: &A2mcpPreparedCandidate) -> u8 {
    let scheme = candidate
        .raw_accept
        .get("scheme")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    match (scheme.as_str(), candidate.authorization_type.as_str()) {
        ("exact", "eip3009") => 0,
        ("exact", "permit2") => 1,
        ("upto", "permit2") => 2,
        ("aggr_deferred", "session") => 3,
        _ => u8::MAX,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::home;
    use serde_json::json;

    fn with_home<F: FnOnce()>(sub: &str, f: F) {
        let _lock = home::TEST_ENV_MUTEX.lock().unwrap();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join(sub);
        let _ = fs::remove_dir_all(&dir);
        std::env::set_var("ONCHAINOS_HOME", &dir);
        f();
        std::env::remove_var("ONCHAINOS_HOME");
        let _ = fs::remove_dir_all(&dir);
    }

    fn candidate(accepts_index: usize) -> super::super::state::Candidate {
        super::super::state::Candidate {
            scheme: "exact".into(),
            accepts_index,
            chain_id: "196".into(),
            chain_name: "X Layer".into(),
            is_mainnet: true,
            token_symbol: "USDC".into(),
            amount: "1000000".into(),
            amount_human: "1".into(),
            decimals: 6,
            has_balance: true,
            balance_status: "sufficient".into(),
            available_amount: "2".into(),
            required_amount: "1".into(),
            shortfall: "0".into(),
            deposit_address: "0x3333333333333333333333333333333333333333".into(),
            recommended: None,
        }
    }

    #[test]
    fn candidate_binding_uses_accepts_index_after_filtered_entry() {
        let unsupported = json!({
            "scheme":"period", "network":"eip155:1",
            "asset":"0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "amount":"9", "payTo":"0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        });
        let supported = json!({
            "scheme":"exact", "network":"eip155:196",
            "asset":"0x1111111111111111111111111111111111111111",
            "amount":"1000000", "payTo":"0x2222222222222222222222222222222222222222"
        });
        let bound =
            brand_candidates(vec![candidate(1)], &[unsupported, supported.clone()]).unwrap();
        let selected = bound.values().next().unwrap();
        assert_eq!(selected.raw_accept(), &supported);
        assert_eq!(selected.raw_accept()["network"], "eip155:196");
        assert_eq!(
            selected.raw_accept()["payTo"],
            "0x2222222222222222222222222222222222222222"
        );
    }

    #[test]
    fn candidate_binding_rejects_out_of_range_index() {
        let err = brand_candidates(vec![candidate(1)], &[json!({})]).unwrap_err();
        assert!(err.to_string().contains("candidate index out of range"));
    }

    fn prepared_payment() -> A2mcpPreparedPayment {
        let raw = json!({
            "scheme":"exact", "network":"eip155:196",
            "asset":"0x1111111111111111111111111111111111111111",
            "amount":"1000000", "payTo":"0x2222222222222222222222222222222222222222"
        });
        let candidates = brand_candidates(vec![candidate(0)], &[raw]).unwrap();
        A2mcpPreparedPayment {
            version: A2MCP_INTENT_VERSION,
            source: A2MCP_SOURCE.into(),
            frozen_request: A2mcpFrozenRequestV1::new(
                "https://example.com/pay".into(),
                "POST".into(),
                Map::from_iter([("count".into(), json!(2))]),
                vec![],
                Some(json!({"url":"https://example.com/pay"})),
            )
            .unwrap(),
            candidates: candidates.into_values().collect(),
            challenge_expires_at: 1_200,
            wallet_error: None,
        }
    }

    #[test]
    fn prepared_state_uses_short_id_and_is_consumed_once() {
        with_home("a2mcp_prepared_state_once", || {
            let prepared_id =
                store_a2mcp_prepared_payment(prepared_payment(), "account_1", 1_000).unwrap();

            assert!(prepared_id.starts_with("a2prep_"));
            assert!(prepared_id.len() < 64);
            let loaded = load_a2mcp_prepared_payment(&prepared_id, "account_1", 1_100).unwrap();
            assert_eq!(loaded.select("candidate_0").unwrap().symbol(), "USDC");

            let consumed =
                consume_a2mcp_prepared_payment(&prepared_id, "account_1", 1_100).unwrap();
            assert_eq!(consumed.frozen_request().typed_params()["count"], json!(2));
            let error =
                consume_a2mcp_prepared_payment(&prepared_id, "account_1", 1_100).unwrap_err();
            assert!(error
                .to_string()
                .starts_with(ERR_PREPARED_EXPIRED_OR_MISSING));

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let path = state::state_path(&prepared_id).unwrap();
                assert!(!path.exists(), "consumption must remove prepared state");
                let payments_mode = fs::metadata(path.parent().unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777;
                assert_eq!(payments_mode, 0o700);
            }
        });
    }

    #[test]
    fn prepared_state_enforces_owner_expiry_and_replacement() {
        with_home("a2mcp_prepared_state_guards", || {
            let prepared_id =
                store_a2mcp_prepared_payment(prepared_payment(), "account_1", 1_000).unwrap();
            let owner_error =
                load_a2mcp_prepared_payment(&prepared_id, "account_2", 1_100).unwrap_err();
            assert!(owner_error.to_string().starts_with(state::TOKEN_CROSS_USER));

            let replacement = replace_a2mcp_prepared_payment(
                &prepared_id,
                prepared_payment(),
                "account_1",
                1_100,
            )
            .unwrap();
            assert_ne!(replacement, prepared_id);
            assert!(load_a2mcp_prepared_payment(&prepared_id, "account_1", 1_100).is_err());
            assert!(load_a2mcp_prepared_payment(&replacement, "account_1", 1_199).is_ok());

            let expired =
                load_a2mcp_prepared_payment(&replacement, "account_1", 1_200).unwrap_err();
            assert!(expired
                .to_string()
                .starts_with(ERR_PREPARED_EXPIRED_OR_MISSING));
            assert!(!state::state_path(&replacement).unwrap().exists());
        });
    }

    #[test]
    fn prepared_replacement_preserves_original_ttl() {
        with_home("a2mcp_prepared_state_fixed_ttl", || {
            let mut prepared = prepared_payment();
            prepared.challenge_expires_at = 0;
            let prepared_id =
                store_a2mcp_prepared_payment(prepared.clone(), "account_1", 1_000).unwrap();
            let replacement =
                replace_a2mcp_prepared_payment(&prepared_id, prepared, "account_1", 1_100).unwrap();

            assert!(load_a2mcp_prepared_payment(&replacement, "account_1", 1_299).is_ok());
            assert!(load_a2mcp_prepared_payment(&replacement, "account_1", 1_300).is_err());
        });
    }

    #[test]
    fn concurrent_replacement_creates_only_one_live_successor() {
        with_home("a2mcp_prepared_state_atomic_replace", || {
            use std::sync::{Arc, Barrier};

            let prepared_id = Arc::new(
                store_a2mcp_prepared_payment(prepared_payment(), "account_1", 1_000).unwrap(),
            );
            let barrier = Arc::new(Barrier::new(3));
            let workers = (0..2)
                .map(|_| {
                    let prepared_id = Arc::clone(&prepared_id);
                    let barrier = Arc::clone(&barrier);
                    std::thread::spawn(move || {
                        barrier.wait();
                        replace_a2mcp_prepared_payment(
                            &prepared_id,
                            prepared_payment(),
                            "account_1",
                            1_100,
                        )
                    })
                })
                .collect::<Vec<_>>();
            barrier.wait();
            let results = workers
                .into_iter()
                .map(|worker| worker.join().unwrap())
                .collect::<Vec<_>>();

            assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
            let replacement = results.into_iter().find_map(Result::ok).unwrap();
            assert!(load_a2mcp_prepared_payment(&prepared_id, "account_1", 1_100).is_err());
            assert!(load_a2mcp_prepared_payment(&replacement, "account_1", 1_100).is_ok());
        });
    }

    #[test]
    fn uncommitted_claim_restores_prepared_state() {
        with_home("a2mcp_prepared_state_claim_rollback", || {
            let prepared_id =
                store_a2mcp_prepared_payment(prepared_payment(), "account_1", 1_000).unwrap();
            let claim = claim_a2mcp_prepared_payment(&prepared_id, "account_1", 1_100).unwrap();
            assert_eq!(claim.prepared().candidates().len(), 1);
            drop(claim);

            assert!(load_a2mcp_prepared_payment(&prepared_id, "account_1", 1_100).is_ok());
        });
    }

    #[test]
    fn challenge_expiry_accepts_rfc3339_and_numeric_values() {
        assert_eq!(
            parse_challenge_expiry(&json!("1970-01-01T00:20:00Z")).unwrap(),
            1_200
        );
        assert_eq!(parse_challenge_expiry(&json!("1200")).unwrap(), 1_200);
        assert!(parse_challenge_expiry(&json!("not-a-time")).is_err());
    }

    #[test]
    fn production_authorization_classifier_allows_exactly_four_combinations() {
        assert_eq!(
            classify_authorization(
                &json!({"scheme":"exact","extra":{"assetTransferMethod":"eip3009"}})
            ),
            Some("eip3009")
        );
        assert_eq!(
            classify_authorization(
                &json!({"scheme":"exact","extra":{"assetTransferMethod":"permit2"}})
            ),
            Some("permit2")
        );
        assert_eq!(
            classify_authorization(
                &json!({"scheme":"upto","extra":{"assetTransferMethod":"permit2"}})
            ),
            Some("permit2")
        );
        assert_eq!(
            classify_authorization(&json!({"scheme":"aggr_deferred"})),
            Some("session")
        );
        assert_eq!(classify_authorization(&json!({"scheme":"upto"})), None);
        assert_eq!(classify_authorization(&json!({"scheme":"period"})), None);
    }

    #[test]
    fn production_priority_is_case_insensitive_and_deterministic() {
        let mut ranked = [
            ("AGGR_DEFERRED", "session"),
            ("UPTO", "permit2"),
            ("EXACT", "permit2"),
            ("EXACT", "eip3009"),
        ]
        .into_iter()
        .map(|(scheme, authorization)| A2mcpPreparedCandidate {
            candidate_id: scheme.into(),
            raw_accept: json!({"scheme":scheme}),
            symbol: "USDC".into(),
            network: "eip155:196".into(),
            chain_id: "196".into(),
            chain_name: "X Layer".into(),
            is_mainnet: true,
            scheme: scheme.into(),
            amount_atomic: "1".into(),
            amount_display: "0.000001".into(),
            decimals: 6,
            authorization_type: authorization.into(),
            balance_status: "sufficient".into(),
            available_amount: "1".into(),
            required_amount: "0.000001".into(),
            shortfall: "0".into(),
            deposit_address: String::new(),
        })
        .collect::<Vec<_>>();
        ranked.sort_by_key(scheme_priority);
        assert_eq!(
            ranked
                .iter()
                .map(|candidate| candidate.authorization_type.as_str())
                .collect::<Vec<_>>(),
            vec!["eip3009", "permit2", "permit2", "session"]
        );
    }

    #[test]
    fn balance_refresh_preserves_frozen_candidate_metadata() {
        let raw = json!({
            "scheme":"exact", "network":"eip155:196",
            "asset":"0x1111111111111111111111111111111111111111",
            "amount":"1000000", "payTo":"0x2222222222222222222222222222222222222222"
        });
        let mut candidates = brand_candidates(vec![candidate(0)], &[raw])
            .unwrap()
            .into_values()
            .collect::<Vec<_>>();
        let frozen = (
            candidates[0].symbol.clone(),
            candidates[0].decimals,
            candidates[0].scheme.clone(),
            candidates[0].amount_atomic.clone(),
            candidates[0].amount_display.clone(),
        );
        let mut balance = candidate(0);
        balance.balance_status = "insufficient".into();
        balance.available_amount = "0.25".into();
        balance.required_amount = "1".into();
        balance.shortfall = "0.75".into();
        balance.deposit_address = "0x4444444444444444444444444444444444444444".into();
        apply_balance_refresh(&mut candidates, vec![balance]).unwrap();

        assert_eq!(
            (
                candidates[0].symbol.clone(),
                candidates[0].decimals,
                candidates[0].scheme.clone(),
                candidates[0].amount_atomic.clone(),
                candidates[0].amount_display.clone(),
            ),
            frozen
        );
        assert_eq!(candidates[0].balance_status, "insufficient");
        assert_eq!(candidates[0].shortfall, "0.75");
    }
}
