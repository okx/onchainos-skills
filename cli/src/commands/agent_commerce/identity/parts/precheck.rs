//! Registration pre-check (powers `agent pre-check`).
//!
//! Sinks references/register.md §2's per-wallet uniqueness logic into the CLI:
//! scope an `/agent-list` envelope to the signing wallet, count by role, and
//! decide whether the requested role can be created (≤1 requester, ≤1 evaluator
//! per address; provider unlimited). The skill renders the verdict instead of
//! filtering / counting agent rows by hand.
//!
//! Split out of `utils.rs` (file-size hygiene); declared there as a `#[path]`
//! child module so `utils::{build_precheck, collect_owned_agents}` stay the
//! same public path for callers. `role_label` is reached via `super::` (a child
//! module can see its parent file's private items).

use serde_json::Value;

use super::role_label;

/// Canonical role key (`requester` / `provider` / `evaluator`) from a row's raw
/// `role` value, accepting both the string enum and the backend integer
/// (`1`/`2`/`3`). Unknown → `None`.
fn role_key_from_value(role: &Value) -> Option<&'static str> {
    match role {
        Value::String(s) => match s.trim() {
            "requester" => Some("requester"),
            "provider" => Some("provider"),
            "evaluator" => Some("evaluator"),
            _ => None,
        },
        Value::Number(n) => match n.as_u64()? {
            1 => Some("requester"),
            2 => Some("provider"),
            3 => Some("evaluator"),
            _ => None,
        },
        _ => None,
    }
}

/// Collect the signing wallet's `(agentId, roleKey, name)` from an
/// `/agent-list` envelope. Tolerates both shapes (single-layer `list[*]`,
/// double-layer `list[*].agentList[*]`); a row/wrapper with no `ownerAddress`
/// is treated as owned (the list endpoint is JWT-scoped to the caller).
pub(crate) fn collect_owned_agents(
    agent_list: &Value,
    signing_address: &str,
) -> Vec<(String, Option<&'static str>, String)> {
    let signing_lower = signing_address.trim().to_ascii_lowercase();
    let owner_matches = |node: &Value| -> bool {
        match node.get("ownerAddress").and_then(Value::as_str) {
            Some(addr) => addr.trim().to_ascii_lowercase() == signing_lower,
            None => true,
        }
    };
    let push = |row: &Value, out: &mut Vec<(String, Option<&'static str>, String)>| {
        let id = match row.get("agentId") {
            Some(Value::String(s)) if !s.trim().is_empty() => s.trim().to_string(),
            Some(Value::Number(n)) => n.to_string(),
            _ => return,
        };
        let role_key = row.get("role").and_then(role_key_from_value);
        let name = row
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        out.push((id, role_key, name));
    };

    let mut owned = Vec::new();
    if let Some(items) = agent_list.get("list").and_then(Value::as_array) {
        for item in items {
            match item.get("agentList").and_then(Value::as_array) {
                Some(rows) if owner_matches(item) => {
                    for r in rows {
                        push(r, &mut owned);
                    }
                }
                Some(_) => {}
                None => {
                    if owner_matches(item) {
                        push(item, &mut owned);
                    }
                }
            }
        }
    }
    owned
}

/// Pure pre-check verdict for the requested role (register.md §2 uniqueness):
///   { role, roleLabel, ownerAddress, uniqueness, canCreate,
///     existingSameRole: [{agentId,name,roleLabel}], providerCount,
///     knownAgentIds }  // CSV of ALL owned ids → create's --known-agent-ids
pub(crate) fn build_precheck(agent_list: &Value, signing_address: &str, role_key: &str) -> Value {
    let owned = collect_owned_agents(agent_list, signing_address);

    let known_ids: Vec<String> = owned.iter().map(|(id, _, _)| id.clone()).collect();
    let provider_count = owned.iter().filter(|(_, rk, _)| *rk == Some("provider")).count();

    let existing_same_role: Vec<Value> = owned
        .iter()
        .filter(|(_, rk, _)| *rk == Some(role_key))
        .map(|(id, rk, name)| {
            serde_json::json!({
                "agentId": id,
                "name": name,
                "roleLabel": rk.and_then(role_label).unwrap_or(""),
            })
        })
        .collect();

    let unique = matches!(role_key, "requester" | "evaluator");
    let can_create = if unique { existing_same_role.is_empty() } else { true };
    let label = role_label(role_key).unwrap_or(role_key);

    let mut out = serde_json::json!({
        "role": role_key,
        "roleLabel": label,
        "ownerAddress": signing_address.trim(),
        "uniqueness": if unique { "single" } else { "multiple" },
        "canCreate": can_create,
        "existingSameRole": existing_same_role,
        "providerCount": provider_count,
        "knownAgentIds": known_ids.join(","),
    });
    // `reason` accompanies every canCreate:false (a single-role identity already
    // exists for this wallet). English canonical; the skill localizes.
    if !can_create {
        out["reason"] = serde_json::json!(format!(
            "A {label} is already registered under this wallet; each address can register only one {label}."
        ));
    }
    out
}
