//! Token selection for new A2A purchases only. Existing jobs use their snapshot.

use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use serde_json::{json, Value};

use super::{XLAYER_CHAIN_ID, XLAYER_CHAIN_INDEX};
use crate::commands::agentic_wallet::balance;
use crate::validators::readable_to_minimal_str;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaymentToken {
    pub symbol: &'static str,
    pub address: &'static str,
}

pub const TOKENS: [PaymentToken; 3] = [
    PaymentToken {
        symbol: "USDC",
        address: "0xB6CEceAB302E2E4948951eE7843FC24E92933061",
    },
    PaymentToken {
        symbol: "USDG",
        address: "0x4Ae46a509f6B1d9056937bA4500cB143933D2DC8",
    },
    PaymentToken {
        symbol: "USDT",
        address: "0x779dEd0c9e1022225f8E0630b35a9b54bE713736",
    },
];

pub fn normalize_symbol(symbol: &str) -> Result<String> {
    let symbol = symbol.trim().replace('₮', "T").to_uppercase();
    match symbol.as_str() {
        "USDT" | "USDT0" => Ok("USDT".into()),
        "USDC" | "USDG" => Ok(symbol),
        _ => bail!("unsupported payment token; use USDC, USDG or USDT"),
    }
}

/// Empty inputs enter the automatic path. Explicit inputs constrain it.
pub fn constraint(chain_id: u64, symbol: &str, address: &str) -> Result<Option<PaymentToken>> {
    if chain_id != XLAYER_CHAIN_ID as u64 {
        bail!("A2A payments require X Layer chainId {XLAYER_CHAIN_ID}");
    }
    let by_symbol = if symbol.is_empty() {
        None
    } else {
        let normalized = normalize_symbol(symbol)?;
        Some(
            *TOKENS
                .iter()
                .find(|token| token.symbol == normalized)
                .unwrap(),
        )
    };
    let by_address = if address.is_empty() {
        None
    } else {
        Some(
            *TOKENS
                .iter()
                .find(|token| token.address.eq_ignore_ascii_case(address))
                .ok_or_else(|| anyhow!("unsupported X Layer payment token address"))?,
        )
    };
    if by_symbol.is_some() && by_address.is_some() && by_symbol != by_address {
        bail!("payment token symbol and service token address differ");
    }
    Ok(by_symbol.or(by_address))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FundingOption {
    pub token_symbol: &'static str,
    pub token_address: &'static str,
    pub required: String,
    pub balance: String,
    pub shortfall: String,
}

pub fn funding_option(
    token: PaymentToken,
    required: &str,
    holding: &str,
    decimals: u32,
) -> Result<FundingOption> {
    // Guard metadata before passing its precision to a padding utility.
    if decimals > 255 {
        bail!("invalid token decimals: {decimals}");
    }
    for amount in [required, holding] {
        let normalized = crate::funding::readable_shortfall(amount, "0")
            .ok_or_else(|| anyhow!("invalid readable payment balance or amount"))?;
        if normalized != "0" {
            readable_to_minimal_str(amount, decimals)?;
        }
    }
    let shortfall = crate::funding::readable_shortfall(required, holding)
        .ok_or_else(|| anyhow!("invalid readable payment balance or amount"))?;
    Ok(FundingOption {
        token_symbol: token.symbol,
        token_address: token.address,
        required: required.into(),
        balance: holding.into(),
        shortfall,
    })
}

pub struct Selection {
    pub token: Option<PaymentToken>,
    pub options: Vec<FundingOption>,
}

/// Reusable asynchronous seam: dependency errors propagate, never become zero.
async fn select_with<F, Fut>(
    required: &str,
    constrained: Option<PaymentToken>,
    all: bool,
    mut query: F,
) -> Result<Selection>
where
    F: FnMut(PaymentToken) -> Fut,
    Fut: std::future::Future<Output = Result<(String, u32)>>,
{
    let zero = crate::funding::readable_shortfall(required, "0")
        .ok_or_else(|| anyhow!("invalid payment amount"))?
        == "0";
    if zero {
        return Ok(Selection {
            token: Some(constrained.unwrap_or(TOKENS[0])),
            options: vec![],
        });
    }
    let mut selected = None;
    let mut options = Vec::new();
    for token in TOKENS {
        let (holding, decimals) = query(token).await?;
        let option = funding_option(token, required, &holding, decimals)?;
        if option.shortfall == "0"
            && constrained.is_none_or(|wanted| wanted == token)
            && selected.is_none()
        {
            selected = Some(token);
        }
        options.push(option);
        if selected.is_some() && !all {
            break;
        }
    }
    Ok(Selection {
        token: selected,
        options,
    })
}

pub async fn select(
    required: &str,
    account_id: &str,
    address: &str,
    constrained: Option<PaymentToken>,
    all: bool,
) -> Result<Selection> {
    select_with(required, constrained, all, |token| async move {
        let holding = balance::query_token_readable_for_account(
            account_id,
            address,
            XLAYER_CHAIN_INDEX,
            token.address,
        )
        .await?;
        let decimals = match holding.as_ref().and_then(|value| value.decimals) {
            Some(decimals) => decimals,
            None => {
                balance::query_token_metadata(XLAYER_CHAIN_INDEX, token.address)
                    .await?
                    .decimals
            }
        };
        Ok((
            holding
                .map(|value| value.balance)
                .unwrap_or_else(|| "0".into()),
            decimals,
        ))
    })
    .await
}

/// Preserve the authoritative service and caller-owned draft; never create a job.
pub fn funding_result(
    selection: &Selection,
    address: &str,
    service: &Value,
    sid: &str,
    chosen: Option<PaymentToken>,
    payment_constraint: Option<PaymentToken>,
) -> Result<Value> {
    for key in ["serviceId", "providerAgentId"] {
        if service
            .get(key)
            .and_then(Value::as_str)
            .is_none_or(|value| value.trim().is_empty())
        {
            bail!("funding recovery requires authoritative {key}");
        }
    }
    if sid.trim().is_empty() {
        bail!("funding recovery requires authoritative sid");
    }
    if chosen.is_some_and(|token| payment_constraint.is_some_and(|constraint| token != constraint))
    {
        bail!("funding target must match the explicit payment token constraint");
    }
    let required = selection
        .options
        .first()
        .ok_or_else(|| anyhow!("missing funding options"))?
        .required
        .clone();
    let funding_options: Vec<_> = selection
        .options
        .iter()
        .filter(|option| {
            payment_constraint.is_none_or(|constraint| {
                option.token_symbol == constraint.symbol
                    && option
                        .token_address
                        .eq_ignore_ascii_case(constraint.address)
            })
        })
        .cloned()
        .collect();
    if payment_constraint.is_some() && funding_options.is_empty() {
        bail!("explicit payment token is missing from funding options");
    }
    let mut payload = service.clone();
    payload["quoteAmount"] = json!(required);
    payload["quoteCurrency"] = json!("USD");
    payload["fundingOptions"] = serde_json::to_value(&funding_options)?;
    let mut resume_context = json!({
        "sid": sid, "serviceId": service["serviceId"], "providerAgentId": service["providerAgentId"],
        "taskType": if service["supportSubscription"].as_bool() == Some(true) { "subscription" } else { "one_time" },
        "paymentWalletAddress": address,
    });
    if let Some(token) = payment_constraint {
        resume_context["paymentTokenSymbol"] = json!(token.symbol);
        resume_context["serviceTokenAddress"] = json!(token.address);
    }
    payload["resumeContext"] = resume_context;
    let funding_target = chosen.or(payment_constraint);
    if let Some(token) = funding_target {
        let option = selection
            .options
            .iter()
            .find(|option| option.token_symbol == token.symbol)
            .ok_or_else(|| anyhow!("missing chosen funding token"))?;
        let bundle = crate::funding::build_funding_bundle_for_address(
            "",
            XLAYER_CHAIN_INDEX,
            address,
            crate::funding::FundingBlockedInput {
                asset: token.symbol,
                token_address: token.address,
                required: &option.required,
                balance: Some(&option.balance),
                operation: Some(crate::funding::FUNDING_OPERATION_TASK_CREATION),
                error_code: None,
                error_message: None,
            },
        )?;
        for (key, value) in bundle["payload"]
            .as_object()
            .ok_or_else(|| anyhow!("invalid Funding bundle"))?
        {
            payload[key] = value.clone();
        }
        payload["fundingTokenSymbol"] = json!(token.symbol);
    }
    Ok(
        json!({ "phase": "funding_required", "decision": "blocked", "reason": "insufficient_balance",
        "nextAction": [{"id": if funding_target.is_some() { "fund_selected_token" } else { "select_funding_token" }, "recommend": true}],
        "payload": payload }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct AgentCli {
        #[command(subcommand)]
        cmd: crate::commands::agent_commerce::AgentCommand,
    }
    #[derive(Parser)]
    struct UserCli {
        #[command(subcommand)]
        cmd: crate::commands::agent_commerce::task::user::TaskCommand,
    }

    #[test]
    fn both_cli_definitions_accept_auto_and_explicit_payment_inputs() {
        // AgentCommand contains the whole CLI tree. Parsing it needs more than
        // libtest's default thread stack; production uses the main-thread stack.
        std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn(check_cli_inputs)
            .unwrap()
            .join()
            .unwrap();
    }

    fn check_cli_inputs() {
        let fields = [
            "--title",
            "Report",
            "--description",
            "Summarize confirmed inputs",
            "--provider-agent-id",
            "asp",
            "--service-id",
            "uuid",
            "--service-token-amount",
            "10",
        ];
        for explicit in [false, true] {
            let mut one = vec!["test", "create-task"];
            one.extend(fields);
            one.extend(["--payment-token-amount", "10"]);
            if explicit {
                one.extend([
                    "--payment-token-symbol",
                    "USDC",
                    "--service-token-address",
                    TOKENS[0].address,
                ]);
            }
            assert!(AgentCli::try_parse_from(&one).is_ok());
            one[1] = "create";
            assert!(UserCli::try_parse_from(&one).is_ok());
            let mut subscription = vec!["test", "create-subscribe"];
            subscription.extend(fields);
            subscription.extend(["--auto-renew", "1"]);
            if explicit {
                subscription.extend(["--service-token-address", TOKENS[1].address]);
            }
            assert!(AgentCli::try_parse_from(&subscription).is_ok());
            assert!(UserCli::try_parse_from(&subscription).is_ok());
        }
        for token in TOKENS {
            let args = [
                "test",
                "task-create-prepare",
                "--sid",
                "123",
                "--funding-token-symbol",
                token.symbol,
            ];
            assert!(AgentCli::try_parse_from(args).is_ok());
            assert!(UserCli::try_parse_from(args).is_ok());
            let constrained_recovery = [
                "test",
                "task-create-prepare",
                "--sid",
                "123",
                "--funding-token-symbol",
                token.symbol,
                "--payment-token-symbol",
                token.symbol,
            ];
            assert!(AgentCli::try_parse_from(constrained_recovery).is_ok());
            assert!(UserCli::try_parse_from(constrained_recovery).is_ok());
        }
    }

    async fn balances(
        values: [&str; 3],
        constrained: Option<PaymentToken>,
        all: bool,
    ) -> Selection {
        select_with("10", constrained, all, |token| {
            let index = TOKENS
                .iter()
                .position(|candidate| *candidate == token)
                .unwrap();
            let value = values[index].to_string();
            async move { Ok((value, 6)) }
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn ordered_selection_never_combines_balances() {
        for (values, expected) in [
            (["10", "10", "10"], Some(TOKENS[0])),
            (["6", "10", "20"], Some(TOKENS[1])),
            (["6", "4", "10"], Some(TOKENS[2])),
            (["6", "4", "0"], None),
        ] {
            let result = balances(values, None, false).await;
            assert_eq!(result.token, expected);
            if expected.is_none() {
                assert_eq!(result.options.len(), 3);
                assert_eq!(
                    result
                        .options
                        .iter()
                        .map(|option| option.shortfall.as_str())
                        .collect::<Vec<_>>(),
                    ["4", "6", "10"]
                );
            }
        }
    }

    #[tokio::test]
    async fn explicit_token_is_never_silently_replaced() {
        assert_eq!(
            balances(["10", "4", "20"], Some(TOKENS[1]), false)
                .await
                .token,
            None
        );
        assert_eq!(
            balances(["10", "10", "20"], Some(TOKENS[1]), false)
                .await
                .token,
            Some(TOKENS[1])
        );
    }

    #[tokio::test]
    async fn zero_fee_needs_no_positive_balance_or_query() {
        let result = select_with("0.000000", None, false, |_| async {
            panic!("zero fee must not query")
        })
        .await
        .unwrap();
        assert_eq!(result.token, Some(TOKENS[0]));
    }

    #[tokio::test]
    async fn dependency_error_stops_before_fallback() {
        let mut calls = 0;
        let result = select_with("10", None, false, |_| {
            calls += 1;
            async { Err(anyhow!("balance unavailable")) }
        })
        .await;
        assert!(result.is_err());
        assert_eq!(calls, 1);
    }

    #[test]
    fn exact_precision_and_invalid_values() {
        assert_eq!(
            funding_option(TOKENS[0], "0.000001", "0", 6)
                .unwrap()
                .shortfall,
            "0.000001"
        );
        assert_eq!(
            funding_option(TOKENS[1], "10.000001", "9.999999", 6)
                .unwrap()
                .shortfall,
            "0.000002"
        );
        assert_eq!(
            funding_option(TOKENS[2], "0.000001", "0.000001000000000001", 18)
                .unwrap()
                .shortfall,
            "0"
        );
        assert!(funding_option(TOKENS[0], "0.000001", "0", 5).is_err());
        for invalid in ["-1", "NaN", "", "1e2", "not a balance"] {
            assert!(funding_option(TOKENS[0], "10", invalid, 6).is_err());
        }
        assert!(funding_option(TOKENS[0], "10", "0", u32::MAX).is_err());
    }

    #[test]
    fn contracts_chain_aliases_and_conflicts() {
        for token in TOKENS {
            assert_eq!(
                constraint(196, "", &token.address.to_lowercase()).unwrap(),
                Some(token)
            );
            assert_eq!(
                constraint(196, token.symbol, token.address).unwrap(),
                Some(token)
            );
            assert!(constraint(1, token.symbol, token.address).is_err());
        }
        assert_eq!(normalize_symbol("USD₮0").unwrap(), "USDT");
        for old in [
            "0x1E4a5963aBFD975d8c9021ce480b42188849D41d",
            "0x74b7F16337b8972027F6196A17a631aC6dE26d22",
        ] {
            assert!(constraint(196, "", old).is_err());
        }
        assert!(constraint(196, "USDC", TOKENS[2].address).is_err());
    }

    #[tokio::test]
    async fn funding_all_targets_changes_and_fresh_recovery() {
        let service = json!({"sid": "123", "serviceId": "uuid", "providerAgentId": "asp", "supportSubscription": true});
        let address = "0x1234567890abcdef1234567890abcdef12345678";
        for chosen in TOKENS {
            let selection = balances(["6", "4", "0"], Some(chosen), true).await;
            let result =
                funding_result(&selection, address, &service, "123", Some(chosen), None).unwrap();
            assert_eq!(result["phase"], "funding_required");
            assert_eq!(result["decision"], "blocked");
            assert_eq!(result["reason"], "insufficient_balance");
            assert_eq!(result["nextAction"][0]["id"], "fund_selected_token");
            assert_eq!(result["payload"]["operation"], "task_creation");
            assert_eq!(result["payload"]["fundingNeed"]["required"], "10");
            assert_eq!(
                result["payload"]["fundingOptions"]
                    .as_array()
                    .unwrap()
                    .len(),
                3
            );
            assert_eq!(result["payload"]["fundingNeed"]["asset"], chosen.symbol);
            assert_eq!(
                result["payload"]["fundingNeed"]["tokenAddress"],
                chosen.address
            );
            assert_eq!(
                result["payload"]["fundingTarget"]["receiveAddress"],
                address
            );
            assert_eq!(result["payload"]["fundingTarget"]["chainIndex"], "196");
            assert!(result["payload"]["qr"].is_object());
            assert_eq!(result["payload"]["resumeContext"]["serviceId"], "uuid");
            assert!(result["payload"].get("jobId").is_none());
        }
        let initial = balances(["6", "4", "0"], None, true).await;
        let result = funding_result(&initial, address, &service, "123", None, None).unwrap();
        assert_eq!(result["nextAction"][0]["id"], "select_funding_token");
        assert!(result["payload"].get("qr").is_none());
        assert!(result["payload"]["resumeContext"]
            .get("paymentTokenSymbol")
            .is_none());
        assert!(result["payload"]["resumeContext"]
            .get("serviceTokenAddress")
            .is_none());
        assert_eq!(
            balances(["6", "10", "0"], Some(TOKENS[1]), true)
                .await
                .token,
            Some(TOKENS[1])
        );
        // Funding target does not lock the create-time selection.
        assert_eq!(
            balances(["10", "10", "0"], None, false).await.token,
            Some(TOKENS[0])
        );
        assert_eq!(
            balances(["6", "10", "0"], None, false).await.token,
            Some(TOKENS[1])
        );
    }

    #[tokio::test]
    async fn explicit_payment_constraint_is_preserved_and_pins_recovery() {
        let address = "0x1234567890abcdef1234567890abcdef12345678";
        let selected = TOKENS[1];
        // USDC and USDT both cover the fee, but an explicit USDG payment choice
        // must remain pinned through one-time and subscription recovery.
        let selection = balances(["10", "0", "10"], Some(selected), true).await;

        for (is_subscription, task_type) in [(false, "one_time"), (true, "subscription")] {
            let service = json!({
                "sid": "123",
                "serviceId": "uuid",
                "providerAgentId": "asp",
                "supportSubscription": is_subscription
            });
            let result = funding_result(
                &selection,
                address,
                &service,
                "123",
                Some(selected),
                Some(selected),
            )
            .unwrap();

            assert_eq!(result["nextAction"][0]["id"], "fund_selected_token");
            assert_eq!(
                result["payload"]["fundingOptions"]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
            assert_eq!(
                result["payload"]["fundingOptions"][0]["tokenSymbol"],
                selected.symbol
            );
            assert_eq!(result["payload"]["fundingNeed"]["asset"], selected.symbol);
            assert_eq!(
                result["payload"]["resumeContext"]["paymentTokenSymbol"],
                selected.symbol
            );
            assert_eq!(
                result["payload"]["resumeContext"]["serviceTokenAddress"],
                selected.address
            );
            assert_eq!(result["payload"]["resumeContext"]["taskType"], task_type);
        }

        let service = json!({"serviceId":"uuid","providerAgentId":"asp"});
        assert!(funding_result(
            &selection,
            address,
            &service,
            "123",
            Some(TOKENS[0]),
            Some(selected),
        )
        .is_err());
    }
}
