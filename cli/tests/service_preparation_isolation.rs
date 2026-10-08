//! Exercise the production preparation modules with a recording API client.
//! Authentication and output are local test doubles; no live wallet is used.

use serde_json::json;

mod output {
    use serde_json::Value;
    use std::cell::RefCell;

    thread_local! {
        static RESULT: RefCell<Option<Value>> = const { RefCell::new(None) };
    }

    pub fn success(value: Value) {
        RESULT.with(|result| *result.borrow_mut() = Some(value));
    }

    pub fn take() -> Value {
        RESULT.with(|result| result.borrow_mut().take().expect("result emitted"))
    }
}

mod commands {
    pub mod agentic_wallet {
        pub mod auth {
            pub async fn ensure_tokens_refreshed() -> anyhow::Result<String> {
                Ok("test-token".into())
            }
        }
    }

    pub mod agent_commerce {
        pub mod task {
            pub mod common {
                pub fn current_account_xlayer_address() -> Option<String> {
                    Some("test-address".into())
                }

                pub mod network {
                    pub mod task_api_client {
                        use serde_json::{json, Value};

                        pub struct TaskApiClient {
                            pub response: Value,
                            pub calls: Vec<Value>,
                        }

                        impl TaskApiClient {
                            pub async fn raw_post_with_identity(
                                &mut self,
                                path: &str,
                                body: Vec<u8>,
                                content_type: &str,
                                agent_id: &str,
                            ) -> anyhow::Result<Value> {
                                self.calls.push(json!({
                                    "method": "POST", "path": path,
                                    "body": serde_json::from_slice::<Value>(&body)?,
                                    "contentType": content_type, "agenticId": agent_id,
                                }));
                                Ok(self.response.clone())
                            }

                            pub async fn get_authed_query(
                                &mut self,
                                path: &str,
                                query: &[(&str, &str)],
                            ) -> anyhow::Result<Value> {
                                self.calls.push(json!({
                                    "method": "GET", "path": path, "query": query,
                                }));
                                Ok(self.response.clone())
                            }
                        }
                    }
                }
            }
        }
    }
}

#[path = "../src/commands/agent_commerce/task/user/service_detail.rs"]
mod a2a_detail;
#[path = "../src/commands/agent_commerce/a2mcp/prepare.rs"]
mod mcp_prepare;

use commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;

#[tokio::test]
async fn a2a_uses_main_api_and_buyer_identity_and_preserves_fee_fields() {
    for fee in [json!(0.1), json!("0.1"), json!(0)] {
        let service = json!({
            "sid": 39218, "serviceId": "a2a-service-uuid", "serviceType": "A2A",
            "feeAmount": fee, "feeTokenSymbol": "USDT", "feeToken": "token-contract",
            "asp": {"aspAgentId": "9967"},
        });
        let mut client = TaskApiClient {
            response: json!({"services": [{"sid": 1}, service.clone()]}),
            calls: vec![],
        };
        a2a_detail::handle_service_detail(&mut client, "39218", "buyer-42")
            .await
            .unwrap();
        assert_eq!(output::take(), service);
        assert_eq!(
            client.calls,
            vec![json!({
                "method": "POST", "path": "/priapi/v1/aieco/task/asp/service/search",
                "body": {"sid": "39218", "limit": 1},
                "contentType": "application/json", "agenticId": "buyer-42",
            })]
        );
    }
}

#[tokio::test]
async fn a2a_subscription_and_trial_data_remain_verbatim() {
    let service = json!({
        "sid": "39218", "serviceId": "subscription-uuid", "serviceType": "A2A",
        "subscription": [{"interval": "month", "fee": "0.1"}],
        "supportTrial": true, "freeTrial": 24, "feeTokenSymbol": "USDT",
    });
    let mut client = TaskApiClient {
        response: json!({"services": [service.clone()]}),
        calls: vec![],
    };
    a2a_detail::handle_service_detail(&mut client, "39218", "buyer-42")
        .await
        .unwrap();
    assert_eq!(output::take(), service);
}

#[tokio::test]
async fn a2a_does_not_fall_back_to_wallet_response_shapes() {
    let mut client = TaskApiClient {
        response: json!([{"list": [{"id": 39218, "fee": "0.1"}]}]),
        calls: vec![],
    };
    assert!(
        a2a_detail::handle_service_detail(&mut client, "39218", "buyer-42")
            .await
            .is_err()
    );
    assert_eq!(client.calls.len(), 1);
    assert_eq!(client.calls[0]["method"], "POST");
}

#[tokio::test]
async fn mcp_only_uses_wallet_api_and_preserves_raw_snapshot() {
    let service = json!({
        "id": 38241, "serviceType": "A2MCP", "fee": "0.1",
        "endpoint": "https://example.invalid/mcp", "reqType": "MCP",
        "inputSchema": {"type": "object"}, "toolName": "wallet_insights_report",
    });
    let mut client = TaskApiClient {
        response: json!([{"list": [service.clone()]}]),
        calls: vec![],
    };
    mcp_prepare::prepare_service(&mut client, "38241", "10191")
        .await
        .unwrap();
    assert_eq!(output::take()["payload"]["serviceSnapshot"], service);
    assert_eq!(
        client.calls,
        vec![json!({
            "method": "GET", "path": "/priapi/v5/wallet/agentic/agent/services",
            "query": [["agentId", "10191"], ["id", "38241"]],
        })]
    );
}

#[tokio::test]
async fn mcp_rejects_a2a_without_switching_to_the_task_api() {
    let mut client = TaskApiClient {
        response: json!([{"list": [{"id": 39218, "serviceType": "A2A", "fee": "0.1"}]}]),
        calls: vec![],
    };
    assert!(mcp_prepare::prepare_service(&mut client, "39218", "9967")
        .await
        .is_err());
    assert_eq!(client.calls.len(), 1);
    assert_eq!(client.calls[0]["method"], "GET");
}
