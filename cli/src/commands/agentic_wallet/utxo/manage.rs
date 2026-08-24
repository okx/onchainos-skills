//! Applies or removes Bitcoin UTXO asset protection.

use anyhow::{bail, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

use crate::commands::agentic_wallet::chain_adapters::bitcoin::{
    api::{BtcApi, UTXO_MANAGE_BATCH_SIZE},
    context::BtcContext,
    models::{collect_outpoints, BtcOutPoint},
};
use crate::commands::agentic_wallet::common::WalletPreviewConfirming;
use crate::commands::agentic_wallet::support::json::shell_arg;
use crate::commands::sink::CodedError;
use crate::output;

/// Reviews or removes asset protection for selected unavailable Bitcoin UTXOs.
pub async fn cmd_unlock(
    outpoints: &[String],
    all: bool,
    operation_token: Option<&str>,
    force: bool,
) -> Result<()> {
    if all != outpoints.is_empty() {
        bail!("use exactly one of --outpoint or --all");
    }
    manage(
        "ignoreAsset",
        "btc_utxo_unlock",
        "UNAVAILABLE_BREAKDOWN",
        outpoints.to_vec(),
        all,
        operation_token,
        force,
    )
    .await
}

/// Reviews or restores asset protection for selected user-released Bitcoin UTXOs.
pub async fn cmd_lock(
    outpoints: &[String],
    all: bool,
    operation_token: Option<&str>,
    force: bool,
) -> Result<()> {
    if all != outpoints.is_empty() {
        bail!("use exactly one of --outpoint or --all");
    }
    manage(
        "cancelIgnore",
        "btc_utxo_lock",
        "USER_IGNORED_LIST",
        outpoints.to_vec(),
        all,
        operation_token,
        force,
    )
    .await
}

/// Runs the shared UTXO protection flow and emits confirmation or mutation results.
async fn manage(
    action: &str,
    scene: &str,
    query_type: &str,
    requested_outpoints: Vec<String>,
    all: bool,
    operation_token: Option<&str>,
    force: bool,
) -> Result<()> {
    validate_manage_continuation(operation_token, force)?;
    let context = BtcContext::load(None).await?;
    let mut api = BtcApi::new()?;
    let snapshot = api.availability_details(&context, query_type).await?;
    let candidates = if query_type == "UNAVAILABLE_BREAKDOWN" {
        if all && has_group_items(&snapshot, "/unavailableBreakdown/assetUncertain") {
            return Err(CodedError::new(
                "INCOMPLETE_SNAPSHOT",
                None,
                "All protected UTXOs cannot be unlocked while assetUncertain contains unresolved outpoints",
            )
            .with_data(json!({
                "assetUncertain": snapshot.pointer("/unavailableBreakdown/assetUncertain")
            }))
            .into());
        }
        let locked = collect_protected_outpoints(&snapshot, false);
        if all
            && locked.is_empty()
            && has_group_items(&snapshot, "/unavailableBreakdown/assetLocked")
        {
            return Err(CodedError::new(
                "INCOMPLETE_SNAPSHOT",
                None,
                "assetLocked reports protected UTXOs without complete outpoints",
            )
            .with_data(json!({
                "assetLocked": snapshot.pointer("/unavailableBreakdown/assetLocked")
            }))
            .into());
        }
        locked
    } else {
        collect_outpoints(snapshot.pointer("/userIgnoredList").unwrap_or(&snapshot))
    };
    let targets = select_targets(&candidates, &requested_outpoints, all)?;
    let operation_reason = resolve_management_reason(action)?;
    let operation_type = resolve_management_operation_type(action)?;
    let canonical_targets = targets
        .iter()
        .map(BtcOutPoint::canonical)
        .collect::<Vec<_>>();

    let preview = json!({
        "operationType": operation_type,
        "chainIndex": context.profile.chain_index,
        "network": "bitcoin",
        "from": context.address.address,
        "targets": canonical_targets,
        "message": operation_reason,
        "snapshot": snapshot,
    });
    let confirmation_token = build_manage_confirmation_token(
        operation_type,
        &context.profile.chain_index,
        &context.account_id,
        &context.address.address,
        &canonical_targets,
    )?;
    let target_flags = targets
        .iter()
        .map(|target| format!(" --outpoint {}", shell_arg(&target.canonical())))
        .collect::<String>();
    let command = if action == "ignoreAsset" {
        "unlock"
    } else {
        "lock"
    };
    let next = format!(
        "onchainos wallet utxo {command} --chain bitcoin{target_flags} --operation-token {} --force",
        shell_arg(&confirmation_token),
    );

    if force {
        if operation_token != Some(confirmation_token.as_str()) {
            return Err(WalletPreviewConfirming {
                message: "The supplied UTXO confirmation does not match the current account, operation, or target outpoints. Review the refreshed preview before confirming again.".to_string(),
                next,
                scene: scene.to_string(),
                preview,
            }
            .into());
        }
        let mut batch_results = Vec::new();
        let mut execution_error = None;
        for (batch_index, batch) in targets.chunks(UTXO_MANAGE_BATCH_SIZE).enumerate() {
            match api
                .manage_utxos(&context, action, operation_reason, batch)
                .await
            {
                Ok(result) => {
                    let normalized = normalize_manage_batch_result(&result, batch_index, batch)?;
                    let succeeded = normalized["result"].as_bool() == Some(true);
                    batch_results.push(normalized);
                    if !succeeded {
                        break;
                    }
                }
                Err(error) => {
                    execution_error = Some(error);
                    break;
                }
            }
        }
        let latest_breakdown = api
            .availability_details(&context, "UNAVAILABLE_BREAKDOWN")
            .await?;
        let latest_ignored = api
            .availability_details(&context, "USER_IGNORED_LIST")
            .await?;
        let result_context = json!({
            "batchResults": batch_results,
            "unavailable": latest_breakdown,
            "userIgnored": latest_ignored,
        });
        if let Some(error) = execution_error {
            return Err(enrich_manage_error(error, result_context));
        }
        let failed: Vec<Value> = batch_results
            .iter()
            .filter(|item| item.get("result").and_then(Value::as_bool) == Some(false))
            .cloned()
            .collect();
        if !failed.is_empty() {
            let any_succeeded = batch_results
                .iter()
                .any(|item| item.get("result").and_then(Value::as_bool) == Some(true));
            return Err(CodedError::new(
                if any_succeeded {
                    "UTXO_MANAGE_PARTIAL_FAILURE"
                } else {
                    "UTXO_MANAGE_REJECTED"
                },
                None,
                "One or more UTXO protection changes were rejected",
            )
            .with_data(json!({
                "batchResults": batch_results,
                "failed": failed,
                "unavailable": latest_breakdown,
                "userIgnored": latest_ignored,
            }))
            .into());
        }
        output::success(json!({
            "message": if action == "ignoreAsset" {
                "UTXO asset protection was removed. The latest UTXO state is included."
            } else {
                "UTXO asset protection was restored. The latest UTXO state is included."
            },
            "batchResults": batch_results,
            "targets": targets.iter().map(BtcOutPoint::canonical).collect::<Vec<_>>(),
            "unavailable": latest_breakdown,
            "userIgnored": latest_ignored,
        }));
        return Ok(());
    }

    Err(WalletPreviewConfirming {
        message: format!(
            "Review {} for {} UTXO(s). Every target and the latest availability snapshot are included in preview. Confirm only if changing protection for these UTXOs is acceptable.",
            if action == "ignoreAsset" { "protection removal" } else { "protection restoration" },
            targets.len()
        ),
        next,
        scene: scene.to_string(),
        preview,
    }
    .into())
}

/// Maps a management action to the message sent to the service.
fn resolve_management_reason(action: &str) -> Result<&'static str> {
    match action {
        "ignoreAsset" => Ok("User confirmed removal of UTXO asset protection"),
        "cancelIgnore" => Ok("User confirmed restoration of UTXO asset protection"),
        _ => bail!("unsupported UTXO management action: {action}"),
    }
}

/// Maps a management action to the operation name shown in confirmation data.
fn resolve_management_operation_type(action: &str) -> Result<&'static str> {
    match action {
        "ignoreAsset" => Ok("UNLOCK_UTXO_PROTECTION"),
        "cancelIgnore" => Ok("LOCK_UTXO_PROTECTION"),
        _ => bail!("unsupported UTXO management action: {action}"),
    }
}

/// Validates the relationship between confirmation mode and its operation token.
fn validate_manage_continuation(operation_token: Option<&str>, force: bool) -> Result<()> {
    if force && operation_token.is_none() {
        bail!("confirmed UTXO protection changes require the preview continuation");
    }
    if !force && operation_token.is_some() {
        bail!("preview continuation parameters are only valid with --force");
    }
    if !force {
        return Ok(());
    }

    let operation_token = operation_token.ok_or_else(|| {
        anyhow::anyhow!("confirmed UTXO protection changes require the preview continuation")
    })?;
    if !is_confirmation_token(operation_token) {
        return Err(CodedError::new(
            "INVALID_PREVIEW_CONTINUATION",
            Some("operationToken"),
            "Invalid UTXO preview continuation: --operation-token must be the sha256 token returned by the preview",
        )
        .into());
    }
    Ok(())
}

/// Checks whether a continuation token contains one complete SHA-256 digest.
fn is_confirmation_token(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

/// Binds the account, action, and target outpoints into a confirmation token.
fn build_manage_confirmation_token(
    operation_type: &str,
    chain_index: &str,
    account_id: &str,
    from: &str,
    targets: &[String],
) -> Result<String> {
    let critical_intent = json!({
        "operationType": operation_type,
        "chainIndex": chain_index,
        "network": "bitcoin",
        "accountId": account_id,
        "from": from,
        "targets": targets,
    });
    let canonical = serde_jcs::to_string(&critical_intent)?;
    Ok(format!(
        "sha256:{}",
        hex::encode(Sha256::digest(canonical.as_bytes()))
    ))
}

/// Resolves requested outpoints against the latest eligible UTXO snapshot.
fn select_targets(
    available: &[BtcOutPoint],
    requested: &[String],
    all: bool,
) -> Result<Vec<BtcOutPoint>> {
    if all {
        if available.is_empty() {
            bail!("no matching UTXOs were returned by the current service snapshot");
        }
        return Ok(available.to_vec());
    }
    if requested.is_empty() {
        bail!("at least one --outpoint is required");
    }
    let mut unique = BTreeSet::new();
    let mut targets = Vec::with_capacity(requested.len());
    for raw in requested {
        let point = BtcOutPoint::parse(raw)?;
        if !unique.insert(point.canonical()) {
            bail!("duplicate --outpoint {}", point.canonical());
        }
        let target = available
            .iter()
            .find(|candidate| **candidate == point)
            .cloned()
            .ok_or_else(|| {
                CodedError::new(
                    "STATE_CHANGED",
                    Some("outpoint"),
                    format!(
                        "The requested outpoint {} is not present in the latest UTXO snapshot",
                        point.canonical()
                    ),
                )
            })?;
        targets.push(target);
    }
    targets.sort();
    Ok(targets)
}

/// Collects protected outpoints, optionally including uncertain asset classifications.
fn collect_protected_outpoints(snapshot: &Value, include_uncertain: bool) -> Vec<BtcOutPoint> {
    let mut points = snapshot
        .pointer("/unavailableBreakdown/assetLocked")
        .map(collect_outpoints)
        .unwrap_or_default();
    if include_uncertain {
        points.extend(collect_uncertain_outpoints(snapshot));
        points.sort();
        points.dedup();
    }
    points
}

/// Collects outpoints whose asset classification is currently uncertain.
fn collect_uncertain_outpoints(snapshot: &Value) -> Vec<BtcOutPoint> {
    snapshot
        .pointer("/unavailableBreakdown/assetUncertain")
        .map(collect_outpoints)
        .unwrap_or_default()
}

/// Returns whether a response group reports or contains at least one outpoint.
fn has_group_items(snapshot: &Value, pointer: &str) -> bool {
    let Some(group) = snapshot.pointer(pointer) else {
        return false;
    };
    group
        .get("count")
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_str().and_then(|raw| raw.parse().ok()))
        })
        .is_some_and(|count| count > 0)
        || !collect_outpoints(group).is_empty()
}

/// Normalizes one UTXO management batch response and preserves its service reason.
fn normalize_manage_batch_result(
    result: &Value,
    batch_index: usize,
    targets: &[BtcOutPoint],
) -> Result<Value> {
    let items = result
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("UTXO management response data must be an array"))?;
    if items.len() != 1 {
        bail!("UTXO management response data must contain exactly one item");
    }
    let item = &items[0];
    let succeeded = item.get("result").and_then(Value::as_bool).ok_or_else(|| {
        anyhow::anyhow!("UTXO management response item is missing boolean result")
    })?;
    let reason = item
        .get("reason")
        .or_else(|| item.get("resaon"))
        .cloned()
        .unwrap_or(Value::Null);
    Ok(json!({
        "batchIndex": batch_index,
        "outpoints": targets.iter().map(BtcOutPoint::canonical).collect::<Vec<_>>(),
        "result": succeeded,
        "reason": reason,
    }))
}

/// Adds operation context to a service error or returns an unknown-result error.
fn enrich_manage_error(error: anyhow::Error, mut context: Value) -> anyhow::Error {
    match error.downcast::<CodedError>() {
        Ok(mut coded) => {
            if let Some(service_data) = coded.data.take() {
                context["serviceData"] = service_data;
            }
            coded.data = Some(context);
            coded.into()
        }
        Err(error) => CodedError::new(
            "UTXO_MANAGE_RESULT_UNKNOWN",
            None,
            format!("UTXO management result is unknown: {error}"),
        )
        .with_data(context)
        .into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmed_manage_rejects_invalid_continuation_without_reporting_state_change() {
        for operation_token in ["", "sha256:not-a-digest"] {
            let error = validate_manage_continuation(Some(operation_token), true).unwrap_err();
            let coded = error.downcast::<CodedError>().unwrap();
            assert_eq!(coded.code, "INVALID_PREVIEW_CONTINUATION");
            assert_eq!(coded.field.as_deref(), Some("operationToken"));
            assert!(!coded.message.contains("state changed"));
        }
    }

    #[test]
    fn confirmed_manage_accepts_well_formed_local_continuation() {
        let token = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        validate_manage_continuation(Some(token), true).unwrap();
    }

    #[test]
    fn confirmation_token_binds_only_critical_intent() {
        let targets =
            vec!["4d3f6a7a45dbb9d3398a8f83c0219b6bedfdcd77d1de63cc09f9cfe360c553c0:0".to_string()];
        let original = build_manage_confirmation_token(
            "UNLOCK_UTXO_PROTECTION",
            "0",
            "account-a",
            "bc1ptest",
            &targets,
        )
        .unwrap();

        let after_non_critical_snapshot_change = build_manage_confirmation_token(
            "UNLOCK_UTXO_PROTECTION",
            "0",
            "account-a",
            "bc1ptest",
            &targets,
        )
        .unwrap();
        assert_eq!(original, after_non_critical_snapshot_change);

        let changed_targets =
            vec!["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:1".to_string()];
        assert_ne!(
            original,
            build_manage_confirmation_token(
                "UNLOCK_UTXO_PROTECTION",
                "0",
                "account-a",
                "bc1ptest",
                &changed_targets,
            )
            .unwrap()
        );
        assert_ne!(
            original,
            build_manage_confirmation_token(
                "LOCK_UTXO_PROTECTION",
                "0",
                "account-a",
                "bc1ptest",
                &targets,
            )
            .unwrap()
        );
        assert_ne!(
            original,
            build_manage_confirmation_token(
                "UNLOCK_UTXO_PROTECTION",
                "0",
                "account-b",
                "bc1ptest",
                &targets,
            )
            .unwrap()
        );
    }

    #[test]
    fn unlock_candidates_only_use_asset_locked_group() {
        let locked = "4d3f6a7a45dbb9d3398a8f83c0219b6bedfdcd77d1de63cc09f9cfe360c553c0";
        let fee_only = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let snapshot = json!({
            "unavailableBreakdown": {
                "assetLocked": {"count": 1, "utxos": [{"txHash": locked, "voutIndex": 0}]},
                "feeUneconomic": {"count": 1, "utxos": [{"txHash": fee_only, "voutIndex": 1}]},
                "assetUncertain": {"count": 0, "utxos": []}
            }
        });
        let candidates = collect_protected_outpoints(&snapshot, false);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].tx_hash, locked);
        assert!(!has_group_items(
            &snapshot,
            "/unavailableBreakdown/assetUncertain"
        ));
    }

    #[test]
    fn utxo_manage_maps_single_batch_result_and_reason_typo() {
        let targets = [
            BtcOutPoint::parse(
                "4d3f6a7a45dbb9d3398a8f83c0219b6bedfdcd77d1de63cc09f9cfe360c553c0:0",
            )
            .unwrap(),
            BtcOutPoint::parse(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:1",
            )
            .unwrap(),
        ];
        let outcome = normalize_manage_batch_result(
            &json!([{"result": false, "resaon": "already spent"}]),
            2,
            &targets,
        )
        .unwrap();
        assert_eq!(outcome["batchIndex"], 2);
        assert_eq!(outcome["outpoints"][0], targets[0].canonical());
        assert_eq!(outcome["result"], false);
        assert_eq!(outcome["reason"], "already spent");
        assert!(normalize_manage_batch_result(&json!({"result": true}), 0, &targets).is_err());
        assert!(normalize_manage_batch_result(
            &json!([{"result": true}, {"result": true}]),
            0,
            &targets
        )
        .is_err());
    }
}
