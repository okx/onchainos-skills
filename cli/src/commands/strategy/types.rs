//! DTOs for limit-order endpoints. Field names mirror BE wire format.
//! Optional fields reflect BE omission semantics (e.g. `transactionInfo`
//! is null while order is pending).

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── Request bodies ────────────────────────────────────────────────────

/// `rule` JSON on createOrder. Phase 1 (U-pegged) uses 4 fields; SwapMode
/// fields (`toAmount` / `exChangeRate` / `minReturnAmount` / `triggerMarketCapacity`)
/// are not modelled until Phase 2.
#[derive(Debug, Clone, Serialize)]
pub struct Rule {
    #[serde(rename = "fromTokenAddress")]
    pub from_token_address: String,
    #[serde(rename = "toTokenAddress")]
    pub to_token_address: String,
    /// Human-readable decimal (e.g. `"0.1"`). The raw-integer form goes only
    /// in `signMsg`'s `From Amount(precision adjusted)` line (BE 2026-05-07).
    #[serde(rename = "fromAmount")]
    pub from_amount: String,
    #[serde(rename = "triggerPrice", skip_serializing_if = "Option::is_none")]
    pub trigger_price: Option<String>,
}

/// `preset` JSON. Kept opaque as `Value` — fields can be typed later when
/// the CLI surfaces custom slippage / fee tuning.
pub type Preset = Value;

/// `verifySignInfo` JSON. BE uses it for KD-002 signature conversion + intent
/// verification. `signature` is produced by `trader_mode::sign_intent`
/// (EVM: EIP-191; Solana: ed25519 over hex). `signMsg` is UTF-8 plaintext —
/// legacy `encoding` field dropped 2026-05-07.
#[derive(Debug, Clone, Serialize)]
pub struct VerifySignInfo {
    #[serde(rename = "accountId")]
    pub account_id: String,
    /// SA wallet address — must match the top-level `userWalletAddress`.
    pub address: String,
    /// Long (number); top-level `chainId` is String — types are different on purpose.
    #[serde(rename = "chainId")]
    pub chain_id: i64,
    /// Signed intent plaintext (replaces legacy `intentData`).
    #[serde(rename = "signMsg")]
    pub sign_msg: String,
    pub signature: String,
    #[serde(rename = "sessionCert")]
    pub session_cert: String,
    /// SA TEE id (BE contract 2026-05-07); sourced from `session.json::teeId`.
    #[serde(rename = "teeId")]
    pub tee_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateOrderReq {
    #[serde(rename = "chainId")]
    pub chain_id: String,
    #[serde(rename = "userWalletAddress")]
    pub user_wallet_address: String,
    pub rule: Rule,
    pub preset: Preset,
    #[serde(rename = "strategyType")]
    pub strategy_type: i32,
    #[serde(rename = "strategyDirection")]
    pub strategy_direction: i32,
    #[serde(rename = "verifySignInfo")]
    pub verify_sign_info: VerifySignInfo,
    #[serde(rename = "expireTime", skip_serializing_if = "Option::is_none")]
    pub expire_time: Option<String>,
    #[serde(rename = "serviceFeeInfo", skip_serializing_if = "Option::is_none")]
    pub service_fee_info: Option<Value>,
    /// 0=swap, 1=meme, 2=market_condition, 3=advancedMode, 4=Agentic
    /// (BE-confirmed 2026-05-12). Strategy CLI sends 4.
    #[serde(rename = "sourceType", skip_serializing_if = "Option::is_none")]
    pub source_type: Option<i32>,
    #[serde(rename = "estimateGasFee", skip_serializing_if = "Option::is_none")]
    pub estimate_gas_fee: Option<String>,
    #[serde(rename = "referrerAddress", skip_serializing_if = "Option::is_none")]
    pub referrer_address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct CancelReq {
    #[serde(rename = "accountId")]
    pub account_id: String,
    #[serde(rename = "orderIds", skip_serializing_if = "Option::is_none")]
    pub order_ids: Option<Vec<String>>,
    #[serde(rename = "cancelAll", skip_serializing_if = "Option::is_none")]
    pub cancel_all: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ListOrdersReq {
    #[serde(rename = "accountId")]
    pub account_id: String,
    #[serde(rename = "walletAddressList")]
    pub wallet_address_list: Vec<String>,
    #[serde(rename = "chainIdList", skip_serializing_if = "Option::is_none")]
    pub chain_id_list: Option<Vec<String>>,
    #[serde(rename = "orderStatusList", skip_serializing_if = "Option::is_none")]
    pub order_status_list: Option<Vec<i32>>,
    #[serde(rename = "orderTypeList", skip_serializing_if = "Option::is_none")]
    pub order_type_list: Option<Vec<i32>>,
    #[serde(rename = "idList", skip_serializing_if = "Option::is_none")]
    pub id_list: Option<Vec<String>>,
    /// Single-value filter (BE schema 2026-05-09 replaced the prior
    /// `tokenAddressList: List<String>`). Multi-token requires multiple calls.
    #[serde(rename = "tokenAddress", skip_serializing_if = "Option::is_none")]
    pub token_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ReactivateReq {
    #[serde(rename = "accountId")]
    pub account_id: String,
    #[serde(rename = "orderIds")]
    pub order_ids: Vec<String>,
}

/// `registerTeeInfo` body. BE uses `timestamp` / `expireTimestamp` (not `timestampMs`).
#[derive(Debug, Clone, Serialize)]
pub struct RegisterTeeInfoReq {
    #[serde(rename = "accountId")]
    pub account_id: String,
    /// Milliseconds.
    pub timestamp: i64,
    /// Milliseconds.
    #[serde(rename = "expireTimestamp")]
    pub expire_timestamp: i64,
    #[serde(rename = "attestDocHex")]
    pub attest_doc_hex: String,
    #[serde(rename = "sessionCert")]
    pub session_cert: String,
    #[serde(rename = "sessionSig")]
    pub session_sig: String,
}

// ── Response bodies ───────────────────────────────────────────────────

/// Single order DTO (createOrder / getOpenOrder / openOrderDetail).
/// Unmodelled fields round-trip via `extra` (flatten) — BE additions don't break us.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderListResp {
    #[serde(rename = "orderId")]
    pub order_id: String,
    #[serde(rename = "strategyId", default)]
    pub strategy_id: Option<String>,
    #[serde(rename = "userWalletAddress", default)]
    pub user_wallet_address: Option<String>,
    pub status: i32,
    #[serde(rename = "strategyMode", default)]
    pub strategy_mode: Option<i32>,
    #[serde(rename = "orderType", default)]
    pub order_type: Option<i32>,
    #[serde(rename = "strategyType", default)]
    pub strategy_type: Option<i32>,
    #[serde(rename = "exchangeDirection", default)]
    pub exchange_direction: Option<i32>,
    #[serde(rename = "chainId", default)]
    pub chain_id: Option<String>,
    #[serde(rename = "chainName", default)]
    pub chain_name: Option<String>,
    #[serde(rename = "canResume", default)]
    pub can_resume: Option<bool>,
    #[serde(rename = "fromToken", default)]
    pub from_token: Option<Value>,
    #[serde(rename = "toToken", default)]
    pub to_token: Option<Value>,
    #[serde(rename = "triggerInfo", default)]
    pub trigger_info: Option<Value>,
    #[serde(rename = "createTime", default)]
    pub create_time: Option<String>,
    #[serde(rename = "expireTime", default)]
    pub expire_time: Option<String>,
    #[serde(rename = "transactionInfo", default)]
    pub transaction_info: Option<Value>,
    #[serde(rename = "executionHistoryList", default)]
    pub execution_history_list: Option<Value>,
    #[serde(rename = "orderStatusUpdateTime", default)]
    pub order_status_update_time: Option<String>,
    #[serde(rename = "estimatedWaitTime", default)]
    pub estimated_wait_time: Option<i64>,
    #[serde(rename = "eventCursor", default)]
    pub event_cursor: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListOrdersResp {
    /// BE name: `dataList`.
    #[serde(rename = "dataList", default)]
    pub list: Vec<OrderListResp>,
    /// BE name: `cursor` (empty string = no more pages). CLI re-exposes as
    /// `nextCursor` in JSON output.
    #[serde(default)]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CancelResp {
    #[serde(rename = "updateNum", default)]
    pub update_num: i64,
    #[serde(rename = "estimatedWaitTime", default)]
    pub estimated_wait_time: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReactivateResp {
    #[serde(rename = "successIds", default)]
    pub success_ids: Vec<String>,
    #[serde(rename = "failIds", default)]
    pub fail_ids: Vec<String>,
}

// ── BE integer constants ──

pub mod strategy_type {
    pub const BUY_DIP: i32 = 2;
    pub const TAKE_PROFIT: i32 = 3;
    pub const STOP_LOSS: i32 = 4;
    pub const CHASE_HIGH: i32 = 5;
}

pub mod direction {
    pub const ALL: i32 = -1;
    pub const BUY: i32 = 0;
    pub const SELL: i32 = 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sanity: deserialize a minimal createOrder response (the actual
    /// pending-order shape, where many fields are null).
    #[test]
    fn deserialize_minimal_order_resp() {
        let raw = r#"{
            "orderId": "ord-1",
            "status": 2,
            "estimatedWaitTime": 12
        }"#;
        let parsed: OrderListResp = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.order_id, "ord-1");
        assert_eq!(parsed.status, 2);
        assert_eq!(parsed.estimated_wait_time, Some(12));
        assert!(parsed.transaction_info.is_none());
    }

    #[test]
    fn deserialize_list_with_pagination() {
        // BE wire shape: dataList + cursor (hasNext is ignored — empty
        // `cursor` indicates no more pages).
        let raw = r#"{
            "dataList": [{"orderId":"a","status":3}, {"orderId":"b","status":4}],
            "cursor": "abc",
            "hasNext": true
        }"#;
        let parsed: ListOrdersResp = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.list.len(), 2);
        assert_eq!(parsed.cursor.as_deref(), Some("abc"));
    }

    /// Current createOrder contract:
    /// - top-level must NOT contain `accountId`, `sessionCert`, `encoding`,
    ///   `sessionSig`, `intentData`, `referralCode`, or `teeId`.
    /// - `verifySignInfo` is required and carries `accountId`, `address`,
    ///   `chainId`, `signMsg`, `signature`, `sessionCert`.
    /// - `verifySignInfo` no longer carries `sessionSig` / `referralCode`
    ///   (BE parses them from `signMsg` internally); `teeId` was re-added
    ///   per the 2026-05-07 BE requirement.
    #[test]
    fn create_order_serialises_with_verify_sign_info() {
        let req = CreateOrderReq {
            chain_id: "1".into(),
            user_wallet_address: "0x".into(),
            rule: Rule {
                from_token_address: "0xA".into(),
                to_token_address: "0xB".into(),
                from_amount: "1".into(),
                trigger_price: None,
            },
            preset: serde_json::json!({}),
            strategy_type: strategy_type::BUY_DIP,
            strategy_direction: direction::BUY,
            verify_sign_info: VerifySignInfo {
                account_id: "acc-1".into(),
                address: "0x".into(),
                chain_id: 1,
                sign_msg: "{\"x\":1}".into(),
                signature: "sig-bytes".into(),
                session_cert: "cert".into(),
                tee_id: "tee-1".into(),
            },
            expire_time: None,
            service_fee_info: None,
            source_type: None,
            estimate_gas_fee: None,
            referrer_address: None,
        };
        let v = serde_json::to_value(&req).unwrap();

        // ── top-level fields that must NOT appear ──
        for k in [
            "accountId", "sessionCert", "encoding", "sessionSig", "intentData",
            "referralCode", "teeId",
        ] {
            assert!(
                v.get(k).is_none(),
                "{k} must NOT appear at top-level"
            );
        }
        // ── top-level kept ──
        for k in ["chainId", "userWalletAddress", "rule", "preset",
                 "strategyType", "strategyDirection", "verifySignInfo"] {
            assert!(v.get(k).is_some(), "{k} must appear at top-level");
        }
        // ── verifySignInfo nested ──
        let vsi = v.get("verifySignInfo").unwrap();
        for k in ["accountId", "address", "chainId", "signMsg", "signature",
                 "sessionCert", "teeId"] {
            assert!(
                vsi.get(k).is_some(),
                "verifySignInfo.{k} must be present"
            );
        }
        // encoding was removed.
        assert!(vsi.get("encoding").is_none(), "verifySignInfo.encoding must NOT be present (removed 2026-05-07)");
        // referralCode / sessionSig are no longer required.
        for k in ["sessionSig", "referralCode"] {
            assert!(
                vsi.get(k).is_none(),
                "verifySignInfo.{k} must NOT be present"
            );
        }
        // Long-vs-String distinction for chainId.
        assert!(vsi.get("chainId").unwrap().is_number(), "verifySignInfo.chainId must be Number");
        assert!(v.get("chainId").unwrap().is_string(), "top-level chainId must be String");
    }

    #[test]
    fn cancel_resp_defaults_to_zero() {
        let raw = r#"{}"#;
        let parsed: CancelResp = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.update_num, 0);
        assert!(parsed.estimated_wait_time.is_none());
    }
}
