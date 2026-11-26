const DISPATCHER: &str = include_str!("../src/commands/payment/dispatcher.rs");
const APP_REFERENCE: &str =
    include_str!("../../skills/okx-agent-payments-protocol/references/app.md");

#[test]
fn app_cancel_signing_does_not_mutate_subscription_cache() {
    let cancel = DISPATCHER
        .split_once("async fn cmd_app_subscription_cancel(")
        .expect("APP subscription cancel function")
        .1
        .split_once("/// `payment app subscription allowance-status`")
        .expect("end of APP subscription cancel function")
        .0;

    assert!(cancel.contains("sign_cancel("));
    assert!(cancel.contains("begin_pending("));
    assert!(!cancel.contains("SubscriptionCache"));
    assert!(!cancel.contains("mark_app_canceled"));
    assert!(!cancel.contains("cache.save"));
    assert!(cancel.contains("matching successful receipt resolves the pending operation"));
}

#[test]
fn app_cancel_flows_resolve_only_from_authoritative_receipts() {
    let result_flow = APP_REFERENCE
        .split_once("For subscription `cancel` and `cancel-pending`, handle the seller result")
        .expect("subscription cancellation result flow")
        .1
        .split_once("For `charge`, `upto`")
        .expect("end of subscription cancellation result flow")
        .0;

    assert!(result_flow.contains("subscription resolve --operation-id"));
    assert!(result_flow.contains("receipt has `status=success`"));
    assert!(result_flow.contains("matches `protocol`, `intent`, `offerId`, and `payer`"));
    assert!(result_flow.contains("unique transaction `reference`"));
    assert!(result_flow.contains("successful `cancel` marks the APP cache entry `canceled`"));
    assert!(result_flow
        .contains("successful\n   `cancel-pending` keeps the current subscription active"));
    assert!(result_flow.contains("leave the\n   cache unchanged"));
    assert!(result_flow.contains("without\n   applying the state change again"));
    assert!(APP_REFERENCE.contains("--sub-id '0x..' --offer-id '<offerId>'"));
    assert!(APP_REFERENCE.contains("app subscription resolve` | `--operation-id`"));
}
