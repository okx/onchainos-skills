//! `subscribe-offline-update` — set a subscription's offline-receive flag.
//!
//! When a buyer is offline, a subscription keeps producing deliverables. This
//! command records what should happen to that offline backlog once the buyer is
//! back online:
//!
//! - `0` (server default, never written by this command's default path) — keep:
//!   the backend keeps receiving and re-pushes the backlog on reconnect.
//! - `1` — discard: offline messages are dropped, the backend stops receiving.
//!
//! Backend-HTTP only (no `--chain`). POSTs the byte-literal body
//! `{"offlineReceiveFlag": <0|1>}` via `post_with_identity`.
//!
//! Success contract note: unlike `subscribe-device-update`'s batch endpoint
//! (which confirms with `data == true`), this endpoint's success `data` is
//! `null` by contract. The success predicate therefore accepts `null` — copying
//! the strict `data == true` check would misread every success as a failure.

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::common::okx_a2a::{self, OfflineReplayCapability};
use crate::commands::agentic_wallet::auth::ensure_tokens_refreshed;
use crate::output;

use super::create::resolve_user_agent;
use super::create_subscribe::SUBSCRIBE_API_PREFIX;
use super::subscription_ops::select_subscription_agent_id;

/// Parse and client-validate the `--flag` argument. Only `0` and `1` are legal;
/// everything else (`2`, `-1`, `true`, empty, …) is rejected locally before any
/// request is sent.
fn parse_offline_flag(raw: &str) -> Result<i32> {
    match raw {
        "0" => Ok(0),
        "1" => Ok(1),
        _ => bail!(
            "--flag must be 0 (keep offline backlog) or 1 (discard offline backlog); got \"{raw}\""
        ),
    }
}

/// Byte-literal request body `{ "offlineReceiveFlag": <0|1> }`.
fn build_offline_body(flag: i32) -> Value {
    json!({ "offlineReceiveFlag": flag })
}

/// `POST /priapi/v1/aieco/task/subscribe/{subId}/setOfflineReceiveFlag`.
fn offline_receive_path(sub_id: &str) -> String {
    format!("{SUBSCRIBE_API_PREFIX}/{sub_id}/setOfflineReceiveFlag")
}

/// Success predicate for the setOfflineReceiveFlag endpoint. The response has
/// already cleared the `code == "0"` gate inside `post_with_identity` (a non-zero
/// code surfaces as an error, never as data here), so this only inspects `data`.
/// The contract returns `data: null` on success, so `null` MUST pass; `true` is
/// tolerated for forward-compatibility. An explicit `false` is the one shape that
/// signals the backend declined the write.
fn is_offline_update_success(data: &Value) -> bool {
    data.is_null() || *data == Value::Bool(true)
}

/// Build the json success envelope. Always echoes `{ jobId, offlineReceiveFlag }`
/// so the skill confirms without a second fetch, and always carries the
/// `offlineReplaySupported` capability flag. When the comm package cannot honor an
/// offline-replay preference, also carries `offlineReplayFixCommands` (upgrade
/// commands). Those offline-replay fields are copy-only — they never change whether
/// or how the write was performed or judged.
fn build_offline_success(
    job_id: &str,
    flag: i32,
    offline_replay: &OfflineReplayCapability,
) -> Value {
    let mut envelope = json!({
        "jobId": job_id,
        "offlineReceiveFlag": flag,
        "offlineReplaySupported": offline_replay.supported,
    });
    if !offline_replay.supported {
        envelope["offlineReplayFixCommands"] = json!(offline_replay.fix_commands_or_default());
    }
    envelope
}

/// `subscribe-offline-update` handler. Validates `--flag` locally (0/1 only)
/// before any network call, then POSTs the byte-literal body and echoes
/// `{ jobId, offlineReceiveFlag }` so the skill confirms without a second fetch.
pub async fn handle_subscribe_offline_update(
    client: &mut TaskApiClient,
    job_id: &str,
    flag: &str,
) -> Result<()> {
    let flag = parse_offline_flag(flag)?;
    if job_id.is_empty() {
        bail!("--job-id must not be empty");
    }

    ensure_tokens_refreshed()
        .await
        .map_err(|e| anyhow!("session has expired; run `onchainos wallet login` first: {e}"))?;
    let (user_agent_id, _) = resolve_user_agent().await?;
    let user_agent_id = select_subscription_agent_id(&user_agent_id, "")?;

    let body = build_offline_body(flag);
    let path = offline_receive_path(job_id);
    let resp = client
        .post_with_identity(&path, &body, &user_agent_id)
        .await
        .map_err(|e| anyhow!("subscribe-offline-update failed: {e}"))?;

    if is_offline_update_success(&resp) {
        // Copy-only capability probe: read AFTER the write has succeeded so its
        // result can never influence whether the write was sent or judged.
        let offline_replay = okx_a2a::probe_offline_replay_capability();
        output::success(build_offline_success(job_id, flag, &offline_replay));
        Ok(())
    } else {
        // HTTP 200 + code "0" but data explicitly denies the write — echo raw body.
        bail!(
            "subscribe-offline-update failed: backend did not confirm the update: {}",
            serde_json::to_string(&resp).unwrap_or_else(|_| resp.to_string())
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_flag_accepts_zero_and_one() {
        assert_eq!(parse_offline_flag("0").unwrap(), 0);
        assert_eq!(parse_offline_flag("1").unwrap(), 1);
    }

    #[test]
    fn parse_flag_rejects_out_of_range_locally() {
        // Boundary: 2 and -1 are rejected before any request is built.
        assert!(parse_offline_flag("2").is_err());
        assert!(parse_offline_flag("-1").is_err());
        assert!(parse_offline_flag("true").is_err());
        assert!(parse_offline_flag("").is_err());
    }

    #[test]
    fn body_is_byte_literal_offline_flag_shape() {
        assert_eq!(build_offline_body(0), json!({ "offlineReceiveFlag": 0 }));
        assert_eq!(build_offline_body(1), json!({ "offlineReceiveFlag": 1 }));
    }

    #[test]
    fn path_targets_set_offline_receive_flag_endpoint() {
        assert_eq!(
            offline_receive_path("0xSUB"),
            "/priapi/v1/aieco/task/subscribe/0xSUB/setOfflineReceiveFlag"
        );
    }

    #[test]
    fn success_predicate_accepts_null_and_true_rejects_false() {
        // Contract: success data is null → MUST pass (do NOT copy batchUpdate's
        // strict data == true check).
        assert!(is_offline_update_success(&Value::Null));
        // Forward-compatible: an explicit true also passes.
        assert!(is_offline_update_success(&json!(true)));
        // An explicit false is the one shape that signals a declined write.
        assert!(!is_offline_update_success(&json!(false)));
    }

    #[test]
    fn success_envelope_reports_offline_replay_supported_without_fix_commands() {
        // Supported ⇒ offlineReplaySupported:true and NO fix-commands field.
        let cap = OfflineReplayCapability {
            supported: true,
            fix_commands: Vec::new(),
        };
        let env = build_offline_success("0xSUB", 1, &cap);
        assert_eq!(env["jobId"], json!("0xSUB"));
        assert_eq!(env["offlineReceiveFlag"], json!(1));
        assert_eq!(env["offlineReplaySupported"], json!(true));
        assert!(env.get("offlineReplayFixCommands").is_none());
    }

    #[test]
    fn success_envelope_includes_fix_commands_when_unsupported() {
        // Unsupported + probe supplied fixCommands → passed through verbatim.
        let cap = OfflineReplayCapability {
            supported: false,
            fix_commands: vec!["npm i -g @okxweb3/a2a-node@1.2.3".to_string()],
        };
        let env = build_offline_success("0xSUB", 0, &cap);
        assert_eq!(env["offlineReplaySupported"], json!(false));
        assert_eq!(
            env["offlineReplayFixCommands"],
            json!(["npm i -g @okxweb3/a2a-node@1.2.3"])
        );
        // Unsupported + no probe fixCommands → packaged default.
        let cap_default = OfflineReplayCapability {
            supported: false,
            fix_commands: Vec::new(),
        };
        let env2 = build_offline_success("0xSUB", 1, &cap_default);
        assert_eq!(
            env2["offlineReplayFixCommands"],
            json!(["npm install -g @okxweb3/a2a-node@latest"])
        );
    }
}
