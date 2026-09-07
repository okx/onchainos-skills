//! Compatibility entrypoint for the retired second evaluation transaction.
//!
//! New one-time and subscription requests both use the combined
//! `approveAndCreateDispute` endpoint. This command remains parseable for older
//! callers and exits before any API or on-chain write.

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as BASE64_URL, Engine as _};

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;

const MAX_REASON_CHARS: usize = 2000;

pub(super) fn decode_reason_input(
    reason: Option<&str>,
    reason_b64: Option<&str>,
) -> Result<String> {
    match (reason, reason_b64) {
        (Some(_), Some(_)) => bail!("Pass exactly one of --reason or --reason-b64"),
        (Some(reason), None) => Ok(reason.to_string()),
        (None, Some(encoded)) => {
            let bytes = BASE64_URL
                .decode(encoded)
                .context("--reason-b64 is not valid URL-safe base64")?;
            String::from_utf8(bytes).context("--reason-b64 does not contain UTF-8 text")
        }
        (None, None) => bail!("Evaluation reason is required. Pass --reason or --reason-b64."),
    }
}

pub async fn handle_dispute_confirm(
    _client: &mut TaskApiClient,
    _job_id: &str,
    reason: &str,
    agent_id: &str,
) -> Result<()> {
    if agent_id.is_empty() {
        bail!("--agent-id is required (pass the ASP's own agentId; beta backend rejects empty agenticId header)");
    }
    if reason.trim().is_empty() {
        bail!("Evaluation reason is required. Pass the original evaluation reason with --reason or --reason-b64.");
    }
    if reason.chars().count() > MAX_REASON_CHARS {
        bail!("Evaluation reason exceeds {MAX_REASON_CHARS} characters. Please shorten it and try again.");
    }
    bail!(
        "dispute confirm has been retired. Use `onchainos agent dispute raise <jobId> --reason <reason> --agent-id <aspAgentId>`; it completes approve and evaluation creation in one transaction"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reason_b64_round_trips_exact_utf8_text() {
        let reason = "The delivery met the agreed requirements";
        let encoded = BASE64_URL.encode(reason.as_bytes());
        assert_eq!(decode_reason_input(None, Some(&encoded)).unwrap(), reason);
    }

    #[test]
    fn reason_input_requires_exactly_one_source() {
        assert!(decode_reason_input(None, None).is_err());
        assert!(decode_reason_input(Some("reason"), Some("cmVhc29u")).is_err());
        assert!(decode_reason_input(None, Some("%%%invalid%%%")).is_err());
    }
}
