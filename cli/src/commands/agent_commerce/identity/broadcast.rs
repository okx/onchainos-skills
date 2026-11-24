//! Identity-specific adapter for the shared post-unsignedInfo broadcast pipeline.

use anyhow::Result;
use serde_json::{Map, Value};

use crate::commands::agentic_wallet::broadcast::{broadcast_unsigned, BroadcastCtx};
use crate::wallet_api::UnsignedInfoResponse;

use super::signing::AgentSigningSession;

const IDENTITY_BROADCAST_ENDPOINT: &str =
    "/priapi/v5/wallet/agentic/agent/pre-transaction/broadcast-transaction";

pub(super) async fn sign_and_broadcast_agent_transaction(
    access_token: &str,
    unsigned: &UnsignedInfoResponse,
    extra_data_overlay: Option<Map<String, Value>>,
    session: &AgentSigningSession,
) -> Result<String> {
    broadcast_unsigned(BroadcastCtx {
        broadcast_endpoint: Some(IDENTITY_BROADCAST_ENDPOINT),
        access_token,
        account_id: &session.account_id,
        addr_info: &session.addr_info,
        session_cert: &session.session_cert,
        signing_seed: &session.signing_seed,
        unsigned,
        is_contract_call: true,
        mev_protection: false,
        force: false,
        extra_data_overlay,
        trace_headers: None,
    })
    .await
}
